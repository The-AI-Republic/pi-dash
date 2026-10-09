//! PR / code-review link handlers (D-18 handlers H, PIDASHCONV-680).
//!
//! Ports the four `apps/api/pi_dash/api/views/github_pr.py:29-100` and
//! `apps/api/pi_dash/api/views/git_code_review.py:24-100` endpoint units,
//! registered by [`super::routes`] at the four
//! `apps/api/pi_dash/api/urls/work_item.py:218-236` paths:
//!
//! * `GET`/`POST .../work-items/<issue_id>/github/pull-requests/`
//! * `DELETE .../pull-requests/<pk>/`
//! * `GET`/`POST .../work-items/<issue_id>/code-reviews/`
//! * `DELETE .../code-reviews/<pk>/`
//!
//! Layering (all foundation use is read-only): row shapes in
//! `pidash_services::v1_work_items::shape_links` (S3, PIDASHCONV-662),
//! queryset semantics in `queries_sub` (Q2, PIDASHCONV-669 — the
//! representative SQL carries `:named` placeholders, so this module binds
//! the executable `$n` form), gate decisions in [`super::perms`] over the
//! F-06 kernel (P1, PIDASHCONV-671), attach/detach verbs in the merged
//! `pidash_services::app_issues::pr_links` (D-26) and
//! `pidash_services::integrations::code_reviews` (D-05) kernels. This
//! module owns the HTTP shell (API-key auth, the slug→UUID rewrite,
//! permission wiring, body parsing, the paginated envelope) plus the two
//! sqlx store implementations the kernels are seamed over — the same
//! split the D-26 app-surface handlers use, mirroring their store and
//! transport implementations (which are private to that surface).
//!
//! Request order (preserved, not redesigned): API-key authentication,
//! then the slug→UUID rewrite (`api/views/base.py:51-98`, skipped for
//! anonymous callers so slugs cannot be probed via 404-vs-401), then
//! `check_permissions`, then the handler body. Timezone activation runs
//! after the gate (`TimezoneMixin.initial` runs after `super().initial()`;
//! an unknown zone 400s only for survivors).
//!
//! Ignored query params (verified against the views, not assumed): the
//! list order comes from URL kwargs (`self.kwargs.get("order_by",
//! "-created_at")`), never from `?order_by=` (ported bug 7 in Q2); the
//! serializers are constructed without `fields=`/`expand=`, so those
//! params never filter these responses. Only `?per_page=`/`?cursor=`
//! affect the list body.
//!
//! Ported bugs and deliberate warts (also listed in the PR):
//!
//! * The detail querysets carry no archived-project guard and no
//!   `.distinct()` (Q2 ported bug 5): detach works on archived projects.
//! * `request.data.get("url")` runs without serializer validation, so a
//!   non-object JSON body 500s (`AttributeError` on `.get`) and a truthy
//!   non-string `url` 500s (`.strip()` on a non-string); only falsy
//!   non-strings collapse to `""` (the invalid-URL 400).
//! * `str(existing.issue_id) != str(issue_id)` re-attach comparisons and
//!   the attach race re-read keep their TOCTOU gap (kernel behavior).
//! * The D-05 `LINK_DELETE_SQL`/`LEGACY_DELETE_SQL` consts say hard
//!   `DELETE`, but both models inherit `SoftDeleteModel`
//!   (`db/mixins.py:71-78`), so detach soft-deletes plus the
//!   `soft_delete_related_objects` fan-out (the D-26 store precedent;
//!   the consts are not executed here).
//!
//! Fixture: `F18-11` (`rust-api/fixtures/v1_work_items/handlers/
//! F18-11.work_items.json`, `pr_*` + `review_*` calls).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_auth::scope::TenantScope;

use crate::state::AppState;
use crate::v1_projects::handlers_project::{
    envelope, preamble, project_base_facts, query_last, rewrite_project_id, QueryMap,
};

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// Exact bytes of the DRF `IsAuthenticated` denial (anonymous on a guarded
/// endpoint).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `APIKeyAuthentication` failure (`api/middleware/api_authentication.py`).
pub const INVALID_TOKEN_BODY: &str = r#"{"detail":"Given API token is not valid"}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`api/views/base.py:154-158`): scoped `.get()` misses.
pub const NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// `Project.resolve` misses raise `Http404("Project not found")`, which
/// DRF re-raises as `NotFound(*exc.args)` (`db/models/project.py:213-218`).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// Attach issue probe (`github_pr.py:69`, `git_code_review.py:67`).
pub const WORK_ITEM_NOT_FOUND_BODY: &str = r#"{"error":"Work item not found."}"#;
/// `handle_exception`'s generic branch (`api/views/base.py:166-170`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `RequestBodySizeLimitMiddleware` past 5 MiB (the D-26 app-surface
/// precedent; this module takes the full `Request` for the proxy fallback,
/// so the limit is enforced here instead of axum's default). Spacing is
/// Django `JsonResponse` default (`json.dumps` separators), matching
/// `crate::middleware::BODY_TOO_LARGE_JSON` — not DRF's compact render.
pub const REQUEST_TOO_LARGE_BODY: &str = r#"{"error": "REQUEST_BODY_TOO_LARGE", "detail": "The size of the request body exceeds the maximum allowed size."}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (no `X-Api-Key` header).
    Unauthorized,
    /// 403, invalid/expired/inactive API or machine token.
    InvalidToken,
    /// 403, the DRF-default `PermissionDenied` body (no D-18 guard class
    /// sets `message`).
    Forbidden,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, `{"Detail":"Project not found"}` (identifier rewrite miss).
    ProjectNotFound,
    /// 404, `{"error": ...}` (attach issue probe).
    WorkItemNotFound,
    /// 400, `{"detail": ...}` (DRF `ParseError`: pagination, JSON).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline).
    BadError(String),
    /// 404, view-inline `{"error": ...}` with a custom message.
    NotFoundError(String),
    /// 409, `{"error": ...}` (already-linked conflicts).
    Conflict(String),
    /// 413, oversized request body.
    RequestTooLarge,
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::InvalidToken => (StatusCode::FORBIDDEN, INVALID_TOKEN_BODY.to_owned()),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                super::perms::CLASS_DENIAL_BODY.to_owned(),
            ),
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            Denial::ProjectNotFound => (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned()),
            Denial::WorkItemNotFound => {
                (StatusCode::NOT_FOUND, WORK_ITEM_NOT_FOUND_BODY.to_owned())
            }
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::NotFoundError(message) => (
                StatusCode::NOT_FOUND,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::Conflict(message) => (
                StatusCode::CONFLICT,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::RequestTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                REQUEST_TOO_LARGE_BODY.to_owned(),
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
        if matches!(self, Denial::ServerError) {
            tracing::warn!("v1_work_items pr-links handler: internal error");
        }
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("pr-links error response")
    }
}

/// Map the reused D-19 shell's denial onto this module's. The variants
/// are pairwise identical (same Python source); `FieldErrors` is the only
/// shape these views never answer (no serializer runs here), so it falls
/// back to the generic 500.
impl From<crate::v1_projects::handlers_project::Denial> for Denial {
    fn from(denial: crate::v1_projects::handlers_project::Denial) -> Self {
        use crate::v1_projects::handlers_project::Denial as D;
        match denial {
            D::Unauthorized => Denial::Unauthorized,
            D::InvalidToken => Denial::InvalidToken,
            D::Forbidden => Denial::Forbidden,
            D::NotFound => Denial::NotFound,
            D::ProjectNotFound => Denial::ProjectNotFound,
            D::BadDetail(message) => Denial::BadDetail(message),
            D::BadError(message) => Denial::BadError(message),
            D::FieldErrors(_) => Denial::ServerError,
            D::NotFoundError(message) => Denial::NotFoundError(message),
            D::Conflict(body) => Denial::Conflict(body),
            D::ServerError => Denial::ServerError,
        }
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Render a 200 JSON response with exact bytes.
fn json_ok(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("handler json response")
}

/// Render a 201 JSON response with exact bytes.
fn json_created(body: String) -> Response {
    Response::builder()
        .status(StatusCode::CREATED)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("handler json response")
}

/// DRF's 204: empty body with NO content type (Django only sets
/// `Content-Type` when it renders a body).
fn no_content() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("handler empty response")
}

// ---------------------------------------------------------------------------
// Cutover wiring
// ---------------------------------------------------------------------------

/// Route registration is the cutover granularity (the pilot `owned()`
/// pattern): the owned methods serve from Rust, every other method on the
/// path proxies to Django so its 405-after-auth and metadata responses are
/// preserved byte for byte.
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
    router
}

/// The two list paths own GET+POST (`urls/work_item.py:218-221,228-231`).
#[allow(dead_code)]
pub fn owned_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "POST"])
}

/// The two detail paths own DELETE only (`urls/work_item.py:223-226,
/// 233-236`).
#[allow(dead_code)]
pub fn owned_detail(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["DELETE"])
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Map a database/driver failure to the generic 500 while logging the site
/// and error for operators (no secrets: messages never include tokens).
fn db_error<E: std::fmt::Display>(error: E, site: &str) -> Denial {
    tracing::warn!(%error, site, "v1_work_items pr-links database failure");
    Denial::ServerError
}

/// True when the failure is an integrity violation (SQLSTATE class 23):
/// Python's `IntegrityError` arm answers the payload 400.
fn is_integrity_error(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().is_some_and(|code| code.starts_with("23")))
}

/// True when the failure is a unique violation (SQLSTATE 23505): the
/// create-race arm.
fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().is_some_and(|code| code == "23505"))
}

/// Django's `<uuid:...>` path converter: strict lowercase hyphenated hex.
/// Anything else never reaches the view (the resolver 404s), so callers
/// proxy the request to Django for its exact bytes (the D-26 precedent).
fn path_uuid(raw: &str) -> Result<uuid::Uuid, ()> {
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
    raw.parse::<uuid::Uuid>().map_err(|_| ())
}

/// Python `bool()` over a JSON value (the `(raw_url or "")` collapse).
fn is_python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn page_denial(error: crate::paginator::PageError) -> Denial {
    use crate::paginator::PageError as E;
    match error {
        E::InvalidPerPage
        | E::PerPageTooLarge(_)
        | E::InvalidCursor
        | E::OffsetTooLarge
        | E::NegativeOffset => Denial::BadDetail(error.detail()),
        E::ZeroLimit | E::NegativeSlice | E::NonFiniteCursor | E::MissingOrderKey => {
            Denial::ServerError
        }
    }
}

// ---------------------------------------------------------------------------
// Gate + timezone + bodies
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

