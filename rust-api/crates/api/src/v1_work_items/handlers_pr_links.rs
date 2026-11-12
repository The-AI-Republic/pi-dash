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
//! SeaORM store implementations the kernels are seamed over — the same
//! split the D-26 app-surface handlers use, mirroring their store and
//! transport implementations (which are private to that surface). The
//! list / detail querysets are SeaORM selects over the link entities
//! (`pr_link_scope`, `review_link_scope`); the store runs the `*::orm`
//! twins beside the `pidash_services::integrations` SQL constants.
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
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_auth::scope::TenantScope;

use crate::domain_hooks::AppTaskEnqueueHook;
use crate::issue_transition::AppTransitionHook;
use crate::middleware::body_limit::read_request_bytes;
use crate::state::AppState;
use crate::v1_projects::handlers_project::{
    envelope, preamble, project_base_facts, query_last, rewrite_project_id, Preamble, QueryMap,
};
use pidash_compat::db::is_unique_violation;
use pidash_compat::response::{json_created, json_ok, no_content};
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::domain::{WriteActor, WriteCtx, WriteScope};
use pidash_services::operations;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, JoinType, QueryFilter, QueryOrder,
    QuerySelect, RelationTrait,
};

use pidash_db::entity::{
    git_code_review_links, github_pull_request_links, project_members, projects, workspaces,
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
            // Unreachable from the reused calls (pure auth + lookup, no
            // `request.data` read), so no 415 can surface here.
            D::UnsupportedMediaType(_) => Denial::ServerError,
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

/// Map an operation failure onto the `Denial` arms the moved handler
/// used (OVL-14, PIDASHCONV-1649). The attach operation reports its
/// 404/409 misses as [`operations::Error::Rejection`] (message +
/// status), which land on this module's message-carrying 404/409 arms;
/// the body-carrying variants it never returns 500 instead.
impl From<operations::Error> for Denial {
    fn from(error: operations::Error) -> Self {
        match error {
            operations::Error::BadError(message) => Denial::BadError(message),
            operations::Error::Rejection {
                status: 404,
                message,
            } => Denial::NotFoundError(message),
            operations::Error::Rejection {
                status: 409,
                message,
            } => Denial::Conflict(message),
            operations::Error::NotFound => Denial::NotFound,
            operations::Error::ServerError => Denial::ServerError,
            operations::Error::FieldErrors(_)
            | operations::Error::NotFoundBody(_)
            | operations::Error::Conflict(_)
            | operations::Error::ProjectNotFound
            | operations::Error::Rejection { .. } => Denial::ServerError,
        }
    }
}

/// Wrap an operation outcome in the v1 response (201 on create, 200
/// when the link already covered this issue).
fn op_response(outcome: operations::Outcome) -> Result<Response, Denial> {
    match outcome.status {
        200 => Ok(json_ok(outcome.body)),
        201 => Ok(json_created(outcome.body)),
        _ => Err(Denial::ServerError),
    }
}

/// The actor/scope context the attach operation runs with.
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

// ---------------------------------------------------------------------------
// List / detail statements (Q2 semantics, SeaORM)
// ---------------------------------------------------------------------------

/// `GithubPullRequestLinkViewSet.get_queryset` scope (`github_pr.py:37-48`,
/// fixture `pr_list_queryset`): `INNER JOIN`s to `workspaces`, `projects`
/// and the project's `project_members` rows, the slug / project / issue
/// predicates, and an *active* membership row for the caller (no
/// `deleted_at` guard on the membership, Q2 ported bug 7). The list adds
/// `DISTINCT` (the membership join can fan out), the archived-project
/// guard and `-created_at`; the detail adds the pk and `LIMIT 1` with no
/// archived guard and no `DISTINCT` (ported bug 5).
fn pr_link_scope(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    actor_id: Uuid,
) -> sea_orm::Select<github_pull_request_links::Entity> {
    github_pull_request_links::Entity::find()
        .join(
            JoinType::InnerJoin,
            github_pull_request_links::Relation::Workspaces.def(),
        )
        .join(
            JoinType::InnerJoin,
            github_pull_request_links::Relation::Projects.def(),
        )
        .join(
            JoinType::InnerJoin,
            projects::Relation::ProjectMembers.def(),
        )
        .filter(github_pull_request_links::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(github_pull_request_links::Column::ProjectId.eq(project_id))
        .filter(github_pull_request_links::Column::IssueId.eq(issue_id))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::MemberId.eq(actor_id))
}

/// PR-link list rows (`github_pr.py:51-56`): the scope, `DISTINCT`, the
/// archived-project guard, `-created_at`.
fn pr_link_list_select(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    actor_id: Uuid,
) -> sea_orm::Select<github_pull_request_links::Entity> {
    pr_link_scope(slug, project_id, issue_id, actor_id)
        .distinct()
        .filter(projects::Column::ArchivedAt.is_null())
        .order_by_desc(github_pull_request_links::Column::CreatedAt)
}

/// PR-link detail scope + pk (`github_pr.py:97-100`): `Meta.ordering`
/// then the `.get()`'s single row.
fn pr_link_detail_select(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    actor_id: Uuid,
    pk: Uuid,
) -> sea_orm::Select<github_pull_request_links::Entity> {
    pr_link_scope(slug, project_id, issue_id, actor_id)
        .filter(github_pull_request_links::Column::Id.eq(pk))
        .order_by_desc(github_pull_request_links::Column::CreatedAt)
        .limit(1)
}

/// `GitCodeReviewLinkViewSet.get_queryset` scope (`git_code_review.py:32-44`,
/// fixture `code_review_list_queryset`): the same join-span shape as
/// [`pr_link_scope`].
fn review_link_scope(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    actor_id: Uuid,
) -> sea_orm::Select<git_code_review_links::Entity> {
    git_code_review_links::Entity::find()
        .join(
            JoinType::InnerJoin,
            git_code_review_links::Relation::Workspaces.def(),
        )
        .join(
            JoinType::InnerJoin,
            git_code_review_links::Relation::Projects.def(),
        )
        .join(
            JoinType::InnerJoin,
            projects::Relation::ProjectMembers.def(),
        )
        .filter(git_code_review_links::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(git_code_review_links::Column::ProjectId.eq(project_id))
        .filter(git_code_review_links::Column::IssueId.eq(issue_id))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::MemberId.eq(actor_id))
}

/// Review-link list rows: the scope, `DISTINCT`, the archived-project
/// guard, `-created_at`.
fn review_link_list_select(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    actor_id: Uuid,
) -> sea_orm::Select<git_code_review_links::Entity> {
    review_link_scope(slug, project_id, issue_id, actor_id)
        .distinct()
        .filter(projects::Column::ArchivedAt.is_null())
        .order_by_desc(git_code_review_links::Column::CreatedAt)
}

/// Review-link detail scope + pk (`git_code_review.py:86-98`, fixture
/// `code_review_detail_queryset`).
fn review_link_detail_select(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    actor_id: Uuid,
    pk: Uuid,
) -> sea_orm::Select<git_code_review_links::Entity> {
    review_link_scope(slug, project_id, issue_id, actor_id)
        .filter(git_code_review_links::Column::Id.eq(pk))
        .order_by_desc(git_code_review_links::Column::CreatedAt)
        .limit(1)
}

/// The pre-delete PR-link read shape (the `github_client` store mapper's
/// field set: `pr_number` widened to `i64`, `pr_updated_at` rendered
/// RFC 3339 with microseconds and `Z`).
fn pr_link_row_of(
    row: &github_pull_request_links::Model,
) -> pidash_services::app_issues::pr_links::PrLinkRow {
    pidash_services::app_issues::pr_links::PrLinkRow {
        id: row.id,
        project_id: row.project_id,
        issue_id: row.issue_id,
        repo_owner: row.repo_owner.clone(),
        repo_name: row.repo_name.clone(),
        pr_number: i64::from(row.pr_number),
        url: row.url.clone(),
        title: row.title.clone(),
        state: row.state.clone(),
        merged: row.merged,
        draft: row.draft,
        pr_updated_at: row.pr_updated_at.map(|stamp| {
            stamp
                .with_timezone(&chrono::Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
        }),
    }
}

// ---------------------------------------------------------------------------
// Row rendering (S3 shapes)
// ---------------------------------------------------------------------------

/// Render one PR-link row through `GithubPullRequestLinkSerializer` (S3
/// `render_pr_link`): full field set, no `fields=`/`expand=` (the view
/// passes neither). The `operations::pull_requests::render_pr_row` shape
/// over the decoded link.
fn render_pr_row(link: &GithubPullRequestLink, tz: &Tz) -> Result<Value, Denial> {
    use pidash_services::v1_work_items::shape_links as shape;
    let dt = |stamp: &chrono::DateTime<chrono::Utc>| {
        pidash_compat::datetime::render_datetime_in(stamp, tz)
    };
    let id = link.id.to_string();
    let issue = link.issue_id.to_string();
    let created_by = link.created_by_id.map(|id| id.to_string());
    let pr_updated_at = link.pr_updated_at.as_ref().map(dt);
    let created_at = dt(&link.created_at);
    let updated_at = dt(&link.updated_at);
    let shape_row = shape::PrLinkRow {
        id: &id,
        issue: &issue,
        repo_owner: &link.repo_owner,
        repo_name: &link.repo_name,
        pr_number: link.pr_number,
        url: &link.url,
        title: &link.title,
        state: &link.state,
        merged: link.merged,
        draft: link.draft,
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
fn render_review_row(link: &GitCodeReviewLink, tz: &Tz) -> Result<Value, Denial> {
    use pidash_services::v1_work_items::shape_links as shape;
    let dt =
        |stamp: &chrono::DateTime<chrono::Utc>| crate::serializer::render_datetime_in(stamp, tz);
    let id = link.id.to_string();
    let issue = link.issue_id.to_string();
    let created_by = link.created_by_id.map(|id| id.to_string());
    let remote_updated_at = link.remote_updated_at.as_ref().map(dt);
    let created_at = dt(&link.created_at);
    let updated_at = dt(&link.updated_at);
    let shape_row = shape::ReviewLinkRow {
        id: &id,
        issue: &issue,
        provider: &link.provider,
        host_url: &link.host_url,
        namespace: &link.namespace,
        repo_name: &link.repo_name,
        repo_external_id: &link.repo_external_id,
        external_id: &link.external_id,
        external_iid: &link.external_iid,
        url: &link.url,
        title: &link.title,
        state: &link.state,
        merged: link.merged,
        draft: link.draft,
        remote_updated_at: remote_updated_at.as_deref(),
        metadata: &link.metadata,
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
// Stores: the `pr_links` + `code_reviews` seams over SeaORM
// ---------------------------------------------------------------------------

use pidash_services::integrations::accounts::{
    GitStore, NewProviderAccount, StoreError as GitStoreError,
};
use pidash_services::integrations::code_reviews::{
    AdapterSource, AttachRequest, CodeReviewError, CodeReviewStore, NewCodeReviewLink,
    NewLegacyLink, ReviewAdapter, ReviewParser,
};
use pidash_services::integrations::repositories::{NewRepository, RepositoryDefaults};

/// Marker prefix for the same class on the review-link writes
/// (`insert_code_review_link` non-unique integrity,
/// `insert/update_legacy_link`): Python's uncaught `IntegrityError` →
/// the payload 400.
const REVIEW_PAYLOAD_INVALID: &str = "pr-links: review payload not valid";

/// Marker for the review-attach race reread miss: Python re-raises the
/// `IntegrityError` → the payload 400 (the kernel reports it as a store
/// error, so the message carries the marker).
const REVIEW_RACE_MISS: &str = "link insert conflicted but the row vanished on reread";

use pidash_db::integrations::git_models::git_code_review_link::GitCodeReviewLink;
use pidash_db::integrations::git_models::git_provider_account::GitProviderAccount;
use pidash_db::integrations::git_models::git_repository::GitRepository;
use pidash_db::integrations::github_models::github_pull_request_link::GithubPullRequestLink;
use pidash_services::integrations::code_reviews::{LegacyLinkUpdate, LinkWriteError};

/// Map a database failure into the integrations store error.
fn git_store_error<E: std::fmt::Display>(context: &str, error: E) -> GitStoreError {
    GitStoreError::Db(format!("{context}: {error}"))
}

/// Map a link-write failure: integrity violations carry the payload
/// marker (Python's `IntegrityError` 400), anything else the generic
/// store error (500).
fn git_write_error(context: &str, error: DbErr) -> GitStoreError {
    if pidash_db::orm_tx::is_integrity_violation(&error) {
        GitStoreError::Db(format!("{REVIEW_PAYLOAD_INVALID}: {context}: {error}"))
    } else {
        git_store_error(context, error)
    }
}

/// True when a SeaORM write failed on a unique violation (SQLSTATE 23505).
fn is_orm_unique_violation(error: &DbErr) -> bool {
    match error {
        DbErr::Exec(sea_orm::RuntimeErr::SqlxError(inner))
        | DbErr::Query(sea_orm::RuntimeErr::SqlxError(inner)) => is_unique_violation(inner),
        _ => false,
    }
}

/// The live `CodeReviewStore`: pool + request actor (CRUM). Audit
/// columns follow `BaseModel.save`. The delete methods soft-delete
/// (`SoftDeleteModel.delete`) with the `soft_delete_related_objects`
/// fan-out inline, preserving the update-then-enqueue order of
/// `mixins.py:71-78`. Every statement is the SeaORM twin beside its SQL
/// constant in `pidash_services::integrations` (`*::orm`), run on the
/// pool's SeaORM connection; the bind transaction stays an sqlx
/// transaction bridged through `TxConn`.
struct ReviewStore {
    pool: sqlx::PgPool,
    db: DatabaseConnection,
    actor: uuid::Uuid,
}

impl ReviewStore {
    fn new(pool: sqlx::PgPool, actor: uuid::Uuid) -> Self {
        let db = pidash_db::pool::sea_orm_connection(&pool);
        Self { pool, db, actor }
    }
}

use pidash_services::integrations::accounts::orm as accounts_orm;
use pidash_services::integrations::code_reviews::orm as reviews_orm;
use pidash_services::integrations::repositories::orm as repos_orm;

impl CodeReviewStore for ReviewStore {
    async fn review_issue_exists(
        &self,
        issue_id: Uuid,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<bool, GitStoreError> {
        reviews_orm::issue_exists(&self.db, issue_id, project_id, workspace_slug)
            .await
            .map_err(|error| git_store_error("review issue exists", error))
    }

    async fn find_legacy_link(
        &self,
        repo_owner_lower: &str,
        repo_name_lower: &str,
        pr_number: i32,
    ) -> Result<Option<GithubPullRequestLink>, GitStoreError> {
        reviews_orm::find_legacy_link(&self.db, repo_owner_lower, repo_name_lower, pr_number)
            .await
            .map_err(|error| git_store_error("legacy link find", error))
    }

    async fn find_path_review_link(
        &self,
        provider: &str,
        host_url: &str,
        namespace_lower: &str,
        repo_name_lower: &str,
        external_iid: &str,
    ) -> Result<Option<GitCodeReviewLink>, GitStoreError> {
        reviews_orm::find_path_review_link(
            &self.db,
            provider,
            host_url,
            namespace_lower,
            repo_name_lower,
            external_iid,
        )
        .await
        .map_err(|error| git_store_error("path review link find", error))
    }

    async fn find_repo_scope_link(
        &self,
        provider: &str,
        host_url: &str,
        external_iid: &str,
        repo_external_id: &str,
    ) -> Result<Option<GitCodeReviewLink>, GitStoreError> {
        reviews_orm::find_repo_scope_link(
            &self.db,
            provider,
            host_url,
            external_iid,
            repo_external_id,
        )
        .await
        .map_err(|error| git_store_error("repo scope link find", error))
    }

    async fn find_path_empty_repo_link(
        &self,
        provider: &str,
        host_url: &str,
        external_iid: &str,
        namespace_lower: &str,
        repo_name_lower: &str,
    ) -> Result<Option<GitCodeReviewLink>, GitStoreError> {
        reviews_orm::find_path_empty_repo_link(
            &self.db,
            provider,
            host_url,
            external_iid,
            namespace_lower,
            repo_name_lower,
        )
        .await
        .map_err(|error| git_store_error("path empty repo link find", error))
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
        // The account and repository halves decode by prefixed name
        // (`pa_` / `r_`), never by position.
        reviews_orm::find_binding_review(
            &self.db,
            workspace_slug,
            project_id,
            provider,
            host_url,
            namespace,
            repo_name,
        )
        .await
        .map_err(|error| git_store_error("binding review find", error))
    }

    async fn find_review_issue_workspace(
        &self,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<Option<Uuid>, GitStoreError> {
        reviews_orm::review_issue_workspace(&self.db, project_id, workspace_slug)
            .await
            .map_err(|error| git_store_error("review issue workspace", error))
    }

    async fn find_review_repository(
        &self,
        provider: &str,
        host_url: &str,
        namespace: &str,
        repo_name: &str,
    ) -> Result<Option<GitRepository>, GitStoreError> {
        reviews_orm::find_review_repository(&self.db, provider, host_url, namespace, repo_name)
            .await
            .map_err(|error| git_store_error("review repository find", error))
    }

    async fn find_review_project_workspace(
        &self,
        project_id: Uuid,
    ) -> Result<Option<Uuid>, GitStoreError> {
        reviews_orm::review_project_workspace(&self.db, project_id)
            .await
            .map_err(|error| git_store_error("review project workspace", error))
    }

    async fn insert_code_review_link(
        &self,
        row: NewCodeReviewLink,
    ) -> Result<GitCodeReviewLink, LinkWriteError> {
        let link = GitCodeReviewLink {
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
        };
        match reviews_orm::insert_code_review_link(&self.db, &link).await {
            Ok(()) => Ok(link),
            Err(error) if is_orm_unique_violation(&error) => Err(LinkWriteError::Conflict),
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
        let link = GithubPullRequestLink {
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
        };
        reviews_orm::insert_legacy_link(&self.db, &link)
            .await
            .map_err(|error| git_write_error("legacy link insert", error))?;
        Ok(link)
    }

    async fn update_legacy_link(
        &self,
        id: Uuid,
        update: LegacyLinkUpdate,
    ) -> Result<GithubPullRequestLink, GitStoreError> {
        reviews_orm::update_legacy_link(&self.db, id, &update)
            .await
            .map_err(|error| git_write_error("legacy link update", error))?;
        // `existing.save(update_fields=[...])` mutates in place; reread
        // the row for the caller.
        reviews_orm::legacy_link_by_id(&self.db, id)
            .await
            .map_err(|error| git_store_error("legacy link reread", error))?
            .ok_or(GitStoreError::NotFound("legacy link"))
    }

    async fn delete_code_review_link(&self, id: Uuid) -> Result<(), GitStoreError> {
        // Instance `.delete()` (`mixins.py:71-78`): soft row, then the
        // fan-out — the port delegates the write mechanics here.
        reviews_orm::soft_delete_code_review_link(&self.db, id, self.actor, chrono::Utc::now())
            .await
            .map_err(|error| git_store_error("review link delete", error))?;
        enqueue_soft_delete(&self.pool, "gitcodereviewlink", &id).await;
        Ok(())
    }

    async fn delete_legacy_link(&self, id: Uuid) -> Result<(), GitStoreError> {
        reviews_orm::soft_delete_legacy_link(&self.db, id, self.actor, chrono::Utc::now())
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
        accounts_orm::list_provider_accounts(&self.db, workspace_id, provider, host_url)
            .await
            .map_err(|error| git_store_error("account list", error))
    }

    async fn get_provider_account(
        &self,
        workspace_id: Uuid,
        provider: &str,
        host_url: &str,
        account_id: Uuid,
    ) -> Result<Option<GitProviderAccount>, GitStoreError> {
        accounts_orm::get_provider_account(&self.db, workspace_id, provider, host_url, account_id)
            .await
            .map_err(|error| git_store_error("account get", error))
    }

    async fn insert_provider_account(
        &self,
        row: NewProviderAccount,
    ) -> Result<GitProviderAccount, GitStoreError> {
        accounts_orm::insert_provider_account(&self.db, &row)
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
        repos_orm::find_repository_by_external(&self.db, provider, host_url, external_id)
            .await
            .map_err(|error| git_store_error("repository find external", error))
    }

    async fn find_repository_by_full_name(
        &self,
        provider: &str,
        host_url: &str,
        full_name: &str,
    ) -> Result<Option<GitRepository>, GitStoreError> {
        repos_orm::find_repository_by_full_name(&self.db, provider, host_url, full_name)
            .await
            .map_err(|error| git_store_error("repository find full name", error))
    }

    async fn update_repository(
        &self,
        id: Uuid,
        defaults: RepositoryDefaults,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<GitRepository, GitStoreError> {
        repos_orm::update_repository(&self.db, id, &defaults, now)
            .await
            .map_err(|error| git_store_error("repository update", error))?;
        repos_orm::repository_by_id(&self.db, id)
            .await
            .map_err(|error| git_store_error("repository reread", error))?
            .ok_or(GitStoreError::NotFound("repository"))
    }

    async fn insert_repository(&self, row: NewRepository) -> Result<GitRepository, GitStoreError> {
        repos_orm::insert_repository(&self.db, &row)
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
        repos_orm::workspace_id(&self.db, slug)
            .await
            .map_err(|error| git_store_error("workspace id", error))
    }

    async fn find_project(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
    ) -> Result<Option<pidash_services::integrations::repositories::ProjectRef>, GitStoreError>
    {
        repos_orm::project(&self.db, workspace_id, project_id)
            .await
            .map_err(|error| git_store_error("project get", error))
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
        let id = Uuid::new_v4();
        let mut metadata = Map::new();
        metadata.insert("raw_url".to_owned(), Value::String(plan.raw_url.clone()));
        {
            let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            repos_orm::delete_project_bindings(&conn, plan.project_id)
                .await
                .map_err(|error| git_store_error("bind delete", error))?;
            repos_orm::delete_project_github_syncs(&conn, plan.project_id)
                .await
                .map_err(|error| git_store_error("bind sync delete", error))?;
            repos_orm::insert_binding(
                &conn,
                repos_orm::NewBinding {
                    id,
                    now: plan.now,
                    created_by_id: plan.actor_id,
                    updated_by_id: plan.actor_id,
                    project_id: plan.project_id,
                    workspace_id: plan.workspace_id,
                    repository_id: plan.repository_id,
                    provider_account_id: plan.provider_account_id,
                    actor_id: plan.actor_id,
                    is_sync_enabled: false,
                    clone_auth_mode: &plan.clone_auth_mode,
                    metadata: Value::Object(metadata.clone()),
                },
            )
            .await
            .map_err(|error| git_store_error("bind insert", error))?;
        }
        // `project.save(update_fields=...)`: only the changed columns
        // plus `updated_at` (`services.py:341-348`).
        pidash_services::domain::projects::save_bind_project_update(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            plan.project_id,
            plan.repo_url_update.as_deref(),
            plan.base_branch_update.as_deref(),
            plan.now,
        )
        .await
        .map_err(|error| git_store_error("bind project update", error))?;
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
        repos_orm::binding_view(&self.db, project_id, workspace_slug)
            .await
            .map_err(|error| git_store_error("binding get", error))
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
        repos_orm::set_binding_sync(&self.db, binding_id, enabled, now)
            .await
            .map_err(|error| git_store_error("binding set sync", error))?;
        repos_orm::binding_by_id(&self.db, binding_id)
            .await
            .map_err(|error| git_store_error("binding reread", error))?
            .ok_or(GitStoreError::NotFound("binding"))
    }

    async fn set_github_syncs_enabled(
        &self,
        project_id: Uuid,
        workspace_slug: &str,
        enabled: bool,
    ) -> Result<u64, GitStoreError> {
        repos_orm::set_project_github_syncs_enabled(&self.db, project_id, workspace_slug, enabled)
            .await
            .map_err(|error| git_store_error("github syncs set enabled", error))
    }

    async fn delete_binding(&self, binding_id: Uuid) -> Result<(), GitStoreError> {
        repos_orm::delete_binding(&self.db, binding_id)
            .await
            .map_err(|error| git_store_error("binding delete", error))?;
        Ok(())
    }

    async fn delete_github_syncs_for_project(
        &self,
        project_id: Uuid,
    ) -> Result<u64, GitStoreError> {
        repos_orm::delete_project_github_syncs(&self.db, project_id)
            .await
            .map_err(|error| git_store_error("github syncs delete", error))
    }
}

// ---------------------------------------------------------------------------
// Task fan-out
// ---------------------------------------------------------------------------

/// Best-effort publish of a Celery message: without a queue table the
/// response still stands (the D-19/D-26 precedent).
async fn enqueue_message(pool: &sqlx::PgPool, message: pidash_jobs::TaskMessage) {
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
    let message = pidash_jobs::TaskMessage::new(
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
// Transports: production `GitLabTransport` (the GitHub twin moved to
// pidash_services::integrations::github_client)
// ---------------------------------------------------------------------------

pub use pidash_compat::response::SERVER_ERROR_BODY;
use pidash_db::config::encryption::Keyring;
use pidash_services::integrations::adapters_github::GitHubAdapter;
use pidash_services::integrations::adapters_gitlab::{
    GitLabAdapter, GitLabConfig, GitLabRequest, GitLabResponse, GitLabTransport,
};
use pidash_services::integrations::github_client::{
    LiveGithubClient, PoolPrStore, PrLinksSurface, TransportContext, TransportGuard,
};
use pidash_types::integrations::registry::UnknownProvider;
use pidash_types::integrations::GitProviderError;

/// Run an async future to completion from the sync `GitLabTransport`
/// seam: a fresh current-thread runtime on a spawned thread. The seam
/// is sync because Python's `requests` is blocking; this keeps the
/// async executor unblocked without new Cargo features. (The GitHub
/// twin moved to `pidash_services::integrations::github_client` with
/// its own copy; OVL-04 canonicalizes the pair.)
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
    let raw = match read_request_bytes(body).await {
        Ok(raw) => raw,
        Err(response) => return response,
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
    let raw = match read_request_bytes(body).await {
        Ok(raw) => raw,
        Err(response) => return response,
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

/// Shared paginated-list core (`BasePaginator.paginate` with the default
/// `OffsetPaginator`, `utils/paginator.py:654-760`) over the evaluated
/// queryset rows: per_page/cursor parsing, then `fetch` (the list
/// statement runs only once the params parsed, as before), the offset
/// window, the 12-key envelope. `render` shapes one row.
async fn paginated_list<T, F>(
    query: &QueryMap,
    tz: &Tz,
    fetch: F,
    render: fn(&T, &Tz) -> Result<Value, Denial>,
) -> Result<Response, Denial>
where
    F: std::future::Future<Output = Result<Vec<T>, Denial>>,
{
    let per_page =
        crate::paginator::parse_per_page(query_last(query, "per_page").as_deref(), 1000, 1000)
            .map_err(page_denial)?;
    let cursor_raw = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = crate::paginator::Cursor::from_string(&cursor_raw).map_err(page_denial)?;
    let window = crate::paginator::offset_window(
        per_page,
        cursor.offset,
        cursor.value,
        cursor.is_prev,
        None,
    )
    .map_err(page_denial)?;
    let rows = fetch.await?;
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
        rendered.push(render(row, tz)?);
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
    let db = pidash_db::pool::sea_orm_connection(&pre.pool);
    paginated_list(
        query,
        &timezone,
        async {
            let rows = pr_link_list_select(slug, project_id, issue_id, pre.actor.id)
                .all(&db)
                .await
                .map_err(|error| db_error(error, "pr-links list"))?;
            Ok(rows
                .into_iter()
                .map(GithubPullRequestLink::from)
                .collect::<Vec<_>>())
        },
        render_pr_row,
    )
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
    let db = pidash_db::pool::sea_orm_connection(&pre.pool);
    paginated_list(
        query,
        &timezone,
        async {
            let rows = review_link_list_select(slug, project_id, issue_id, pre.actor.id)
                .all(&db)
                .await
                .map_err(|error| db_error(error, "pr-links list"))?;
            Ok(rows
                .into_iter()
                .map(GitCodeReviewLink::from)
                .collect::<Vec<_>>())
        },
        render_review_row,
    )
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
    // `TimezoneMixin.initial` precedes the view body: an unknown stored
    // zone 400s here, before body parsing below.
    activate_timezone(pre.actor.timezone.as_deref())?;
    let body = parse_json_body(raw_body)?;
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
    let write = operations::pull_requests::PullRequestWrite {
        slug,
        project_id,
        issue_id,
        body: &body,
        redis: state.redis().cloned(),
        secret: state.settings().secret_key.clone(),
    };
    let outcome = operations::pull_requests::attach(&deps, &ctx, &write).await?;
    op_response(outcome)
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
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    activate_timezone(pre.actor.timezone.as_deref())?;
    let db = pidash_db::pool::sea_orm_connection(&pre.pool);
    let row = pr_link_detail_select(slug, project_id, issue_id, pre.actor.id, pk)
        .one(&db)
        .await
        .map_err(|error| db_error(error, "pr detail fetch"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let store = PoolPrStore::new(pre.pool.clone(), PrLinksSurface::PrLinks);
    let link = pr_link_row_of(&row);
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
        let message = pidash_jobs::TaskMessage::new(task.task, task.args, task.kwargs);
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
    let raw_url = operations::pull_requests::extract_raw_url(&body)?;
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
    // Re-read the saved link by id for the create response (any manager
    // scope, as the `.get(pk)` on the saved instance).
    let saved =
        reviews_orm::review_link_by_id(&pidash_db::pool::sea_orm_connection(&pre.pool), link.id)
            .await
            .map_err(|error| db_error(error, "review create reread"))?;
    let Some(saved) = saved else {
        return Err(Denial::ServerError);
    };
    let shape = render_review_row(&saved, &timezone)?;
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
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    activate_timezone(pre.actor.timezone.as_deref())?;
    let row = review_link_detail_select(slug, project_id, issue_id, pre.actor.id, pk)
        .one(&pidash_db::pool::sea_orm_connection(&pre.pool))
        .await
        .map_err(|error| db_error(error, "review detail fetch"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let store = ReviewStore::new(pre.pool.clone(), pre.actor.id);
    let link = GitCodeReviewLink::from(row);
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

    // -- SeaORM statements ---------------------------------------------------

    use sea_orm::{DbBackend, QueryTrait};

    fn ids() -> (Uuid, Uuid, Uuid, Uuid) {
        (
            Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid"),
            Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("uuid"),
            Uuid::parse_str("33333333-3333-3333-3333-333333333333").expect("uuid"),
            Uuid::parse_str("44444444-4444-4444-4444-444444444444").expect("uuid"),
        )
    }

    #[test]
    fn pr_link_list_pins_scope_distinct_archived_guard_and_order() {
        let (project, issue, actor, _) = ids();
        let sql = pr_link_list_select("acme", project, issue, actor)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.starts_with("SELECT DISTINCT "), "{sql}");
        assert!(
            sql.contains(r#"INNER JOIN "workspaces" ON "github_pull_request_links"."workspace_id" = "workspaces"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "projects" ON "github_pull_request_links"."project_id" = "projects"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "project_members" ON "projects"."id" = "project_members"."project_id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""github_pull_request_links"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(sql.contains(r#""workspaces"."slug" = 'acme'"#), "{sql}");
        assert!(
            sql.contains(r#""project_members"."is_active" = TRUE"#),
            "{sql}"
        );
        assert!(
            sql.contains(
                r#""project_members"."member_id" = '33333333-3333-3333-3333-333333333333'"#
            ),
            "{sql}"
        );
        // No `deleted_at` guard on the membership row (Q2 ported bug 7).
        assert!(!sql.contains(r#""project_members"."deleted_at""#), "{sql}");
        assert!(sql.contains(r#""projects"."archived_at" IS NULL"#), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "github_pull_request_links"."created_at" DESC"#),
            "{sql}"
        );
    }

    #[test]
    fn pr_link_detail_pins_pk_no_distinct_no_archived_guard_limit_one() {
        let (project, issue, actor, pk) = ids();
        let sql = pr_link_detail_select("acme", project, issue, actor, pk)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            sql.starts_with("SELECT \"github_pull_request_links\""),
            "{sql}"
        );
        assert!(!sql.contains("DISTINCT"), "{sql}");
        assert!(!sql.contains("archived_at"), "{sql}");
        assert!(
            sql.contains(
                r#""github_pull_request_links"."id" = '44444444-4444-4444-4444-444444444444'"#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#""project_members"."member_id" = "#), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "github_pull_request_links"."created_at" DESC LIMIT 1"#),
            "{sql}"
        );
    }

    #[test]
    fn review_link_statements_mirror_the_pr_link_shape() {
        let (project, issue, actor, pk) = ids();
        let list = review_link_list_select("acme", project, issue, actor)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(list.starts_with("SELECT DISTINCT "), "{list}");
        assert!(
            list.contains(r#"INNER JOIN "project_members" ON "projects"."id" = "project_members"."project_id""#),
            "{list}"
        );
        assert!(
            list.contains(r#""git_code_review_links"."deleted_at" IS NULL"#),
            "{list}"
        );
        assert!(
            list.contains(
                r#""git_code_review_links"."issue_id" = '22222222-2222-2222-2222-222222222222'"#
            ),
            "{list}"
        );
        assert!(
            list.contains(r#""projects"."archived_at" IS NULL"#),
            "{list}"
        );
        assert!(
            list.ends_with(r#"ORDER BY "git_code_review_links"."created_at" DESC"#),
            "{list}"
        );
        let detail = review_link_detail_select("acme", project, issue, actor, pk)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(!detail.contains("DISTINCT"), "{detail}");
        assert!(!detail.contains("archived_at"), "{detail}");
        assert!(
            detail.contains(
                r#""git_code_review_links"."id" = '44444444-4444-4444-4444-444444444444'"#
            ),
            "{detail}"
        );
        assert!(
            detail.ends_with(r#"ORDER BY "git_code_review_links"."created_at" DESC LIMIT 1"#),
            "{detail}"
        );
    }

    fn pr_link_fixture() -> GithubPullRequestLink {
        let (project, issue, actor, pk) = ids();
        let at = |text: &str| {
            chrono::DateTime::parse_from_rfc3339(text)
                .expect("rfc3339")
                .with_timezone(&chrono::Utc)
        };
        GithubPullRequestLink {
            id: pk,
            created_at: at("2024-03-04T05:06:07.123456Z"),
            updated_at: at("2024-03-05T05:06:07Z"),
            created_by_id: Some(actor),
            updated_by_id: None,
            deleted_at: None,
            project_id: project,
            workspace_id: Uuid::nil(),
            issue_id: issue,
            repo_owner: "octo".to_owned(),
            repo_name: "repo".to_owned(),
            pr_number: 42,
            url: "https://github.com/octo/repo/pull/42".to_owned(),
            title: "Fix".to_owned(),
            state: "open".to_owned(),
            merged: false,
            draft: true,
            pr_updated_at: Some(at("2024-03-06T05:06:07.000001Z")),
        }
    }

    #[test]
    fn pr_link_row_of_widens_pr_number_and_renders_micros_z() {
        let link = pr_link_fixture();
        let model = github_pull_request_links::Model {
            created_at: link.created_at.into(),
            updated_at: link.updated_at.into(),
            deleted_at: None,
            id: link.id,
            repo_owner: link.repo_owner.clone(),
            repo_name: link.repo_name.clone(),
            pr_number: link.pr_number,
            url: link.url.clone(),
            title: link.title.clone(),
            state: link.state.clone(),
            merged: link.merged,
            draft: link.draft,
            pr_updated_at: link.pr_updated_at.map(Into::into),
            created_by_id: link.created_by_id,
            issue_id: link.issue_id,
            project_id: link.project_id,
            updated_by_id: None,
            workspace_id: link.workspace_id,
        };
        let row = pr_link_row_of(&model);
        assert_eq!(row.pr_number, 42_i64);
        assert_eq!(
            row.pr_updated_at.as_deref(),
            Some("2024-03-06T05:06:07.000001Z")
        );
        assert_eq!(
            (row.id, row.project_id, row.issue_id),
            (link.id, link.project_id, link.issue_id)
        );
        assert_eq!(row.state, "open");
        assert!(row.draft && !row.merged);
    }

    #[test]
    fn render_pr_row_renders_the_serializer_fields_in_the_request_zone() {
        let link = pr_link_fixture();
        let tz: Tz = "Asia/Tokyo".parse().expect("zone");
        let rendered = render_pr_row(&link, &tz).expect("render");
        let map = rendered.as_object().expect("object");
        assert_eq!(map["id"], Value::String(link.id.to_string()));
        assert_eq!(map["issue"], Value::String(link.issue_id.to_string()));
        assert_eq!(map["pr_number"], Value::from(42));
        assert_eq!(map["created_by"], Value::String(ids().2.to_string()));
        assert_eq!(
            map["created_at"],
            Value::String("2024-03-04T14:06:07.123456+09:00".to_owned())
        );
        assert_eq!(
            map["pr_updated_at"],
            Value::String("2024-03-06T14:06:07.000001+09:00".to_owned())
        );
        let utc = render_pr_row(&link, &chrono_tz::UTC).expect("render");
        assert_eq!(
            utc["updated_at"],
            Value::String("2024-03-05T05:06:07Z".to_owned())
        );
    }

    /// The list / detail scopes and the store wiring against constraint-free
    /// temp copies on a one-connection pool.
    #[tokio::test]
    async fn live_link_scopes_and_store() {
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
            "workspaces",
            "projects",
            "project_members",
            "issues",
            "github_pull_request_links",
            "git_code_review_links",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        // The duplicate-key arm needs a key.
        sqlx::query("ALTER TABLE git_code_review_links ADD PRIMARY KEY (id)")
            .execute(&pool)
            .await
            .expect("pk");
        let (ws, project, archived_project, issue, other_issue, actor, stranger) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let (pr_old, pr_new, pr_dead, pr_other, pr_archived, review, review_dead) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let pr_row = |id: Uuid, project_id: Uuid, issue_id: Uuid, created: &str, deleted: &str| {
            format!(
                "INSERT INTO github_pull_request_links (id, created_at, updated_at, deleted_at, \
                 repo_owner, repo_name, pr_number, url, title, state, merged, draft, \
                 issue_id, project_id, workspace_id) VALUES ('{id}', '{created}', '{created}', \
                 {deleted}, 'octo', 'repo', 7, 'https://x', 'T', 'open', false, false, \
                 '{issue_id}', '{project_id}', '{ws}')"
            )
        };
        let review_row = |id: Uuid, deleted: &str| {
            format!(
                "INSERT INTO git_code_review_links (id, created_at, updated_at, deleted_at, \
                 provider, host_url, namespace, repo_name, repo_external_id, external_id, \
                 external_iid, url, title, state, merged, draft, metadata, issue_id, \
                 project_id, workspace_id) VALUES ('{id}', now(), now(), {deleted}, 'gitlab', \
                 'https://gitlab.com', 'grp', 'repo', '9', '77', '3', 'https://x', 'T', \
                 'opened', false, false, '{{}}', '{issue}', '{project}', '{ws}')"
            )
        };
        for sql in [
            format!("INSERT INTO workspaces (id, slug) VALUES ('{ws}', 'acme')"),
            format!(
                "INSERT INTO projects (id, workspace_id, archived_at, deleted_at) VALUES \
                 ('{project}', '{ws}', NULL, NULL), ('{archived_project}', '{ws}', now(), NULL)"
            ),
            // Two active rows for the actor on the project: the join fans
            // out and `DISTINCT` must fold it back.
            format!(
                "INSERT INTO project_members (id, project_id, member_id, workspace_id, is_active) VALUES \
                 (gen_random_uuid(), '{project}', '{actor}', '{ws}', true), \
                 (gen_random_uuid(), '{project}', '{actor}', '{ws}', true), \
                 (gen_random_uuid(), '{project}', '{stranger}', '{ws}', false), \
                 (gen_random_uuid(), '{archived_project}', '{actor}', '{ws}', true)"
            ),
            format!(
                "INSERT INTO issues (id, project_id, workspace_id, deleted_at) VALUES \
                 ('{issue}', '{project}', '{ws}', NULL)"
            ),
            pr_row(pr_old, project, issue, "2024-01-01T00:00:00Z", "NULL"),
            pr_row(pr_new, project, issue, "2024-02-01T00:00:00Z", "NULL"),
            pr_row(pr_dead, project, issue, "2024-03-01T00:00:00Z", "now()"),
            pr_row(pr_other, project, other_issue, "2024-03-01T00:00:00Z", "NULL"),
            pr_row(pr_archived, archived_project, issue, "2024-03-01T00:00:00Z", "NULL"),
            review_row(review, "NULL"),
            review_row(review_dead, "now()"),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed");
        }
        let db = pidash_db::pool::sea_orm_connection(&pool);

        // PR-link list: newest first, no duplicate from the membership
        // fan-out, dead / other-issue rows out.
        let listed: Vec<Uuid> = pr_link_list_select("acme", project, issue, actor)
            .all(&db)
            .await
            .expect("list")
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(listed, vec![pr_new, pr_old]);
        assert!(pr_link_list_select("acme", project, issue, stranger)
            .all(&db)
            .await
            .expect("list")
            .is_empty());
        assert!(pr_link_list_select("nope", project, issue, actor)
            .all(&db)
            .await
            .expect("list")
            .is_empty());
        // Archived project: the list guards, the detail does not (bug 5).
        assert!(pr_link_list_select("acme", archived_project, issue, actor)
            .all(&db)
            .await
            .expect("list")
            .is_empty());
        let archived_hit =
            pr_link_detail_select("acme", archived_project, issue, actor, pr_archived)
                .one(&db)
                .await
                .expect("detail");
        assert_eq!(archived_hit.map(|row| row.id), Some(pr_archived));
        let hit = pr_link_detail_select("acme", project, issue, actor, pr_old)
            .one(&db)
            .await
            .expect("detail")
            .expect("found");
        assert_eq!(pr_link_row_of(&hit).pr_number, 7);
        assert!(
            pr_link_detail_select("acme", project, issue, actor, pr_dead)
                .one(&db)
                .await
                .expect("detail")
                .is_none()
        );
        assert!(
            pr_link_detail_select("acme", project, issue, stranger, pr_old)
                .one(&db)
                .await
                .expect("detail")
                .is_none()
        );

        // Review links: same scope semantics.
        let reviews: Vec<Uuid> = review_link_list_select("acme", project, issue, actor)
            .all(&db)
            .await
            .expect("list")
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(reviews, vec![review]);
        let detail = review_link_detail_select("acme", project, issue, actor, review)
            .one(&db)
            .await
            .expect("detail")
            .map(GitCodeReviewLink::from)
            .expect("found");
        assert_eq!((detail.id, detail.external_iid.as_str()), (review, "3"));
        assert!(
            review_link_detail_select("acme", project, issue, actor, review_dead)
                .one(&db)
                .await
                .expect("detail")
                .is_none()
        );

        // Store wiring over the same connection.
        let store = ReviewStore {
            pool: pool.clone(),
            db: pidash_db::pool::sea_orm_connection(&pool),
            actor,
        };
        assert!(store
            .review_issue_exists(issue, project, "acme")
            .await
            .expect("exists"));
        assert!(!store
            .review_issue_exists(other_issue, project, "acme")
            .await
            .expect("exists"));
        let now = chrono::Utc::now();
        let legacy_id = Uuid::new_v4();
        let inserted = store
            .insert_legacy_link(NewLegacyLink {
                id: legacy_id,
                created_at: now,
                updated_at: now,
                created_by_id: None,
                updated_by_id: None,
                project_id: project,
                workspace_id: ws,
                issue_id: issue,
                repo_owner: "octo".to_owned(),
                repo_name: "mirror".to_owned(),
                pr_number: 9,
                url: "https://github.com/octo/mirror/pull/9".to_owned(),
                title: "Nine".to_owned(),
                state: "open".to_owned(),
                merged: false,
                draft: false,
                pr_updated_at: None,
            })
            .await
            .expect("insert legacy");
        assert_eq!(inserted.created_by_id, Some(actor));
        let found = store
            .find_legacy_link("octo", "mirror", 9)
            .await
            .expect("find")
            .expect("found");
        assert_eq!((found.id, found.created_by_id), (legacy_id, Some(actor)));
        let updated = store
            .update_legacy_link(
                legacy_id,
                LegacyLinkUpdate {
                    url: found.url.clone(),
                    title: "Nine!".to_owned(),
                    state: "closed".to_owned(),
                    merged: true,
                    draft: false,
                    pr_updated_at: Some(now),
                    updated_at: now,
                },
            )
            .await
            .expect("update");
        assert_eq!(
            (
                updated.title.as_str(),
                updated.state.as_str(),
                updated.merged
            ),
            ("Nine!", "closed", true)
        );
        assert_eq!(
            updated.pr_updated_at.map(|stamp| stamp.timestamp_micros()),
            Some(now.timestamp_micros())
        );
        let new_review = |id: Uuid| NewCodeReviewLink {
            id,
            created_at: now,
            updated_at: now,
            created_by_id: None,
            updated_by_id: None,
            project_id: project,
            workspace_id: ws,
            issue_id: issue,
            provider: "gitlab".to_owned(),
            host_url: "https://gitlab.com".to_owned(),
            namespace: "grp".to_owned(),
            repo_name: "repo".to_owned(),
            repo_external_id: "9".to_owned(),
            external_id: "78".to_owned(),
            external_iid: "4".to_owned(),
            url: "https://gitlab.com/grp/repo/-/merge_requests/4".to_owned(),
            title: "Four".to_owned(),
            state: "opened".to_owned(),
            merged: false,
            draft: false,
            remote_updated_at: None,
            metadata: Value::Object(Map::new()),
        };
        let review_id = Uuid::new_v4();
        let link = store
            .insert_code_review_link(new_review(review_id))
            .await
            .expect("insert review");
        assert_eq!(link.created_by_id, Some(actor));
        // The same pk again: the unique violation is the `Conflict` arm.
        assert!(matches!(
            store.insert_code_review_link(new_review(review_id)).await,
            Err(LinkWriteError::Conflict)
        ));
        let path_hit = store
            .find_path_review_link("gitlab", "https://gitlab.com", "grp", "repo", "4")
            .await
            .expect("find")
            .expect("found");
        assert_eq!(path_hit.id, review_id);
        assert_eq!(
            store
                .find_review_project_workspace(project)
                .await
                .expect("ws"),
            Some(ws)
        );
    }
}
