#![forbid(unsafe_code)]

//! Issue version + description-version + move handlers (D-26, stage 5, PIDASHCONV-657).
//!
//! Ports `app/views/issue/version.py` (2 units, 144 lines) and
//! `app/views/issue/move.py` (1 unit, 46 lines):
//!
//! - `GET issues/<issue_id>/versions/` (`IssueVersionEndpoint.get`,
//!   `version.py:36-74`): the 9-key `paginate` envelope over 10-key rows
//!   (`required_fields`, model `-created_at` order). No parent checks at
//!   all — a missing issue is an empty page, not a 404.
//! - `GET issues/<issue_id>/versions/<pk>/` (`:38-44`): the 30-key
//!   `IssueVersionDetailSerializer` row (the dup-`name` collapses to one,
//!   first position — the 640 finding).
//! - `GET work-items/<work_item_id>/description-versions/`
//!   (`WorkItemDescriptionVersionEndpoint.get`, `:86-144`):
//!   `Project.get` + `Issue.get` (404s), the guest-view 403 gate, then
//!   the same 9-key envelope over the explicit
//!   `.order_by("-created_at")` list.
//! - `GET work-items/<work_item_id>/description-versions/<pk>/`
//!   (`:107-116`): the 14-key `IssueDescriptionVersionDetailSerializer`
//!   row (`description_binary` renders base64, null stays null).
//! - `POST work-items/<pk>/move/` (`IssueMoveEndpoint.post`,
//!   `move.py:28-46`): the ADMIN/MEMBER gate, the non-dict-body `{}` coercion,
//!   `domain::issues::move_to_project` over the 650 kernel, `MoveRejection`
//!   mapped to `{"error"}` + status, the 594 signal pair and the enqueues
//!   folded into the commit chain around the state-changing save, then the
//!   22-key APP `IssueSerializer` read shape (a bare instance: the 7
//!   annotation keys are `SkipField`-omitted, the 639 rule, verified live).
//!
//! Every other method on those paths proxies to Django (route table in
//! `super::routes` via [`routes`]); `HEAD` rides axum's `get` handling
//! on the GET-owned paths like Django's GET-backed `HEAD`.
//!
//! Reuse (call, don't copy): the shared SeaORM gates (`super::access`),
//! the version reads as SeaORM selects (batch 2c; the 649 key order in
//! `super::queries_engage`), pilot-2's `v2_page` cursor math
//! (`super::render`), the 640 version detail rendering
//! (`serializers_assoc`), the 639 base-shape rendering
//! (`serializers_detail`), the move domain service
//! (`domain::issues::move_to_project`, with the 594 fire, the D-14
//! pod drains and the D-12 creation drain behind its hooks), and the
//! shared DRF body negotiator (`v1_cycles_modules::body`).
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * The versions list never 404s: a missing issue (or project, past the
//!   gate) is an empty 200 page (`filter`, never `get`).
//! * `IssueDescriptionVersion` declares no `Meta.ordering`, so the detail
//!   `.get()` carries no `ORDER BY` (unlike its version sibling) — the
//!   SeaORM detail select preserves the asymmetry.
//! * `PaginateCursor`'s offset part is parsed but never read (`start_index`
//!   derives from `current_page` alone) — inherited from pilot-2's
//!   `v2_page`, reused, not re-ported.
//! * The move's `now` is drawn once per request (Python draws it inside
//!   the transaction, after the locks); the 650 driver takes it frozen.
//! * A non-finite `sort_order` answers 500 (DRF renders with
//!   `allow_nan=False` under the default `STRICT_JSON=True`).
//!
//! # Known gaps (documented, not silent)
//!
//! * Past a ticking-state transition the 594 fire needs the same runtime
//!   (ticker reconcile, entry-run dispatch); the seam answers store
//!   errors there, which the fire swallows by design (counter + the
//!   verbatim log line) while the move stands. Non-trigger transitions
//!   (every gate seed, the backlog norm) run no seam I/O past the two
//!   state lookups and are fully faithful.
//! * `v2_page` parses cursor ints strictly (Rust `parse`), where Python's
//!   `int()` accepts whitespace/signs/underscores — pilot-2 shared code,
//!   reused, not re-ported.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use pidash_db::entity::{issue_description_versions, issue_versions, issues, users, workspaces};
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, FromQueryResult, JoinType,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait, Select,
};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::edge;
use pidash_services::app_issues::serializers_assoc::{
    issue_description_version_detail_to_representation, issue_version_detail_to_representation,
    IssueDescriptionVersionDetailRow, IssueVersionDetailRow,
};
use pidash_services::app_issues::serializers_engage::{git_issue_synced, github_issue_synced};
use pidash_services::app_issues::{
    issue_detail_base_to_representation, issue_is_actively_synced, IssueDetailBaseRow,
    SourceIssueRow,
};
use pidash_services::domain;
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::issues as domain_issues;
use pidash_services::domain::issues::{
    CreationDeps, MoveDomainOutcome, MovePostCommitHook, MoveRejection,
};
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::domain::{WriteActor, WriteCtx, WriteScope};

use super::access;
use super::queries_engage::VERSION_PAGE_KEYS;
use super::render::{v2_page, V2Page};
use super::{query_last, QueryMap};
use crate::domain_hooks::{AppMoveAdmissionCache, AppMovePostCommitHook, AppTaskEnqueueHook};
use crate::issue_transition::AppTransitionHook;
use crate::middleware::body_limit::read_request_bytes;
use crate::orchestration::SpanCreationDeps;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json::{
    parse_json_text, to_serde_publish, JsonFail, JSON_PARSE_PREFIX,
};
use pidash_compat::datetime::{render_date, render_datetime_in, render_uuid_opt, uuid_string};
use pidash_compat::response::escape_u2028 as escape_json_text;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// `GET .../issues/<issue_id>/versions/` (`urls/issue.py:279-283`).
pub const VERSIONS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/";
/// `GET .../issues/<issue_id>/versions/<pk>/` (`urls/issue.py:284-288`).
pub const VERSION_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/{pk}/";
/// `GET .../work-items/<work_item_id>/description-versions/` (`:289-293`).
pub const DESC_VERSIONS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/";
/// `GET .../work-items/<work_item_id>/description-versions/<pk>/` (`:294-298`).
pub const DESC_VERSION_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/{pk}/";
/// `POST .../work-items/<pk>/move/` (`:309-313`,
/// `http_method_names=["post"]`).
pub const MOVE_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/";

/// Register the five versions/move routes. Nothing else: sibling paths
/// stay unmatched and proxy to Django.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            VERSIONS_PATH,
            edge::owned(axum::routing::get(version_list), &["GET"]),
        )
        .route(
            VERSION_PATH,
            edge::owned(axum::routing::get(version_detail), &["GET"]),
        )
        .route(
            DESC_VERSIONS_PATH,
            edge::owned(axum::routing::get(desc_version_list), &["GET"]),
        )
        .route(
            DESC_VERSION_PATH,
            edge::owned(axum::routing::get(desc_version_detail), &["GET"]),
        )
        .route(
            MOVE_PATH,
            edge::owned(axum::routing::post(move_issue), &["POST"]),
        )
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug)]
enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` body.
    Forbidden,
    /// 403, the description-versions guest-view gate
    /// (`version.py:102-105`).
    GuestView,
    /// 404, `handle_exception`'s `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, `{"detail":"Project not found"}` (project-kwarg rewrite miss
    /// and the move's `Project.resolve` miss — DRF propagates `Http404`
    /// args via `NotFound(*exc.args)`).
    ProjectNotFound,
    /// 400, `{"detail": ...}` (JSON `ParseError`).
    BadDetail(String),
    /// 400/403/409, `{"error": ...}` (the move's `IssueMoveError` mapping).
    MoveRejection { status: u16, message: String },
    /// 415, `UnsupportedMediaType` (content negotiation).
    UnsupportedMediaType(String),
    /// 502, truncated request-body read (the shared bad-gateway shape).
    TruncatedBody,
    /// 500, generic branch.
    ServerError,
}