/// Run the route's permission gate over caller-fetched membership facts
/// (P1, [`super::perms`]). All four PR/review routes carry
/// `ProjectEntityPermission`.
async fn require_entity_gate(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    route: super::perms::V1WorkItemsRoute,
    method: &str,
) -> Result<(), Denial> {
    let facts = project_base_facts(pool, workspace_id, workspace_slug, user_id, project_id).await?;
    let scope = TenantScope::new(pidash_types::WorkspaceId::from(workspace_slug.to_owned()));
    let gate = super::perms::gate_for(route, method);
    if super::perms::decide(gate, method, &scope, &facts) {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

/// Parse a POST body the way DRF's `JSONParser` does for these views
/// (the D-19 api-surface precedent): empty → `{}`; malformed → the
/// `ParseError` 400; non-object JSON → the 500 Python raises calling
/// `.get` on it (`AttributeError` — these views run no serializer, so the
/// `non_field_errors` shape never applies here).
fn parse_json_body(raw: &[u8]) -> Result<Map<String, Value>, Denial> {
    if raw.is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_slice::<Value>(raw) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(Denial::ServerError),
        Err(error) => Err(Denial::BadDetail(format!("JSON parse error - {error}"))),
    }
}

/// `(request.data.get("url") or "")` (`github_pr.py:61`,
/// `git_code_review.py:59`, `code_reviews.py:168`): missing/null and falsy
/// non-strings collapse to `""` (the invalid-URL 400 below); truthy
/// non-strings 500 on `.strip()`; strings strip inside the port's parser.
fn extract_raw_url(body: &Map<String, Value>) -> Result<String, Denial> {
    match body.get("url").unwrap_or(&Value::Null) {
        Value::Null => Ok(String::new()),
        Value::String(text) => Ok(text.clone()),
        other if !is_python_truthy(other) => Ok(String::new()),
        _ => Err(Denial::ServerError),
    }
}

// ---------------------------------------------------------------------------
// List / detail SQL (Q2 semantics, executable `$n` binds)
// ---------------------------------------------------------------------------

/// PR-link list rows in fixture order (`github_pr.py:37-49`, fixture
/// `github_pr_list_queryset`): the join-span form verbatim — soft-delete
/// scope on the link table only, `DISTINCT`, `-created_at`. `$1` slug,
/// `$2` project id, `$3` issue id, `$4` member id.
const PR_LIST_SQL: &str = r#"SELECT DISTINCT "github_pull_request_links"."created_at", "github_pull_request_links"."updated_at", "github_pull_request_links"."created_by_id", "github_pull_request_links"."updated_by_id", "github_pull_request_links"."deleted_at", "github_pull_request_links"."id", "github_pull_request_links"."project_id", "github_pull_request_links"."workspace_id", "github_pull_request_links"."issue_id", "github_pull_request_links"."repo_owner", "github_pull_request_links"."repo_name", "github_pull_request_links"."pr_number", "github_pull_request_links"."url", "github_pull_request_links"."title", "github_pull_request_links"."state", "github_pull_request_links"."merged", "github_pull_request_links"."draft", "github_pull_request_links"."pr_updated_at" FROM "github_pull_request_links" INNER JOIN "workspaces" ON ("github_pull_request_links"."workspace_id" = "workspaces"."id") INNER JOIN "projects" ON ("github_pull_request_links"."project_id" = "projects"."id") INNER JOIN "project_members" ON ("projects"."id" = "project_members"."project_id") WHERE ("github_pull_request_links"."deleted_at" IS NULL AND "workspaces"."slug" = $1 AND "github_pull_request_links"."project_id" = $2 AND "github_pull_request_links"."issue_id" = $3 AND "project_members"."is_active" AND "project_members"."member_id" = $4 AND "projects"."archived_at" IS NULL) ORDER BY "github_pull_request_links"."created_at" DESC"#;

/// PR-link detail scope + pk (`github_pr.py:86-98`, fixture
/// `github_pr_detail_queryset`): no archived guard, no `DISTINCT`,
/// `Meta.ordering` (`-created_at`). `$5` pk.
const PR_DETAIL_SQL: &str = r#"SELECT "github_pull_request_links"."created_at", "github_pull_request_links"."updated_at", "github_pull_request_links"."created_by_id", "github_pull_request_links"."updated_by_id", "github_pull_request_links"."deleted_at", "github_pull_request_links"."id", "github_pull_request_links"."project_id", "github_pull_request_links"."workspace_id", "github_pull_request_links"."issue_id", "github_pull_request_links"."repo_owner", "github_pull_request_links"."repo_name", "github_pull_request_links"."pr_number", "github_pull_request_links"."url", "github_pull_request_links"."title", "github_pull_request_links"."state", "github_pull_request_links"."merged", "github_pull_request_links"."draft", "github_pull_request_links"."pr_updated_at" FROM "github_pull_request_links" INNER JOIN "workspaces" ON ("github_pull_request_links"."workspace_id" = "workspaces"."id") INNER JOIN "projects" ON ("github_pull_request_links"."project_id" = "projects"."id") INNER JOIN "project_members" ON ("projects"."id" = "project_members"."project_id") WHERE ("github_pull_request_links"."deleted_at" IS NULL AND "workspaces"."slug" = $1 AND "github_pull_request_links"."project_id" = $2 AND "github_pull_request_links"."issue_id" = $3 AND "project_members"."is_active" AND "project_members"."member_id" = $4 AND "github_pull_request_links"."id" = $5) ORDER BY "github_pull_request_links"."created_at" DESC LIMIT 1"#;

/// Review-link list rows (`git_code_review.py:32-44`, fixture
/// `code_review_list_queryset`): same join-span shape as [`PR_LIST_SQL`].
const REVIEW_LIST_SQL: &str = r#"SELECT DISTINCT "git_code_review_links"."created_at", "git_code_review_links"."updated_at", "git_code_review_links"."created_by_id", "git_code_review_links"."updated_by_id", "git_code_review_links"."deleted_at", "git_code_review_links"."id", "git_code_review_links"."project_id", "git_code_review_links"."workspace_id", "git_code_review_links"."issue_id", "git_code_review_links"."provider", "git_code_review_links"."host_url", "git_code_review_links"."namespace", "git_code_review_links"."repo_name", "git_code_review_links"."repo_external_id", "git_code_review_links"."external_id", "git_code_review_links"."external_iid", "git_code_review_links"."url", "git_code_review_links"."title", "git_code_review_links"."state", "git_code_review_links"."merged", "git_code_review_links"."draft", "git_code_review_links"."remote_updated_at", "git_code_review_links"."metadata" FROM "git_code_review_links" INNER JOIN "workspaces" ON ("git_code_review_links"."workspace_id" = "workspaces"."id") INNER JOIN "projects" ON ("git_code_review_links"."project_id" = "projects"."id") INNER JOIN "project_members" ON ("projects"."id" = "project_members"."project_id") WHERE ("git_code_review_links"."deleted_at" IS NULL AND "workspaces"."slug" = $1 AND "git_code_review_links"."project_id" = $2 AND "git_code_review_links"."issue_id" = $3 AND "project_members"."is_active" AND "project_members"."member_id" = $4 AND "projects"."archived_at" IS NULL) ORDER BY "git_code_review_links"."created_at" DESC"#;

/// Review-link detail scope + pk (`git_code_review.py:86-98`, fixture
/// `code_review_detail_queryset`): same shape as [`PR_DETAIL_SQL`].
const REVIEW_DETAIL_SQL: &str = r#"SELECT "git_code_review_links"."created_at", "git_code_review_links"."updated_at", "git_code_review_links"."created_by_id", "git_code_review_links"."updated_by_id", "git_code_review_links"."deleted_at", "git_code_review_links"."id", "git_code_review_links"."project_id", "git_code_review_links"."workspace_id", "git_code_review_links"."issue_id", "git_code_review_links"."provider", "git_code_review_links"."host_url", "git_code_review_links"."namespace", "git_code_review_links"."repo_name", "git_code_review_links"."repo_external_id", "git_code_review_links"."external_id", "git_code_review_links"."external_iid", "git_code_review_links"."url", "git_code_review_links"."title", "git_code_review_links"."state", "git_code_review_links"."merged", "git_code_review_links"."draft", "git_code_review_links"."remote_updated_at", "git_code_review_links"."metadata" FROM "git_code_review_links" INNER JOIN "workspaces" ON ("git_code_review_links"."workspace_id" = "workspaces"."id") INNER JOIN "projects" ON ("git_code_review_links"."project_id" = "projects"."id") INNER JOIN "project_members" ON ("projects"."id" = "project_members"."project_id") WHERE ("git_code_review_links"."deleted_at" IS NULL AND "workspaces"."slug" = $1 AND "git_code_review_links"."project_id" = $2 AND "git_code_review_links"."issue_id" = $3 AND "project_members"."is_active" AND "project_members"."member_id" = $4 AND "git_code_review_links"."id" = $5) ORDER BY "git_code_review_links"."created_at" DESC LIMIT 1"#;

// ---------------------------------------------------------------------------
// Row rendering (S3 shapes)
// ---------------------------------------------------------------------------

/// Render one PR-link row through `GithubPullRequestLinkSerializer`
/// (S3 [`render_pr_link`](pidash_services::v1_work_items::shape_links::render_pr_link)):
/// full field set, no `fields=`/`expand=` (the view passes neither).
fn render_pr_row(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<Value, Denial> {
    use pidash_services::v1_work_items::shape_links as shape;
    use sqlx::Row as _;
    let dt = |key: &str| -> Result<String, Denial> {
        row.try_get::<chrono::DateTime<chrono::Utc>, _>(key)
            .map(|stamp| crate::serializer::render_datetime_in(&stamp, tz))
            .map_err(|error| db_error(error, "pr row datetime"))
    };
    let dt_opt = |key: &str| -> Result<Option<String>, Denial> {
        row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(key)
            .map(|stamp| stamp.map(|stamp| crate::serializer::render_datetime_in(&stamp, tz)))
            .map_err(|error| db_error(error, "pr row datetime"))
    };
    let id: Uuid = row
        .try_get("id")
        .map_err(|error| db_error(error, "pr row map"))?;
    let issue_id: Uuid = row
        .try_get("issue_id")
        .map_err(|error| db_error(error, "pr row map"))?;
    let created_by: Option<Uuid> = row
        .try_get("created_by_id")
        .map_err(|error| db_error(error, "pr row map"))?;
    let id = id.to_string();
    let issue = issue_id.to_string();
    let created_by = created_by.map(|id| id.to_string());
    let pr_updated_at = dt_opt("pr_updated_at")?;
    let created_at = dt("created_at")?;
    let updated_at = dt("updated_at")?;
    let repo_owner: String = row
        .try_get("repo_owner")
        .map_err(|error| db_error(error, "pr row map"))?;
    let repo_name: String = row
        .try_get("repo_name")
        .map_err(|error| db_error(error, "pr row map"))?;
    let url: String = row
        .try_get("url")
        .map_err(|error| db_error(error, "pr row map"))?;
    let title: String = row
        .try_get("title")
        .map_err(|error| db_error(error, "pr row map"))?;
    let state: String = row
        .try_get("state")
        .map_err(|error| db_error(error, "pr row map"))?;
    let shape_row = shape::PrLinkRow {
        id: &id,
        issue: &issue,
        repo_owner: &repo_owner,
        repo_name: &repo_name,
        pr_number: row
            .try_get("pr_number")
            .map_err(|error| db_error(error, "pr row map"))?,
        url: &url,
        title: &title,
        state: &state,
        merged: row
            .try_get("merged")
            .map_err(|error| db_error(error, "pr row map"))?,
        draft: row
            .try_get("draft")
            .map_err(|error| db_error(error, "pr row map"))?,
        pr_updated_at: pr_updated_at.as_deref(),
        created_at: &created_at,
        updated_at: &updated_at,
        created_by: created_by.as_deref(),
    };
    let map = shape::render_pr_link(&shape::PrLinkShowInput {
        row: &shape_row,
        fields: None,
        expand: &[],
        expansions: &[],
    })
    .map_err(|error| db_error(error, "pr row render"))?;
    Ok(Value::Object(map))
}

/// Render one review-link row through `GitCodeReviewLinkSerializer` (S3).
fn render_review_row(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<Value, Denial> {
    use pidash_services::v1_work_items::shape_links as shape;
    use sqlx::Row as _;
    let dt = |key: &str| -> Result<String, Denial> {
        row.try_get::<chrono::DateTime<chrono::Utc>, _>(key)
            .map(|stamp| crate::serializer::render_datetime_in(&stamp, tz))
            .map_err(|error| db_error(error, "review row datetime"))
    };
    let dt_opt = |key: &str| -> Result<Option<String>, Denial> {
        row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(key)
            .map(|stamp| stamp.map(|stamp| crate::serializer::render_datetime_in(&stamp, tz)))
            .map_err(|error| db_error(error, "review row datetime"))
    };
    let id: Uuid = row
        .try_get("id")
        .map_err(|error| db_error(error, "review row map"))?;
    let issue_id: Uuid = row
        .try_get("issue_id")
        .map_err(|error| db_error(error, "review row map"))?;
    let created_by: Option<Uuid> = row
        .try_get("created_by_id")
        .map_err(|error| db_error(error, "review row map"))?;
    let metadata: Value = row
        .try_get("metadata")
        .map_err(|error| db_error(error, "review row map"))?;
    let id = id.to_string();
    let issue = issue_id.to_string();
    let created_by = created_by.map(|id| id.to_string());
    let remote_updated_at = dt_opt("remote_updated_at")?;
    let created_at = dt("created_at")?;
    let updated_at = dt("updated_at")?;
    let provider: String = row
        .try_get("provider")
        .map_err(|error| db_error(error, "review row map"))?;
    let host_url: String = row
        .try_get("host_url")
        .map_err(|error| db_error(error, "review row map"))?;
    let namespace: String = row
        .try_get("namespace")
        .map_err(|error| db_error(error, "review row map"))?;
    let repo_name: String = row
        .try_get("repo_name")
        .map_err(|error| db_error(error, "review row map"))?;
    let repo_external_id: String = row
        .try_get("repo_external_id")
        .map_err(|error| db_error(error, "review row map"))?;
    let external_id: String = row
        .try_get("external_id")
        .map_err(|error| db_error(error, "review row map"))?;
    let external_iid: String = row
        .try_get("external_iid")
        .map_err(|error| db_error(error, "review row map"))?;
    let url: String = row
        .try_get("url")
        .map_err(|error| db_error(error, "review row map"))?;
    let title: String = row
        .try_get("title")
        .map_err(|error| db_error(error, "review row map"))?;
    let state: String = row
        .try_get("state")
        .map_err(|error| db_error(error, "review row map"))?;
    let shape_row = shape::ReviewLinkRow {
        id: &id,
        issue: &issue,
        provider: &provider,
        host_url: &host_url,
        namespace: &namespace,
        repo_name: &repo_name,
        repo_external_id: &repo_external_id,
        external_id: &external_id,
        external_iid: &external_iid,
        url: &url,
        title: &title,
        state: &state,
        merged: row
            .try_get("merged")
            .map_err(|error| db_error(error, "review row map"))?,
        draft: row
            .try_get("draft")
            .map_err(|error| db_error(error, "review row map"))?,
        remote_updated_at: remote_updated_at.as_deref(),
        metadata: &metadata,
        created_at: &created_at,
        updated_at: &updated_at,
        created_by: created_by.as_deref(),
    };
    let map = shape::render_review_link(&shape::ReviewLinkShowInput {
        row: &shape_row,
        fields: None,
        expand: &[],
        expansions: &[],
    })
    .map_err(|error| db_error(error, "review row render"))?;
    Ok(Value::Object(map))
}

// ---------------------------------------------------------------------------
// Stores: the `pr_links` + `code_reviews` seams over sqlx
// ---------------------------------------------------------------------------

use pidash_services::app_issues::pr_links::{
    AttachError as PrAttachError, CreatePrLinkOutcome, InstallationRow, NewPrLink, NewReviewLink,
    PrLinkRow, PrLinksStore, ReviewLinkRow, ReviewMirrorFields, StoreError as PrStoreError,
};
use pidash_services::integrations::accounts::{
    GitStore, NewProviderAccount, StoreError as GitStoreError,
};
use pidash_services::integrations::code_reviews::{
    AdapterSource, AttachRequest, CodeReviewError, CodeReviewStore, NewCodeReviewLink,
    NewLegacyLink, ReviewAdapter, ReviewParser,
};
use pidash_services::integrations::repositories::{NewRepository, RepositoryDefaults};

/// Map a sqlx failure into the `pr_links` store error.
fn pr_store_error(context: &str, error: sqlx::Error) -> PrStoreError {
    let _ = context;
    PrStoreError(format!("{context}: {error}"))
}

/// Sentinel for the deleted-project lookup miss inside PR attach: the
/// handler maps it to the 404 `DoesNotExist` body (Python's
/// `self.project` fetch on `save()`).
const PROJECT_WORKSPACE_MISS: &str = "pr-links: project workspace not found";

/// Sentinel for the lost create race when the re-lookup also misses:
/// Python re-raises the `IntegrityError` → the payload 400.
const PR_RACE_MISS: &str = "pr link lost the create race";

/// Marker prefix for mid-attach integrity failures (FK races on the
/// mirror writes): Python's uncaught `IntegrityError` → the payload
/// 400, so the handler maps these to 400, not 500.
const PR_PAYLOAD_INVALID: &str = "pr-links: payload not valid";

/// Marker prefix for the same class on the review-link writes
/// (`insert_code_review_link` non-unique integrity,
/// `insert/update_legacy_link`): Python's uncaught `IntegrityError` →
/// the payload 400.
const REVIEW_PAYLOAD_INVALID: &str = "pr-links: review payload not valid";

/// Marker for the review-attach race reread miss: Python re-raises the
/// `IntegrityError` → the payload 400 (the kernel reports it as a store
/// error, so the message carries the marker).
const REVIEW_RACE_MISS: &str = "link insert conflicted but the row vanished on reread";

/// Map a mid-attach write failure: integrity violations carry the
/// payload marker (Python's `IntegrityError` 400), anything else the
/// generic store error (500).
fn pr_write_error(context: &str, error: sqlx::Error) -> PrStoreError {
    if is_integrity_error(&error) {
        PrStoreError(format!("{PR_PAYLOAD_INVALID}: {context}: {error}"))
    } else {
        pr_store_error(context, error)
    }
}

/// The live `PrLinksStore`: pool + request actor. Audit columns follow
/// CRUM (`BaseModel.save`): creates stamp `created_by`, updates stamp
/// `updated_by`, deletes stamp `updated_by`. The project workspace
/// resolves at write time (mirroring `save()`'s lazy project fetch),
/// deleted projects miss → the 404 sentinel.
struct PrStore {
    pool: sqlx::PgPool,
}

impl PrStore {
    fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    async fn workspace_of(&self, project_id: &uuid::Uuid) -> Result<uuid::Uuid, PrStoreError> {
        // `PROJECT_WORKSPACE_LOOKUP_SQL`, plus the default-manager
        // `deleted_at` scope: Python's `self.project` traversal misses
        // soft-deleted projects (`DoesNotExist` → 404), so the lookup
        // must too.
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| pr_store_error("project workspace", error))?;
        row.map(|row| row.0)
            .ok_or_else(|| PrStoreError(PROJECT_WORKSPACE_MISS.to_owned()))
    }
}

fn pr_link_row_from(row: &sqlx::postgres::PgRow) -> Result<PrLinkRow, PrStoreError> {
    use sqlx::Row as _;
    Ok(PrLinkRow {
        id: row
            .try_get("id")
            .map_err(|error| pr_store_error("pr link map", error))?,
        project_id: row
            .try_get("project_id")
            .map_err(|error| pr_store_error("pr link map", error))?,
        issue_id: row
            .try_get("issue_id")
            .map_err(|error| pr_store_error("pr link map", error))?,
        repo_owner: row
            .try_get("repo_owner")
            .map_err(|error| pr_store_error("pr link map", error))?,
        repo_name: row
            .try_get("repo_name")
            .map_err(|error| pr_store_error("pr link map", error))?,
        pr_number: row
            .try_get::<i32, _>("pr_number")
            .map(i64::from)
            .map_err(|error| pr_store_error("pr link map", error))?,
        url: row
            .try_get("url")
            .map_err(|error| pr_store_error("pr link map", error))?,
        title: row
            .try_get("title")
            .map_err(|error| pr_store_error("pr link map", error))?,
        state: row
            .try_get("state")
            .map_err(|error| pr_store_error("pr link map", error))?,
        merged: row
            .try_get("merged")
            .map_err(|error| pr_store_error("pr link map", error))?,
        draft: row
            .try_get("draft")
            .map_err(|error| pr_store_error("pr link map", error))?,
        pr_updated_at: row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("pr_updated_at")
            .map(|stamp| {
                stamp.map(|stamp| stamp.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
            })
            .map_err(|error| pr_store_error("pr link map", error))?,
    })
}

fn review_link_row_from(row: &sqlx::postgres::PgRow) -> Result<ReviewLinkRow, PrStoreError> {
    use sqlx::Row as _;
    Ok(ReviewLinkRow {
        id: row
            .try_get("id")
            .map_err(|error| pr_store_error("review link map", error))?,
        issue_id: row
            .try_get("issue_id")
            .map_err(|error| pr_store_error("review link map", error))?,
        metadata: row
            .try_get("metadata")
            .map_err(|error| pr_store_error("review link map", error))?,
    })
}

impl PrLinksStore for PrStore {
    async fn installation_for_account(
        &self,
        workspace_slug: &str,
        owner: &str,
    ) -> Result<Option<InstallationRow>, PrStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::app_issues::pr_links::INSTALLATION_LOOKUP_SQL)
                .bind(workspace_slug)
                .bind(owner)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| pr_store_error("installation lookup", error))?;
        row.map(|row| {
            use sqlx::Row as _;
            let id: Uuid = row
                .try_get("id")
                .map_err(|error| pr_store_error("installation map", error))?;
            let installation_id: i64 = row
                .try_get("installation_id")
                .map_err(|error| pr_store_error("installation map", error))?;
            Ok(InstallationRow {
                id,
                installation_id,
            })
        })
        .transpose()
    }

    async fn issue_exists(
        &self,
        issue_id: Uuid,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<bool, PrStoreError> {
        let row: Option<(i32,)> =
            sqlx::query_as(pidash_services::app_issues::pr_links::ISSUE_EXISTS_SQL)
                .bind(issue_id)
                .bind(project_id)
                .bind(workspace_slug)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| pr_store_error("issue exists", error))?;
        Ok(row.is_some())
    }

    async fn pr_link_by_pr(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<Option<PrLinkRow>, PrStoreError> {
        let number_i32 = i32::try_from(number).map_err(|_| {
            PrStoreError(format!(
                "pr_number {number} out of range for integer column"
            ))
        })?;
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::app_issues::pr_links::PR_LINK_LOOKUP_SQL)
                .bind(owner)
                .bind(name)
                .bind(number_i32)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| pr_store_error("pr link lookup", error))?;
        row.map(|row| pr_link_row_from(&row)).transpose()
    }

    async fn review_link_by_pr(
        &self,
        owner: &str,
        name: &str,
        number: &str,
    ) -> Result<Option<ReviewLinkRow>, PrStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::app_issues::pr_links::REVIEW_LINK_LOOKUP_SQL)
                .bind(owner)
                .bind(name)
                .bind(number)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| pr_store_error("review link lookup", error))?;
        row.map(|row| review_link_row_from(&row)).transpose()
    }

    async fn create_pr_link(
        &self,
        new: &NewPrLink,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<CreatePrLinkOutcome, PrStoreError> {
        let workspace_id = self.workspace_of(&new.project_id).await?;
        let pr_number = i32::try_from(new.pr_number).map_err(|_| {
            PrStoreError(format!(
                "pr_number {} out of range for integer column",
                new.pr_number
            ))
        })?;
        let outcome = sqlx::query(pidash_services::app_issues::pr_links::PR_LINK_INSERT_SQL)
            .bind(new.id)
            .bind(now)
            .bind(now)
            .bind(actor_id)
            .bind(new.project_id)
            .bind(workspace_id)
            .bind(new.issue_id)
            .bind(&new.repo_owner)
            .bind(&new.repo_name)
            .bind(pr_number)
            .bind(&new.url)
            .bind(&new.title)
            .bind(&new.state)
            .bind(new.merged)
            .bind(new.draft)
            .bind(new.pr_updated_at.as_deref().and_then(parse_snapshot_dt))
            .execute(&self.pool)
            .await;
        match outcome {
            Ok(_) => Ok(CreatePrLinkOutcome::Created(Box::new(PrLinkRow {
                id: new.id,
                project_id: new.project_id,
                issue_id: new.issue_id,
                repo_owner: new.repo_owner.clone(),
                repo_name: new.repo_name.clone(),
                pr_number: new.pr_number,
                url: new.url.clone(),
                title: new.title.clone(),
                state: new.state.clone(),
                merged: new.merged,
                draft: new.draft,
                pr_updated_at: new.pr_updated_at.clone(),
            }))),
            Err(error) if is_unique_violation(&error) => Ok(CreatePrLinkOutcome::Conflict),
            Err(error) if is_integrity_error(&error) => Err(PrStoreError(format!(
                "{PR_PAYLOAD_INVALID}: pr link insert: {error}"
            ))),
            Err(error) => Err(pr_store_error("pr link insert", error)),
        }
    }

    async fn create_review_link(
        &self,
        new: &NewReviewLink,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<ReviewLinkRow, PrStoreError> {
        let workspace_id = self.workspace_of(&new.project_id).await?;
        sqlx::query(pidash_services::app_issues::pr_links::REVIEW_LINK_INSERT_SQL)
            .bind(new.id)
            .bind(now)
            .bind(now)
            .bind(actor_id)
            .bind(new.project_id)
            .bind(workspace_id)
            .bind(new.issue_id)
            .bind(&new.url)
            .bind(&new.title)
            .bind(&new.state)
            .bind(new.merged)
            .bind(new.draft)
            .bind(new.remote_updated_at.as_deref().and_then(parse_snapshot_dt))
            .bind(&new.metadata)
            .bind(&new.namespace)
            .bind(&new.repo_name)
            .bind(&new.external_iid)
            .execute(&self.pool)
            .await
            .map_err(|error| pr_write_error("review link insert", error))?;
        Ok(ReviewLinkRow {
            id: new.id,
            issue_id: new.issue_id,
            metadata: new.metadata.clone(),
        })
    }

    async fn update_review_link(
        &self,
        id: Uuid,
        fields: &ReviewMirrorFields,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<(), PrStoreError> {
        let workspace_id = self.workspace_of(&fields.project_id).await?;
        sqlx::query(pidash_services::app_issues::pr_links::REVIEW_LINK_UPDATE_SQL)
            .bind(now)
            .bind(actor_id)
            .bind(fields.project_id)
            .bind(workspace_id)
            .bind(fields.issue_id)
            .bind(&fields.url)
            .bind(&fields.title)
            .bind(&fields.state)
            .bind(fields.merged)
            .bind(fields.draft)
            .bind(
                fields
                    .remote_updated_at
                    .as_deref()
                    .and_then(parse_snapshot_dt),
            )
            .bind(&fields.metadata)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|error| pr_write_error("review link update", error))?;
        Ok(())
    }

    async fn delete_pr_link(
        &self,
        id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<(), PrStoreError> {
        sqlx::query(pidash_services::app_issues::pr_links::PR_LINK_SOFT_DELETE_SQL)
            .bind(now)
            .bind(actor_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|error| pr_store_error("pr link delete", error))?;
        Ok(())
    }

    async fn delete_review_link(
        &self,
        id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<(), PrStoreError> {
        sqlx::query(pidash_services::app_issues::pr_links::REVIEW_LINK_SOFT_DELETE_SQL)
            .bind(now)
            .bind(actor_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|error| pr_store_error("review link delete", error))?;
        Ok(())
    }
}

/// Parse a snapshot ISO datetime (`GithubAdapter.parse_dt` format) for a
/// timestamptz bind.
fn parse_snapshot_dt(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|aware| aware.with_timezone(&chrono::Utc))
}

use pidash_db::integrations::git_models::git_code_review_link::GitCodeReviewLink;
use pidash_db::integrations::git_models::git_provider_account::GitProviderAccount;
use pidash_db::integrations::git_models::git_repository::GitRepository;
use pidash_db::integrations::github_models::github_pull_request_link::GithubPullRequestLink;
use pidash_services::integrations::code_reviews::{LegacyLinkUpdate, LinkWriteError};

/// Map a sqlx failure into the integrations store error.
fn git_store_error(context: &str, error: sqlx::Error) -> GitStoreError {
    GitStoreError::Db(format!("{context}: {error}"))
}

/// Map a link-write failure: integrity violations carry the payload
/// marker (Python's `IntegrityError` 400), anything else the generic
/// store error (500).
fn git_write_error(context: &str, error: sqlx::Error) -> GitStoreError {
    if is_integrity_error(&error) {
        GitStoreError::Db(format!("{REVIEW_PAYLOAD_INVALID}: {context}: {error}"))
    } else {
        git_store_error(context, error)
    }
}

/// The live `CodeReviewStore`: pool + request actor (CRUM). Audit
/// columns follow `BaseModel.save`. The delete methods soft-delete
/// (`SoftDeleteModel.delete`) with the `soft_delete_related_objects`
/// fan-out inline, preserving the update-then-enqueue order of
/// `mixins.py:71-78`.
struct ReviewStore {
    pool: sqlx::PgPool,
    actor: uuid::Uuid,
}

impl ReviewStore {
    fn new(pool: sqlx::PgPool, actor: uuid::Uuid) -> Self {
        Self { pool, actor }
    }
}

fn review_link_from_row(row: &sqlx::postgres::PgRow) -> Result<GitCodeReviewLink, GitStoreError> {
    use sqlx::Row as _;
    macro_rules! req {
        ($key:literal, $ty:ty) => {
            row.try_get::<$ty, _>($key)
                .map_err(|error| git_store_error("review link map", error))
        };
    }
    Ok(GitCodeReviewLink {
        id: req!("id", Uuid)?,
        created_at: req!("created_at", chrono::DateTime<chrono::Utc>)?,
        updated_at: req!("updated_at", chrono::DateTime<chrono::Utc>)?,
        created_by_id: req!("created_by_id", Option<Uuid>)?,
        updated_by_id: req!("updated_by_id", Option<Uuid>)?,
        deleted_at: req!("deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
        project_id: req!("project_id", Uuid)?,
        workspace_id: req!("workspace_id", Uuid)?,
        issue_id: req!("issue_id", Uuid)?,
        provider: req!("provider", String)?,
        host_url: req!("host_url", String)?,
        namespace: req!("namespace", String)?,
        repo_name: req!("repo_name", String)?,
        repo_external_id: req!("repo_external_id", String)?,
        external_id: req!("external_id", String)?,
        external_iid: req!("external_iid", String)?,
        url: req!("url", String)?,
        title: req!("title", String)?,
        state: req!("state", String)?,
        merged: req!("merged", bool)?,
        draft: req!("draft", bool)?,
        remote_updated_at: req!("remote_updated_at", Option<chrono::DateTime<chrono::Utc>>)?,
        metadata: req!("metadata", Value)?,
    })
}

fn legacy_link_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<GithubPullRequestLink, GitStoreError> {
    use sqlx::Row as _;
    macro_rules! req {
        ($key:literal, $ty:ty) => {
            row.try_get::<$ty, _>($key)
                .map_err(|error| git_store_error("legacy link map", error))
        };
    }
    Ok(GithubPullRequestLink {
        id: req!("id", Uuid)?,
        created_at: req!("created_at", chrono::DateTime<chrono::Utc>)?,
        updated_at: req!("updated_at", chrono::DateTime<chrono::Utc>)?,
        created_by_id: req!("created_by_id", Option<Uuid>)?,
        updated_by_id: req!("updated_by_id", Option<Uuid>)?,
        deleted_at: req!("deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
        project_id: req!("project_id", Uuid)?,
        workspace_id: req!("workspace_id", Uuid)?,
        issue_id: req!("issue_id", Uuid)?,
        repo_owner: req!("repo_owner", String)?,
        repo_name: req!("repo_name", String)?,
        pr_number: req!("pr_number", i32)?,
        url: req!("url", String)?,
        title: req!("title", String)?,
        state: req!("state", String)?,
        merged: req!("merged", bool)?,
        draft: req!("draft", bool)?,
        pr_updated_at: req!("pr_updated_at", Option<chrono::DateTime<chrono::Utc>>)?,
    })
}

fn provider_account_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<GitProviderAccount, GitStoreError> {
    use sqlx::Row as _;
    macro_rules! req {
        ($key:literal, $ty:ty) => {
            row.try_get::<$ty, _>($key)
                .map_err(|error| git_store_error("account map", error))
        };
    }
    Ok(GitProviderAccount {
        id: req!("id", Uuid)?,
        created_at: req!("created_at", chrono::DateTime<chrono::Utc>)?,
        updated_at: req!("updated_at", chrono::DateTime<chrono::Utc>)?,
        created_by_id: req!("created_by_id", Option<Uuid>)?,
        updated_by_id: req!("updated_by_id", Option<Uuid>)?,
        deleted_at: req!("deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
        workspace_id: req!("workspace_id", Uuid)?,
        provider: req!("provider", String)?,
        host_url: req!("host_url", String)?,
        auth_type: req!("auth_type", String)?,
        external_account_id: req!("external_account_id", String)?,
        external_account_login: req!("external_account_login", String)?,
        display_name: req!("display_name", String)?,
        capabilities: req!("capabilities", Value)?,
        credential_config: req!("credential_config", Value)?,
        workspace_integration_id: req!("workspace_integration_id", Option<Uuid>)?,
        status: req!("status", String)?,
        verified_at: req!("verified_at", Option<chrono::DateTime<chrono::Utc>>)?,
        last_check_error: req!("last_check_error", String)?,
        metadata: req!("metadata", Value)?,
    })
}

fn repository_from_row(row: &sqlx::postgres::PgRow) -> Result<GitRepository, GitStoreError> {
    use sqlx::Row as _;
    macro_rules! req {
        ($key:literal, $ty:ty) => {
            row.try_get::<$ty, _>($key)
                .map_err(|error| git_store_error("repository map", error))
        };
    }
    Ok(GitRepository {
        id: req!("id", Uuid)?,
        created_at: req!("created_at", chrono::DateTime<chrono::Utc>)?,
        updated_at: req!("updated_at", chrono::DateTime<chrono::Utc>)?,
        created_by_id: req!("created_by_id", Option<Uuid>)?,
        updated_by_id: req!("updated_by_id", Option<Uuid>)?,
        deleted_at: req!("deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
        provider: req!("provider", String)?,
        host_url: req!("host_url", String)?,
        external_id: req!("external_id", String)?,
        namespace: req!("namespace", String)?,
        name: req!("name", String)?,
        full_name: req!("full_name", String)?,
        web_url: req!("web_url", String)?,
        clone_url_http: req!("clone_url_http", String)?,
        clone_url_ssh: req!("clone_url_ssh", String)?,
        default_branch: req!("default_branch", String)?,
        is_private: req!("is_private", bool)?,
        metadata: req!("metadata", Value)?,
    })
}

impl CodeReviewStore for ReviewStore {
    async fn review_issue_exists(
        &self,
        issue_id: Uuid,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<bool, GitStoreError> {
        let row: Option<(i32,)> =
            sqlx::query_as(pidash_services::integrations::code_reviews::ISSUE_EXISTS_SQL)
                .bind(issue_id)
                .bind(project_id)
                .bind(workspace_slug)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("review issue exists", error))?;
        Ok(row.is_some())
    }

    async fn find_legacy_link(
        &self,
        repo_owner_lower: &str,
        repo_name_lower: &str,
        pr_number: i32,
    ) -> Result<Option<GithubPullRequestLink>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::code_reviews::LEGACY_LINK_FIND_SQL)
                .bind(repo_owner_lower)
                .bind(repo_name_lower)
                .bind(pr_number)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("legacy link find", error))?;
        row.map(|row| legacy_link_from_row(&row)).transpose()
    }

    async fn find_path_review_link(
        &self,
        provider: &str,
        host_url: &str,
        namespace_lower: &str,
        repo_name_lower: &str,
        external_iid: &str,
    ) -> Result<Option<GitCodeReviewLink>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::code_reviews::PATH_REVIEW_LINK_FIND_SQL)
                .bind(provider)
                .bind(host_url)
                .bind(namespace_lower)
                .bind(repo_name_lower)
                .bind(external_iid)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("path review link find", error))?;
        row.map(|row| review_link_from_row(&row)).transpose()
    }

    async fn find_repo_scope_link(
        &self,
        provider: &str,
        host_url: &str,
        external_iid: &str,
        repo_external_id: &str,
    ) -> Result<Option<GitCodeReviewLink>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::code_reviews::REPO_SCOPE_LINK_FIND_SQL)
                .bind(provider)
                .bind(host_url)
                .bind(external_iid)
                .bind(repo_external_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("repo scope link find", error))?;
        row.map(|row| review_link_from_row(&row)).transpose()
    }

    async fn find_path_empty_repo_link(
        &self,
        provider: &str,
        host_url: &str,
        external_iid: &str,
        namespace_lower: &str,
        repo_name_lower: &str,
    ) -> Result<Option<GitCodeReviewLink>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::code_reviews::PATH_EMPTY_REPO_LINK_FIND_SQL)
                .bind(provider)
                .bind(host_url)
                .bind(external_iid)
                .bind(namespace_lower)
                .bind(repo_name_lower)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("path empty repo link find", error))?;
        row.map(|row| review_link_from_row(&row)).transpose()
    }

    async fn find_binding_review(
        &self,
        workspace_slug: &str,
        project_id: Uuid,
        provider: &str,
        host_url: &str,
        namespace: &str,
        repo_name: &str,
    ) -> Result<Option<(GitProviderAccount, GitRepository)>, GitStoreError> {
        // `SELECT pa.*, r.*`: duplicate names resolve to the account
        // half by name, so the account maps by name and the repository
        // by position (past the 20 account columns, in table order —
        // pinned against `information_schema`).
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::code_reviews::BINDING_FOR_REVIEW_SQL)
                .bind(workspace_slug)
                .bind(project_id)
                .bind(provider)
                .bind(host_url)
                .bind(namespace)
                .bind(repo_name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("binding review find", error))?;
        let Some(row) = row else {
            return Ok(None);
        };
        let account = provider_account_from_row(&row)?;
        let repo = {
            use sqlx::Row as _;
            macro_rules! at {
                ($pos:literal, $ty:ty) => {
                    row.try_get::<$ty, _>(20 + $pos)
                        .map_err(|error| git_store_error("binding repo map", error))
                };
            }
            // `git_repositories` table order: created_at, updated_at,
            // deleted_at, id, provider, host_url, external_id,
            // namespace, name, full_name, web_url, clone_url_http,
            // clone_url_ssh, default_branch, is_private, metadata,
            // created_by_id, updated_by_id.
            GitRepository {
                created_at: at!(0, chrono::DateTime<chrono::Utc>)?,
                updated_at: at!(1, chrono::DateTime<chrono::Utc>)?,
                deleted_at: at!(2, Option<chrono::DateTime<chrono::Utc>>)?,
                id: at!(3, Uuid)?,
                provider: at!(4, String)?,
                host_url: at!(5, String)?,
                external_id: at!(6, String)?,
                namespace: at!(7, String)?,
                name: at!(8, String)?,
                full_name: at!(9, String)?,
                web_url: at!(10, String)?,
                clone_url_http: at!(11, String)?,
                clone_url_ssh: at!(12, String)?,
                default_branch: at!(13, String)?,
                is_private: at!(14, bool)?,
                metadata: at!(15, Value)?,
                created_by_id: at!(16, Option<Uuid>)?,
                updated_by_id: at!(17, Option<Uuid>)?,
            }
        };
        Ok(Some((account, repo)))
    }

    async fn find_review_issue_workspace(
        &self,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<Option<Uuid>, GitStoreError> {
        let row: Option<(Uuid,)> =
            sqlx::query_as(pidash_services::integrations::code_reviews::REVIEW_ISSUE_WORKSPACE_SQL)
                .bind(project_id)
                .bind(workspace_slug)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("review issue workspace", error))?;
        Ok(row.map(|row| row.0))
    }

    async fn find_review_repository(
        &self,
        provider: &str,
        host_url: &str,
        namespace: &str,
        repo_name: &str,
    ) -> Result<Option<GitRepository>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::code_reviews::REVIEW_REPOSITORY_FIND_SQL)
                .bind(provider)
                .bind(host_url)
                .bind(namespace)
                .bind(repo_name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("review repository find", error))?;
        row.map(|row| repository_from_row(&row)).transpose()
    }

    async fn find_review_project_workspace(
        &self,
        project_id: Uuid,
    ) -> Result<Option<Uuid>, GitStoreError> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            pidash_services::integrations::code_reviews::REVIEW_PROJECT_WORKSPACE_SQL,
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| git_store_error("review project workspace", error))?;
        Ok(row.map(|row| row.0))
    }

    async fn insert_code_review_link(
        &self,
        row: NewCodeReviewLink,
    ) -> Result<GitCodeReviewLink, LinkWriteError> {
        let outcome =
            sqlx::query(pidash_services::integrations::code_reviews::CODE_REVIEW_LINK_INSERT_SQL)
                .bind(row.id)
                .bind(row.created_at)
                .bind(row.updated_at)
                .bind(Some(self.actor))
                .bind(None::<Uuid>)
                .bind(row.project_id)
                .bind(row.workspace_id)
                .bind(row.issue_id)
                .bind(&row.provider)
                .bind(&row.host_url)
                .bind(&row.namespace)
                .bind(&row.repo_name)
                .bind(&row.repo_external_id)
                .bind(&row.external_id)
                .bind(&row.external_iid)
                .bind(&row.url)
                .bind(&row.title)
                .bind(&row.state)
                .bind(row.merged)
                .bind(row.draft)
                .bind(row.remote_updated_at)
                .bind(&row.metadata)
                .execute(&self.pool)
                .await;
        match outcome {
            Ok(_) => Ok(GitCodeReviewLink {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                created_by_id: Some(self.actor),
                updated_by_id: None,
                deleted_at: None,
                project_id: row.project_id,
                workspace_id: row.workspace_id,
                issue_id: row.issue_id,
                provider: row.provider,
                host_url: row.host_url,
                namespace: row.namespace,
                repo_name: row.repo_name,
                repo_external_id: row.repo_external_id,
                external_id: row.external_id,
                external_iid: row.external_iid,
                url: row.url,
                title: row.title,
                state: row.state,
                merged: row.merged,
                draft: row.draft,
                remote_updated_at: row.remote_updated_at,
                metadata: row.metadata,
            }),
            Err(error) if is_unique_violation(&error) => Err(LinkWriteError::Conflict),
            Err(error) => Err(LinkWriteError::Store(git_write_error(
                "code review link insert",
                error,
            ))),
        }
    }

    async fn insert_legacy_link(
        &self,
        row: NewLegacyLink,
    ) -> Result<GithubPullRequestLink, GitStoreError> {
        sqlx::query(pidash_services::integrations::code_reviews::LEGACY_LINK_INSERT_SQL)
            .bind(row.id)
            .bind(row.created_at)
            .bind(row.updated_at)
            .bind(Some(self.actor))
            .bind(None::<Uuid>)
            .bind(row.project_id)
            .bind(row.workspace_id)
            .bind(row.issue_id)
            .bind(&row.repo_owner)
            .bind(&row.repo_name)
            .bind(row.pr_number)
            .bind(&row.url)
            .bind(&row.title)
            .bind(&row.state)
            .bind(row.merged)
            .bind(row.draft)
            .bind(row.pr_updated_at)
            .execute(&self.pool)
            .await
            .map_err(|error| git_write_error("legacy link insert", error))?;
        Ok(GithubPullRequestLink {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            created_by_id: Some(self.actor),
            updated_by_id: None,
            deleted_at: None,
            project_id: row.project_id,
            workspace_id: row.workspace_id,
            issue_id: row.issue_id,
            repo_owner: row.repo_owner,
            repo_name: row.repo_name,
            pr_number: row.pr_number,
            url: row.url,
            title: row.title,
            state: row.state,
            merged: row.merged,
            draft: row.draft,
            pr_updated_at: row.pr_updated_at,
        })
    }

    async fn update_legacy_link(
        &self,
        id: Uuid,
        update: LegacyLinkUpdate,
    ) -> Result<GithubPullRequestLink, GitStoreError> {
        sqlx::query(pidash_services::integrations::code_reviews::LEGACY_LINK_UPDATE_SQL)
            .bind(id)
            .bind(&update.url)
            .bind(&update.title)
            .bind(&update.state)
            .bind(update.merged)
            .bind(update.draft)
            .bind(update.pr_updated_at)
            .bind(update.updated_at)
            .execute(&self.pool)
            .await
            .map_err(|error| git_write_error("legacy link update", error))?;
        // `existing.save(update_fields=[...])` mutates in place; reread
        // the row for the caller.
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(
            "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                    project_id, workspace_id, issue_id, repo_owner, repo_name, pr_number,
                    url, title, state, merged, draft, pr_updated_at
             FROM github_pull_request_links WHERE id = $1 LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| git_store_error("legacy link reread", error))?;
        row.map(|row| legacy_link_from_row(&row))
            .transpose()?
            .ok_or(GitStoreError::NotFound("legacy link"))
    }

    async fn delete_code_review_link(&self, id: Uuid) -> Result<(), GitStoreError> {
        // Instance `.delete()` (`mixins.py:71-78`): soft row, then the
        // fan-out — the port delegates the write mechanics here.
        let now = chrono::Utc::now();
        sqlx::query(
            "UPDATE git_code_review_links
             SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3",
        )
        .bind(now)
        .bind(self.actor)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|error| git_store_error("review link delete", error))?;
        enqueue_soft_delete(&self.pool, "gitcodereviewlink", &id).await;
        Ok(())
    }

    async fn delete_legacy_link(&self, id: Uuid) -> Result<(), GitStoreError> {
        let now = chrono::Utc::now();
        sqlx::query(
            "UPDATE github_pull_request_links
             SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3",
        )
        .bind(now)
        .bind(self.actor)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|error| git_store_error("legacy link delete", error))?;
        enqueue_soft_delete(&self.pool, "githubpullrequestlink", &id).await;
        Ok(())
    }
}

impl GitStore for ReviewStore {
    async fn list_provider_accounts(
        &self,
        workspace_id: Uuid,
        provider: &str,
        host_url: &str,
    ) -> Result<Vec<GitProviderAccount>, GitStoreError> {
        let rows: Vec<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::accounts::PROVIDER_ACCOUNT_LIST_SQL)
                .bind(workspace_id)
                .bind(provider)
                .bind(host_url)
                .fetch_all(&self.pool)
                .await
                .map_err(|error| git_store_error("account list", error))?;
        rows.iter().map(provider_account_from_row).collect()
    }

    async fn get_provider_account(
        &self,
        workspace_id: Uuid,
        provider: &str,
        host_url: &str,
        account_id: Uuid,
    ) -> Result<Option<GitProviderAccount>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::accounts::PROVIDER_ACCOUNT_GET_SQL)
                .bind(workspace_id)
                .bind(provider)
                .bind(host_url)
                .bind(account_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("account get", error))?;
        row.map(|row| provider_account_from_row(&row)).transpose()
    }

    async fn insert_provider_account(
        &self,
        row: NewProviderAccount,
    ) -> Result<GitProviderAccount, GitStoreError> {
        sqlx::query(pidash_services::integrations::accounts::PROVIDER_ACCOUNT_INSERT_SQL)
            .bind(row.id)
            .bind(row.created_at)
            .bind(row.updated_at)
            .bind(row.created_by_id)
            .bind(row.updated_by_id)
            .bind(row.workspace_id)
            .bind(&row.provider)
            .bind(&row.host_url)
            .bind(&row.auth_type)
            .bind(&row.external_account_id)
            .bind(&row.external_account_login)
            .bind(&row.display_name)
            .bind(&row.capabilities)
            .bind(&row.credential_config)
            .bind(&row.status)
            .bind(row.verified_at)
            .bind(&row.metadata)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("account insert", error))?;
        Ok(GitProviderAccount {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            created_by_id: row.created_by_id,
            updated_by_id: row.updated_by_id,
            deleted_at: None,
            workspace_id: row.workspace_id,
            provider: row.provider,
            host_url: row.host_url,
            auth_type: row.auth_type,
            external_account_id: row.external_account_id,
            external_account_login: row.external_account_login,
            display_name: row.display_name,
            capabilities: row.capabilities,
            credential_config: row.credential_config,
            workspace_integration_id: None,
            status: row.status,
            verified_at: Some(row.verified_at),
            last_check_error: String::new(),
            metadata: row.metadata,
        })
    }

    async fn find_repository_by_external(
        &self,
        provider: &str,
        host_url: &str,
        external_id: &str,
    ) -> Result<Option<GitRepository>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(
            pidash_services::integrations::repositories::REPOSITORY_FIND_BY_EXTERNAL_SQL,
        )
        .bind(provider)
        .bind(host_url)
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| git_store_error("repository find external", error))?;
        row.map(|row| repository_from_row(&row)).transpose()
    }

    async fn find_repository_by_full_name(
        &self,
        provider: &str,
        host_url: &str,
        full_name: &str,
    ) -> Result<Option<GitRepository>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(
            pidash_services::integrations::repositories::REPOSITORY_FIND_BY_FULL_NAME_SQL,
        )
        .bind(provider)
        .bind(host_url)
        .bind(full_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| git_store_error("repository find full name", error))?;
        row.map(|row| repository_from_row(&row)).transpose()
    }

    async fn update_repository(
        &self,
        id: Uuid,
        defaults: RepositoryDefaults,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<GitRepository, GitStoreError> {
        sqlx::query(pidash_services::integrations::repositories::REPOSITORY_UPDATE_SQL)
            .bind(id)
            .bind(&defaults.external_id)
            .bind(&defaults.namespace)
            .bind(&defaults.name)
            .bind(&defaults.full_name)
            .bind(&defaults.web_url)
            .bind(&defaults.clone_url_http)
            .bind(&defaults.clone_url_ssh)
            .bind(&defaults.default_branch)
            .bind(defaults.is_private)
            .bind(&defaults.metadata)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("repository update", error))?;
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(
            "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                    provider, host_url, external_id, namespace, name, full_name, web_url,
                    clone_url_http, clone_url_ssh, default_branch, is_private, metadata
             FROM git_repositories WHERE id = $1 LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| git_store_error("repository reread", error))?;
        row.map(|row| repository_from_row(&row))
            .transpose()?
            .ok_or(GitStoreError::NotFound("repository"))
    }

    async fn insert_repository(&self, row: NewRepository) -> Result<GitRepository, GitStoreError> {
        sqlx::query(pidash_services::integrations::repositories::REPOSITORY_INSERT_SQL)
            .bind(row.id)
            .bind(row.created_at)
            .bind(row.updated_at)
            .bind(&row.provider)
            .bind(&row.host_url)
            .bind(&row.defaults.external_id)
            .bind(&row.defaults.namespace)
            .bind(&row.defaults.name)
            .bind(&row.defaults.full_name)
            .bind(&row.defaults.web_url)
            .bind(&row.defaults.clone_url_http)
            .bind(&row.defaults.clone_url_ssh)
            .bind(&row.defaults.default_branch)
            .bind(row.defaults.is_private)
            .bind(&row.defaults.metadata)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("repository insert", error))?;
        Ok(GitRepository {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            provider: row.provider,
            host_url: row.host_url,
            external_id: row.defaults.external_id,
            namespace: row.defaults.namespace,
            name: row.defaults.name,
            full_name: row.defaults.full_name,
            web_url: row.defaults.web_url,
            clone_url_http: row.defaults.clone_url_http,
            clone_url_ssh: row.defaults.clone_url_ssh,
            default_branch: row.defaults.default_branch,
            is_private: row.defaults.is_private,
            metadata: row.defaults.metadata,
        })
    }

    async fn find_workspace_id(&self, slug: &str) -> Result<Option<Uuid>, GitStoreError> {
        let row: Option<(Uuid,)> =
            sqlx::query_as(pidash_services::integrations::repositories::WORKSPACE_ID_SQL)
                .bind(slug)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("workspace id", error))?;
        Ok(row.map(|row| row.0))
    }

    async fn find_project(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
    ) -> Result<Option<pidash_services::integrations::repositories::ProjectRef>, GitStoreError>
    {
        let row: Option<(Uuid, Uuid, String, String)> =
            sqlx::query_as(pidash_services::integrations::repositories::PROJECT_GET_SQL)
                .bind(project_id)
                .bind(workspace_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("project get", error))?;
        Ok(row.map(|(id, workspace_id, repo_url, base_branch)| {
            pidash_services::integrations::repositories::ProjectRef {
                id,
                workspace_id,
                repo_url,
                base_branch,
            }
        }))
    }

    async fn apply_bind(
        &self,
        plan: pidash_services::integrations::repositories::BindPlan,
    ) -> Result<
        pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding,
        GitStoreError,
    > {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| git_store_error("bind tx", error))?;
        sqlx::query(pidash_services::integrations::repositories::BINDING_DELETE_SQL)
            .bind(plan.project_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| git_store_error("bind delete", error))?;
        sqlx::query(pidash_services::integrations::repositories::GITHUB_SYNC_DELETE_SQL)
            .bind(plan.project_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| git_store_error("bind sync delete", error))?;
        let id = Uuid::new_v4();
        let mut metadata = Map::new();
        metadata.insert("raw_url".to_owned(), Value::String(plan.raw_url.clone()));
        sqlx::query(pidash_services::integrations::repositories::BINDING_INSERT_SQL)
            .bind(id)
            .bind(plan.now)
            .bind(plan.now)
            .bind(plan.actor_id)
            .bind(plan.actor_id)
            .bind(plan.project_id)
            .bind(plan.workspace_id)
            .bind(plan.repository_id)
            .bind(plan.provider_account_id)
            .bind(plan.actor_id)
            .bind(false)
            .bind(&plan.clone_auth_mode)
            .bind(Value::Object(metadata.clone()))
            .execute(&mut *tx)
            .await
            .map_err(|error| git_store_error("bind insert", error))?;
        // `project.save(update_fields=...)`: only the changed columns
        // plus `updated_at` (`services.py:341-348`).
        match (&plan.repo_url_update, &plan.base_branch_update) {
            (None, None) => {}
            (repo_url, base_branch) => {
                let mut sets: Vec<String> = Vec::new();
                if repo_url.is_some() {
                    sets.push("repo_url = $2".to_owned());
                }
                if base_branch.is_some() {
                    sets.push(format!("base_branch = ${}", sets.len() + 3));
                }
                sets.push(format!("updated_at = ${}", sets.len() + 3));
                let sql = format!("UPDATE projects SET {} WHERE id = $1", sets.join(", "));
                let mut query = sqlx::query(&sql).bind(plan.project_id);
                if let Some(url) = repo_url {
                    query = query.bind(url);
                }
                if let Some(branch) = base_branch {
                    query = query.bind(branch);
                }
                query = query.bind(plan.now);
                query
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| git_store_error("bind project update", error))?;
            }
        }
        tx.commit()
            .await
            .map_err(|error| git_store_error("bind commit", error))?;
        Ok(
            pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding {
                id,
                created_at: plan.now,
                updated_at: plan.now,
                created_by_id: plan.actor_id,
                updated_by_id: plan.actor_id,
                deleted_at: None,
                project_id: plan.project_id,
                workspace_id: plan.workspace_id,
                repository_id: plan.repository_id,
                provider_account_id: plan.provider_account_id,
                actor_id: plan.actor_id.ok_or(GitStoreError::NotFound("actor"))?,
                is_sync_enabled: false,
                clone_auth_mode: plan.clone_auth_mode,
                last_synced_at: None,
                last_sync_error: String::new(),
                metadata: Value::Object(metadata),
            },
        )
    }

    async fn get_binding(
        &self,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<Option<pidash_services::integrations::repositories::BindingView>, GitStoreError>
    {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::repositories::BINDING_GET_SQL)
                .bind(project_id)
                .bind(workspace_slug)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("binding get", error))?;
        let Some(row) = row else {
            return Ok(None);
        };
        use sqlx::Row as _;
        macro_rules! req {
            ($key:literal, $ty:ty) => {
                row.try_get::<$ty, _>($key)
                    .map_err(|error| git_store_error("binding map", error))
            };
        }
        let binding =
            pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding {
                id: req!("b_id", Uuid)?,
                created_at: req!("b_created_at", chrono::DateTime<chrono::Utc>)?,
                updated_at: req!("b_updated_at", chrono::DateTime<chrono::Utc>)?,
                created_by_id: req!("b_created_by_id", Option<Uuid>)?,
                updated_by_id: req!("b_updated_by_id", Option<Uuid>)?,
                deleted_at: req!("b_deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
                project_id: req!("b_project_id", Uuid)?,
                workspace_id: req!("b_workspace_id", Uuid)?,
                repository_id: req!("b_repository_id", Uuid)?,
                provider_account_id: req!("b_provider_account_id", Uuid)?,
                actor_id: req!("b_actor_id", Uuid)?,
                is_sync_enabled: req!("b_is_sync_enabled", bool)?,
                clone_auth_mode: req!("b_clone_auth_mode", String)?,
                last_synced_at: req!("b_last_synced_at", Option<chrono::DateTime<chrono::Utc>>)?,
                last_sync_error: req!("b_last_sync_error", String)?,
                metadata: req!("b_metadata", Value)?,
            };
        let repository = GitRepository {
            id: req!("r_id", Uuid)?,
            created_at: req!("r_created_at", chrono::DateTime<chrono::Utc>)?,
            updated_at: req!("r_updated_at", chrono::DateTime<chrono::Utc>)?,
            created_by_id: req!("r_created_by_id", Option<Uuid>)?,
            updated_by_id: req!("r_updated_by_id", Option<Uuid>)?,
            deleted_at: req!("r_deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
            provider: req!("r_provider", String)?,
            host_url: req!("r_host_url", String)?,
            external_id: req!("r_external_id", String)?,
            namespace: req!("r_namespace", String)?,
            name: req!("r_name", String)?,
            full_name: req!("r_full_name", String)?,
            web_url: req!("r_web_url", String)?,
            clone_url_http: req!("r_clone_url_http", String)?,
            clone_url_ssh: req!("r_clone_url_ssh", String)?,
            default_branch: req!("r_default_branch", String)?,
            is_private: req!("r_is_private", bool)?,
            metadata: req!("r_metadata", Value)?,
        };
        let account = GitProviderAccount {
            id: req!("a_id", Uuid)?,
            created_at: req!("a_created_at", chrono::DateTime<chrono::Utc>)?,
            updated_at: req!("a_updated_at", chrono::DateTime<chrono::Utc>)?,
            created_by_id: req!("a_created_by_id", Option<Uuid>)?,
            updated_by_id: req!("a_updated_by_id", Option<Uuid>)?,
            deleted_at: req!("a_deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
            workspace_id: req!("a_workspace_id", Uuid)?,
            provider: req!("a_provider", String)?,
            host_url: req!("a_host_url", String)?,
            auth_type: req!("a_auth_type", String)?,
            external_account_id: req!("a_external_account_id", String)?,
            external_account_login: req!("a_external_account_login", String)?,
            display_name: req!("a_display_name", String)?,
            capabilities: req!("a_capabilities", Value)?,
            credential_config: req!("a_credential_config", Value)?,
            workspace_integration_id: req!("a_workspace_integration_id", Option<Uuid>)?,
            status: req!("a_status", String)?,
            verified_at: req!("a_verified_at", Option<chrono::DateTime<chrono::Utc>>)?,
            last_check_error: req!("a_last_check_error", String)?,
            metadata: req!("a_metadata", Value)?,
        };
        Ok(Some(
            pidash_services::integrations::repositories::BindingView {
                binding,
                repository,
                account,
            },
        ))
    }

    async fn set_binding_sync(
        &self,
        binding_id: Uuid,
        enabled: bool,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<
        pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding,
        GitStoreError,
    > {
        sqlx::query(pidash_services::integrations::repositories::BINDING_SET_SYNC_SQL)
            .bind(binding_id)
            .bind(enabled)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("binding set sync", error))?;
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(
            "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                    project_id, workspace_id, repository_id, provider_account_id, actor_id,
                    is_sync_enabled, clone_auth_mode, last_synced_at, last_sync_error, metadata
             FROM git_repository_bindings WHERE id = $1 LIMIT 1",
        )
        .bind(binding_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| git_store_error("binding reread", error))?;
        let Some(row) = row else {
            return Err(GitStoreError::NotFound("binding"));
        };
        use sqlx::Row as _;
        macro_rules! req {
            ($key:literal, $ty:ty) => {
                row.try_get::<$ty, _>($key)
                    .map_err(|error| git_store_error("binding map", error))
            };
        }
        Ok(
            pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding {
                id: req!("id", Uuid)?,
                created_at: req!("created_at", chrono::DateTime<chrono::Utc>)?,
                updated_at: req!("updated_at", chrono::DateTime<chrono::Utc>)?,
                created_by_id: req!("created_by_id", Option<Uuid>)?,
                updated_by_id: req!("updated_by_id", Option<Uuid>)?,
                deleted_at: req!("deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
                project_id: req!("project_id", Uuid)?,
                workspace_id: req!("workspace_id", Uuid)?,
                repository_id: req!("repository_id", Uuid)?,
                provider_account_id: req!("provider_account_id", Uuid)?,
                actor_id: req!("actor_id", Uuid)?,
                is_sync_enabled: req!("is_sync_enabled", bool)?,
                clone_auth_mode: req!("clone_auth_mode", String)?,
                last_synced_at: req!("last_synced_at", Option<chrono::DateTime<chrono::Utc>>)?,
                last_sync_error: req!("last_sync_error", String)?,
                metadata: req!("metadata", Value)?,
            },
        )
    }

    async fn set_github_syncs_enabled(
        &self,
        project_id: Uuid,
        workspace_slug: &str,
        enabled: bool,
    ) -> Result<u64, GitStoreError> {
        let done =
            sqlx::query(pidash_services::integrations::repositories::GITHUB_SYNC_SET_ENABLED_SQL)
                .bind(enabled)
                .bind(project_id)
                .bind(workspace_slug)
                .execute(&self.pool)
                .await
                .map_err(|error| git_store_error("github syncs set enabled", error))?;
        Ok(done.rows_affected())
    }

    async fn delete_binding(&self, binding_id: Uuid) -> Result<(), GitStoreError> {
        sqlx::query(pidash_services::integrations::repositories::BINDING_DELETE_ONE_SQL)
            .bind(binding_id)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("binding delete", error))?;
        Ok(())
    }

    async fn delete_github_syncs_for_project(
        &self,
        project_id: Uuid,
    ) -> Result<u64, GitStoreError> {
        let done = sqlx::query(pidash_services::integrations::repositories::GITHUB_SYNC_DELETE_SQL)
            .bind(project_id)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("github syncs delete", error))?;
        Ok(done.rows_affected())
    }
}