/// The description-versions guest-view refusal
/// (`version.py:102-105`).
const GUEST_VIEW_BODY: &str = r#"{"error":"You are not allowed to view this issue"}"#;

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                super::UNAUTHENTICATED_BODY.to_owned(),
            ),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                crate::permissions::PERMISSION_DENIED_BODY.to_owned(),
            ),
            Denial::GuestView => (StatusCode::FORBIDDEN, GUEST_VIEW_BODY.to_owned()),
            Denial::NotFound => (StatusCode::NOT_FOUND, super::NOT_FOUND_BODY.to_owned()),
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                super::PROJECT_NOT_FOUND_BODY.to_owned(),
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
            Denial::TruncatedBody => (
                StatusCode::BAD_GATEWAY,
                crate::edge::BAD_GATEWAY_BODY.to_owned(),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                super::SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        if matches!(self, Denial::ServerError) {
            tracing::warn!("app_issues versions/move handler: internal error");
        }
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(escape_json_text(&body)))
            .expect("versions/move error response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Render `body` (already exact JSON bytes) with an explicit status,
/// escaped through the canonical `escape_u2028` (OVL-04).
fn json_response(status: StatusCode, body: String) -> Response {
    pidash_compat::response::json_response(status, escape_json_text(&body))
}

type HandlerResult = Result<Response, Denial>;

// ---------------------------------------------------------------------------
// Request plumbing: auth + project rewrite + permission gates
// ---------------------------------------------------------------------------

/// `request.user` from the Django session (`_auth_user_id`). No session,
/// no key, a non-UUID id, or a session pointing at no user row means
/// anonymous → 401. (Django PKs are UUIDs; a session id that is not a
/// UUID cannot be a user.)
async fn actor_user_id(
    pool: &PgPool,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Uuid, Denial> {
    let handle = extension.ok_or(Denial::Unauthorized)?.0;
    let mut session = handle.snapshot();
    let raw = session
        .get("_auth_user_id")
        .and_then(|value| value.as_str().to_owned())
        .ok_or(Denial::Unauthorized)?;
    let id = raw.parse::<Uuid>().map_err(|_| Denial::Unauthorized)?;
    let exists: Option<Uuid> = users::Entity::find()
        .select_only()
        .column(users::Column::Id)
        .filter(users::Column::Id.eq(id))
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    if exists.is_none() {
        return Err(Denial::Unauthorized);
    }
    Ok(id)
}

/// `_rewrite_project_kwarg` (`app/views/base.py:49-80`): UUIDs pass
/// through unchecked; other identifiers resolve `UPPER(identifier)` in
/// the workspace, else `Http404("Project not found")`.
async fn resolve_project_id(pool: &PgPool, slug: &str, raw: &str) -> Result<Uuid, Denial> {
    if let Ok(id) = raw.parse::<Uuid>() {
        return Ok(id);
    }
    let upper = raw.trim().to_uppercase();
    access::project_id_by_identifier(&orm(pool), slug, &upper)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ProjectNotFound)
}

/// Parse a `<uuid:>` path segment the way Django's `UUIDConverter` does:
/// lowercase hex, hyphenated, 36 chars (`django/urls/converters.py:25`).
/// Uppercase / simple / braced / `urn:` spellings never route in Django
/// (resolver 404), so they proxy here instead of serving.
fn path_uuid(raw: &str) -> Result<Uuid, ()> {
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
    raw.parse::<Uuid>().map_err(|_| ())
}

/// `allow_permission(allowed_roles)` at `PROJECT` level
/// (`app/permissions/base.py:19-86`) through the shared
/// [`access::allows`] gate: an allowed active project role, or any active
/// project membership plus an active workspace ADMIN membership
/// (soft-deleted rows excluded, `SoftDeletionManager`). Denies with the
/// decorator `error` body.
async fn check_allow(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
    allowed: &[i32],
) -> Result<(), Denial> {
    let allowed = access::allows(&orm(pool), slug, *project_id, *user_id, allowed)
        .await
        .map_err(|_| Denial::ServerError)?;
    if allowed {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

/// View-body tenant facts: the actor's render timezone
/// (`TimezoneMixin` activation; a bad zone or a missing user row is a
/// 500). Project existence is per-path (only the reads whose Python
/// calls `Project.objects.get` 404 on a missing row).
struct Tenant {
    timezone: Tz,
}

async fn fetch_tenant(pool: &PgPool, user_id: &Uuid) -> Result<Tenant, Denial> {
    let timezone_name = access::user_timezone(&orm(pool), *user_id)
        .await
        .map_err(|_| Denial::ServerError)?;
    let timezone: Tz = timezone_name
        .ok_or(Denial::ServerError)?
        .parse()
        .map_err(|_| Denial::ServerError)?;
    Ok(Tenant { timezone })
}

/// The pool behind the request. `None` until the binary connects, so a
/// missing pool is a 500, never a panic.
fn pool_of(state: &AppState) -> Result<PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// A SeaORM connection over the request pool (no I/O).
fn orm(pool: &PgPool) -> DatabaseConnection {
    pidash_db::pool::sea_orm_connection(pool)
}

// ---------------------------------------------------------------------------
// Datetime rendering
// ---------------------------------------------------------------------------

/// Render a timestamptz exactly like DRF `DateTimeField` with the default
/// `iso-8601` format: ISO-8601 in the request's zone with `+00:00`
/// rewritten to `Z` (serializer kernel rule — DRF converts aware values
/// to the current timezone, `fields.py:1168-1231`, which `TimezoneMixin`
/// set to the actor's).
fn render_dt(dt: &DateTime<Utc>, tz: &Tz) -> String {
    render_datetime_in(dt, tz)
}

fn render_dt_opt(dt: Option<DateTime<Utc>>, tz: &Tz) -> Option<String> {
    dt.map(|dt| render_dt(&dt, tz))
}

/// Render a timestamptz exactly like DRF's `JSONEncoder` (the `.values()`
/// list rows, which bypass `DateTimeField`): the stored instant as-is,
/// `+00:00` rewritten to `Z` (`encoders.py:28-32` — no timezone
/// conversion, unlike the field path above).
fn render_dt_utc(dt: &DateTime<Utc>) -> String {
    crate::serializer::render_datetime(dt)
}

/// A finite `sort_order` for the wire; a non-finite one answers 500
/// (DRF renders with `allow_nan=False` under the default
/// `STRICT_JSON=True`, so `ValueError` escapes into the generic 500).
fn render_sort_order(value: f64) -> Result<f64, Denial> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Denial::ServerError)
    }
}

// ---------------------------------------------------------------------------
// Issue versions (`version.py:27-74`)
// ---------------------------------------------------------------------------