// ---------------------------------------------------------------------------
// Task fan-out
// ---------------------------------------------------------------------------

/// Best-effort publish of a Celery message: without a queue table the
/// response still stands (the D-19/D-26 precedent).
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

/// `soft_delete_related_objects.delay("db", <model>, pk, using=None)`
/// (`db/mixins.py:77`): three positional args, `using` stays a null
/// kwarg. Fires on every instance `.delete()` in these views.
async fn enqueue_soft_delete(pool: &sqlx::PgPool, model: &str, pk: &uuid::Uuid) {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String(model.to_owned()),
            Value::String(pk.to_string()),
        ],
        kwargs,
    );
    enqueue_message(pool, message).await;
}

// ---------------------------------------------------------------------------
// Transports: production `GithubClient` + `GitLabTransport`
// ---------------------------------------------------------------------------

use pidash_db::app_integrations::queries_github::{build_app_jwt_claims, normalize_private_key};
use pidash_db::config::encryption::Keyring;
use pidash_services::integrations::adapters_github::{
    ClientAuth, GitHubAdapter, GithubClient, GithubError,
};
use pidash_services::integrations::adapters_gitlab::{
    GitLabAdapter, GitLabConfig, GitLabRequest, GitLabResponse, GitLabTransport,
};
use pidash_types::integrations::registry::UnknownProvider;
use pidash_types::integrations::GitProviderError;

/// Run an async future to completion from the sync transport seams: a
/// fresh current-thread runtime on a spawned thread. The seams
/// (`GithubClient`, `GitLabTransport`) are sync because Python's
/// `requests` is blocking; this keeps the async executor unblocked
/// without new Cargo features.
fn run_sync<F>(future: F) -> F::Output
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("pr-links transport runtime")
            .block_on(future)
    })
    .join()
    .expect("pr-links transport thread")
}

/// What the transports need beyond the request: pool (app-config
/// reads), cache (installation-token cache), instance secret (config
/// decrypt + PAT keyring source).
#[derive(Clone)]
struct TransportContext {
    pool: sqlx::PgPool,
    redis: Option<pidash_db::redis::RedisHandle>,
    secret: String,
}

/// Read app-config values through the instance-configurations
/// registry (`get_configuration_value`), like the GitHub App handler.
async fn app_config_values(
    ctx: &TransportContext,
    keys: &[&str],
) -> Option<HashMap<String, pidash_db::config::ConfigValue>> {
    let store = pidash_db::config::PgConfigStore::new(ctx.pool.clone());
    let registry = pidash_db::config::registry::global();
    let keyring = pidash_services::license::encryption::Keyring::from_secret(&ctx.secret);
    pidash_db::config::accessor::get_many_in(registry, &store, &keyring, keys)
        .await
        .ok()
}