/// `GET .../issues/<issue_id>/versions/`: the gate, the count, the
/// `paginate` page over the model's `-created_at` order, the 10-key rows
/// with `created_at`/`updated_at` shifted to the actor's zone. No parent
/// checks — a missing issue is an empty 200 page.
async fn version_list(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let Ok(issue_id) = path_uuid(&issue_raw) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_allow(&pool, &slug, &project_id, &user_id, &[20, 15, 5]).await?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    let total = version_count(&pool, &slug, &project_id, &issue_id).await?;
    let cursor = query_last(&query, "cursor");
    let page = v2_page(cursor.as_deref(), total).map_err(|_| Denial::ServerError)?;
    let rows = version_page_query(&slug, project_id, issue_id, &page)?
        .into_model::<VersionPageRow>()
        .all(&orm(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        results.push(version_page_row(row, &tenant.timezone)?);
    }
    Ok(json_response(
        StatusCode::OK,
        version_envelope(&page, total, results),
    ))
}

/// `GET .../issues/<issue_id>/versions/<pk>/`: the gate, then the full
/// row through the 640 detail rendering. A miss (no row, or a row
/// outside the slug/project/issue scope) is the 404.
async fn version_detail(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let (Ok(issue_id), Ok(version_id)) = (path_uuid(&issue_raw), path_uuid(&pk_raw)) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_allow(&pool, &slug, &project_id, &user_id, &[20, 15, 5]).await?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    let row = version_detail_query(&slug, project_id, issue_id, version_id)
        .one(&orm(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let body = render_version_detail(&row, &tenant.timezone)?;
    Ok(json_response(StatusCode::OK, body))
}

/// `paginate`'s `base_queryset.count()`.
async fn version_count(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<i64, Denial> {
    let total = version_scope(slug, *project_id, *issue_id)
        .count(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    i64::try_from(total).map_err(|_| Denial::ServerError)
}

/// The issue-version list scope (`version.py:61-74`): live rows of this
/// issue + project under the workspace slug (`INNER JOIN workspaces`).
fn version_scope(slug: &str, project_id: Uuid, issue_id: Uuid) -> Select<issue_versions::Entity> {
    issue_versions::Entity::find()
        .join(
            JoinType::InnerJoin,
            issue_versions::Relation::Workspaces.def(),
        )
        .filter(issue_versions::Column::DeletedAt.is_null())
        .filter(issue_versions::Column::IssueId.eq(issue_id))
        .filter(issue_versions::Column::ProjectId.eq(project_id))
        .filter(workspaces::Column::Slug.eq(slug))
}

/// The issue-version page (`version.py:61-74`): the 10
/// `required_fields` over the model's `-created_at` ordering (no explicit
/// `order_by`), sliced per [`V2Page`].
fn version_page_query(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    page: &V2Page,
) -> Result<Select<issue_versions::Entity>, Denial> {
    paged(
        version_scope(slug, project_id, issue_id)
            .select_only()
            .columns(VERSION_PAGE_COLUMNS)
            .order_by_desc(issue_versions::Column::CreatedAt),
        page,
    )
}

/// The description-version page (`version.py:133-144`): the same 10
/// columns over the endpoint's explicit `.order_by("-created_at")` (the
/// model has no default ordering, so the call is load-bearing).
fn desc_version_page_query(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    page: &V2Page,
) -> Result<Select<issue_description_versions::Entity>, Denial> {
    paged(
        desc_version_scope(slug, project_id, issue_id)
            .select_only()
            .columns(DESC_VERSION_PAGE_COLUMNS)
            .order_by_desc(issue_description_versions::Column::CreatedAt),
        page,
    )
}

/// The issue-version detail (`version.py:38-44`): the full row under the
/// list scope plus the pk, model `-created_at` ordering. `.one()` reads
/// `LIMIT 1`, matching Django's `.get()` on a unique pk.
fn version_detail_query(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    version_id: Uuid,
) -> Select<issue_versions::Entity> {
    version_scope(slug, project_id, issue_id)
        .filter(issue_versions::Column::Id.eq(version_id))
        .order_by_desc(issue_versions::Column::CreatedAt)
}

/// The description-version scope (`version.py:133-144`), same predicates
/// as [`version_scope`] over `issue_description_versions`.
fn desc_version_scope(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
) -> Select<issue_description_versions::Entity> {
    issue_description_versions::Entity::find()
        .join(
            JoinType::InnerJoin,
            issue_description_versions::Relation::Workspaces.def(),
        )
        .filter(issue_description_versions::Column::DeletedAt.is_null())
        .filter(issue_description_versions::Column::IssueId.eq(issue_id))
        .filter(issue_description_versions::Column::ProjectId.eq(project_id))
        .filter(workspaces::Column::Slug.eq(slug))
}

/// The description-version detail (`version.py:107-116`).
/// `IssueDescriptionVersion` declares no `Meta.ordering`, so there is no
/// `ORDER BY` (unlike its version sibling) — the asymmetry is ported.
fn desc_version_detail_query(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    version_id: Uuid,
) -> Select<issue_description_versions::Entity> {
    desc_version_scope(slug, project_id, issue_id)
        .filter(issue_description_versions::Column::Id.eq(version_id))
}

/// The [`VERSION_PAGE_KEYS`] columns with FK attnames, in call order —
/// both version lists select the same `required_fields`.
const VERSION_PAGE_COLUMNS: [issue_versions::Column; 10] = [
    issue_versions::Column::Id,
    issue_versions::Column::WorkspaceId,
    issue_versions::Column::ProjectId,
    issue_versions::Column::IssueId,
    issue_versions::Column::LastSavedAt,
    issue_versions::Column::OwnedById,
    issue_versions::Column::CreatedAt,
    issue_versions::Column::UpdatedAt,
    issue_versions::Column::CreatedById,
    issue_versions::Column::UpdatedById,
];

/// [`VERSION_PAGE_COLUMNS`] over `issue_description_versions`.
const DESC_VERSION_PAGE_COLUMNS: [issue_description_versions::Column; 10] = [
    issue_description_versions::Column::Id,
    issue_description_versions::Column::WorkspaceId,
    issue_description_versions::Column::ProjectId,
    issue_description_versions::Column::IssueId,
    issue_description_versions::Column::LastSavedAt,
    issue_description_versions::Column::OwnedById,
    issue_description_versions::Column::CreatedAt,
    issue_description_versions::Column::UpdatedAt,
    issue_description_versions::Column::CreatedById,
    issue_description_versions::Column::UpdatedById,
];

/// The `paginate` slice for a [`V2Page`]: `LIMIT end - start`, plus
/// `OFFSET start` past the first row. A negative length (a negative
/// cursor page size) is the generic 500, as Postgres rejecting
/// `LIMIT -n` was.
fn paged<E: EntityTrait>(select: Select<E>, page: &V2Page) -> Result<Select<E>, Denial> {
    let length = u64::try_from(page.end - page.start).map_err(|_| Denial::ServerError)?;
    let start = u64::try_from(page.start).map_err(|_| Denial::ServerError)?;
    let select = select.limit(length);
    Ok(if start == 0 {
        select
    } else {
        select.offset(start)
    })
}

/// One `.values(*required_fields)` version row, decoded by name. Every
/// value reads DRF-nullable.
#[derive(Debug, Clone, Copy, FromQueryResult)]
struct VersionPageRow {
    id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    project_id: Option<Uuid>,
    issue_id: Option<Uuid>,
    last_saved_at: Option<DateTimeWithTimeZone>,
    owned_by_id: Option<Uuid>,
    created_at: Option<DateTimeWithTimeZone>,
    updated_at: Option<DateTimeWithTimeZone>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
}

fn utc(at: DateTimeWithTimeZone) -> DateTime<Utc> {
    at.with_timezone(&Utc)
}

/// One `.values(*required_fields)` row in [`VERSION_PAGE_KEYS`] order.
/// Every value renders DRF-nullable (a NULL reads `null`, never 500s);
/// `created_at`/`updated_at` shift to the actor's zone (the explicit
/// `user_timezone_converter` pass), `last_saved_at` renders as stored
/// (the JSON-encoder path converts nothing).
fn version_page_row(row: &VersionPageRow, tz: &Tz) -> Result<Value, Denial> {
    let VersionPageRow {
        id,
        workspace_id,
        project_id,
        issue_id,
        last_saved_at,
        owned_by_id,
        created_at,
        updated_at,
        created_by_id,
        updated_by_id,
    } = *row;
    let (last_saved_at, created_at, updated_at) = (
        last_saved_at.map(utc),
        created_at.map(utc),
        updated_at.map(utc),
    );
    let last_saved_at = last_saved_at
        .as_ref()
        .map(render_dt_utc)
        .map(Value::String)
        .unwrap_or(Value::Null);
    let created_at = render_dt_opt(created_at, tz)
        .map(Value::String)
        .unwrap_or(Value::Null);
    let updated_at = render_dt_opt(updated_at, tz)
        .map(Value::String)
        .unwrap_or(Value::Null);
    // Iterate the fixture's key list so the row order is structural,
    // not a second literal that could drift from `required_fields`.
    let mut map = Map::with_capacity(VERSION_PAGE_KEYS.len());
    for &key in VERSION_PAGE_KEYS.iter() {
        let value = match key {
            "id" => render_uuid_opt(&id),
            "workspace" => render_uuid_opt(&workspace_id),
            "project" => render_uuid_opt(&project_id),
            "issue" => render_uuid_opt(&issue_id),
            "last_saved_at" => last_saved_at.clone(),
            "owned_by" => render_uuid_opt(&owned_by_id),
            "created_at" => created_at.clone(),
            "updated_at" => updated_at.clone(),
            "created_by" => render_uuid_opt(&created_by_id),
            "updated_by" => render_uuid_opt(&updated_by_id),
            _ => unreachable!("VERSION_PAGE_KEYS"),
        };
        map.insert(key.to_owned(), value);
    }
    Ok(Value::Object(map))
}

/// The 9-key `paginate` envelope (`global_paginator.py:75-85`), in dict
/// order. `page_count` is this page's row count.
fn version_envelope(page: &V2Page, total_results: i64, results: Vec<Value>) -> String {
    let mut map = Map::with_capacity(9);
    map.insert(
        "prev_cursor".to_owned(),
        Value::String(page.prev_cursor.clone()),
    );
    map.insert("cursor".to_owned(), Value::String(page.cursor.clone()));
    map.insert(
        "next_cursor".to_owned(),
        page.next_cursor
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    map.insert(
        "prev_page_results".to_owned(),
        Value::Bool(page.prev_page_results),
    );
    map.insert(
        "next_page_results".to_owned(),
        Value::Bool(page.next_page_results),
    );
    map.insert(
        "page_count".to_owned(),
        Value::Number((results.len() as i64).into()),
    );
    map.insert(
        "total_results".to_owned(),
        Value::Number(total_results.into()),
    );
    map.insert(
        "total_pages".to_owned(),
        Value::Number(page.total_pages.into()),
    );
    map.insert("results".to_owned(), Value::Array(results));
    serde_json::to_string(&Value::Object(map)).expect("version envelope")
}

/// The full version row through the 640 detail rendering. All datetimes
/// render in the request's zone (DRF `DateTimeField`); dates render
/// bare; the UUID arrays render hyphenated strings.
fn render_version_detail(row: &issue_versions::Model, tz: &Tz) -> Result<String, Denial> {
    let id = uuid_string(&row.id);
    let workspace = uuid_string(&row.workspace_id);
    let project = uuid_string(&row.project_id);
    let issue = uuid_string(&row.issue_id);
    let parent = row.parent.map(|id| uuid_string(&id));
    let state = row.state.map(|id| uuid_string(&id));
    let estimate_point = row.estimate_point.map(|id| uuid_string(&id));
    let name = &row.name;
    let priority = &row.priority;
    let start_date = row.start_date.as_ref().map(render_date);
    let target_date = row.target_date.as_ref().map(render_date);
    let assignees: Vec<String> = row.assignees.iter().map(uuid_string).collect();
    let labels: Vec<String> = row.labels.iter().map(uuid_string).collect();
    let modules: Vec<String> = row.modules.iter().map(uuid_string).collect();
    let assignees_ref: Vec<&str> = assignees.iter().map(String::as_str).collect();
    let labels_ref: Vec<&str> = labels.iter().map(String::as_str).collect();
    let modules_ref: Vec<&str> = modules.iter().map(String::as_str).collect();
    let sequence_id = row.sequence_id;
    let sort_order = render_sort_order(row.sort_order)?;
    let completed_at = render_dt_opt(row.completed_at.map(utc), tz);
    let archived_at = row.archived_at.as_ref().map(render_date);
    let is_draft = row.is_draft;
    let external_source = row.external_source.clone();
    let external_id = row.external_id.clone();
    let r#type = row.r#type.map(|id| uuid_string(&id));
    let cycle = row.cycle.map(|id| uuid_string(&id));
    let meta = &row.meta;
    let last_saved_at = render_dt(&utc(row.last_saved_at), tz);
    let owned_by = uuid_string(&row.owned_by_id);
    let created_at = render_dt_opt(Some(utc(row.created_at)), tz);
    let updated_at = render_dt_opt(Some(utc(row.updated_at)), tz);
    let created_by = row.created_by_id.map(|id| uuid_string(&id));
    let updated_by = row.updated_by_id.map(|id| uuid_string(&id));
    let row = IssueVersionDetailRow {
        id: &id,
        workspace: &workspace,
        project: &project,
        issue: &issue,
        parent: parent.as_deref(),
        state: state.as_deref(),
        estimate_point: estimate_point.as_deref(),
        name,
        priority,
        start_date: start_date.as_deref(),
        target_date: target_date.as_deref(),
        assignees: &assignees_ref,
        sequence_id,
        labels: &labels_ref,
        sort_order,
        completed_at: completed_at.as_deref(),
        archived_at: archived_at.as_deref(),
        is_draft,
        external_source: external_source.as_deref(),
        external_id: external_id.as_deref(),
        r#type: r#type.as_deref(),
        cycle: cycle.as_deref(),
        modules: &modules_ref,
        meta,
        last_saved_at: &last_saved_at,
        owned_by: &owned_by,
        created_at: created_at.as_deref(),
        updated_at: updated_at.as_deref(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
    };
    let view = issue_version_detail_to_representation(&row);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Description versions (`version.py:77-144`)
// ---------------------------------------------------------------------------

/// `GET .../work-items/<work_item_id>/description-versions/`: the parent
/// `Project.get` + `Issue.get`, the guest-view gate, then the `paginate`
/// page over the explicit `.order_by("-created_at")` list.
async fn desc_version_list(
    State(state): State<AppState>,
    Path((slug, project_raw, work_item_raw)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let Ok(work_item_id) = path_uuid(&work_item_raw) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_allow(&pool, &slug, &project_id, &user_id, &[20, 15, 5]).await?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    desc_version_parents(&pool, &slug, &project_id, &work_item_id, &user_id).await?;
    let total = desc_version_count(&pool, &slug, &project_id, &work_item_id).await?;
    let cursor = query_last(&query, "cursor");
    let page = v2_page(cursor.as_deref(), total).map_err(|_| Denial::ServerError)?;
    let rows = desc_version_page_query(&slug, project_id, work_item_id, &page)?
        .into_model::<VersionPageRow>()
        .all(&orm(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        results.push(version_page_row(row, &tenant.timezone)?);
    }
    Ok(json_response(
        StatusCode::OK,
        version_envelope(&page, total, results),
    ))
}

/// `GET .../work-items/<work_item_id>/description-versions/<pk>/`: the
/// parents + gate, then the full row through the 640 rendering.
async fn desc_version_detail(
    State(state): State<AppState>,
    Path((slug, project_raw, work_item_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let (Ok(work_item_id), Ok(version_id)) = (path_uuid(&work_item_raw), path_uuid(&pk_raw)) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_allow(&pool, &slug, &project_id, &user_id, &[20, 15, 5]).await?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    desc_version_parents(&pool, &slug, &project_id, &work_item_id, &user_id).await?;
    let row = desc_version_detail_query(&slug, project_id, work_item_id, version_id)
        .one(&orm(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let body = render_desc_version_detail(&row, &tenant.timezone)?;
    Ok(json_response(StatusCode::OK, body))
}

/// The description-versions view preamble (`version.py:88-105`):
/// `Project.objects.get(pk)` (no slug scope, soft-delete scope only),
/// `Issue.objects.get(workspace, project, pk)` (soft-delete scope only
/// — triage/archived/draft rows stay visible here), then the guest-view
/// gate: an active GUEST membership + `not guest_view_all_features` +
/// a foreign `created_by` refuses 403.
async fn desc_version_parents(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    work_item_id: &Uuid,
    user_id: &Uuid,
) -> Result<(), Denial> {
    let db = orm(pool);
    let Some((_, guest_view_all)) = access::project_tenant(&db, *project_id)
        .await
        .map_err(|_| Denial::ServerError)?
    else {
        return Err(Denial::NotFound);
    };
    let Some(issue_created_by) = desc_version_issue_creator(&db, slug, *project_id, *work_item_id)
        .await
        .map_err(|_| Denial::ServerError)?
    else {
        return Err(Denial::NotFound);
    };
    let is_guest = access::project_role(&db, slug, *project_id, *user_id)
        .await
        .map_err(|_| Denial::ServerError)?
        == Some(5);
    if is_guest && !guest_view_all && issue_created_by != Some(*user_id) {
        return Err(Denial::GuestView);
    }
    Ok(())
}

/// `Issue.objects.get(workspace__slug, project_id, pk)` for the
/// description-version preamble: the live issue's `created_by_id`
/// (outer `None` is the miss; triage/archived/draft rows stay visible).
async fn desc_version_issue_creator(
    db: &impl ConnectionTrait,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
) -> Result<Option<Option<Uuid>>, sea_orm::DbErr> {
    issues::Entity::find()
        .select_only()
        .column(issues::Column::CreatedById)
        .join(JoinType::InnerJoin, issues::Relation::Workspaces.def())
        .filter(issues::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issues::Column::ProjectId.eq(project_id))
        .filter(issues::Column::Id.eq(issue_id))
        .into_tuple()
        .one(db)
        .await
}

/// `paginate`'s `base_queryset.count()` for description versions.
async fn desc_version_count(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    work_item_id: &Uuid,
) -> Result<i64, Denial> {
    let total = desc_version_scope(slug, *project_id, *work_item_id)
        .count(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    i64::try_from(total).map_err(|_| Denial::ServerError)
}

/// The full description-version row through the 640 rendering
/// (`description_binary` re-encodes base64 inside the port).
fn render_desc_version_detail(
    row: &issue_description_versions::Model,
    tz: &Tz,
) -> Result<String, Denial> {
    let id = uuid_string(&row.id);
    let workspace = uuid_string(&row.workspace_id);
    let project = uuid_string(&row.project_id);
    let issue = uuid_string(&row.issue_id);
    let last_saved_at = render_dt(&utc(row.last_saved_at), tz);
    let owned_by = uuid_string(&row.owned_by_id);
    let created_at = render_dt_opt(Some(utc(row.created_at)), tz);
    let updated_at = render_dt_opt(Some(utc(row.updated_at)), tz);
    let created_by = row.created_by_id.map(|id| uuid_string(&id));
    let updated_by = row.updated_by_id.map(|id| uuid_string(&id));
    let row = IssueDescriptionVersionDetailRow {
        id: &id,
        workspace: &workspace,
        project: &project,
        issue: &issue,
        description_binary: row.description_binary.as_deref(),
        description_html: &row.description_html,
        description_stripped: row.description_stripped.as_deref(),
        description_json: &row.description_json,
        last_saved_at: &last_saved_at,
        owned_by: &owned_by,
        created_at: created_at.as_deref(),
        updated_at: updated_at.as_deref(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
    };
    let view = issue_description_version_detail_to_representation(&row);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Move (`move.py:17-46`)
// ---------------------------------------------------------------------------

/// Move body shape for the shared negotiator: a single scalar,
/// last-value-wins; uploads are dropped (the view reads `request.data`
/// only, and no contract body is multipart).
const MOVE_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &[],
};

/// `POST .../work-items/<pk>/move/`: the ADMIN/MEMBER gate, the body
/// read, the pre-save snapshot, `domain::issues::move_to_project`
/// inside a write transaction (the 650 kernel over the SeaORM move
/// store, with the creation-outbox drain, the post-commit
/// cancel/drain, the 594 fire, the completion comment and the
/// enqueues folded into the commit chain), then the refreshed issue
/// in the 22-key APP read shape.
async fn move_issue(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let Ok(pk) = path_uuid(&pk_raw) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_allow(&pool, &slug, &project_id, &user_id, &[20, 15]).await?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    let target_ref = read_move_target(req).await?;
    let origin = move_origin(&state)?;
    let web_base_url = move_web_base_url(&state);
    let now = Utc::now();
    // The pre-save snapshot, before the move's own write.
    let prev = crate::issue_transition::capture_prior_state_pool(&pool, Some(pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = WriteCtx::new(WriteActor {
        user_id,
        timezone: Some(tenant.timezone.name().to_owned()),
    })
    .with_scope(WriteScope {
        workspace_id: None,
        project_id: Some(project_id),
    });
    let transition: Arc<dyn IssueTransitionHook> = Arc::new(AppTransitionHook::new(&state, &pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pool));
    let span_deps = SpanCreationDeps::from_state(&state);
    let drain: Arc<dyn MovePostCommitHook> = Arc::new(AppMovePostCommitHook::new(
        &pool,
        &state,
        span_deps.cache.clone(),
    ));
    let creation = CreationDeps {
        pool: pool.clone(),
        cloud: span_deps.cloud.clone(),
        managed: span_deps.managed.clone(),
        now_unix_secs: span_deps.now_unix_secs,
        toolsets: span_deps.toolsets.clone(),
        assistant_seam: span_deps.assistant_seam.clone(),
        admission: Arc::new(AppMoveAdmissionCache::new(span_deps.cache)),
    };
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
                    slug: &slug,
                    project_id,
                    pk,
                    target_ref: &target_ref,
                    origin: Some(origin.clone()),
                    web_base_url: web_base_url.as_deref(),
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
        MoveDomainOutcome::AlreadyThere { row, .. } => {
            // Same project: no writes at all, no signals, no enqueues —
            // just the issue in the read shape.
            let body = render_moved_issue(&pool, &row, &tenant.timezone).await?;
            Ok(json_response(StatusCode::OK, body))
        }
        MoveDomainOutcome::Moved { row, chain } => {
            if chain.stopped().is_some() {
                return Err(Denial::ServerError);
            }
            let body = render_moved_issue(&pool, &row.row, &tenant.timezone).await?;
            Ok(json_response(StatusCode::OK, body))
        }
        MoveDomainOutcome::Rejected(rejection) => Err(map_move_rejection(rejection)),
    }
}

/// The move's `target_ref` (`move.py:33-39`): `request.data` coerced to
/// `{}` when it is not a dict, then `.get("project")` (missing reads
/// JSON null, which the driver blanks to the required-400).
async fn read_move_target(req: axum::extract::Request) -> Result<Value, Denial> {
    let (parts, body) = req.into_parts();
    let bytes = read_request_bytes(body)
        .await
        .map_err(|_| Denial::TruncatedBody)?;
    match shared_body::negotiate_body(&parts.headers, &bytes, &MOVE_BODY_SPEC) {
        Ok(shared_body::NegotiatedBody::Empty) => Ok(Value::Null),
        Ok(shared_body::NegotiatedBody::JsonText { text, .. }) => {
            match parse_json_text(&text) {
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
            }
        }
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

/// `base_host(request, is_app=True)`: `APP_BASE_URL` when set, else the
/// `WEB_URL or APP_BASE_URL` origin — unset everywhere is a 500
/// (`ImproperlyConfigured`). Empty strings fall through like Python's
/// `or`, as in the v1 twin.
fn move_origin(state: &AppState) -> Result<String, Denial> {
    let urls = &state.settings().urls;
    urls.app_base_url
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| urls.web_url.clone().filter(|s| !s.is_empty()))
        .ok_or(Denial::ServerError)
}

/// `web_base_url()`: `WEB_URL`, falling back to `APP_BASE_URL`, trailing
/// slash stripped — `None` when neither is configured, so the
/// `current_instance` render omits the url. Empty strings fall through
/// like Python's `or`, as in the v1 twin.
fn move_web_base_url(state: &AppState) -> Option<String> {
    let urls = &state.settings().urls;
    urls.web_url
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| urls.app_base_url.clone().filter(|s| !s.is_empty()))
        .map(|base| base.trim_end_matches('/').to_owned())
}

/// Map the 650 driver's failures onto the view's responses: the
/// recoverable `{"error"}` + status, the source-miss 404, the
/// resolve-miss 404 detail, anything else the 500.
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
        // Unreachable: this driver passes `TargetOnly` (the view checked
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

// ---------------------------------------------------------------------------
// Move response
// ---------------------------------------------------------------------------

/// The move's 200 body: `IssueSerializer(issue).data` over the
/// refreshed (or same-project) issue — a bare instance, so the 7
/// queryset-annotation keys omit via `SkipField` and the 639 base shape
/// renders 24 keys. Datetimes render in the request's zone (DRF
/// `DateTimeField`); `archived_at` renders a bare date; `is_synced`
/// answers the merged predicate against the live sync tables.
async fn render_moved_issue(
    pool: &PgPool,
    issue: &SourceIssueRow,
    tz: &Tz,
) -> Result<String, Denial> {
    let sort_order = render_sort_order(issue.sort_order)?;
    let completed_at = render_dt_opt(issue.completed_at, tz);
    let created_at = render_dt(&issue.created_at, tz);
    let updated_at = render_dt(&issue.updated_at, tz);
    let archived_at = issue
        .archived_at
        .as_ref()
        .map(|at| render_date(&at.date_naive()));
    let is_synced = moved_is_synced(pool, issue).await?;
    let is_epic = moved_is_epic(pool, issue.type_id.as_deref()).await?;
    let id = uuid_string(&issue.id);
    let state_id = issue.state_id.map(|id| uuid_string(&id));
    let parent_id = issue.parent_id.map(|id| uuid_string(&id));
    let assigned_pod_id = issue.assigned_pod_id.map(|id| uuid_string(&id));
    let assigned_runner_id = issue.assigned_runner_id.map(|id| uuid_string(&id));
    let project_id = uuid_string(&issue.project_id);
    let estimate_point = issue.estimate_point_id.map(|id| uuid_string(&id));
    let created_by = issue.created_by_id.map(|id| uuid_string(&id));
    let updated_by = issue.updated_by_id.map(|id| uuid_string(&id));
    let sequence_id = i32::try_from(issue.sequence_id).map_err(|_| Denial::ServerError)?;
    let complexity_score =
        i32::try_from(issue.complexity_score).map_err(|_| Denial::ServerError)?;
    let start_date = issue.start_date.as_ref().map(render_date);
    let target_date = issue.target_date.as_ref().map(render_date);
    let row = IssueDetailBaseRow {
        id: &id,
        name: &issue.name,
        state_id: state_id.as_deref(),
        sort_order,
        completed_at: completed_at.as_deref(),
        estimate_point: estimate_point.as_deref(),
        priority: &issue.priority,
        complexity_score,
        start_date: start_date.as_deref(),
        target_date: target_date.as_deref(),
        sequence_id,
        project_id: &project_id,
        parent_id: parent_id.as_deref(),
        cycle_id: None,
        type_id: issue.type_id.as_deref(),
        is_epic,
        assigned_pod_id: assigned_pod_id.as_deref(),
        assigned_runner_id: assigned_runner_id.as_deref(),
        agent_executor: issue.agent_executor.as_deref(),
        module_ids: None,
        label_ids: None,
        assignee_ids: None,
        sub_issues_count: None,
        created_at: &created_at,
        updated_at: &updated_at,
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        attachment_count: None,
        link_count: None,
        is_draft: issue.is_draft,
        archived_at: archived_at.as_deref(),
        is_synced,
    };
    let view = issue_detail_base_to_representation(&row);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

/// `get_is_epic` (`issue.py:1109-1112`): false when the row is untyped,
/// without a query; otherwise the type row's flag. A missing row is
/// Django's `DoesNotExist` → the generic 500 (unreachable — the FK
/// holds). No soft-delete guard: `IssueType` rides the plain manager.
async fn moved_is_epic(pool: &PgPool, type_id: Option<&str>) -> Result<bool, Denial> {
    let Some(raw) = type_id else {
        return Ok(false);
    };
    let id = raw.parse::<Uuid>().map_err(|_| Denial::ServerError)?;
    access::issue_type_is_epic(&orm(pool), id)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ServerError)
}

/// The move's `is_synced` (`serializers/issue.py:297-323`): the bare
/// instance carries no annotation, so the merged predicate runs — no
/// probe when `external_source` is blank, else git then github.
async fn moved_is_synced(pool: &PgPool, issue: &SourceIssueRow) -> Result<bool, Denial> {
    let blank = issue
        .external_source
        .as_deref()
        .is_none_or(|source| source.trim().is_empty());
    if blank {
        return Ok(issue_is_actively_synced(
            issue.external_source.as_deref(),
            None,
            || false,
            || false,
        ));
    }
    let db = orm(pool);
    let git_hit = git_issue_synced(&db, issue.id)
        .await
        .map_err(|_| Denial::ServerError)?;
    if git_hit {
        return Ok(issue_is_actively_synced(
            issue.external_source.as_deref(),
            None,
            || true,
            || false,
        ));
    }
    let github_hit = github_issue_synced(&db, issue.id)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(issue_is_actively_synced(
        issue.external_source.as_deref(),
        None,
        || false,
        || github_hit,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_match_django_routes() {
        assert_eq!(
            VERSIONS_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/"
        );
        assert_eq!(
            VERSION_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/{pk}/"
        );
        assert_eq!(
            DESC_VERSIONS_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/"
        );
        assert_eq!(
            DESC_VERSION_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/{pk}/"
        );
        assert_eq!(
            MOVE_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/"
        );
    }

    #[test]
    fn envelope_key_order_and_empty_page() {
        let page = v2_page(None, 0).expect("empty page");
        let body = version_envelope(&page, 0, vec![]);
        let value: Value = serde_json::from_str(&body).expect("json");
        let keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "prev_cursor",
                "cursor",
                "next_cursor",
                "prev_page_results",
                "next_page_results",
                "page_count",
                "total_results",
                "total_pages",
                "results",
            ]
        );
        assert_eq!(
            body,
            // `prev_cursor` is `"1000:-1:0"` even on the empty first page
            // (Python formats `current_page - 1` unconditionally).
            r#"{"prev_cursor":"1000:-1:0","cursor":"1000:0:0","next_cursor":null,"prev_page_results":false,"next_page_results":false,"page_count":0,"total_results":0,"total_pages":0,"results":[]}"#
        );
    }

    #[test]
    fn envelope_mid_page_cursors() {
        let page = v2_page(Some("1000:2:0"), 2500).expect("page");
        let body = version_envelope(&page, 2500, vec![Value::Bool(true)]);
        assert_eq!(
            body,
            r#"{"prev_cursor":"1000:1:0","cursor":"1000:2:0","next_cursor":null,"prev_page_results":true,"next_page_results":false,"page_count":1,"total_results":2500,"total_pages":3,"results":[true]}"#
        );
    }

    #[test]
    fn envelope_first_page_next_cursor() {
        let page = v2_page(None, 1500).expect("page");
        let body = version_envelope(&page, 1500, vec![]);
        let value: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(value["next_cursor"], Value::String("1000:1:0".to_owned()));
        assert_eq!(value["next_page_results"], Value::Bool(true));
        assert_eq!(value["total_pages"], Value::Number(2.into()));
    }

    #[test]
    fn bad_cursors_answer_server_error() {
        assert!(v2_page(Some("nope"), 10).is_err());
        assert!(v2_page(Some("0:0:0"), 10).is_err());
        assert!(v2_page(Some("1000:0"), 10).is_err());
    }

    #[test]
    fn denial_bodies() {
        let (status, body) = Denial::Unauthorized.status_and_body();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, super::super::UNAUTHENTICATED_BODY);
        let (status, body) = Denial::Forbidden.status_and_body();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, crate::permissions::PERMISSION_DENIED_BODY);
        let (status, body) = Denial::GuestView.status_and_body();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            r#"{"error":"You are not allowed to view this issue"}"#
        );
        let (status, body) = Denial::NotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, super::super::NOT_FOUND_BODY);
        let (status, body) = Denial::ProjectNotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, super::super::PROJECT_NOT_FOUND_BODY);
        let (status, body) = Denial::BadDetail("JSON parse error - x".to_owned()).status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, r#"{"detail":"JSON parse error - x"}"#);
        let (status, body) = Denial::MoveRejection {
            status: 409,
            message: "Target project has no default pod".to_owned(),
        }
        .status_and_body();
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body, r#"{"error":"Target project has no default pod"}"#);
        let (status, body) = Denial::TruncatedBody.status_and_body();
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body, crate::edge::BAD_GATEWAY_BODY);
        let (status, body) = Denial::UnsupportedMediaType("m".to_owned()).status_and_body();
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(body, r#"{"detail":"m"}"#);
        let (status, body) = Denial::ServerError.status_and_body();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body, super::super::SERVER_ERROR_BODY);
    }

    #[test]
    fn move_rejection_statuses() {
        for (status, code) in [
            (400, StatusCode::BAD_REQUEST),
            (403, StatusCode::FORBIDDEN),
            (409, StatusCode::CONFLICT),
        ] {
            let (got, _) = Denial::MoveRejection {
                status,
                message: "x".to_owned(),
            }
            .status_and_body();
            assert_eq!(got, code);
        }
    }

    #[test]
    fn json_text_escapes_line_separators() {
        assert_eq!(
            escape_json_text("a b c"),
            "a\u{2028}b\u{2029}c"
                .replace('\u{2028}', "\\u2028")
                .replace('\u{2029}', "\\u2029")
        );
        assert_eq!(escape_json_text("a\u{2028}b\u{2029}c"), "a\\u2028b\\u2029c");
        assert_eq!(escape_json_text(r#"{"a":1}"#), r#"{"a":1}"#);
    }

    #[test]
    fn sort_order_finite_only() {
        assert_eq!(render_sort_order(65535.0).expect("finite"), 65535.0);
        assert!(render_sort_order(f64::INFINITY).is_err());
        assert!(render_sort_order(f64::NAN).is_err());
    }

    #[test]
    fn path_uuid_strictness() {
        let id = "12345678-1234-1234-1234-1234567890ab";
        assert_eq!(path_uuid(id).expect("valid").to_string(), id);
        assert!(path_uuid("12345678-1234-1234-1234-1234567890AB").is_err());
        assert!(path_uuid("123456781234123412341234567890ab").is_err());
        assert!(path_uuid("{12345678-1234-1234-1234-1234567890ab}").is_err());
        assert!(path_uuid("urn:uuid:12345678-1234-1234-1234-1234567890ab").is_err());
        assert!(path_uuid("").is_err());
        assert!(path_uuid("not-a-uuid-at-all----------------").is_err());
    }

    #[test]
    fn move_error_mapping() {
        let denial = map_move_rejection(MoveRejection::SourceNotFound);
        assert!(matches!(denial, Denial::NotFound));
        let denial = map_move_rejection(MoveRejection::TargetNotFound);
        assert!(matches!(denial, Denial::ProjectNotFound));
        let denial = map_move_rejection(MoveRejection::SourceDenied);
        assert!(matches!(denial, Denial::Forbidden));
        let denial = map_move_rejection(MoveRejection::Issue {
            message: "m".to_owned(),
            status_code: 403,
        });
        match denial {
            Denial::MoveRejection { status, message } => {
                assert_eq!(status, 403);
                assert_eq!(message, "m");
            }
            other => panic!("unexpected {other:?}"),
        }
        let denial = map_move_domain_error(domain::DomainError::Work("m".to_owned()));
        match denial {
            Denial::MoveRejection { status, message } => {
                assert_eq!(status, 409);
                assert_eq!(message, "m");
            }
            other => panic!("unexpected {other:?}"),
        }
        let denial = map_move_domain_error(domain::DomainError::Db(sea_orm::DbErr::Custom(
            "x".to_owned(),
        )));
        assert!(matches!(denial, Denial::ServerError));
    }

    #[test]
    fn page_keys_cover_both_lists() {
        assert_eq!(
            VERSION_PAGE_KEYS,
            [
                "id",
                "workspace",
                "project",
                "issue",
                "last_saved_at",
                "owned_by",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
            ]
        );
    }

    #[test]
    fn datetime_renders() {
        let utc: Tz = "UTC".parse().expect("utc");
        let dt = chrono::DateTime::parse_from_rfc3339("2026-10-04T03:04:05.123456789Z")
            .expect("dt")
            .with_timezone(&Utc);
        // Sub-micro nanos truncate to micros like DRF (PIDASHCONV-867).
        assert_eq!(render_dt(&dt, &utc), "2026-10-04T03:04:05.123456Z");
        assert_eq!(render_dt_utc(&dt), "2026-10-04T03:04:05.123456Z");
        let kolkata: Tz = "Asia/Kolkata".parse().expect("tz");
        assert_eq!(render_dt(&dt, &kolkata), "2026-10-04T08:34:05.123456+05:30");
        assert_eq!(render_dt_utc(&dt), "2026-10-04T03:04:05.123456Z");
        assert_eq!(render_dt_opt(None, &utc), None);
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 4).expect("date");
        assert_eq!(render_date(&date), "2026-10-04");
    }

    const SLUG: &str = "acme";
    const PROJECT: Uuid = Uuid::from_u128(0x10);
    const ISSUE: Uuid = Uuid::from_u128(0x20);
    const VERSION: Uuid = Uuid::from_u128(0x30);

    fn pg(select: impl sea_orm::QueryTrait) -> String {
        select.build(sea_orm::DbBackend::Postgres).to_string()
    }

    /// The version reads keep the old statements' scope, joins, ordering
    /// and slicing: `INNER JOIN workspaces`, the soft-delete guard, the
    /// model `-created_at` order, `LIMIT` alone on the first page.
    #[test]
    fn version_statement_shapes() {
        let first = v2_page(None, 2500).expect("page");
        let sql = pg(version_page_query(SLUG, PROJECT, ISSUE, &first).expect("query"));
        assert!(sql.starts_with(
            r#"SELECT "issue_versions"."id", "issue_versions"."workspace_id", "issue_versions"."project_id", "issue_versions"."issue_id", "issue_versions"."last_saved_at", "issue_versions"."owned_by_id", "issue_versions"."created_at", "issue_versions"."updated_at", "issue_versions"."created_by_id", "issue_versions"."updated_by_id" FROM "issue_versions" INNER JOIN "workspaces" ON "issue_versions"."workspace_id" = "workspaces"."id""#
        ), "{sql}");
        assert!(sql.contains(r#""issue_versions"."deleted_at" IS NULL"#));
        assert!(sql.contains(r#""workspaces"."slug" = 'acme'"#));
        assert!(
            sql.ends_with(r#"ORDER BY "issue_versions"."created_at" DESC LIMIT 1000"#),
            "{sql}"
        );
        let later = v2_page(Some("10:2:0"), 100).expect("page");
        let sql = pg(version_page_query(SLUG, PROJECT, ISSUE, &later).expect("query"));
        assert!(sql.ends_with("DESC LIMIT 10 OFFSET 20"), "{sql}");

        let detail = pg(version_detail_query(SLUG, PROJECT, ISSUE, VERSION));
        assert!(detail.starts_with(r#"SELECT "issue_versions"."created_at""#));
        assert!(detail.contains(r#""issue_versions"."meta""#));
        assert!(detail.contains(&format!(r#""issue_versions"."id" = '{VERSION}'"#)));
        assert!(
            detail.ends_with(r#"ORDER BY "issue_versions"."created_at" DESC"#),
            "{detail}"
        );
    }

    /// `IssueDescriptionVersion` has no `Meta.ordering`: the detail
    /// carries no `ORDER BY`, the list orders explicitly.
    #[test]
    fn description_version_ordering_differs_from_issue_version() {
        let detail = pg(desc_version_detail_query(SLUG, PROJECT, ISSUE, VERSION));
        assert!(!detail.contains("ORDER BY"), "{detail}");
        assert!(detail.contains(
            r#"INNER JOIN "workspaces" ON "issue_description_versions"."workspace_id" = "workspaces"."id""#
        ));
        assert!(detail.contains(r#""issue_description_versions"."deleted_at" IS NULL"#));
        let page = v2_page(None, 5).expect("page");
        let sql = pg(desc_version_page_query(SLUG, PROJECT, ISSUE, &page).expect("query"));
        assert!(
            sql.ends_with(r#"ORDER BY "issue_description_versions"."created_at" DESC LIMIT 5"#),
            "{sql}"
        );
    }

    /// A negative cursor page size made `LIMIT -n`, which Postgres
    /// rejected into the generic 500; the builder refuses it the same way.
    #[test]
    fn negative_page_length_is_server_error() {
        let page = v2_page(Some("-5:0:0"), 10).expect("page");
        assert!(matches!(
            version_page_query(SLUG, PROJECT, ISSUE, &page),
            Err(Denial::ServerError)
        ));
    }

    /// The versions/move reads against constraint-free temp tables on a
    /// one-connection pool.
    #[tokio::test]
    async fn live_version_reads() {
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
            "users",
            "workspaces",
            "projects",
            "project_members",
            "workspace_members",
            "issues",
            "issue_versions",
            "issue_description_versions",
            "issue_types",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        let id = Uuid::new_v4;
        let (ws, project, issue, actor, guest, admin) = (id(), id(), id(), id(), id(), id());
        let (v_old, v_new, v_gone, d_one, d_two, epic) = (id(), id(), id(), id(), id(), id());
        let version = |vid: Uuid, age: i32, deleted: bool| {
            format!(
                "INSERT INTO issue_versions (id, created_at, updated_at, deleted_at, name, priority, \
                 sequence_id, sort_order, is_draft, last_saved_at, owned_by_id, assignees, labels, \
                 modules, properties, meta, issue_id, project_id, workspace_id, created_by_id) \
                 VALUES ('{vid}', now() - interval '{age} min', now(), {}, 'n', 'none', 1, 65535, \
                 false, now(), '{actor}', '{{{actor}}}', '{{}}', '{{}}', '{{}}', '{{\"k\": 1}}', \
                 '{issue}', '{project}', '{ws}', '{actor}')",
                if deleted { "now()" } else { "NULL" }
            )
        };
        let desc = |did: Uuid, age: i32| {
            format!(
                "INSERT INTO issue_description_versions (id, created_at, updated_at, deleted_at, \
                 description_html, description_json, last_saved_at, owned_by_id, issue_id, \
                 project_id, workspace_id) VALUES ('{did}', now() - interval '{age} min', now(), \
                 NULL, '<p/>', '{{}}', now(), '{actor}', '{issue}', '{project}', '{ws}')"
            )
        };
        for sql in [
            format!(
                "INSERT INTO users (id, user_timezone) VALUES ('{actor}', 'Asia/Kolkata'), \
                 ('{guest}', 'UTC'), ('{admin}', 'UTC')"
            ),
            format!("INSERT INTO workspaces (id, slug) VALUES ('{ws}', 'acme')"),
            format!(
                "INSERT INTO projects (id, workspace_id, identifier, guest_view_all_features, \
                 deleted_at) VALUES ('{project}', '{ws}', 'ENG', false, NULL)"
            ),
            format!(
                "INSERT INTO project_members (id, member_id, project_id, workspace_id, role, \
                 is_active, deleted_at) VALUES ('{}', '{actor}', '{project}', '{ws}', 15, true, NULL), \
                 ('{}', '{guest}', '{project}', '{ws}', 5, true, NULL), \
                 ('{}', '{admin}', '{project}', '{ws}', 10, true, NULL)",
                id(),
                id(),
                id()
            ),
            format!(
                "INSERT INTO workspace_members (id, member_id, workspace_id, role, is_active, \
                 deleted_at) VALUES ('{}', '{admin}', '{ws}', 20, true, NULL)",
                id()
            ),
            format!(
                "INSERT INTO issues (id, workspace_id, project_id, created_by_id, deleted_at) \
                 VALUES ('{issue}', '{ws}', '{project}', '{actor}', NULL)"
            ),
            version(v_old, 2, false),
            version(v_new, 1, false),
            version(v_gone, 0, true),
            desc(d_one, 2),
            desc(d_two, 1),
            format!("INSERT INTO issue_types (id, is_epic) VALUES ('{epic}', true)"),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed");
        }

        // Tenant and gates.
        assert_eq!(
            resolve_project_id(&pool, "acme", " eng ")
                .await
                .expect("identifier"),
            project
        );
        assert!(matches!(
            resolve_project_id(&pool, "other", "ENG").await,
            Err(Denial::ProjectNotFound)
        ));
        assert_eq!(
            resolve_project_id(&pool, "x", &project.to_string())
                .await
                .expect("uuid"),
            project
        );
        check_allow(&pool, "acme", &project, &actor, &[20, 15])
            .await
            .expect("member");
        assert!(matches!(
            check_allow(&pool, "acme", &project, &guest, &[20, 15]).await,
            Err(Denial::Forbidden)
        ));
        check_allow(&pool, "acme", &project, &guest, &[20, 15, 5])
            .await
            .expect("guest");
        // A non-listed project role passes through the workspace-admin fallback.
        check_allow(&pool, "acme", &project, &admin, &[20, 15])
            .await
            .expect("admin");
        let tenant = fetch_tenant(&pool, &actor).await.expect("tenant");
        assert_eq!(tenant.timezone.name(), "Asia/Kolkata");
        assert!(matches!(
            fetch_tenant(&pool, &Uuid::new_v4()).await,
            Err(Denial::ServerError)
        ));

        // Issue versions: the soft-deleted row is out, newest first.
        let db = orm(&pool);
        assert_eq!(
            version_count(&pool, "acme", &project, &issue)
                .await
                .expect("count"),
            2
        );
        assert_eq!(
            version_count(&pool, "other", &project, &issue)
                .await
                .expect("count"),
            0
        );
        let page = v2_page(None, 2).expect("page");
        let rows = version_page_query("acme", project, issue, &page)
            .expect("query")
            .into_model::<VersionPageRow>()
            .all(&db)
            .await
            .expect("page rows");
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![Some(v_new), Some(v_old)]
        );
        let value = version_page_row(&rows[0], &tenant.timezone).expect("row");
        assert_eq!(value["workspace"], Value::String(ws.to_string()));
        assert_eq!(value["created_by"], Value::String(actor.to_string()));
        let second = v2_page(Some("1:1:0"), 2).expect("page");
        let rows = version_page_query("acme", project, issue, &second)
            .expect("query")
            .into_model::<VersionPageRow>()
            .all(&db)
            .await
            .expect("second page");
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![Some(v_old)]
        );
        let row = version_detail_query("acme", project, issue, v_new)
            .one(&db)
            .await
            .expect("detail")
            .expect("hit");
        let body: Value =
            serde_json::from_str(&render_version_detail(&row, &tenant.timezone).expect("render"))
                .expect("json");
        assert_eq!(body["id"], Value::String(v_new.to_string()));
        assert_eq!(body["assignees"], serde_json::json!([actor.to_string()]));
        assert_eq!(body["meta"], serde_json::json!({"k": 1}));
        assert!(version_detail_query("acme", project, issue, v_gone)
            .one(&db)
            .await
            .expect("detail")
            .is_none());

        // Description versions: parents, guest gate, list, detail.
        desc_version_parents(&pool, "acme", &project, &issue, &actor)
            .await
            .expect("creator");
        assert!(matches!(
            desc_version_parents(&pool, "acme", &project, &issue, &guest).await,
            Err(Denial::GuestView)
        ));
        desc_version_parents(&pool, "acme", &project, &issue, &admin)
            .await
            .expect("non-guest");
        assert!(matches!(
            desc_version_parents(&pool, "acme", &project, &Uuid::new_v4(), &actor).await,
            Err(Denial::NotFound)
        ));
        assert!(matches!(
            desc_version_parents(&pool, "acme", &Uuid::new_v4(), &issue, &actor).await,
            Err(Denial::NotFound)
        ));
        assert_eq!(
            desc_version_count(&pool, "acme", &project, &issue)
                .await
                .expect("count"),
            2
        );
        let rows = desc_version_page_query("acme", project, issue, &page)
            .expect("query")
            .into_model::<VersionPageRow>()
            .all(&db)
            .await
            .expect("desc rows");
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![Some(d_two), Some(d_one)]
        );
        let row = desc_version_detail_query("acme", project, issue, d_one)
            .one(&db)
            .await
            .expect("detail")
            .expect("hit");
        let body: Value = serde_json::from_str(
            &render_desc_version_detail(&row, &tenant.timezone).expect("render"),
        )
        .expect("json");
        assert_eq!(body["description_html"], Value::String("<p/>".to_owned()));
        assert_eq!(body["description_binary"], Value::Null);

        // Move response: the epic flag.
        assert!(moved_is_epic(&pool, Some(&epic.to_string()))
            .await
            .expect("epic"));
        assert!(!moved_is_epic(&pool, None).await.expect("untyped"));
        assert!(matches!(
            moved_is_epic(&pool, Some(&Uuid::new_v4().to_string())).await,
            Err(Denial::ServerError)
        ));
    }

    fn origin_state(app_base_url: Option<&str>, web_url: Option<&str>) -> AppState {
        let mut settings = pidash_db::config::Settings::test_defaults();
        settings.urls.app_base_url = app_base_url.map(str::to_owned);
        settings.urls.web_url = web_url.map(str::to_owned);
        AppState::with_settings("test", settings)
    }

    /// `base_host(request, is_app=True)` treats `""` as missing
    /// (PIDASHCONV-1577): an empty `APP_BASE_URL` falls through to
    /// `WEB_URL`, and empty everywhere is the 500.
    #[test]
    fn move_origin_falls_through_empty_strings() {
        let web = "https://web.example.invalid";
        let state = origin_state(Some(""), Some(web));
        assert_eq!(move_origin(&state).expect("falls through"), web);
        let state = origin_state(Some(""), Some(""));
        assert!(matches!(move_origin(&state), Err(Denial::ServerError)));
        let state = origin_state(None, None);
        assert!(matches!(move_origin(&state), Err(Denial::ServerError)));
        let state = origin_state(Some("https://app.example.invalid"), Some(web));
        assert_eq!(
            move_origin(&state).expect("app base wins"),
            "https://app.example.invalid"
        );
    }

    /// `web_base_url()` treats `""` as missing (PIDASHCONV-1577): empty
    /// everywhere is `None` (callers omit the url), and an empty
    /// `WEB_URL` falls back to `APP_BASE_URL`.
    #[test]
    fn move_web_base_url_falls_through_empty_strings() {
        let app = "https://app.example.invalid";
        let state = origin_state(Some(""), Some(""));
        assert_eq!(move_web_base_url(&state), None);
        let state = origin_state(None, None);
        assert_eq!(move_web_base_url(&state), None);
        let state = origin_state(Some(app), Some(""));
        assert_eq!(move_web_base_url(&state).as_deref(), Some(app));
        let state = origin_state(Some(app), Some("https://web.example.invalid/"));
        assert_eq!(
            move_web_base_url(&state).as_deref(),
            Some("https://web.example.invalid")
        );
    }
}