/// `require_github_app_config()` (`github_app_auth.py:63-74`): app id +
/// slug + private key, each stripped, missing → the config error.
async fn github_app_config(
    ctx: &TransportContext,
) -> Result<(String, String, String), GithubError> {
    let values = app_config_values(
        ctx,
        &["GITHUB_APP_ID", "GITHUB_APP_SLUG", "GITHUB_APP_PRIVATE_KEY"],
    )
    .await
    .unwrap_or_default();
    let string = |key: &str| match values.get(key) {
        Some(pidash_db::config::ConfigValue::Str(value)) => value.clone(),
        Some(pidash_db::config::ConfigValue::Int(value)) => value.to_string(),
        Some(pidash_db::config::ConfigValue::Float(value)) => value.to_string(),
        Some(pidash_db::config::ConfigValue::Bool(true)) => "True".to_owned(),
        Some(pidash_db::config::ConfigValue::Bool(false)) => "False".to_owned(),
        _ => String::new(),
    };
    let app_id = string("GITHUB_APP_ID");
    let app_slug = string("GITHUB_APP_SLUG");
    let private_key = string("GITHUB_APP_PRIVATE_KEY");
    let config = (
        app_id.trim().to_owned(),
        app_slug.trim().to_owned(),
        normalize_private_key(Some(&private_key)),
    );
    let mut missing: Vec<&str> = Vec::new();
    if config.0.is_empty() {
        missing.push("app_id");
    }
    if config.1.is_empty() {
        missing.push("app_slug");
    }
    if config.2.is_empty() {
        missing.push("private_key");
    }
    if !missing.is_empty() {
        return Err(GithubError::Transport(format!(
            "GitHub App config missing: {}",
            missing.join(", ")
        )));
    }
    Ok(config)
}

/// `build_app_jwt` (`github_app_auth.py:77-89`): RS256 over
/// `iat=now-60, exp=now+540, iss=app_id`.
fn build_app_jwt_token(app_id: &str, private_key: &str) -> Result<String, GithubError> {
    let now = chrono::Utc::now().timestamp();
    let claims = build_app_jwt_claims(now, app_id);
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(private_key.as_bytes())
        .map_err(|error| GithubError::Transport(format!("GitHub App JWT: {error}")))?;
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.typ = Some("JWT".to_owned());
    #[derive(serde::Serialize)]
    struct Claims {
        iat: i64,
        exp: i64,
        iss: String,
    }
    jsonwebtoken::encode(
        &header,
        &Claims {
            iat: claims.iat,
            exp: claims.exp,
            iss: claims.iss,
        },
        &key,
    )
    .map_err(|error| GithubError::Transport(format!("GitHub App JWT: {error}")))
}

/// `installation_token` (`github_app_auth.py:174-193`): cached token or
/// a fresh mint. Without a Redis handle the mint runs through (the cache
/// only saves the POST; a mint failure still fails the connect either
/// way).
async fn installation_token(
    ctx: &TransportContext,
    installation_id: i64,
) -> Result<String, GithubError> {
    let cache_key = format!("github_app_installation_token:{installation_id}");
    if let Some(redis) = ctx.redis.as_ref() {
        match redis.get_string(&cache_key).await {
            Ok(Some(token)) if !token.is_empty() => return Ok(token),
            Ok(_) => {}
            Err(error) => {
                return Err(GithubError::Transport(format!(
                    "installation token cache: {error}"
                )))
            }
        }
    }
    let (app_id, _slug, private_key) = github_app_config(ctx).await?;
    let jwt = build_app_jwt_token(&app_id, &private_key)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| GithubError::Transport(error.to_string()))?;
    let response = client
        .post(format!(
            "https://api.github.com/app/installations/{installation_id}/access_tokens"
        ))
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "pi-dash-github-app")
        .send()
        .await
        .map_err(|error| GithubError::Transport(error.to_string()))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(GithubError::Transport(format!(
            "installation token mint: HTTP {}: {text}",
            status.as_u16()
        )));
    }
    let payload: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let token = payload
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if token.is_empty() {
        return Err(GithubError::Transport(
            "GitHub did not return an installation token".to_owned(),
        ));
    }
    if let Some(redis) = ctx.redis.as_ref() {
        // `ttl = max(60, expires-now-60)`, default 55 minutes
        // (`github_app_auth.py:184-188`).
        let mut ttl: u64 = 55 * 60;
        if let Some(expires) = payload.get("expires_at").and_then(Value::as_str) {
            let normalized = expires.replace('Z', "+00:00");
            if let Ok(when) = chrono::DateTime::parse_from_rfc3339(&normalized) {
                let seconds =
                    (when.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds() - 60;
                ttl = u64::try_from(seconds.max(60)).unwrap_or(60);
            }
        }
        if let Err(error) = redis.set_ex(&cache_key, &token, ttl).await {
            return Err(GithubError::Transport(format!(
                "installation token cache: {error}"
            )));
        }
    }
    Ok(token)
}

/// The production `GithubClient` (`github_client.py:38-220`): a bearer
/// credential plus the shared context for installation mints.
struct LiveGithubClient {
    token: String,
    ctx: TransportContext,
}

impl LiveGithubClient {
    fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Authorization", format!("Bearer {}", self.token)),
            ("Accept", "application/vnd.github+json".to_owned()),
            ("X-GitHub-Api-Version", "2022-11-28".to_owned()),
            ("User-Agent", "pi-dash-github-sync".to_owned()),
        ]
    }

    async fn request_json(
        &self,
        method: reqwest::Method,
        url: &str,
        json: Option<&Value>,
    ) -> Result<(Value, Option<String>), GithubError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|error| GithubError::Transport(error.to_string()))?;
        let mut request = client.request(method, url);
        for (name, value) in self.headers() {
            request = request.header(name, value);
        }
        if let Some(body) = json {
            let text = serde_json::to_string(body).unwrap_or_default();
            request = request
                .header("Content-Type", "application/json")
                .body(text);
        }
        let response = request
            .send()
            .await
            .map_err(|error| GithubError::Transport(error.to_string()))?;
        let status = response.status().as_u16();
        let link = response
            .headers()
            .get("link")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let text = response.text().await.unwrap_or_default();
        // `_request` (`github_client.py:59-71`): 401/403/404 map by
        // status with the response text; anything else failing raises.
        if status == 401 {
            return Err(GithubError::Auth(text));
        }
        if status == 403 {
            return Err(GithubError::Permission(text));
        }
        if status == 404 {
            return Err(GithubError::NotFound(text));
        }
        if !(200..300).contains(&status) {
            return Err(GithubError::Transport(format!("HTTP {status}: {text}")));
        }
        let payload: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok((payload, link))
    }

    /// `_next_url`: the `rel="next"` link, if any.
    fn next_url(link: Option<&str>) -> Option<String> {
        let link = link?;
        for part in link.split(',') {
            let part = part.trim_start();
            if !part.starts_with('<') {
                continue;
            }
            let Some(end) = part.find('>') else {
                continue;
            };
            let url = &part[1..end];
            let rest = part[end + 1..].trim_start();
            if let Some(rest) = rest.strip_prefix(';') {
                let rest = rest.trim_start();
                if rest.starts_with("rel=\"next\"") {
                    return Some(url.to_owned());
                }
            }
        }
        None
    }

    async fn paginate(&self, mut url: String) -> Result<Vec<Value>, GithubError> {
        let mut items = Vec::new();
        loop {
            let (payload, link) = self.request_json(reqwest::Method::GET, &url, None).await?;
            match payload {
                Value::Array(page) => items.extend(page),
                Value::Null => {}
                // A mapping payload iterates its keys, like Python's
                // `for item in response.json()` (the API never sends
                // one here, but the iteration is the ported behavior).
                Value::Object(map) => {
                    items.extend(map.keys().map(|key| Value::String(key.clone())));
                }
                _ => {}
            }
            match Self::next_url(link.as_deref()) {
                Some(next) => url = next,
                None => return Ok(items),
            }
        }
    }
}

impl GithubClient for LiveGithubClient {
    fn connect(auth: &ClientAuth) -> Result<Self, GithubError> {
        // The context rides a thread-local: the sync seam has no
        // parameter for it, and `attach` always runs on the request
        // task that installed it just before calling into the port.
        let ctx = TRANSPORT_CONTEXT.with(|slot| slot.borrow().clone());
        let ctx =
            ctx.ok_or_else(|| GithubError::Transport("github transport context".to_owned()))?;
        match auth {
            ClientAuth::Token(token) => {
                if token.is_empty() {
                    return Err(GithubError::Auth("empty token".to_owned()));
                }
                Ok(Self {
                    token: token.clone(),
                    ctx,
                })
            }
            ClientAuth::Installation(id) => {
                let id = *id;
                let minted = ctx.clone();
                let token = run_sync(async move { installation_token(&minted, id).await })?;
                Ok(Self { token, ctx })
            }
        }
    }

    fn get_authenticated_user(&self) -> Result<Value, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        run_sync(async move {
            this.request_json(reqwest::Method::GET, "https://api.github.com/user", None)
                .await
                .map(|(payload, _)| payload)
        })
    }

    fn list_user_repos(&self, page: i64) -> Result<(Vec<Value>, bool), GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        run_sync(async move {
            let url = format!(
                "https://api.github.com/user/repos?affiliation=owner%2Ccollaborator%2Corganization_member&per_page=100&sort=updated&page={page}"
            );
            let (payload, link) = this.request_json(reqwest::Method::GET, &url, None).await?;
            let repos = match payload {
                Value::Array(repos) => repos,
                _ => Vec::new(),
            };
            Ok((repos, Self::next_url(link.as_deref()).is_some()))
        })
    }

    fn get_repo(&self, owner: &str, name: &str) -> Result<Value, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        let url = format!("https://api.github.com/repos/{owner}/{name}");
        run_sync(async move {
            this.request_json(reqwest::Method::GET, &url, None)
                .await
                .map(|(payload, _)| payload)
        })
    }

    fn list_all_open_issues(&self, owner: &str, name: &str) -> Result<Vec<Value>, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        // `{"state": "open", "per_page": 100, "sort": "updated",
        // "direction": "desc"}` in insertion order (`:136-140`).
        let url = format!(
            "https://api.github.com/repos/{owner}/{name}/issues?state=open&per_page=100&sort=updated&direction=desc"
        );
        run_sync(async move { this.paginate(url).await })
    }

    fn list_issue_comments(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<Vec<Value>, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        // `{"per_page": 100, "sort": "created", "direction": "asc"}`
        // (`:149-153`).
        let url = format!(
            "https://api.github.com/repos/{owner}/{name}/issues/{number}/comments?per_page=100&sort=created&direction=asc"
        );
        run_sync(async move { this.paginate(url).await })
    }

    fn post_issue_comment(
        &self,
        owner: &str,
        name: &str,
        number: i64,
        body: &str,
    ) -> Result<Value, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        let url = format!("https://api.github.com/repos/{owner}/{name}/issues/{number}/comments");
        let payload = serde_json::json!({"body": body});
        run_sync(async move {
            this.request_json(reqwest::Method::POST, &url, Some(&payload))
                .await
                .map(|(payload, _)| payload)
        })
    }

    fn get_pull_request(&self, owner: &str, name: &str, number: i64) -> Result<Value, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        let url = format!("https://api.github.com/repos/{owner}/{name}/pulls/{number}");
        run_sync(async move {
            this.request_json(reqwest::Method::GET, &url, None)
                .await
                .map(|(payload, _)| payload)
        })
    }
}

std::thread_local! {
    /// The ambient transport context for the sync `GithubClient` seam.
    static TRANSPORT_CONTEXT: std::cell::RefCell<Option<TransportContext>> =
        const { std::cell::RefCell::new(None) };
}

/// Install the ambient transport context around one sync-seam call.
/// The guard restores the previous value on drop.
struct TransportGuard {
    previous: Option<TransportContext>,
}

impl TransportGuard {
    fn install(ctx: TransportContext) -> Self {
        let previous = TRANSPORT_CONTEXT.with(|slot| slot.borrow_mut().replace(ctx));
        Self { previous }
    }
}

impl Drop for TransportGuard {
    fn drop(&mut self) {
        TRANSPORT_CONTEXT.with(|slot| *slot.borrow_mut() = self.previous.take());
    }
}

/// The production `GitLabTransport`: `requests.request` over async
/// reqwest, redirects surfaced (never followed).
struct LiveGitlabTransport;

impl GitLabTransport for LiveGitlabTransport {
    fn request(&self, request: &GitLabRequest) -> Result<GitLabResponse, GitProviderError> {
        let request = GitLabRequest {
            method: request.method.clone(),
            url: request.url.clone(),
            headers: request.headers.clone(),
            query: request.query.clone(),
            form: request.form.clone(),
            timeout_secs: request.timeout_secs,
        };
        run_sync(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(request.timeout_secs))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|error| GitProviderError::General(error.to_string()))?;
            let method: reqwest::Method =
                request
                    .method
                    .parse()
                    .map_err(|error: http::method::InvalidMethod| {
                        GitProviderError::General(error.to_string())
                    })?;
            let mut call = client.request(method, &request.url);
            for (name, value) in &request.headers {
                call = call.header(name.as_str(), value.as_str());
            }
            if !request.query.is_empty() {
                call = call.query(&request.query);
            }
            if !request.form.is_empty() {
                // The pair vector serializes in order (repeats kept),
                // like `requests`' `data=` list of tuples.
                call = call.form(&request.form);
            }
            let response = call
                .send()
                .await
                .map_err(|error| GitProviderError::General(error.to_string()))?;
            let status = response.status().as_u16();
            let mut headers = Vec::new();
            for (name, value) in response.headers() {
                headers.push((name.to_string(), value.to_str().unwrap_or("").to_owned()));
            }
            let body = response.text().await.unwrap_or_default();
            Ok(GitLabResponse {
                status,
                headers,
                body,
            })
        })
    }
}

/// `decrypt_data` (`license/utils/encryption.py:34-44`) as the
/// context-free decryptor the GitLab seam takes: Fernet over
/// `SECRET_KEY`, failures (and empties) → `None` so the adapter falls
/// back to the raw token (`gitlab.py:191-198`).
fn gitlab_decrypt(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    let decrypted = Keyring::from_env().decrypt(value);
    if decrypted.is_empty() {
        None
    } else {
        Some(decrypted)
    }
}

/// The live [`AdapterSource`]: GitHub (keyring over the instance
/// secret) + GitLab (production transport, Django-effective config),
/// in registry order. The GitLab transport is owned by the caller and
/// borrowed here (the adapter borrows its transport).
struct V1Adapters<'t> {
    github: GitHubAdapter<LiveGithubClient>,
    gitlab: GitLabAdapter<'t, LiveGitlabTransport>,
}

impl<'t> V1Adapters<'t> {
    fn new(
        transport: &'t LiveGitlabTransport,
        keyring: Keyring,
        allowed_hosts: Vec<String>,
    ) -> Self {
        // `getattr(settings, "GITLAB_HOST", "")`: Django never defines
        // the setting, so the effective host is always `""`.
        let config = GitLabConfig {
            gitlab_host: String::new(),
            allowed_hosts,
        };
        Self {
            github: GitHubAdapter::new(keyring),
            gitlab: GitLabAdapter::with_decrypt(config, transport, gitlab_decrypt),
        }
    }
}

impl AdapterSource for V1Adapters<'_> {
    fn adapter_for(&self, provider: &str) -> Result<&dyn ReviewAdapter, UnknownProvider> {
        let key = pidash_types::integrations::registry::resolve_adapter_key(provider)?;
        // `PROVIDERS` is exactly github + gitlab, so the key is one of
        // the two (Python's `KeyError` arm is the `Err` above).
        match key {
            "github" => Ok(&self.github as &dyn ReviewAdapter),
            "gitlab" => Ok(&self.gitlab as &dyn ReviewAdapter),
            _ => unreachable!("adapter keys are github|gitlab"),
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `RequestBodySizeLimitMiddleware` answers 413 before the view runs.
const MAX_BODY: usize = 5_242_880;

/// Re-read one PR link by id for the create response (the serializer
/// reads the saved instance, whose audit columns the store stamped).
const PR_OWNED_SQL: &str = r#"SELECT id, issue_id, repo_owner, repo_name, pr_number, url, title, state,
              merged, draft, pr_updated_at, created_at, updated_at, created_by_id
       FROM github_pull_request_links WHERE id = $1 LIMIT 1"#;

/// Re-read one review link by id for the create response.
const REVIEW_OWNED_SQL: &str = r#"SELECT id, issue_id, provider, host_url, namespace, repo_name, repo_external_id,
              external_id, external_iid, url, title, state, merged, draft,
              remote_updated_at, metadata, created_at, updated_at, created_by_id
       FROM git_code_review_links WHERE id = $1 LIMIT 1"#;

/// `GET .../work-items/<issue_id>/github/pull-requests/`
/// (`github_pr.py:51-56`): the member-scoped queryset, `-created_at`,
/// paginated through the shared envelope.
pub async fn pr_list(
    State(state): State<AppState>,
    axum::extract::Path(params): axum::extract::Path<HashMap<String, String>>,
    Query(query): Query<QueryMap>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    // The `<uuid:issue_id>` converter rejects anything else before the
    // view runs; proxy for Django's exact bytes.
    if path_uuid(&issue_raw).is_err() {
        return crate::edge::proxy(State(state), req).await;
    }
    match pr_list_inner(
        &state,
        req.headers(),
        &slug,
        &project_raw,
        &issue_raw,
        &query,
    )
    .await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `POST .../work-items/<issue_id>/github/pull-requests/`
/// (`github_pr.py:58-76`).
pub async fn pr_create(
    State(state): State<AppState>,
    axum::extract::Path(params): axum::extract::Path<HashMap<String, String>>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    if path_uuid(&issue_raw).is_err() {
        return crate::edge::proxy(State(state), req).await;
    }
    let (parts, body) = req.into_parts();
    let raw = match axum::body::to_bytes(body, MAX_BODY).await {
        Ok(raw) => raw,
        Err(_) => return Denial::RequestTooLarge.into_response(),
    };
    match pr_create_inner(
        &state,
        &parts.headers,
        &slug,
        &project_raw,
        &issue_raw,
        &raw,
    )
    .await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE .../github/pull-requests/<pk>/` (`github_pr.py:97-100`).
pub async fn pr_destroy(
    State(state): State<AppState>,
    axum::extract::Path(params): axum::extract::Path<HashMap<String, String>>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    let pk_raw = params.get("pk").cloned().unwrap_or_default();
    if path_uuid(&issue_raw).is_err() || path_uuid(&pk_raw).is_err() {
        return crate::edge::proxy(State(state), req).await;
    }
    match pr_destroy_inner(
        &state,
        req.headers(),
        &slug,
        &project_raw,
        &issue_raw,
        &pk_raw,
    )
    .await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `GET .../work-items/<issue_id>/code-reviews/`
/// (`git_code_review.py:46-51`).
pub async fn review_list(
    State(state): State<AppState>,
    axum::extract::Path(params): axum::extract::Path<HashMap<String, String>>,
    Query(query): Query<QueryMap>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    if path_uuid(&issue_raw).is_err() {
        return crate::edge::proxy(State(state), req).await;
    }
    match review_list_inner(
        &state,
        req.headers(),
        &slug,
        &project_raw,
        &issue_raw,
        &query,
    )
    .await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `POST .../work-items/<issue_id>/code-reviews/`
/// (`git_code_review.py:53-76`).
pub async fn review_create(
    State(state): State<AppState>,
    axum::extract::Path(params): axum::extract::Path<HashMap<String, String>>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    if path_uuid(&issue_raw).is_err() {
        return crate::edge::proxy(State(state), req).await;
    }
    let (parts, body) = req.into_parts();
    let raw = match axum::body::to_bytes(body, MAX_BODY).await {
        Ok(raw) => raw,
        Err(_) => return Denial::RequestTooLarge.into_response(),
    };
    match review_create_inner(
        &state,
        &parts.headers,
        &slug,
        &project_raw,
        &issue_raw,
        &raw,
    )
    .await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE .../code-reviews/<pk>/` (`git_code_review.py:97-100`).
pub async fn review_destroy(
    State(state): State<AppState>,
    axum::extract::Path(params): axum::extract::Path<HashMap<String, String>>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    let pk_raw = params.get("pk").cloned().unwrap_or_default();
    if path_uuid(&issue_raw).is_err() || path_uuid(&pk_raw).is_err() {
        return crate::edge::proxy(State(state), req).await;
    }
    match review_destroy_inner(
        &state,
        req.headers(),
        &slug,
        &project_raw,
        &issue_raw,
        &pk_raw,
    )
    .await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// Inputs for [`paginated_list`]: the list scope plus the statement and
/// the row renderer.
struct ListScope<'a> {
    pool: &'a sqlx::PgPool,
    slug: &'a str,
    project_id: &'a uuid::Uuid,
    issue_id: &'a uuid::Uuid,
    actor_id: &'a uuid::Uuid,
    tz: &'a Tz,
    query: &'a QueryMap,
    sql: &'a str,
    render: fn(&sqlx::postgres::PgRow, &Tz) -> Result<Value, Denial>,
}

/// Shared paginated-list core (`BasePaginator.paginate` with the default
/// `OffsetPaginator`, `utils/paginator.py:654-760`): per_page/cursor
/// parsing, the offset window over the evaluated rows, the 12-key
/// envelope. `render` shapes one row.
async fn paginated_list(scope: ListScope<'_>) -> Result<Response, Denial> {
    let per_page = crate::paginator::parse_per_page(
        query_last(scope.query, "per_page").as_deref(),
        1000,
        1000,
    )
    .map_err(page_denial)?;
    let cursor_raw = query_last(scope.query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = crate::paginator::Cursor::from_string(&cursor_raw).map_err(page_denial)?;
    let window = crate::paginator::offset_window(
        per_page,
        cursor.offset,
        cursor.value,
        cursor.is_prev,
        None,
    )
    .map_err(page_denial)?;
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(scope.sql)
        .bind(scope.slug)
        .bind(scope.project_id)
        .bind(scope.issue_id)
        .bind(scope.actor_id)
        .fetch_all(scope.pool)
        .await
        .map_err(|error| db_error(error, "pr-links list"))?;
    let total_count = rows.len() as i64;
    // `queryset[offset:stop]` over the evaluated rows.
    let start = (window.offset as usize).min(rows.len());
    let stop = (window.stop as usize).min(rows.len());
    let window_rows = &rows[start..stop];
    // Backwards walk with a mismatched cursor value reads nothing
    // (`results[-(limit+1):]` on the lazy queryset raises into the 500).
    if !cursor.value.equals_limit(per_page) && cursor.is_prev {
        return Err(Denial::ServerError);
    }
    let has_more = window_rows.len() as i64 > per_page;
    // `results[:limit]` over the evaluated window (negative limits already
    // errored in `offset_window`).
    let trim = usize::try_from(per_page)
        .unwrap_or(usize::MAX)
        .min(window_rows.len());
    let page_rows = &window_rows[..trim];
    let mut rendered: Vec<Value> = Vec::with_capacity(page_rows.len());
    for row in page_rows {
        rendered.push((scope.render)(row, scope.tz)?);
    }
    let next = crate::paginator::next_cursor(per_page, cursor.offset, has_more);
    let prev = crate::paginator::prev_cursor(per_page, cursor.offset);
    Ok(envelope(
        total_count,
        per_page,
        &next,
        &prev,
        Value::Array(rendered),
    )?)
}

pub async fn pr_list_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    issue_raw: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    use super::perms::V1WorkItemsRoute as R;
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let issue_id = path_uuid(issue_raw).map_err(|_| Denial::ServerError)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_entity_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        R::GithubPrList,
        "GET",
    )
    .await?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    let timezone = activate_timezone(pre.actor.timezone.as_deref())?;
    paginated_list(ListScope {
        pool: &pre.pool,
        slug,
        project_id: &project_id,
        issue_id: &issue_id,
        actor_id: &pre.actor.id,
        tz: &timezone,
        query,
        sql: PR_LIST_SQL,
        render: render_pr_row,
    })
    .await
}

pub async fn review_list_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    issue_raw: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    use super::perms::V1WorkItemsRoute as R;
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let issue_id = path_uuid(issue_raw).map_err(|_| Denial::ServerError)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_entity_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        R::CodeReviewList,
        "GET",
    )
    .await?;
    let timezone = activate_timezone(pre.actor.timezone.as_deref())?;
    paginated_list(ListScope {
        pool: &pre.pool,
        slug,
        project_id: &project_id,
        issue_id: &issue_id,
        actor_id: &pre.actor.id,
        tz: &timezone,
        query,
        sql: REVIEW_LIST_SQL,
        render: render_review_row,
    })
    .await
}

/// `POST github/pull-requests/` (`github_pr.py:58-76`): attach runs pinned
/// to this thread (`block_in_place`) so the sync `GithubClient` seam sees
/// the installed transport context.
pub async fn pr_create_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    issue_raw: &str,
    raw_body: &[u8],
) -> Result<Response, Denial> {
    use super::perms::V1WorkItemsRoute as R;
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let issue_id = path_uuid(issue_raw).map_err(|_| Denial::ServerError)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_entity_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        R::GithubPrList,
        "POST",
    )
    .await?;
    let timezone = activate_timezone(pre.actor.timezone.as_deref())?;
    let body = parse_json_body(raw_body)?;
    let raw_url = extract_raw_url(&body)?;
    let ctx = TransportContext {
        pool: pre.pool.clone(),
        redis: state.redis().cloned(),
        secret: state.settings().secret_key.clone(),
    };
    let _guard = TransportGuard::install(ctx);
    let store = PrStore::new(pre.pool.clone());
    let now = chrono::Utc::now();
    let outcome = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(
            pidash_services::app_issues::pr_links::attach_pull_request::<PrStore, LiveGithubClient>(
                &store,
                project_id,
                issue_id,
                slug,
                &raw_url,
                &now,
                Some(pre.actor.id),
            ),
        )
    });
    drop(_guard);
    let (link, created) = match outcome {
        Ok(attached) => (attached.link, attached.created),
        Err(PrAttachError::InvalidUrl(_)) => {
            return Err(Denial::BadError(
                "A valid github.com pull request URL is required.".to_owned(),
            ))
        }
        Err(PrAttachError::IssueNotFound(_)) => return Err(Denial::WorkItemNotFound),
        Err(PrAttachError::AlreadyLinked(conflict)) => {
            return Err(Denial::Conflict(format!(
                "This pull request is already linked to issue {}.",
                conflict.issue_id
            )))
        }
        Err(PrAttachError::Store(PrStoreError(message))) => {
            if message == PROJECT_WORKSPACE_MISS {
                return Err(Denial::NotFound);
            }
            if message.starts_with(PR_PAYLOAD_INVALID) || message.contains(PR_RACE_MISS) {
                return Err(Denial::BadError("The payload is not valid".to_owned()));
            }
            return Err(Denial::ServerError);
        }
    };
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(PR_OWNED_SQL)
        .bind(link.id)
        .fetch_optional(&pre.pool)
        .await
        .map_err(|error| db_error(error, "pr create reread"))?;
    let Some(row) = row else {
        return Err(Denial::ServerError);
    };
    let shape = render_pr_row(&row, &timezone)?;
    let text = serde_json::to_string(&shape).map_err(|_| Denial::ServerError)?;
    if created {
        Ok(json_created(text))
    } else {
        Ok(json_ok(text))
    }
}

/// `DELETE github/pull-requests/<pk>/` (`github_pr.py:97-100`): scoped
/// `.get(pk)` (404 `{"error": ...}`), detach through the port (mirror
/// culled only when it hangs off the same issue), 204. The port's
/// `DetachOutcome.enqueues` fan out in Python order after the soft
/// deletes.
pub async fn pr_destroy_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    issue_raw: &str,
    pk_raw: &str,
) -> Result<Response, Denial> {
    use super::perms::V1WorkItemsRoute as R;
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let issue_id = path_uuid(issue_raw).map_err(|_| Denial::ServerError)?;
    let pk = path_uuid(pk_raw).map_err(|_| Denial::ServerError)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_entity_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        R::GithubPrDetail,
        "DELETE",
    )
    .await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(PR_DETAIL_SQL)
        .bind(slug)
        .bind(project_id)
        .bind(issue_id)
        .bind(pre.actor.id)
        .bind(pk)
        .fetch_optional(&pre.pool)
        .await
        .map_err(|error| db_error(error, "pr detail fetch"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let store = PrStore::new(pre.pool.clone());
    let link = pr_link_row_from(&row).map_err(|_| Denial::ServerError)?;
    let now = chrono::Utc::now();
    let outcome = match pidash_services::app_issues::pr_links::detach_pull_request_link(
        &store,
        &link,
        &now,
        Some(pre.actor.id),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(_) => return Err(Denial::ServerError),
    };
    // The recorded fan-out, verbatim and in Python order.
    for task in outcome.enqueues {
        let message =
            pidash_jobs::celery::CeleryTaskMessage::new(task.task, task.args, task.kwargs);
        enqueue_message(&pre.pool, message).await;
    }
    Ok(no_content())
}

/// `POST code-reviews/` (`git_code_review.py:53-76`): same body rules as
/// the PR create; attach through the D-05 port with the live adapters,
/// pinned to this thread for the sync seams.
pub async fn review_create_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    issue_raw: &str,
    raw_body: &[u8],
) -> Result<Response, Denial> {
    use super::perms::V1WorkItemsRoute as R;
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let issue_id = path_uuid(issue_raw).map_err(|_| Denial::ServerError)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_entity_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        R::CodeReviewList,
        "POST",
    )
    .await?;
    let timezone = activate_timezone(pre.actor.timezone.as_deref())?;
    let body = parse_json_body(raw_body)?;
    let raw_url = extract_raw_url(&body)?;
    let ctx = TransportContext {
        pool: pre.pool.clone(),
        redis: state.redis().cloned(),
        secret: state.settings().secret_key.clone(),
    };
    let keyring = Keyring::from_secret(&state.settings().secret_key);
    let allowed_hosts = state.settings().gitlab_allowed_hosts.clone();
    let _guard = TransportGuard::install(ctx);
    let store = ReviewStore::new(pre.pool.clone(), pre.actor.id);
    let transport = LiveGitlabTransport;
    let outcome = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let adapters = V1Adapters::new(&transport, keyring, allowed_hosts);
            let parsers: Vec<&dyn ReviewParser> = vec![&adapters.github, &adapters.gitlab];
            pidash_services::integrations::code_reviews::attach_code_review(
                &store,
                &parsers,
                &adapters,
                &AttachRequest {
                    project_id,
                    issue_id,
                    workspace_slug: slug.to_owned(),
                    raw_url: raw_url.clone(),
                },
                chrono::Utc::now(),
            )
            .await
        })
    });
    drop(_guard);
    let (link, created) = match outcome {
        Ok(attached) => attached,
        Err(CodeReviewError::InvalidUrl) => {
            return Err(Denial::BadError(
                "A supported GitHub pull request or GitLab merge request URL is required."
                    .to_owned(),
            ))
        }
        Err(CodeReviewError::IssueNotFound) => return Err(Denial::WorkItemNotFound),
        Err(CodeReviewError::AlreadyLinked { issue_id }) => {
            return Err(Denial::Conflict(
                pidash_services::integrations::code_reviews::already_linked_message(&issue_id),
            ))
        }
        Err(CodeReviewError::ProviderNotFound(_)) => {
            return Err(Denial::NotFoundError("Code review not found.".to_owned()))
        }
        Err(CodeReviewError::BadPrNumber(_)) => return Err(Denial::ServerError),
        Err(CodeReviewError::UnknownProvider(_)) => return Err(Denial::ServerError),
        Err(CodeReviewError::Store(GitStoreError::NotFound(_))) => return Err(Denial::NotFound),
        Err(CodeReviewError::Store(GitStoreError::Db(message))) => {
            if message.starts_with(REVIEW_PAYLOAD_INVALID) || message.contains(REVIEW_RACE_MISS) {
                return Err(Denial::BadError("The payload is not valid".to_owned()));
            }
            return Err(Denial::ServerError);
        }
    };
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(REVIEW_OWNED_SQL)
        .bind(link.id)
        .fetch_optional(&pre.pool)
        .await
        .map_err(|error| db_error(error, "review create reread"))?;
    let Some(row) = row else {
        return Err(Denial::ServerError);
    };
    let shape = render_review_row(&row, &timezone)?;
    let text = serde_json::to_string(&shape).map_err(|_| Denial::ServerError)?;
    if created {
        Ok(json_created(text))
    } else {
        Ok(json_ok(text))
    }
}

/// `DELETE code-reviews/<pk>/` (`git_code_review.py:97-100`): scoped
/// `.get(pk)`, detach through the D-05 port (legacy culled only when it
/// hangs off the same issue; the store soft-deletes + fans out), 204.
pub async fn review_destroy_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    issue_raw: &str,
    pk_raw: &str,
) -> Result<Response, Denial> {
    use super::perms::V1WorkItemsRoute as R;
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let issue_id = path_uuid(issue_raw).map_err(|_| Denial::ServerError)?;
    let pk = path_uuid(pk_raw).map_err(|_| Denial::ServerError)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_entity_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        R::CodeReviewDetail,
        "DELETE",
    )
    .await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(REVIEW_DETAIL_SQL)
        .bind(slug)
        .bind(project_id)
        .bind(issue_id)
        .bind(pre.actor.id)
        .bind(pk)
        .fetch_optional(&pre.pool)
        .await
        .map_err(|error| db_error(error, "review detail fetch"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let store = ReviewStore::new(pre.pool.clone(), pre.actor.id);
    let link = review_link_from_row(&row).map_err(|_| Denial::ServerError)?;
    if pidash_services::integrations::code_reviews::detach_code_review_link(&store, &link)
        .await
        .is_err()
    {
        return Err(Denial::ServerError);
    }
    Ok(no_content())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denied_status(denial: &Denial) -> (StatusCode, String) {
        denial.status_and_body()
    }

    fn is_server_error(denial: Denial) -> bool {
        matches!(
            denial.status_and_body().0,
            StatusCode::INTERNAL_SERVER_ERROR
        )
    }

    // -- denial bytes --------------------------------------------------------

    #[test]
    fn denial_bodies_are_byte_exact() {
        assert_eq!(
            denied_status(&Denial::Unauthorized),
            (
                StatusCode::UNAUTHORIZED,
                r#"{"detail":"Authentication credentials were not provided."}"#.to_owned()
            )
        );
        assert_eq!(
            denied_status(&Denial::InvalidToken),
            (
                StatusCode::FORBIDDEN,
                r#"{"detail":"Given API token is not valid"}"#.to_owned()
            )
        );
        assert_eq!(
            denied_status(&Denial::Forbidden),
            (
                StatusCode::FORBIDDEN,
                r#"{"detail":"You do not have permission to perform this action."}"#.to_owned()
            )
        );
        assert_eq!(
            denied_status(&Denial::NotFound),
            (
                StatusCode::NOT_FOUND,
                r#"{"error":"The requested resource does not exist."}"#.to_owned()
            )
        );
        assert_eq!(
            denied_status(&Denial::ProjectNotFound),
            (
                StatusCode::NOT_FOUND,
                r#"{"detail":"Project not found"}"#.to_owned()
            )
        );
        assert_eq!(
            denied_status(&Denial::WorkItemNotFound),
            (
                StatusCode::NOT_FOUND,
                r#"{"error":"Work item not found."}"#.to_owned()
            )
        );
        assert_eq!(
            denied_status(&Denial::Conflict(
                "This pull request is already linked.".to_owned()
            )),
            (
                StatusCode::CONFLICT,
                r#"{"error":"This pull request is already linked."}"#.to_owned()
            )
        );
        assert_eq!(
            denied_status(&Denial::RequestTooLarge),
            (
                StatusCode::PAYLOAD_TOO_LARGE,
                r#"{"error": "REQUEST_BODY_TOO_LARGE", "detail": "The size of the request body exceeds the maximum allowed size."}"#.to_owned()
            )
        );
        assert!(is_server_error(Denial::ServerError));
    }

    // -- path uuids ----------------------------------------------------------

    #[test]
    fn path_uuid_accepts_strict_lowercase() {
        let raw = "12345678-1234-1234-1234-1234567890ab";
        assert_eq!(path_uuid(raw).expect("valid").to_string(), raw);
    }

    #[test]
    fn path_uuid_rejects_anything_else() {
        for raw in [
            "12345678-1234-1234-1234-1234567890AB",
            "123456781234123412341234567890ab",
            "{12345678-1234-1234-1234-1234567890ab}",
            "urn:uuid:12345678-1234-1234-1234-1234567890ab",
            "not-a-uuid",
            "",
        ] {
            assert!(path_uuid(raw).is_err(), "{raw}");
        }
    }

    // -- bodies --------------------------------------------------------------

    #[test]
    fn json_body_empty_is_empty_map() {
        assert!(parse_json_body(b"").expect("empty").is_empty());
    }

    #[test]
    fn json_body_objects_pass_through() {
        let map = parse_json_body(br#"{"url": "https://github.com/a/b/pull/1"}"#).expect("object");
        assert_eq!(
            map.get("url").and_then(Value::as_str),
            Some("https://github.com/a/b/pull/1")
        );
    }

    #[test]
    fn json_body_non_objects_are_server_errors() {
        // No serializer runs on these views: `.get` on a non-dict is the
        // `AttributeError` 500, never `non_field_errors`.
        for text in ["[1, 2]", "null", "1", "\"x\"", "true"] {
            assert!(
                is_server_error(parse_json_body(text.as_bytes()).expect_err(text)),
                "{text}"
            );
        }
    }

    #[test]
    fn json_body_malformed_is_parse_detail() {
        let denial = parse_json_body(b"{oops").expect_err("malformed");
        let (status, body) = denied_status(&denial);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.starts_with(r#"{"detail":"JSON parse error - "#),
            "{body}"
        );
    }

    #[test]
    fn raw_url_extraction_collapses_falsy_and_rejects_truthy_non_strings() {
        let url_of = |value: Value| {
            let mut map = Map::new();
            map.insert("url".to_owned(), value);
            extract_raw_url(&map)
        };
        // Missing / null / falsy → "" (the invalid-URL 400 downstream).
        assert_eq!(
            extract_raw_url(&Map::new()).expect("missing"),
            String::new()
        );
        for value in [
            Value::Null,
            Value::Bool(false),
            Value::Number(0.into()),
            Value::Number(serde_json::Number::from_f64(0.0).expect("zero")),
            Value::String(String::new()),
            Value::Array(vec![]),
            Value::Object(Map::new()),
        ] {
            assert_eq!(
                url_of(value.clone()).expect("falsy"),
                String::new(),
                "{value}"
            );
        }
        // Strings pass through (the parser strips).
        assert_eq!(
            url_of(Value::String(
                "  https://github.com/a/b/pull/1  ".to_owned()
            ))
            .expect("string"),
            "  https://github.com/a/b/pull/1  "
        );
        // Truthy non-strings 500 on `.strip()`.
        for value in [
            Value::Bool(true),
            Value::Number(42.into()),
            Value::Array(vec![Value::Null]),
            Value::Object({
                let mut map = Map::new();
                map.insert("a".to_owned(), Value::Null);
                map
            }),
        ] {
            assert!(
                is_server_error(url_of(value.clone()).expect_err("truthy non-string")),
                "{value}"
            );
        }
    }

    // -- timezone ------------------------------------------------------------

    #[test]
    fn timezone_defaults_to_utc_and_rejects_unknown_with_key_error_body() {
        assert_eq!(
            activate_timezone(None).expect("utc").to_string(),
            "UTC".parse::<Tz>().expect("utc").to_string()
        );
        let denial = activate_timezone(Some("Nope/Nowhere")).expect_err("unknown");
        assert_eq!(
            denied_status(&denial),
            (
                StatusCode::BAD_REQUEST,
                r#"{"error":"The required key does not exist."}"#.to_owned()
            )
        );
    }

    #[test]
    fn activate_timezone_empty_zone_500s() {
        // `ZoneInfo('')` raises `ValueError` (not `KeyError`), so an
        // empty stored zone is the generic 500 while an unknown zone is
        // the `KeyError`-branch 400 (PIDASHCONV-747, live-probed).
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
}
