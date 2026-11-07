//! Activity + comments + reactions + subscribers + worklogs handlers
//! (D-26 handlers-D, plus the PIDASHCONV-1676 worklog drift port).
//!
//! Ports `apps/api/pi_dash/app/views/issue/activity.py:24-86`
//! (`IssueActivityEndpoint`), `comment.py:36-258` (`IssueCommentViewSet`,
//! `CommentReactionViewSet`), `reaction.py:25-85` (`IssueReactionViewSet`),
//! `subscriber.py:16-104` (`IssueSubscriberViewSet`) and
//! `worklog.py:32-168` (`IssueWorklogViewSet`) — 6 units over the
//! 12 routes in `app/urls/issue.py:172-256`.
//!
//! Fixture ids: FX-ISS-17 (`rust-api/fixtures/app_issues/handlers/`) pins
//! every behavior below; FX-ISS-21 (`guards/`) pins the enqueue payloads.
//!
//! Layering: the gates, single-table leaves, scoped lookups and every
//! write run on SeaORM (entities in `pidash_db::entity`, decoded by name;
//! comment writes through `pidash_services::domain::comments`). The
//! history / comment-list / comment-reaction-list joins are still the
//! `super::queries_engage` builders decoded positionally (the builder
//! columns are unaliased); activity / subscriber
//! / reaction-lite shapes plus the sync predicates are
//! `pidash_services::app_issues::serializers_engage`; the member roster
//! renders through D-25's `app_project::ser_shared`
//! (`ProjectMemberLiteSerializer`); user / project / workspace lite nests
//! reuse `ser_shared` / `ser_member`. The app comment shape (33 keys with
//! `is_synced` and 12-key reaction nests) and the full issue-reaction
//! shape have no services port — the space `issue_graph` twins differ
//! (no `is_synced`, lite nests, space flats) — so they render here,
//! composed from the same lite ports. Like the sibling handler files this
//! module is self-contained (private `Denial`, private validators);
//! merges keep both sides.
//!
//! Enqueues (`issue_activity`, `model_activity` by Celery wire name, plus
//! the `soft_delete_related_objects` every instance `.delete()` fires)
//! publish best-effort through `pidash_jobs::queue`; without a queue the
//! response still stands (the cycles-handler precedent). Subscriber paths
//! enqueue nothing (no task imports in `subscriber.py`).
//!
//! No `Issue` row is written on any path here and comments carry no
//! post-save receiver (`signals.py:12`), so nothing fires the D-12
//! orchestration pair — the port calls no transition entry.
//!
//! Ported bugs (translate, don't redesign — also listed in the PR):
//!
//! * The unfiltered history branch (retired by PIDASHCONV-1680): Django
//!   `b4defc50f` serializes each side and merges the dicts sorted by
//!   `created_at`, so the old `instance["created_at"]` `TypeError` →
//!   generic 500 is gone on both backends; the merged feed answers 200.
//! * Duplicate issue reactions surface the generic `IntegrityError` body
//!   (`{"error": "The payload is not valid"}`): unlike comment reactions,
//!   the view catches nothing (`reaction.py:46-62`).
//! * The subscriber list returns the active project-member roster through
//!   `ProjectMemberLiteSerializer`, not the subscription rows
//!   (`subscriber.py:52-57`).
//! * A guest commenting on a foreign issue is refused with 400, not 403
//!   (`comment.py:86-89`).
//! * `PUT` on a comment runs DRF's default `update` (no decorator gate,
//!   no enqueues, no `edited_at` logic, missing fields untouched) while
//!   `PATCH` runs the custom `partial_update` (ADMIN + creator gate,
//!   both enqueues, conditional `edited_at`).
//! * The create response carries `updated_by` set (the description-link
//!   second save stamps the in-memory instance) while the stored row
//!   keeps `updated_by` NULL (`update_fields=["description_id"]`).
//! * `subscribe` passes no `workspace` kwarg; the save backfills it from
//!   the project (`subscriber.py:81-83`, `project.py:309-311`).
//! * The comment `PUT` response carries `is_member` (the `get_object`
//!   queryset is annotated) while create / PATCH responses omit it
//!   (plain `.get()` / fresh instance, DRF `SkipField`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono_tz::Tz;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, FromQueryResult, JoinType, QueryFilter,
    QueryOrder, QuerySelect, RelationTrait,
};
use serde_json::{Map, Value};

use super::queries_engage::{ISSUE_PREFIX, PROJECT_PREFIX, USER_PREFIX, WORKSPACE_PREFIX};
use crate::edge;
use crate::middleware::body_limit::read_request_bytes;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use pidash_compat::datetime::{render_date, render_datetime_in};
use pidash_compat::py::{py_float_repr_probed, py_str_value, python_dumps};
use pidash_compat::response::{empty_response, json_response};
use pidash_compat::validate::uuid_from_json_int;
use pidash_db::app_pages::strip::ml_strip_tags;
use pidash_db::entity::comment_hooks::description_stripped_for;
use pidash_db::entity::{
    comment_reactions, descriptions, file_assets, intake_issues, issue_activities, issue_assignees,
    issue_comments, issue_reactions, issue_subscribers, issue_worklogs, issues, project_members,
    projects, users, workspaces,
};
use pidash_db::orm_tx::is_integrity_violation;
use pidash_services::domain::comments as comment_domain;
use pidash_services::v1_projects::tasks::model_activity_kwargs;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// `IssueActivityEndpoint` (`app/urls/issue.py:172-176`).
pub const HISTORY_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/history/";
/// `IssueCommentViewSet` collection (`app/urls/issue.py:179-183`).
pub const COMMENTS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/";
/// `IssueCommentViewSet` detail (`app/urls/issue.py:184-195`).
pub const COMMENT_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/";
/// `IssueSubscriberViewSet` collection (`app/urls/issue.py:198-202`).
pub const SUBSCRIBERS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-subscribers/";
/// `IssueSubscriberViewSet` destroy (`app/urls/issue.py:203-207`). The
/// kwarg is a *user* id, not the row pk.
pub const SUBSCRIBER_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-subscribers/{subscriber_id}/";
/// `subscribe` / `unsubscribe` / `subscription_status`
/// (`app/urls/issue.py:208-212`).
pub const SUBSCRIBE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/subscribe/";
/// `IssueReactionViewSet` collection (`app/urls/issue.py:215-219`).
pub const REACTIONS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/reactions/";
/// `IssueReactionViewSet` destroy (`app/urls/issue.py:220-224`). The
/// kwarg is the reaction *string*, not a pk.
pub const REACTION_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/reactions/{reaction_code}/";
/// `CommentReactionViewSet` collection (`app/urls/issue.py:227-231`).
/// Note: no `issue_id` on these two routes.
pub const COMMENT_REACTIONS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/comments/{comment_id}/reactions/";
/// `CommentReactionViewSet` destroy (`app/urls/issue.py:232-236`).
pub const COMMENT_REACTION_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/comments/{comment_id}/reactions/{reaction_code}/";
/// `IssueWorklogViewSet` collection (`app/urls/issue.py:241-244`).
pub const WORKLOGS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/worklogs/";
/// `IssueWorklogViewSet` detail (`app/urls/issue.py:245-256`).
pub const WORKLOG_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/worklogs/{pk}/";

/// Register the twelve engage routes. Owned methods serve from Rust;
/// every other method on these paths falls through to Django (its
/// 405-after-auth, `HEAD`, and metadata responses live there).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            HISTORY_PATH,
            edge::owned(axum::routing::get(history), &["GET"]),
        )
        .route(
            COMMENTS_PATH,
            edge::owned(
                axum::routing::get(comment_list).post(comment_create),
                &["GET", "POST"],
            ),
        )
        .route(
            COMMENT_PATH,
            edge::owned(
                axum::routing::get(comment_retrieve)
                    .put(comment_update)
                    .patch(comment_partial_update)
                    .delete(comment_destroy),
                &["GET", "PUT", "PATCH", "DELETE"],
            ),
        )
        .route(
            SUBSCRIBERS_PATH,
            edge::owned(
                axum::routing::get(subscriber_list).post(subscriber_create),
                &["GET", "POST"],
            ),
        )
        .route(
            SUBSCRIBER_PATH,
            edge::owned(axum::routing::delete(subscriber_destroy), &["DELETE"]),
        )
        .route(
            SUBSCRIBE_PATH,
            edge::owned(
                axum::routing::get(subscription_status)
                    .post(subscribe)
                    .delete(unsubscribe),
                &["GET", "POST", "DELETE"],
            ),
        )
        .route(
            REACTIONS_PATH,
            edge::owned(
                axum::routing::get(issue_reaction_list).post(issue_reaction_create),
                &["GET", "POST"],
            ),
        )
        .route(
            REACTION_PATH,
            edge::owned(axum::routing::delete(issue_reaction_destroy), &["DELETE"]),
        )
        .route(
            COMMENT_REACTIONS_PATH,
            edge::owned(
                axum::routing::get(comment_reaction_list).post(comment_reaction_create),
                &["GET", "POST"],
            ),
        )
        .route(
            COMMENT_REACTION_PATH,
            edge::owned(axum::routing::delete(comment_reaction_destroy), &["DELETE"]),
        )
        .route(
            WORKLOGS_PATH,
            edge::owned(
                axum::routing::get(worklog_list).post(worklog_create),
                &["GET", "POST"],
            ),
        )
        .route(
            WORKLOG_PATH,
            edge::owned(
                axum::routing::get(worklog_retrieve)
                    .patch(worklog_partial_update)
                    .delete(worklog_destroy),
                &["GET", "PATCH", "DELETE"],
            ),
        )
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub(crate) enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` body.
    Forbidden,
    /// 403, DRF `PermissionDenied` (class-permission denials).
    ForbiddenDetail,
    /// 403, `{"error": ...}` (view-inline refusals like `_can_log`).
    ForbiddenError(String),
    /// 404, `ObjectDoesNotExist` branch (bare `.get()` misses).
    NotFound,
    /// 404, `get_object_or_404` over comments (retrieve / PUT misses).
    NotFoundDetail,
    /// 404, `get_object_or_404` over worklogs (retrieve misses).
    NotFoundWorklog,
    /// 404, `{"detail": "Project not found"}` (project-kwarg rewrite miss).
    ProjectNotFound,
    /// 400, `{"detail": ...}` (JSON `ParseError`).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline + `IntegrityError` mapping).
    BadError(String),
    /// 400, `{"message": ...}` (already-subscribed).
    BadMessage(String),
    /// 400, serializer-errors dict.
    BadJson(Value),
    /// 415, `UnsupportedMediaType` (content negotiation).
    UnsupportedMediaType(String),
    /// 502, truncated request-body read (the shared bad-gateway shape).
    TruncatedBody,
    /// 500, generic branch.
    ServerError,
}

/// DRF's default permission-denied body.
const VIEWSET_FORBIDDEN_BODY: &str =
    r#"{"detail":"You do not have permission to perform this action."}"#;
/// DRF `get_object_or_404` over comments: the `Http404` message names
/// the model (`django/shortcuts.py`), rendered under lowercase
/// `detail` — verified against live Django.
const COMMENT_NOT_FOUND_BODY: &str = r#"{"detail":"No IssueComment matches the given query."}"#;

/// DRF `get_object_or_404` over worklogs: same `Http404` shape as the
/// comment twin, naming this model — verified against live Django
/// (PIDASHCONV-1676 byte probe).
const WORKLOG_NOT_FOUND_BODY: &str = r#"{"detail":"No IssueWorklog matches the given query."}"#;

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
            Denial::ForbiddenDetail => (StatusCode::FORBIDDEN, VIEWSET_FORBIDDEN_BODY.to_owned()),
            Denial::NotFound => (StatusCode::NOT_FOUND, super::NOT_FOUND_BODY.to_owned()),
            Denial::NotFoundDetail => (StatusCode::NOT_FOUND, COMMENT_NOT_FOUND_BODY.to_owned()),
            Denial::NotFoundWorklog => (StatusCode::NOT_FOUND, WORKLOG_NOT_FOUND_BODY.to_owned()),
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                super::PROJECT_NOT_FOUND_BODY.to_owned(),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::ForbiddenError(message) => (
                StatusCode::FORBIDDEN,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadMessage(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"message\":{}}}", json_string(message)),
            ),
            Denial::BadJson(body) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(body).unwrap_or_else(|_| super::SERVER_ERROR_BODY.to_owned()),
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
            tracing::warn!("app_issues engage handler: internal error");
        }
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("engage error response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// Map a write failure the way `handle_exception` does: integrity
/// violations (unique / FK / not-null / check) are the `IntegrityError`
/// 400, anything else the fallback 500.
fn integrity_denial(error: sqlx::Error) -> Denial {
    if let sqlx::Error::Database(db) = &error {
        if db.code().is_some_and(|code| code.starts_with("23")) {
            return Denial::BadError("The payload is not valid".to_owned());
        }
    }
    let _ = error;
    Denial::ServerError
}

/// [`integrity_denial`] for a SeaORM write failure.
fn integrity_denial_orm(error: DbErr) -> Denial {
    if is_integrity_violation(&error) {
        return Denial::BadError("The payload is not valid".to_owned());
    }
    Denial::ServerError
}

/// A SeaORM connection over the request pool (no I/O).
fn orm(pool: &sqlx::PgPool) -> DatabaseConnection {
    pidash_db::pool::sea_orm_connection(pool)
}

/// Any read failure is the generic 500.
fn db_denial(_: DbErr) -> Denial {
    Denial::ServerError
}

// ---------------------------------------------------------------------------
// Request plumbing: auth + project rewrite + permission gates
// ---------------------------------------------------------------------------

/// `request.user` from the Django session (`_auth_user_id`). No session,
/// no key, a non-UUID id, or a session pointing at no user row means
/// anonymous → 401. (Django PKs are UUIDs; a session id that is not a
/// UUID cannot be a user.)
async fn actor_user_id(
    pool: &sqlx::PgPool,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<uuid::Uuid, Denial> {
    let handle = extension.ok_or(Denial::Unauthorized)?.0;
    let mut session = handle.snapshot();
    let raw = session
        .get("_auth_user_id")
        .and_then(|value| value.as_str().to_owned())
        .ok_or(Denial::Unauthorized)?;
    let id = raw
        .parse::<uuid::Uuid>()
        .map_err(|_| Denial::Unauthorized)?;
    let exists: Option<uuid::Uuid> = users::Entity::find()
        .select_only()
        .column(users::Column::Id)
        .filter(users::Column::Id.eq(id))
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(db_denial)?;
    if exists.is_none() {
        return Err(Denial::Unauthorized);
    }
    Ok(id)
}

/// Python `str.strip()` membership (`db/models/project.py:210`): Rust
/// `White_Space` plus U+001C-U+001F (verified by exhaustively diffing
/// `str.strip` against `char::is_whitespace` over all code points —
/// those four are the only differences).
fn is_py_strip_ws(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// Normalize a non-UUID identifier for the equality lookup
/// (`db/models/project.py:210`): `str(value).strip().upper()`.
fn normalize_resolve_identifier(raw: &str) -> String {
    raw.trim_matches(is_py_strip_ws).to_uppercase()
}

/// `_rewrite_project_kwarg` (`app/views/base.py:49-80`): UUIDs pass
/// through unchecked; other identifiers resolve `UPPER(identifier)` in
/// the workspace, else `Http404("Project not found")`.
async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    let upper = normalize_resolve_identifier(raw);
    super::access::project_id_by_identifier(&orm(pool), slug, &upper)
        .await
        .map_err(db_denial)?
        .ok_or(Denial::ProjectNotFound)
}

/// Parse a `<uuid:>` path segment the way Django's `UUIDConverter` does:
/// lowercase hex, hyphenated, 36 chars (`django/urls/converters.py:25`).
/// Uppercase / simple / braced / `urn:` spellings never route in Django
/// (resolver 404), so they proxy here instead of serving. `Result` (not
/// `Option`) so call sites keep their `Ok` / `Err` proxy arms.
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

/// Membership facts for one `(user, slug, project)` over the same rows
/// the decorator and the permission classes read: active project and
/// workspace memberships scoped by slug/project, soft-deleted rows
/// excluded (`SoftDeletionManager`).
struct Membership {
    project_role: Option<i16>,
    workspace_role: Option<i16>,
}

async fn fetch_membership(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<Membership, Denial> {
    let db = orm(pool);
    let project_role = super::access::project_role(&db, slug, *project_id, *user_id)
        .await
        .map_err(db_denial)?;
    let workspace_role = super::access::workspace_role(&db, slug, *user_id)
        .await
        .map_err(db_denial)?;
    Ok(Membership {
        project_role,
        workspace_role,
    })
}

/// `allow_permission(allowed_roles)` at `PROJECT` level
/// (`app/permissions/base.py:19-86`): an allowed project role, or any
/// active project membership plus an active workspace ADMIN membership.
/// Denies with the decorator `error` body.
fn check_allow(membership: &Membership, allowed: &[i16]) -> Result<(), Denial> {
    if membership
        .project_role
        .is_some_and(|role| allowed.contains(&role))
    {
        return Ok(());
    }
    if membership.project_role.is_some() && membership.workspace_role == Some(20) {
        return Ok(());
    }
    Err(Denial::Forbidden)
}

/// `allow_permission([ADMIN], creator=True, model)` (`base.py:19-39`):
/// the workspace-member pre-check, then the creator bypass
/// (`model.objects.filter(id=pk, created_by=user)`), then the ADMIN role
/// check. Denies with the decorator `error` body.
async fn check_admin_creator(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    model: CreatorModel,
    pk: &uuid::Uuid,
) -> Result<(), Denial> {
    let membership = fetch_membership(pool, slug, project_id, user_id).await?;
    if membership.workspace_role.is_none() {
        return Err(Denial::Forbidden);
    }
    let created = created_by_probe(&orm(pool), model, *pk, *user_id)
        .await
        .map_err(db_denial)?;
    if created {
        return Ok(());
    }
    check_allow(&membership, &[20])
}

/// The `model` behind an `allow_permission(creator=True, model=...)`.
#[derive(Debug, Clone, Copy)]
enum CreatorModel {
    IssueComment,
    IssueWorklog,
}

/// The creator bypass `model.objects.filter(id=pk, created_by=user)`
/// over the default manager (soft-deleted rows do not count).
async fn created_by_probe(
    db: &impl sea_orm::ConnectionTrait,
    model: CreatorModel,
    pk: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<bool, DbErr> {
    let hit: Option<uuid::Uuid> = match model {
        CreatorModel::IssueComment => {
            issue_comments::Entity::find()
                .select_only()
                .column(issue_comments::Column::Id)
                .filter(issue_comments::Column::Id.eq(pk))
                .filter(issue_comments::Column::CreatedById.eq(user_id))
                .filter(issue_comments::Column::DeletedAt.is_null())
                .into_tuple()
                .one(db)
                .await?
        }
        CreatorModel::IssueWorklog => {
            issue_worklogs::Entity::find()
                .select_only()
                .column(issue_worklogs::Column::Id)
                .filter(issue_worklogs::Column::Id.eq(pk))
                .filter(issue_worklogs::Column::CreatedById.eq(user_id))
                .filter(issue_worklogs::Column::DeletedAt.is_null())
                .into_tuple()
                .one(db)
                .await?
        }
    };
    Ok(hit.is_some())
}

/// `ProjectEntityPermission` (`project.py:85-116`): safe methods need any
/// active project membership; unsafe methods need ADMIN or MEMBER.
/// Denies with DRF's default `detail` body.
fn check_entity(membership: &Membership, safe: bool) -> Result<(), Denial> {
    if safe {
        if membership.project_role.is_some() {
            return Ok(());
        }
    } else if matches!(membership.project_role, Some(20) | Some(15)) {
        return Ok(());
    }
    Err(Denial::ForbiddenDetail)
}

/// `ProjectLitePermission` (`project.py:133-143`): any active project
/// membership, every method. Denies with DRF's default `detail` body.
fn check_lite(membership: &Membership) -> Result<(), Denial> {
    if membership.project_role.is_some() {
        return Ok(());
    }
    Err(Denial::ForbiddenDetail)
}

/// View-body tenant facts: the actor's timezone. Project existence
/// is per-path (only the reads whose Python calls
/// `Project.objects.get` 404 on a missing row); a missing project
/// here is simply empty scope.
struct Tenant {
    timezone: Tz,
}

async fn tenant_context(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<Tenant, Denial> {
    let timezone_name = super::access::user_timezone(&orm(pool), *user_id)
        .await
        .map_err(db_denial)?;
    let timezone: Tz = timezone_name
        .ok_or(Denial::ServerError)?
        .parse()
        .map_err(|_| Denial::ServerError)?;
    Ok(Tenant { timezone })
}

// ---------------------------------------------------------------------------
// Datetime rendering
// ---------------------------------------------------------------------------

/// Render a timestamptz exactly like DRF `DateTimeField` with the default
/// `iso-8601` format: ISO-8601 in the request's zone with `+00:00`
/// rewritten to `Z` (serializer kernel rule).
fn render_dt(dt: &chrono::DateTime<chrono::Utc>, tz: &Tz) -> String {
    render_datetime_in(dt, tz)
}

fn render_dt_opt(dt: Option<chrono::DateTime<chrono::Utc>>, tz: &Tz) -> Option<String> {
    dt.map(|dt| render_dt(&dt, tz))
}

/// A SeaORM `timestamptz` as the UTC instant the renderers take.
fn utc(dt: sea_orm::prelude::DateTimeWithTimeZone) -> chrono::DateTime<chrono::Utc> {
    dt.with_timezone(&chrono::Utc)
}

// ---------------------------------------------------------------------------
// Request bodies (DRF JSON parsing)
// ---------------------------------------------------------------------------

/// Engage form semantics for the shared body negotiator: the two
/// `ArrayField`s arrive as arrays; `access`/`speaker_type` (not
/// required, `ChoiceField` has no `allow_blank`) skip blank form
/// input; every other blank rule is validated from the map.
const ENGAGE_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &["attachments", "labels"],
    skip_blank_fields: &["access", "speaker_type"],
};

/// Read the request body the way DRF does: content negotiation (JSON /
/// form / multipart, 415 otherwise), empty → `{}`, malformed JSON →
/// `ParseError` 400, non-object JSON → the `non_field_errors` 400.
/// Over-limit bodies never reach here (`BodyLimitLayer` 413'd them).
///
/// Form notes: `comment_json` form strings are JSON-*parsed* (DRF
/// `JSONString` — `""` → "Value must be valid JSON."); blank form
/// input for the nullable scalars (`deleted_at`, `edited_at`,
/// `speaker_agent_run_id`) coerces to `None` (`Field.get_value`);
/// uploads are dropped (they live in `request.FILES`, which no engage
/// view reads, so Django never sees them either).
/// A negotiated request body: the validator map plus whether it came
/// from HTML input (form/multipart). Validators branch on `is_html`
/// exactly where DRF does (`get_value`, `JSONString`, file objects) —
/// a JSON `{"__file__": …}` dict is ordinary data, never a file.
struct RequestBody {
    map: Map<String, Value>,
    is_html: bool,
}

async fn read_body(req: axum::extract::Request) -> Result<RequestBody, Denial> {
    let (parts, body) = req.into_parts();
    let bytes = read_request_bytes(body)
        .await
        .map_err(|_| Denial::TruncatedBody)?;
    match shared_body::negotiate_body(&parts.headers, &bytes, &ENGAGE_BODY_SPEC) {
        Ok(shared_body::NegotiatedBody::Empty) => Ok(RequestBody {
            map: Map::new(),
            is_html: false,
        }),
        Ok(shared_body::NegotiatedBody::JsonText { text, .. }) => {
            parse_json_body(&text).map(|map| RequestBody {
                map,
                is_html: false,
            })
        }
        Ok(shared_body::NegotiatedBody::Form { map, .. }) => {
            // The views validate `request.data` only — uploads live in
            // `request.FILES`, which no engage path reads — so files are
            // dropped here instead of merging as sentinels.
            form_body_map(map, shared_body::FilesMap::new())
                .map(|map| RequestBody { map, is_html: true })
        }
        Err(shared_body::BodyError::UnsupportedMediaType(message)) => {
            Err(Denial::UnsupportedMediaType(message))
        }
        Err(shared_body::BodyError::ParseDetail(message)) => Err(Denial::BadDetail(message)),
        Err(shared_body::BodyError::ServerError) => Err(Denial::ServerError),
    }
}

/// Parse negotiated JSON source text: objects → the map (floats
/// rewritten to their CPython literal — see `normalize_parse_floats`);
/// `null` → "No data provided"; other shapes → the
/// `non_field_errors` "Expected a dictionary" 400; malformed → the
/// engine-native `ParseError` text (the accepted cross-engine gap —
/// see the D-20 `body.rs` precedent).
fn parse_json_body(text: &str) -> Result<Map<String, Value>, Denial> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => {
            let mut normalized: Value = Value::Object(map);
            normalize_parse_floats(&mut normalized);
            match normalized {
                Value::Object(map) => Ok(map),
                _ => unreachable!("object stays an object"),
            }
        }
        Ok(Value::Null) => Err(Denial::BadJson(serde_json::json!({
            "non_field_errors": ["No data provided"],
        }))),
        Ok(other) => Err(Denial::BadJson(serde_json::json!({
            "non_field_errors": [format!(
                "Invalid data. Expected a dictionary, but got {}.",
                json_type_name(&other),
            )],
        }))),
        Err(error) => Err(Denial::BadDetail(format!("JSON parse error - {error}"))),
    }
}

/// A `{"__file__": filename}` upload sentinel (see `read_body`).
fn file_sentinel(filename: &str) -> Value {
    Value::Object(Map::from_iter([(
        "__file__".to_owned(),
        Value::String(filename.to_owned()),
    )]))
}

/// Merge a negotiated form body into the validator map: blank
/// nullable scalars coerce to `None`, and `comment_json` strings
/// JSON-parse (bare `NaN`/`Infinity` → the 500 Django's `json.dumps`
/// round-trip would die with — the form parser accepts them, the
/// `JSONField` save does not). The `files` map arrives empty from
/// `read_body` (uploads live in `request.FILES`, unread here); the
/// merge arms below only run under the unit tests, which pin the
/// sentinel shapes for reference.
fn form_body_map(
    map: Map<String, Value>,
    files: shared_body::FilesMap,
) -> Result<Map<String, Value>, Denial> {
    let mut out = map;
    for (key, parts) in &files {
        if ENGAGE_BODY_SPEC.list_fields.contains(&key.as_str()) {
            // Indexed assemblies (`labels[N]`, `labels[N]suffix`) hold
            // Null placeholders / dict-form arrays for their uploads
            // (form parsing otherwise never emits nulls — the shapes
            // are unambiguous): resolve them positionally from the
            // front of the upload queue, then append leftover
            // exact-key uploads after the texts (DRF `_full_data`
            // extends texts with files).
            let mut consumed = 0usize;
            if let Some(Value::Array(items)) = out.get_mut(key) {
                for item in items.iter_mut() {
                    match item {
                        Value::Null => {
                            if let Some(part) = parts.get(consumed) {
                                *item = file_sentinel(&part.filename);
                                consumed += 1;
                            }
                        }
                        Value::Array(rendered) => {
                            // Dict-form `{suffix: [value]}` pairs: pop
                            // one upload per null value to keep the
                            // queue aligned (the child rejects the
                            // dict either way).
                            for pair in rendered.iter() {
                                if let Value::Object(pair) = pair {
                                    for slot in pair.values() {
                                        if let Value::Array(slot) = slot {
                                            for value in slot.iter() {
                                                if value.is_null() && consumed < parts.len() {
                                                    consumed += 1;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                items.extend(
                    parts[consumed..]
                        .iter()
                        .map(|part| file_sentinel(&part.filename)),
                );
            } else {
                out.insert(
                    key.clone(),
                    Value::Array(
                        parts
                            .iter()
                            .map(|part| file_sentinel(&part.filename))
                            .collect(),
                    ),
                );
            }
        } else if let Some(last) = parts.last() {
            out.insert(key.clone(), file_sentinel(&last.filename));
        }
    }
    for field in ["deleted_at", "edited_at", "speaker_agent_run_id"] {
        if out.get(field) == Some(&Value::String(String::new())) {
            out.insert(field.to_owned(), Value::Null);
        }
    }
    if let Some(value) = out.get("comment_json").cloned() {
        // Multi-valued form input reads the LAST value
        // (`QueryDict.__getitem__`); dict-shaped input (indexed keys)
        // is not a JSON string → invalid. Uploads stringify to their
        // filename first (`JSONString(str(file))`) and then parse —
        // a file named `123` yields the number 123.
        let text = match &value {
            Value::String(text) => Some(text.clone()),
            Value::Array(items) => items.last().and_then(|last| match last {
                Value::String(text) => Some(text.clone()),
                other => uploaded_file_name(other),
            }),
            other => uploaded_file_name(other),
        };
        let parsed = match text {
            Some(text) => match serde_json::from_str::<Value>(&text) {
                Ok(parsed) => Some(parsed),
                Err(_) => {
                    if json_has_bare_nonfinite(&text) {
                        // Django's `json.loads` accepts the token, then
                        // the `JSONField` save dies in `get_prep_value`:
                        // the 500, not a field error.
                        return Err(Denial::ServerError);
                    }
                    return Err(Denial::BadJson(serde_json::json!({
                        "comment_json": ["Value must be valid JSON."],
                    })));
                }
            },
            None => {
                return Err(Denial::BadJson(serde_json::json!({
                    "comment_json": ["Value must be valid JSON."],
                })));
            }
        };
        if let Some(parsed) = parsed {
            out.insert("comment_json".to_owned(), parsed);
        }
    }
    let mut normalized = Value::Object(out);
    normalize_parse_floats(&mut normalized);
    match normalized {
        Value::Object(map) => Ok(map),
        _ => unreachable!("object stays an object"),
    }
}

/// Scan JSON source text (string-aware) for bare `NaN`/`Infinity`
/// tokens outside strings. Shared with the `v1_assets` sticky JSONField
/// HTML-form path (`crate::v1_assets::sticky`), which hits the same
/// `JSONField` save blow-up.
pub(crate) fn json_has_bare_nonfinite(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut token = String::new();
    let flush = |token: &mut String| {
        let hit = matches!(
            token.as_str(),
            "NaN" | "Infinity" | "-Infinity" | "+Infinity"
        );
        token.clear();
        hit
    };
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            if flush(&mut token) {
                return true;
            }
            in_string = true;
            index += 1;
            continue;
        }
        if byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'+' || byte == b'-' {
            token.push(byte as char);
        } else if flush(&mut token) {
            return true;
        }
        index += 1;
    }
    flush(&mut token)
}

/// Back-compat shim for the unit tests: parse JSON source bytes the
/// way `JsonText` does (the header-free tests pin the JSON shapes;
/// empty bodies never reach `JsonText` — `Empty` short-circuits).
#[cfg(test)]
fn parse_body(raw: &[u8]) -> Result<Map<String, Value>, Denial> {
    if raw.is_empty() {
        return Ok(Map::new());
    }
    match std::str::from_utf8(raw) {
        Ok(text) => parse_json_body(text),
        Err(_) => Err(Denial::BadDetail(
            "JSON parse error - invalid utf-8".to_owned(),
        )),
    }
}

/// CPython `type(data).__name__` for a parsed JSON value.
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

// ---------------------------------------------------------------------------
// `json.dumps` with CPython defaults (Celery `requested_data` payloads)
// ---------------------------------------------------------------------------

// Comment `save()` computes `comment_stripped` through
// `pi_dash.utils.html_processor.strip_tags` (`MLStripper`,
// `db/models/issue.py:606`; `""` stays `""`) — the shared
// `ml_strip_tags` port, not Django's `strip_tags`.

// ---------------------------------------------------------------------------
// CPython value semantics (`str` / `repr` / `==` / float rendering)
// ---------------------------------------------------------------------------

/// CPython `==` over parsed JSON values (the sync-lock and
/// `_changes_on_save` comparisons): `True == 1`, `1 == 1.0`, objects
/// order-insensitive, arrays ordered, `1 == "1"` false.
fn py_json_eq(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Bool(a), Value::Number(b)) | (Value::Number(b), Value::Bool(a)) => {
            let num = if *a { 1.0 } else { 0.0 };
            json_number_to_f64(b) == Some(num)
        }
        (Value::Number(a), Value::Number(b)) => {
            if !a.is_f64() && !b.is_f64() {
                // Integers (JSON spellings are canonical — no `+`, no
                // leading zeros): exact, like CPython `int == int`.
                a.to_string() == b.to_string()
            } else {
                json_number_to_f64(a) == json_number_to_f64(b)
            }
        }
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| py_json_eq(x, y))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| match b.get(key) {
                    Some(other) => py_json_eq(value, other),
                    None => false,
                })
        }
        _ => false,
    }
}

/// A JSON number as `f64` for [`py_json_eq`] cross-type comparison.
fn json_number_to_f64(number: &serde_json::Number) -> Option<f64> {
    number
        .as_i64()
        .map(|int| int as f64)
        .or_else(|| number.as_u64().map(|uint| uint as f64))
        .or_else(|| number.as_f64())
}

/// Rewrite every float in a parsed body to its [`py_float_repr_probed`]
/// literal (kept as a `Number`, so ints stay ints and dumps/responses
/// render the CPython spelling: `1e16` in, `1e+16` out).
fn normalize_parse_floats(value: &mut Value) {
    match value {
        Value::Number(number) if number.is_f64() => {
            if let Some(float) = number.as_f64() {
                let literal = py_float_repr_probed(float);
                if let Ok(Value::Number(replacement)) = serde_json::from_str::<Value>(&literal) {
                    *number = replacement;
                }
            }
        }
        Value::Number(_) => {}
        Value::Array(items) => {
            for item in items {
                normalize_parse_floats(item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                normalize_parse_floats(item);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Tasks: best-effort publishing through the queue
// ---------------------------------------------------------------------------

/// Re-exports of the canonical [`pidash_services::domain::activity`] task names.
pub use pidash_services::domain::activity::{ISSUE_ACTIVITY_TASK, MODEL_ACTIVITY_TASK};

/// Enqueue a worker message for the worker to forward to the broker
/// (Python-owned task). Best-effort: without a queue the response still
/// stands (the intake-handler precedent).
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

/// Best-effort kwargs-only publish (every `.delay(...)` call below).
async fn enqueue_kwargs(pool: &sqlx::PgPool, task: &str, kwargs: Map<String, Value>) {
    let message = pidash_jobs::TaskMessage::new(task, vec![], kwargs);
    enqueue_message(pool, message).await;
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

/// `base_host(request, is_app=True)` (`utils/host.py:17-67`): the app
/// origin for task kwargs, never the inbound host. Empty strings fall
/// through like Python's `or` (PIDASHCONV-1587).
fn app_origin(state: &AppState) -> String {
    let urls = &state.settings().urls;
    if let Some(url) = urls.app_base_url.as_deref().filter(|s| !s.is_empty()) {
        return url.to_owned();
    }
    if let Some(url) = urls.web_url.as_deref().filter(|s| !s.is_empty()) {
        return url.to_owned();
    }
    "http://localhost".to_owned()
}

// ---------------------------------------------------------------------------
// Asset URLs + nested lite shapes
// ---------------------------------------------------------------------------

/// Port of `FileAsset.asset_url` (`db/models/asset.py:80-99`):
/// static-asset branches render `/api/assets/v2/static/<id>/`;
/// attachment/description branches interpolate ids; anything else
/// renders `None`.
fn asset_url(
    asset_id: &str,
    entity_type: Option<&str>,
    workspace_slug: Option<&str>,
    project_id: Option<&str>,
    issue_id: Option<&str>,
) -> Result<Option<String>, Denial> {
    match entity_type {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Ok(Some(format!("/api/assets/v2/static/{asset_id}/")))
        }
        Some("ISSUE_ATTACHMENT") => {
            // A null workspace/project/issue is Django's `None.slug`
            // `AttributeError` → the generic 500, not a quiet null.
            let (slug, project, issue) = match (workspace_slug, project_id, issue_id) {
                (Some(s), Some(p), Some(i)) => (s, p, i),
                _ => return Err(Denial::ServerError),
            };
            Ok(Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/issues/{issue}/attachments/{asset_id}/"
            )))
        }
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => {
            let (slug, project) = match (workspace_slug, project_id) {
                (Some(s), Some(p)) => (s, p),
                _ => return Err(Denial::ServerError),
            };
            Ok(Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/{asset_id}/"
            )))
        }
        _ => Ok(None),
    }
}

/// Resolve a logo/cover/avatar URL the way the model properties do: the
/// asset's `asset_url` when the FK is set (even when that computes to
/// `None` — no fall-through), else the raw string column when
/// non-empty, else `None`.
async fn logo_or_cover_url(
    pool: &sqlx::PgPool,
    raw: Option<String>,
    asset_id: Option<uuid::Uuid>,
) -> Result<Option<String>, Denial> {
    let Some(asset_id) = asset_id else {
        return Ok(raw.filter(|text| !text.is_empty()));
    };
    let row = fetch_asset_url_row(&orm(pool), asset_id)
        .await
        .map_err(db_denial)?
        .ok_or(Denial::ServerError)?;
    asset_url(
        &row.id.to_string(),
        row.entity_type.as_deref(),
        row.workspace_slug.as_deref(),
        row.project_id.map(|id| id.to_string()).as_deref(),
        row.issue_id.map(|id| id.to_string()).as_deref(),
    )
}

/// The asset columns behind `FileAsset.asset_url`, plus the workspace
/// slug through a LEFT JOIN (a null workspace leaves it null).
#[derive(Debug, FromQueryResult)]
pub(super) struct AssetUrlRow {
    pub(super) id: uuid::Uuid,
    pub(super) entity_type: Option<String>,
    pub(super) project_id: Option<uuid::Uuid>,
    pub(super) issue_id: Option<uuid::Uuid>,
    pub(super) workspace_slug: Option<String>,
}

fn asset_url_select(asset_id: uuid::Uuid) -> sea_orm::Select<file_assets::Entity> {
    // The asset FK traversal is `_base_manager` (a soft-deleted asset
    // still renders its URL), so the read is unguarded.
    file_assets::Entity::find()
        .select_only()
        .columns([
            file_assets::Column::Id,
            file_assets::Column::EntityType,
            file_assets::Column::ProjectId,
            file_assets::Column::IssueId,
        ])
        .column_as(workspaces::Column::Slug, "workspace_slug")
        .join(JoinType::LeftJoin, file_assets::Relation::Workspaces.def())
        .filter(file_assets::Column::Id.eq(asset_id))
}

pub(super) async fn fetch_asset_url_row(
    db: &impl sea_orm::ConnectionTrait,
    asset_id: uuid::Uuid,
) -> Result<Option<AssetUrlRow>, DbErr> {
    asset_url_select(asset_id)
        .into_model::<AssetUrlRow>()
        .one(db)
        .await
}

/// Owned app `UserLiteSerializer` row (`user.py:141-153`).
pub(crate) struct UserLiteOwned {
    id: String,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_url: Option<String>,
    is_bot: bool,
    display_name: String,
}

pub(crate) fn user_lite_value(row: &UserLiteOwned) -> Result<Value, Denial> {
    use pidash_services::app_project::ser_shared::{user_lite_to_representation, UserLiteRow};
    let input = UserLiteRow {
        id: &row.id,
        first_name: &row.first_name,
        last_name: &row.last_name,
        avatar: &row.avatar,
        avatar_url: row.avatar_url.as_deref(),
        is_bot: row.is_bot,
        display_name: &row.display_name,
    };
    let view = user_lite_to_representation(&input);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// `actor_detail` leaf over a user id. The `users` table has no
/// `deleted_at`, so the read is unguarded; a dangling FK is a 500 like
/// Python's `DoesNotExist` into the fallback envelope.
pub(crate) async fn fetch_user_lite(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
) -> Result<UserLiteOwned, Denial> {
    let row = users::Entity::find()
        .select_only()
        .columns([
            users::Column::Id,
            users::Column::FirstName,
            users::Column::LastName,
            users::Column::Avatar,
            users::Column::AvatarAssetId,
            users::Column::IsBot,
            users::Column::DisplayName,
        ])
        .filter(users::Column::Id.eq(*user_id))
        .into_model::<UserLiteDb>()
        .one(&orm(pool))
        .await
        .map_err(db_denial)?
        .ok_or(Denial::ServerError)?;
    user_lite_owned(pool, row).await
}

/// [`UserLiteOwned`] over fetched columns (the avatar URL resolved).
async fn user_lite_owned(pool: &sqlx::PgPool, row: UserLiteDb) -> Result<UserLiteOwned, Denial> {
    let avatar_url = logo_or_cover_url(pool, Some(row.avatar.clone()), row.avatar_asset_id).await?;
    Ok(UserLiteOwned {
        id: row.id.to_string(),
        first_name: row.first_name,
        last_name: row.last_name,
        avatar: row.avatar,
        avatar_url,
        is_bot: row.is_bot,
        display_name: row.display_name,
    })
}

/// The `users` columns behind [`UserLiteOwned`].
#[derive(Debug, FromQueryResult)]
struct UserLiteDb {
    id: uuid::Uuid,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_asset_id: Option<uuid::Uuid>,
    is_bot: bool,
    display_name: String,
}

/// Owned app `ProjectLiteSerializer` row (`project.py:120-133`).
pub(crate) struct ProjectLiteOwned {
    id: String,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    cover_image_url: Option<String>,
    logo_props: Value,
    description: String,
    is_default: bool,
}

pub(crate) fn project_lite_value(row: &ProjectLiteOwned) -> Result<Value, Denial> {
    use pidash_services::app_project::ser_member::{
        project_lite_to_representation, ProjectLiteRow,
    };
    let input = ProjectLiteRow {
        id: &row.id,
        identifier: &row.identifier,
        name: &row.name,
        cover_image: row.cover_image.as_deref(),
        cover_image_url: row.cover_image_url.as_deref(),
        logo_props: &row.logo_props,
        description: &row.description,
        is_default: row.is_default,
    };
    let view = project_lite_to_representation(&input);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// `project_detail` leaf over a project id. Forward-FK traversal uses
/// `_base_manager` (soft-deleted rows render), so the read is
/// unguarded; only a truly absent row 500s.
pub(crate) async fn fetch_project_lite(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
) -> Result<ProjectLiteOwned, Denial> {
    let row = projects::Entity::find()
        .select_only()
        .columns([
            projects::Column::Id,
            projects::Column::Identifier,
            projects::Column::Name,
            projects::Column::CoverImage,
            projects::Column::CoverImageAssetId,
            projects::Column::LogoProps,
            projects::Column::Description,
            projects::Column::IsDefault,
        ])
        .filter(projects::Column::Id.eq(*project_id))
        .into_model::<ProjectLiteDb>()
        .one(&orm(pool))
        .await
        .map_err(db_denial)?
        .ok_or(Denial::ServerError)?;
    project_lite_owned(pool, row).await
}

/// [`ProjectLiteOwned`] over fetched columns (the cover URL resolved).
async fn project_lite_owned(
    pool: &sqlx::PgPool,
    row: ProjectLiteDb,
) -> Result<ProjectLiteOwned, Denial> {
    let cover_image_url =
        logo_or_cover_url(pool, row.cover_image.clone(), row.cover_image_asset_id).await?;
    Ok(ProjectLiteOwned {
        id: row.id.to_string(),
        identifier: row.identifier,
        name: row.name,
        cover_image: row.cover_image,
        cover_image_url,
        logo_props: row.logo_props,
        description: row.description,
        is_default: row.is_default,
    })
}

/// The `projects` columns behind [`ProjectLiteOwned`].
#[derive(Debug, FromQueryResult)]
struct ProjectLiteDb {
    id: uuid::Uuid,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    cover_image_asset_id: Option<uuid::Uuid>,
    logo_props: Value,
    description: String,
    is_default: bool,
}

/// Owned app `WorkspaceLiteSerializer` row (`workspace.py:79-83`).
pub(crate) struct WorkspaceLiteOwned {
    name: String,
    slug: String,
    id: String,
    logo_url: Option<String>,
}

pub(crate) fn workspace_lite_value(row: &WorkspaceLiteOwned) -> Result<Value, Denial> {
    use pidash_services::app_project::ser_member::{
        workspace_lite_to_representation, WorkspaceLiteRow,
    };
    let input = WorkspaceLiteRow {
        name: &row.name,
        slug: &row.slug,
        id: &row.id,
        logo_url: row.logo_url.as_deref(),
    };
    let view = workspace_lite_to_representation(&input);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// `workspace_detail` leaf over a workspace id. Forward-FK traversal
/// uses `_base_manager` (soft-deleted rows render), so the read is
/// unguarded; only a truly absent row 500s.
pub(crate) async fn fetch_workspace_lite(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
) -> Result<WorkspaceLiteOwned, Denial> {
    let row = workspaces::Entity::find()
        .select_only()
        .columns([
            workspaces::Column::Id,
            workspaces::Column::Name,
            workspaces::Column::Slug,
            workspaces::Column::Logo,
            workspaces::Column::LogoAssetId,
        ])
        .filter(workspaces::Column::Id.eq(*workspace_id))
        .into_model::<WorkspaceLiteDb>()
        .one(&orm(pool))
        .await
        .map_err(db_denial)?
        .ok_or(Denial::ServerError)?;
    workspace_lite_owned(pool, row).await
}

/// [`WorkspaceLiteOwned`] over fetched columns (the logo URL resolved).
async fn workspace_lite_owned(
    pool: &sqlx::PgPool,
    row: WorkspaceLiteDb,
) -> Result<WorkspaceLiteOwned, Denial> {
    let logo_url = logo_or_cover_url(pool, row.logo, row.logo_asset_id).await?;
    Ok(WorkspaceLiteOwned {
        name: row.name,
        slug: row.slug,
        id: row.id.to_string(),
        logo_url,
    })
}

/// The `workspaces` columns behind [`WorkspaceLiteOwned`].
#[derive(Debug, FromQueryResult)]
struct WorkspaceLiteDb {
    id: uuid::Uuid,
    name: String,
    slug: String,
    logo: Option<String>,
    logo_asset_id: Option<uuid::Uuid>,
}

/// Owned app `IssueFlatSerializer` row (`issue.py:105-123`).
struct IssueFlatOwned {
    id: String,
    name: String,
    description_json: Value,
    description_html: Option<String>,
    priority: String,
    complexity_score: i32,
    start_date: Option<String>,
    target_date: Option<String>,
    sequence_id: i32,
    sort_order: f64,
    is_draft: bool,
}

fn issue_flat_value(row: &IssueFlatOwned) -> Result<Value, Denial> {
    use pidash_services::app_issues::serializers_engage::{
        app_issue_flat_to_representation, AppIssueFlatRow,
    };
    let input = AppIssueFlatRow {
        id: &row.id,
        name: &row.name,
        description_json: &row.description_json,
        description_html: row.description_html.as_deref().unwrap_or(""),
        priority: &row.priority,
        complexity_score: row.complexity_score,
        start_date: row.start_date.as_deref(),
        target_date: row.target_date.as_deref(),
        sequence_id: row.sequence_id,
        sort_order: row.sort_order,
        is_draft: row.is_draft,
    };
    let view = app_issue_flat_to_representation(&input);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// `issue_detail` leaf over an issue id (default-manager guard).
async fn fetch_issue_flat(
    pool: &sqlx::PgPool,
    issue_id: &uuid::Uuid,
) -> Result<IssueFlatOwned, Denial> {
    // Forward-FK traversal (`_base_manager`): soft-deleted issues
    // render, so the read is unguarded.
    let row = issues::Entity::find()
        .select_only()
        .columns([
            issues::Column::Id,
            issues::Column::Name,
            issues::Column::DescriptionJson,
            issues::Column::DescriptionHtml,
            issues::Column::Priority,
            issues::Column::ComplexityScore,
            issues::Column::StartDate,
            issues::Column::TargetDate,
            issues::Column::SequenceId,
            issues::Column::SortOrder,
            issues::Column::IsDraft,
        ])
        .filter(issues::Column::Id.eq(*issue_id))
        .into_model::<IssueFlatDb>()
        .one(&orm(pool))
        .await
        .map_err(db_denial)?
        .ok_or(Denial::ServerError)?;
    Ok(issue_flat_owned(row))
}

/// [`IssueFlatOwned`] over fetched columns.
fn issue_flat_owned(row: IssueFlatDb) -> IssueFlatOwned {
    IssueFlatOwned {
        id: row.id.to_string(),
        name: row.name,
        description_json: row.description_json,
        description_html: Some(row.description_html),
        priority: row.priority,
        complexity_score: row.complexity_score,
        start_date: row.start_date.map(|d| render_date(&d)),
        target_date: row.target_date.map(|d| render_date(&d)),
        sequence_id: row.sequence_id,
        sort_order: row.sort_order,
        is_draft: row.is_draft,
    }
}

/// The `issues` columns behind [`IssueFlatOwned`].
#[derive(Debug, FromQueryResult)]
struct IssueFlatDb {
    id: uuid::Uuid,
    name: String,
    description_json: Value,
    description_html: String,
    priority: String,
    complexity_score: i32,
    start_date: Option<chrono::NaiveDate>,
    target_date: Option<chrono::NaiveDate>,
    sequence_id: i32,
    sort_order: f64,
    is_draft: bool,
}

// ---------------------------------------------------------------------------
// Joined builder rows, decoded by name (batch 2h)
// ---------------------------------------------------------------------------
//
// The `queries_engage` history / comment selects return the base table's
// columns unprefixed plus each joined nest under its own prefix
// (`queries_engage::{PROJECT_PREFIX, WORKSPACE_PREFIX, ISSUE_PREFIX,
// USER_PREFIX}`); the row types below decode every value by name.

/// The project / workspace / issue nests of one joined row.
struct NestDb {
    project: ProjectLiteDb,
    workspace: WorkspaceLiteDb,
    issue: IssueFlatDb,
}

impl NestDb {
    fn decode(res: &sea_orm::QueryResult) -> Result<Self, DbErr> {
        Ok(Self {
            project: ProjectLiteDb::from_query_result(res, PROJECT_PREFIX)?,
            workspace: WorkspaceLiteDb::from_query_result(res, WORKSPACE_PREFIX)?,
            issue: IssueFlatDb::from_query_result(res, ISSUE_PREFIX)?,
        })
    }

    /// Resolve the nests' asset URLs into the owned lite shapes.
    async fn into_owned(
        self,
        pool: &sqlx::PgPool,
    ) -> Result<(ProjectLiteOwned, WorkspaceLiteOwned, IssueFlatOwned), Denial> {
        let project = project_lite_owned(pool, self.project).await?;
        let workspace = workspace_lite_owned(pool, self.workspace).await?;
        Ok((project, workspace, issue_flat_owned(self.issue)))
    }
}

/// The LEFT-joined actor nest: `None` when the join missed (null actor),
/// like the null source rendering `actor_detail` as `null`.
fn decode_actor(res: &sea_orm::QueryResult) -> Result<Option<UserLiteDb>, DbErr> {
    match res.try_get::<Option<uuid::Uuid>>(USER_PREFIX, "id")? {
        None => Ok(None),
        Some(_) => UserLiteDb::from_query_result(res, USER_PREFIX).map(Some),
    }
}

async fn actor_owned(
    pool: &sqlx::PgPool,
    actor: Option<UserLiteDb>,
) -> Result<Option<UserLiteOwned>, Denial> {
    match actor {
        None => Ok(None),
        Some(row) => user_lite_owned(pool, row).await.map(Some),
    }
}

/// One `history_activities_select` row.
struct HistoryActivityRow {
    activity: issue_activities::Model,
    nests: NestDb,
    actor: Option<UserLiteDb>,
}

impl FromQueryResult for HistoryActivityRow {
    fn from_query_result(res: &sea_orm::QueryResult, pre: &str) -> Result<Self, DbErr> {
        Ok(Self {
            activity: issue_activities::Model::from_query_result(res, pre)?,
            nests: NestDb::decode(res)?,
            actor: decode_actor(res)?,
        })
    }
}

/// One `history_comments_select` row.
struct HistoryCommentRow {
    comment: issue_comments::Model,
    nests: NestDb,
    actor: Option<UserLiteDb>,
}

impl FromQueryResult for HistoryCommentRow {
    fn from_query_result(res: &sea_orm::QueryResult, pre: &str) -> Result<Self, DbErr> {
        Ok(Self {
            comment: issue_comments::Model::from_query_result(res, pre)?,
            nests: NestDb::decode(res)?,
            actor: decode_actor(res)?,
        })
    }
}

/// One `comment_list_select` row (no actor join).
struct ListCommentRow {
    comment: issue_comments::Model,
    is_member: bool,
    nests: NestDb,
}

impl FromQueryResult for ListCommentRow {
    fn from_query_result(res: &sea_orm::QueryResult, pre: &str) -> Result<Self, DbErr> {
        Ok(Self {
            comment: issue_comments::Model::from_query_result(res, pre)?,
            is_member: res.try_get(pre, "is_member")?,
            nests: NestDb::decode(res)?,
        })
    }
}

/// One `history_reactions_prefetch_select` row: the reaction plus its
/// actor's `display_name`.
struct PrefetchedReactionRow {
    reaction: comment_reactions::Model,
    display_name: String,
}

impl FromQueryResult for PrefetchedReactionRow {
    fn from_query_result(res: &sea_orm::QueryResult, pre: &str) -> Result<Self, DbErr> {
        Ok(Self {
            reaction: comment_reactions::Model::from_query_result(res, pre)?,
            display_name: res.try_get(USER_PREFIX, "display_name")?,
        })
    }
}

/// One `history_intake_prefetch_select` row.
#[derive(FromQueryResult)]
struct IntakeSourceDb {
    source: Option<String>,
    source_email: Option<String>,
    extra: Value,
}

// ---------------------------------------------------------------------------
// Sync predicates
// ---------------------------------------------------------------------------

/// `_comment_is_actively_synced` (`issue.py:79-87`): native rows
/// (`external_source` empty) short-circuit before any DB probe. Git-only
/// after PDASHOSS01-354.
async fn comment_is_synced(
    pool: &sqlx::PgPool,
    external_source: Option<&str>,
    comment_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    use pidash_services::app_issues::serializers_engage::git_comment_synced;
    if external_source.unwrap_or("").is_empty() {
        return Ok(false);
    }
    let db = orm(pool);
    git_comment_synced(&db, *comment_id)
        .await
        .map_err(db_denial)
}

/// The destroy guard (`comment.py:158-160`): git-only sync table, with NO
/// `external_source` short-circuit — unlike [`comment_is_synced`].
async fn comment_has_sync_row(
    pool: &sqlx::PgPool,
    comment_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    use pidash_services::app_issues::serializers_engage::git_comment_synced;
    let db = orm(pool);
    git_comment_synced(&db, *comment_id)
        .await
        .map_err(db_denial)
}

// ---------------------------------------------------------------------------
// Serializer validation (DRF field-error parity)
// ---------------------------------------------------------------------------

/// Push one message onto a field's error list, preserving field order
/// (the map is insertion-ordered).
fn push_error(errors: &mut Map<String, Value>, field: &str, message: String) {
    match errors.get_mut(field) {
        Some(Value::Array(list)) => list.push(Value::String(message)),
        _ => {
            errors.insert(field.to_owned(), Value::Array(vec![Value::String(message)]));
        }
    }
}

/// DRF field-input outcome: missing → the caller applies partial-skip
/// / defaults; null / value otherwise. (Errors push into `errors` and
/// the caller bails when it is non-empty.)
enum Presence<T> {
    Missing,
    Null,
    Value(T),
}

/// DRF `CharField` input (`fields.py:692-788`): ints/floats coerce
/// through `str()` (bools do NOT — `True` is invalid); the blank gate
/// runs on the whitespace-stripped spelling (`"   "` → blank error, or
/// `""` when `allow_blank`); then `MaxLengthValidator` on the stripped
/// value (in *characters*, like Python `len`) and
/// `ProhibitNullCharactersValidator`. Uploaded files (the
/// `{"__file__": …}` sentinel) and other shapes → invalid.
fn check_text(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    max_length: Option<usize>,
    allow_null: bool,
    allow_blank: bool,
) -> Presence<String> {
    match body.get(field) {
        None => Presence::Missing,
        Some(Value::Null) => {
            if allow_null {
                Presence::Null
            } else {
                push_error(errors, field, "This field may not be null.".to_owned());
                Presence::Missing
            }
        }
        Some(Value::Number(number)) => {
            let coerced = py_str_value(&Value::Number(number.clone()));
            check_text_tail(errors, field, &coerced, max_length, allow_blank)
        }
        Some(Value::String(text)) => check_text_tail(errors, field, text, max_length, allow_blank),
        _ => {
            push_error(errors, field, "Not a valid string.".to_owned());
            Presence::Missing
        }
    }
}

/// The string tail of [`check_text`]: strip, blank gate, `max_length`,
/// NUL gate. Returns `Missing` after pushing an error.
fn check_text_tail(
    errors: &mut Map<String, Value>,
    field: &str,
    text: &str,
    max_length: Option<usize>,
    allow_blank: bool,
) -> Presence<String> {
    let stripped = text.trim();
    if stripped.is_empty() {
        if !allow_blank {
            push_error(errors, field, "This field may not be blank.".to_owned());
            return Presence::Missing;
        }
        return Presence::Value(String::new());
    }
    // DRF `run_validators` collects EVERY failure: overlong + NUL
    // yields both messages in validator order.
    let mut failed = false;
    if let Some(max) = max_length {
        if stripped.chars().count() > max {
            push_error(
                errors,
                field,
                format!("Ensure this field has no more than {max} characters."),
            );
            failed = true;
        }
    }
    if stripped.contains('\0') {
        push_error(errors, field, "Null characters are not allowed.".to_owned());
        failed = true;
    }
    if failed {
        return Presence::Missing;
    }
    Presence::Value(stripped.to_owned())
}

/// DRF `ChoiceField` semantics (`fields.py:1177-1250`): null → "may not
/// be null" (none of ours allow null); anything else stringifies
/// through CPython `str()` — scalars raw, containers with `repr()`
/// elements (`"['INTERNAL']"`, `"{'a': 1}"`, `"1e+16"`, all probed
/// live) — for the membership test. HTML-input uploads stringify to
/// their filename. No blank/whitespace handling, no validators.
fn check_choice(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    valid: &[&str],
    is_html: bool,
) -> Presence<String> {
    match body.get(field) {
        None => Presence::Missing,
        Some(Value::Null) => {
            push_error(errors, field, "This field may not be null.".to_owned());
            Presence::Missing
        }
        Some(value) => {
            let input = match (is_html, uploaded_file_name(value)) {
                (true, Some(name)) => name,
                _ => py_str_value(value),
            };
            if valid.contains(&input.as_str()) {
                Presence::Value(input)
            } else {
                push_error(errors, field, format!("\"{input}\" is not a valid choice."));
                Presence::Missing
            }
        }
    }
}

/// The multipart filename behind a `{"__file__": name}` sentinel value
/// (see `read_body`), if the value is one.
fn uploaded_file_name(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => match map.get("__file__") {
            Some(Value::String(name)) if map.len() == 1 => Some(name.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// DRF `DateTimeField` input: Django `parse_datetime` is
/// `datetime.fromisoformat` (plus a regex fallback that only accepts what
/// `fromisoformat` already does). Naive reads as UTC under `USE_TZ`;
/// offsets convert. Anything else → the wrong-format message.
///
/// Grammar (all probed live): date `YYYY-MM-DD` (1-2 digit month/day) /
/// `YYYYMMDD` / `YYYY-Www-D` / `YYYYWwwD` (no ordinal, uppercase `W`
/// only); optional time after ANY single separator char (separator
/// required when time is present); time `HH[:MM[:SS[.ffffff]]]` with
/// 1-2 digit parts, hour-alone exactly 2 digits, or basic `HHMM[SS]` +
/// optional `[.,]fraction` (truncated to 6, never rounded); optional
/// `Z` (uppercase, needs a time) or `±HH:MM[:SS[.f]]` / `±HHMM[SS]` /
/// `±HH` with hour < 24. No surrounding-whitespace trim.
fn parse_input_datetime(text: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if text.is_empty() || text.len() > 64 {
        return None;
    }
    if !text.is_ascii() {
        return None;
    }
    let bytes = text.as_bytes();
    let (date, rest) = split_iso_date(bytes)?;
    if rest.is_empty() {
        return Some(date.and_hms_opt(0, 0, 0)?.and_utc());
    }
    // One separator char (any), then the time; the time must be
    // non-empty, so a lone date+`Z` fails while `+05:00` parses as a
    // time with a `+` separator (both probed live).
    let time_text = rest.get(1..)?;
    if time_text.is_empty() {
        return None;
    }
    let (hour, minute, second, micros, tz_offset) = split_iso_time(time_text)?;
    let naive = date.and_hms_micro_opt(hour, minute, second, micros)?;
    match tz_offset {
        None => Some(naive.and_utc()),
        Some(offset_micros) => naive
            .checked_sub_signed(chrono::Duration::microseconds(offset_micros))
            .map(|utc| utc.and_utc()),
    }
}

/// Split the leading ISO date (`YYYY-MM-DD`, `YYYYMMDD`, `YYYY-Www-D`,
/// `YYYYWwwD`), returning the date plus the unparsed remainder.
fn split_iso_date(bytes: &[u8]) -> Option<(chrono::NaiveDate, &[u8])> {
    if bytes.len() >= 4 && bytes[..4].iter().all(|b| b.is_ascii_digit()) {
        let year: i32 = text_to_int(&bytes[..4])?;
        if !(1..=9999).contains(&year) {
            return None;
        }
        let tail = &bytes[4..];
        // Week dates: `-Www-D` / `WwwD` (uppercase `W` only).
        if let Some(week_tail) = tail.strip_prefix(b"-W").or_else(|| tail.strip_prefix(b"W")) {
            if week_tail.len() >= 2
                && week_tail[..2].iter().all(|b| b.is_ascii_digit())
                && week_tail.get(2) == Some(&b'-')
                && week_tail.len() >= 4
            {
                let week: u32 = text_to_int(&week_tail[..2])?;
                let weekday = week_tail[3];
                if !(b'1'..=b'7').contains(&weekday) {
                    return None;
                }
                let date = chrono::NaiveDate::from_isoywd_opt(year, week, iso_weekday(weekday)?)?;
                return Some((date, &week_tail[4..]));
            }
            if tail.starts_with(b"W")
                && week_tail.len() >= 3
                && week_tail[..2].iter().all(|b| b.is_ascii_digit())
                && (b'1'..=b'7').contains(&week_tail[2])
            {
                let week: u32 = text_to_int(&week_tail[..2])?;
                let date =
                    chrono::NaiveDate::from_isoywd_opt(year, week, iso_weekday(week_tail[2])?)?;
                return Some((date, &week_tail[3..]));
            }
            return None;
        }
        // Calendar dates: `-M-D` / `-MM-DD` / `MMDD`.
        if let Some(dashed) = tail.strip_prefix(b"-") {
            let (month, rest) = split_digits(dashed, 1, 2)?;
            let rest = rest.strip_prefix(b"-")?;
            let (day, rest) = split_digits(rest, 1, 2)?;
            let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
            return Some((date, rest));
        }
        if tail.len() >= 4 && tail[..4].iter().all(|b| b.is_ascii_digit()) {
            let month: u32 = text_to_int(&tail[..2])?;
            let day: u32 = text_to_int(&tail[2..4])?;
            let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
            return Some((date, &tail[4..]));
        }
    }
    None
}

/// ISO weekday digit (`b'1'`–`b'7'`) → [`chrono::Weekday`].
fn iso_weekday(digit: u8) -> Option<chrono::Weekday> {
    match digit {
        b'1' => Some(chrono::Weekday::Mon),
        b'2' => Some(chrono::Weekday::Tue),
        b'3' => Some(chrono::Weekday::Wed),
        b'4' => Some(chrono::Weekday::Thu),
        b'5' => Some(chrono::Weekday::Fri),
        b'6' => Some(chrono::Weekday::Sat),
        b'7' => Some(chrono::Weekday::Sun),
        _ => None,
    }
}

/// Split 1-`max` ASCII digits (at least `min`), returning the value plus
/// the remainder.
fn split_digits(bytes: &[u8], min: usize, max: usize) -> Option<(u32, &[u8])> {
    let mut len = 0;
    while len < max && len < bytes.len() && bytes[len].is_ascii_digit() {
        len += 1;
    }
    if len < min {
        return None;
    }
    Some((text_to_int(&bytes[..len])?, &bytes[len..]))
}

/// ASCII digits → int (no sign, no whitespace, no empty — Rust's
/// `parse` would accept a leading `+`, which `fromisoformat` rejects).
fn text_to_int<T: std::str::FromStr>(bytes: &[u8]) -> Option<T> {
    if bytes.is_empty() || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

/// Split the time + zone: `(hour, minute, second, micros,
/// offset_micros?)`.
fn split_iso_time(text: &[u8]) -> Option<(u32, u32, u32, u32, Option<i64>)> {
    // Zone suffix first: `Z` or a trailing `±...` offset. `Z` needs a
    // non-empty time before it (the caller guarantees non-empty text,
    // so a bare `Z` fails below as a bad time).
    if text == b"Z" {
        return None;
    }
    if let Some(time_text) = text.strip_suffix(b"Z") {
        if time_text.is_empty() {
            return None;
        }
        let (hour, minute, second, micros) = split_iso_clock(time_text)?;
        return Some((hour, minute, second, micros, Some(0)));
    }
    // Trailing offset: scan the last `+`/`-` past position 0 (a leading
    // sign belongs to nothing valid here — dates never start with one).
    let mut sign_at = None;
    for (index, byte) in text.iter().enumerate().skip(1) {
        if *byte == b'+' || *byte == b'-' {
            sign_at = Some(index);
        }
    }
    if let Some(at) = sign_at {
        let (time_text, zone_text) = text.split_at(at);
        if time_text.is_empty() {
            return None;
        }
        let (hour, minute, second, micros) = split_iso_clock(time_text)?;
        let offset = split_iso_offset(zone_text)?;
        return Some((hour, minute, second, micros, Some(offset)));
    }
    let (hour, minute, second, micros) = split_iso_clock(text)?;
    Some((hour, minute, second, micros, None))
}

/// Split the clock (`HH[:MM[:SS[.ffffff]]]`, 1-2 digit parts, or basic
/// `HHMM[SS]` + fraction), returning `(hour, minute, second, micros)`.
fn split_iso_clock(text: &[u8]) -> Option<(u32, u32, u32, u32)> {
    if text.contains(&b':') {
        let mut parts = text.split(|b| *b == b':');
        let hour = parts.next()?;
        let minute = parts.next()?;
        let second = parts.next();
        if parts.next().is_some() {
            return None;
        }
        if hour.is_empty() || hour.len() > 2 || minute.is_empty() || minute.len() > 2 {
            return None;
        }
        let hour: u32 = text_to_int(hour)?;
        let minute: u32 = text_to_int(minute)?;
        let (second, micros) = match second {
            None => (0, 0),
            Some(second) => {
                if second.is_empty() {
                    return None;
                }
                split_iso_seconds(second)?
            }
        };
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        return Some((hour, minute, second, micros));
    }
    // Basic: 2/4/6 digits + optional fraction.
    let (digits, fraction) = match text.iter().position(|b| *b == b'.' || *b == b',') {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (hour, minute, second) = match digits.len() {
        2 => (text_to_int::<u32>(digits)?, 0, 0),
        4 => (
            text_to_int::<u32>(&digits[..2])?,
            text_to_int::<u32>(&digits[2..])?,
            0,
        ),
        6 => (
            text_to_int::<u32>(&digits[..2])?,
            text_to_int::<u32>(&digits[2..4])?,
            text_to_int::<u32>(&digits[4..])?,
        ),
        _ => return None,
    };
    let micros = match fraction {
        None => 0,
        Some(fraction) => iso_fraction_micros(fraction)?,
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some((hour, minute, second, micros))
}

/// Split `SS[.ffffff]` (1-2 digit seconds), returning `(second, micros)`.
fn split_iso_seconds(text: &[u8]) -> Option<(u32, u32)> {
    let (digits, fraction) = match text.iter().position(|b| *b == b'.' || *b == b',') {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    if digits.is_empty() || digits.len() > 2 {
        return None;
    }
    let second: u32 = text_to_int(digits)?;
    let micros = match fraction {
        None => 0,
        Some(fraction) => iso_fraction_micros(fraction)?,
    };
    Some((second, micros))
}

/// Fraction digits → micros, truncated (never rounded) to 6 places.
fn iso_fraction_micros(fraction: &[u8]) -> Option<u32> {
    if fraction.is_empty() || !fraction.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut digits = [b'0'; 6];
    let take = fraction.len().min(6);
    digits[..take].copy_from_slice(&fraction[..take]);
    text_to_int::<u32>(&digits)
}

/// `±HH:MM[:SS[.f]]` / `±HHMM[SS]` / `±HH` → offset micros.
fn split_iso_offset(text: &[u8]) -> Option<i64> {
    let (negative, text) = match text.first() {
        Some(b'+') => (false, &text[1..]),
        Some(b'-') => (true, &text[1..]),
        _ => return None,
    };
    if text.is_empty() {
        return None;
    }
    let (hour, minute, second, micros) = if text.contains(&b':') {
        let mut parts = text.split(|b| *b == b':');
        let hour = parts.next()?;
        let minute = parts.next()?;
        let second = parts.next();
        if parts.next().is_some() || hour.len() != 2 || minute.len() != 2 {
            return None;
        }
        let (second, micros) = match second {
            None => (0, 0),
            Some(second) => {
                if second.len() < 2 {
                    return None;
                }
                split_iso_seconds(second)?
            }
        };
        (
            text_to_int::<i64>(hour)?,
            text_to_int::<i64>(minute)?,
            second as i64,
            micros,
        )
    } else {
        if !text.iter().all(|b| b.is_ascii_digit()) || ![2, 4, 6].contains(&text.len()) {
            return None;
        }
        let hour = text_to_int::<i64>(&text[..2])?;
        let minute = if text.len() >= 4 {
            text_to_int::<i64>(&text[2..4])?
        } else {
            0
        };
        let second = if text.len() == 6 {
            text_to_int::<i64>(&text[4..])?
        } else {
            0
        };
        (hour, minute, second, 0)
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let total = hour * 3_600_000_000 + minute * 60_000_000 + second * 1_000_000 + micros as i64;
    Some(if negative { -total } else { total })
}

const DATETIME_FORMAT_MESSAGE: &str = "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";

fn check_datetime(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    allow_null: bool,
) -> Presence<chrono::DateTime<chrono::Utc>> {
    match body.get(field) {
        None => Presence::Missing,
        Some(Value::Null) => {
            if allow_null {
                Presence::Null
            } else {
                push_error(errors, field, "This field may not be null.".to_owned());
                Presence::Missing
            }
        }
        // DRF runs `strptime(value, 'iso-8601')` when `parse_datetime`
        // returns `None` (PIDASHCONV-773): the literal matches
        // case-insensitively and yields naive 1900-01-01, read as UTC
        // like every naive input here. Kept at this DRF caller — the
        // shared parser also serves the `?created_at__gt=` query path
        // (model `to_python`, no `strptime` step), which must stay
        // invalid there. Exact match: padding fails on both sides
        // (probed); ASCII-only (765 unicode-gap family).
        Some(Value::String(text)) => match parse_input_datetime(text).or_else(|| {
            text.eq_ignore_ascii_case("iso-8601").then(|| {
                let naive = chrono::NaiveDate::from_ymd_opt(1900, 1, 1)
                    .and_then(|date| date.and_hms_opt(0, 0, 0))
                    .expect("1900-01-01 valid");
                naive.and_utc()
            })
        }) {
            Some(dt) => Presence::Value(dt),
            None => {
                push_error(errors, field, DATETIME_FORMAT_MESSAGE.to_owned());
                Presence::Missing
            }
        },
        _ => {
            push_error(errors, field, DATETIME_FORMAT_MESSAGE.to_owned());
            Presence::Missing
        }
    }
}

/// DRF `UUIDField` input (`fields.py:1469-1503`): ints (bools
/// included) go through `UUID(int=…)` (`True` → `…0001`, out of range →
/// invalid); strings go through `UUID(hex=…)` (all four spellings —
/// dashed, plain, braced, `urn:` — parsed, all probed live); anything
/// else (floats, lists, dicts, `""`) → "Must be a valid UUID."
fn check_uuid(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    allow_null: bool,
) -> Presence<uuid::Uuid> {
    match body.get(field) {
        None => Presence::Missing,
        Some(Value::Null) => {
            if allow_null {
                Presence::Null
            } else {
                push_error(errors, field, "This field may not be null.".to_owned());
                Presence::Missing
            }
        }
        Some(Value::Bool(flag)) => {
            Presence::Value(uuid::Uuid::from_u128(if *flag { 1 } else { 0 }))
        }
        Some(Value::Number(number)) => match uuid_from_json_int(number) {
            Some(id) => Presence::Value(id),
            None => {
                push_error(errors, field, "Must be a valid UUID.".to_owned());
                Presence::Missing
            }
        },
        Some(Value::String(text)) => match uuid_from_json_hex(text) {
            Some(id) => Presence::Value(id),
            None => {
                // `uuid.UUID(hex="")` raises too (`ValueError` → the same
                // `invalid` message), so `""` needs no special case.
                push_error(errors, field, "Must be a valid UUID.".to_owned());
                Presence::Missing
            }
        },
        _ => {
            push_error(errors, field, "Must be a valid UUID.".to_owned());
            Presence::Missing
        }
    }
}

/// `uuid.UUID(hex=…)` over a string: dashed, plain-hex, braced, and
/// `urn:uuid:` spellings (all four probed live through the PK path).
fn uuid_from_json_hex(text: &str) -> Option<uuid::Uuid> {
    if text.is_empty() {
        return None;
    }
    text.parse::<uuid::Uuid>().ok()
}

/// DRF `PrimaryKeyRelatedField` input over `table`
/// (`relations.py:103-150` + `RelatedField.run_validation`): HTML-input
/// `""` coerces to `None` (`Field.get_value`, allow_null branch —
/// then the null gate); JSON `""` runs `to_internal_value` instead
/// (`UUID(hex="")` → the curly error, `UUIDField.get_prep_value` via
/// `to_python`); bools → `incorrect_type`; ints go through
/// `UUID(int=…)` (out of range → the curly error); strings go through
/// `UUID(hex=…)`; floats/lists/dicts and bad UUIDs surface Django's
/// `ValidationError` — `“<py-str>” is not a valid UUID.` (curly quotes
/// verbatim, CPython `str()` rendering, all probed live) — as a FIELD
/// error, caught per-field by DRF, never the invalid-detail 400.
/// Valid-but-missing pks quote the RAW input: `Invalid pk "<raw>" -
/// object does not exist.` Existence reads the default manager
/// (soft-deleted rows do not count), except `users` which has no
/// `deleted_at`. HTML-input uploads stringify to their filename for
/// the curly error. (HTML `""` on a non-nullable field falls through
/// to the curly error too — right for our one such caller, the
/// required subscriber FK, where `get_value` also falls through.)
async fn check_fk(
    pool: &sqlx::PgPool,
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    table: &str,
    allow_null: bool,
    is_html: bool,
) -> Result<Presence<uuid::Uuid>, Denial> {
    let raw: Option<(String, Option<uuid::Uuid>)> = match body.get(field) {
        None => return Ok(Presence::Missing),
        Some(Value::Null) => {
            if allow_null {
                return Ok(Presence::Null);
            }
            push_error(errors, field, "This field may not be null.".to_owned());
            return Ok(Presence::Missing);
        }
        Some(Value::String(text)) if text.is_empty() && is_html && allow_null => {
            return Ok(Presence::Null);
        }
        Some(Value::Bool(_)) => {
            push_error(
                errors,
                field,
                "Incorrect type. Expected pk value, received bool.".to_owned(),
            );
            return Ok(Presence::Missing);
        }
        Some(Value::Number(number)) => {
            // The echo renders through CPython `str()` (overflow floats
            // read `inf`, not their source spelling).
            let rendered = py_str_value(&Value::Number(number.clone()));
            Some((rendered, uuid_from_json_int(number)))
        }
        Some(Value::String(text)) => Some((text.clone(), uuid_from_json_hex(text))),
        Some(value) => {
            let rendered = match (is_html, uploaded_file_name(value)) {
                (true, Some(name)) => name,
                _ => py_str_value(value),
            };
            Some((rendered, None))
        }
    };
    let (raw, id) = raw.expect("fk match always yields Some here");
    let Some(id) = id else {
        push_error(
            errors,
            field,
            format!("\u{201c}{raw}\u{201d} is not a valid UUID."),
        );
        return Ok(Presence::Missing);
    };
    let exists = fk_target_exists(&orm(pool), table, id)
        .await
        .map_err(db_denial)?
        .ok_or(Denial::ServerError)?;
    if !exists {
        push_error(
            errors,
            field,
            format!("Invalid pk \"{raw}\" - object does not exist."),
        );
        return Ok(Presence::Missing);
    }
    Ok(Presence::Value(id))
}

/// The `PrimaryKeyRelatedField` queryset lookup behind [`check_fk`]:
/// `users` has no soft delete; the other targets read their default
/// manager (soft-deleted rows do not count). `None` for a table no
/// engage serializer points at.
async fn fk_target_exists(
    db: &impl sea_orm::ConnectionTrait,
    table: &str,
    id: uuid::Uuid,
) -> Result<Option<bool>, DbErr> {
    let hit: Option<uuid::Uuid> = match table {
        "users" => {
            users::Entity::find()
                .select_only()
                .column(users::Column::Id)
                .filter(users::Column::Id.eq(id))
                .into_tuple()
                .one(db)
                .await?
        }
        "descriptions" => {
            descriptions::Entity::find()
                .select_only()
                .column(descriptions::Column::Id)
                .filter(descriptions::Column::Id.eq(id))
                .filter(descriptions::Column::DeletedAt.is_null())
                .into_tuple()
                .one(db)
                .await?
        }
        "issue_comments" => {
            issue_comments::Entity::find()
                .select_only()
                .column(issue_comments::Column::Id)
                .filter(issue_comments::Column::Id.eq(id))
                .filter(issue_comments::Column::DeletedAt.is_null())
                .into_tuple()
                .one(db)
                .await?
        }
        _ => return Ok(None),
    };
    Ok(Some(hit.is_some()))
}

/// Django `URLValidator` (`django/core/validators.py:69-160`), line for
/// line: length/unsafe-chars/scheme gates, the full URL regex
/// (case-insensitive), the IDN→ACE retry, strict IPv6, the 253-char
/// hostname cap. All verdicts below probed live.
fn is_valid_url(text: &str) -> bool {
    if text.chars().count() > 2048 {
        return false;
    }
    if text.contains(['\t', '\r', '\n']) {
        return false;
    }
    // The scheme is the text before the first `://`, lowercased.
    let scheme = text.split("://").next().unwrap_or("");
    if !matches!(
        scheme.to_lowercase().as_str(),
        "http" | "https" | "ftp" | "ftps"
    ) {
        return false;
    }
    // `urlsplit` must not raise: unmatched brackets / bad `%` escapes /
    // NFKC-rejected netloc fail here (all fail the regex too, except a
    // few `urlsplit`-only shapes below).
    if urlsplit_raises(text) {
        return false;
    }
    if match_url_regex(text) {
        return check_ipv6_netloc(text) && check_hostname_length(text);
    }
    // IDN retry: ACE-encode the host and re-run the regex.
    let Some(ascii) = idna_retry_url(text) else {
        return false;
    };
    match_url_regex(&ascii) && check_ipv6_netloc(&ascii) && check_hostname_length(&ascii)
}

/// Shapes `urllib.parse.urlsplit` rejects with `ValueError`: an
/// unmatched `[` in the authority, or a non-ASCII authority holding a
/// char whose NFKC form contains `/?#@:` (`_checknetloc`: fullwidth
/// `／？＃＠：`, small-form `﹕﹖﹟﹫`, presentation `︓︖`, and the
/// `a/c`-style expansions `℀℁℅℆` — all probed live). Bad `%` escapes
/// do NOT raise (`user%zz@…` verifies 201).
fn urlsplit_raises(text: &str) -> bool {
    let authority = url_authority(text);
    if authority.contains('[') && !authority.contains(']') {
        return true;
    }
    if authority.is_ascii() {
        return false;
    }
    authority.chars().any(|ch| {
        matches!(
            ch,
            '\u{ff0f}'
                | '\u{ff1f}'
                | '\u{ff03}'
                | '\u{ff20}'
                | '\u{ff1a}'
                | '\u{fe55}'
                | '\u{fe56}'
                | '\u{fe5f}'
                | '\u{fe6b}'
                | '\u{fe13}'
                | '\u{fe16}'
                | '\u{2100}'
                | '\u{2101}'
                | '\u{2105}'
                | '\u{2106}'
        )
    })
}

/// The authority (userinfo + host + port): after `://`, before the
/// first `/`, `?` or `#`.
fn url_authority(text: &str) -> &str {
    let after_scheme = text.split("://").nth(1).unwrap_or("");
    let end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    &after_scheme[..end]
}

/// The full `URLValidator.regex` match (case-insensitive): optional
/// userinfo, IPv4 / bracketed-IPv6 / hostname, optional port, optional
/// resource path, full-string.
fn match_url_regex(text: &str) -> bool {
    let after_scheme = match text.split_once("://") {
        Some((_, rest)) => rest,
        None => return false,
    };
    // Optional `user[:pass]@` (no whitespace, `:`, `@` or `/` inside;
    // must precede any `/`, `?`, `#`).
    let after_userinfo = match url_split_userinfo(after_scheme) {
        Some(rest) => rest,
        None => return false,
    };
    // Host: IPv4, bracketed IPv6, or hostname/`localhost`.
    let Some((host_end, host_kind)) = url_split_host(after_userinfo) else {
        return false;
    };
    let mut rest = &after_userinfo[host_end..];
    // Optional `:port` (1-5 digits).
    if let Some(port_text) = rest.strip_prefix(':') {
        let digits: String = port_text
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        if digits.is_empty() || digits.len() > 5 {
            return false;
        }
        rest = &port_text[digits.len()..];
    }
    // Hostname flavor also gates the host match (TLD validity).
    if !host_kind {
        return false;
    }
    // Optional resource path: starts with `/`, `?` or `#`, no
    // whitespace after.
    if rest.is_empty() {
        return true;
    }
    match rest.chars().next() {
        Some('/' | '?' | '#') => !rest.chars().any(char::is_whitespace),
        _ => false,
    }
}

/// Split optional userinfo, returning the host remainder. More than one
/// `@` before the path can never match (the userinfo class excludes
/// `@`, the host classes exclude it too).
fn url_split_userinfo(text: &str) -> Option<&str> {
    let path_at = text.find(['/', '?', '#']).unwrap_or(text.len());
    let (head, _) = text.split_at(path_at);
    let at_count = head.chars().filter(|ch| *ch == '@').count();
    if at_count == 0 {
        return Some(text);
    }
    if at_count > 1 {
        return None;
    }
    let at = head.find('@')?;
    let (user, _) = head.split_at(at);
    if user.is_empty() {
        return None;
    }
    let (name, pass) = match user.split_once(':') {
        Some((name, pass)) => (name, pass),
        None => (user, ""),
    };
    if name.is_empty()
        || name
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, ':' | '@' | '/'))
        || pass
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, ':' | '@' | '/'))
    {
        return None;
    }
    Some(&text[at + 1..])
}

/// Split the host, returning `(byte length, valid)`: strict-quad IPv4,
/// loose-bracket IPv6 (validated later), or hostname/`localhost`.
fn url_split_host(text: &str) -> Option<(usize, bool)> {
    // IPv6: `[{hex}:.]+]` (loose here; strict in `check_ipv6_netloc`).
    if let Some(rest) = text.strip_prefix('[') {
        let end = rest.find(']')?;
        let inner = &rest[..end];
        if inner.is_empty()
            || !inner
                .chars()
                .all(|ch| ch.is_ascii_hexdigit() || ch == ':' || ch == '.')
        {
            return None;
        }
        return Some((end + 2, true));
    }
    // The host runs to the first `:`, `/`, `?` or `#`.
    let end = text.find([':', '/', '?', '#']).unwrap_or(text.len());
    let host = &text[..end];
    if host.is_empty() {
        return None;
    }
    if host.eq_ignore_ascii_case("localhost") {
        return Some((end, true));
    }
    if is_strict_ipv4(host) {
        return Some((end, true));
    }
    // Otherwise the hostname branch decides (labels may start with
    // digits; only the TLD rule can fail a numeric quad here, e.g.
    // `1.2.3.4.5` or `01.2.3.4` — both probed 400).
    Some((end, is_valid_hostname(host)))
}

/// Strict IPv4 quad: four `0`-`255` parts, no leading zeros (except `0`
/// itself): `(?:0|25[0-5]|2[0-4][0-9]|1[0-9]?[0-9]?|[1-9][0-9]?)(\.\1){3}`.
fn is_strict_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|part| {
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        if part.len() > 1 && part.starts_with('0') {
            return false;
        }
        part.parse::<u32>().is_ok_and(|num| num <= 255)
    })
}

/// Hostname: first label 1-63 (`[a-z\u00a1-\uffff0-9]`, dashes inside
/// only), middle labels 1-63 (no leading/trailing dash), TLD dot +
/// (2-63 `[a-z\u00a1-\uffff-]` or `xn--[a-z0-9]{1,59}`, no
/// leading/trailing dash) + optional trailing dot. Case-insensitive.
fn is_valid_hostname(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let (tld, rest) = labels.split_last().expect("hostname has labels");
    if !is_valid_tld(tld) {
        return false;
    }
    let (first, middle) = rest.split_first().expect("hostname has labels");
    if !is_valid_first_label(first) {
        return false;
    }
    middle.iter().all(|label| is_valid_middle_label(label))
}

/// First hostname label: starts/ends `[a-z\u00a1-\uffff0-9]`, up to 61
/// middle `[a-z\u00a1-\uffff0-9-]` chars.
fn is_valid_first_label(label: &str) -> bool {
    let count = label.chars().count();
    if count == 0 || count > 63 {
        return false;
    }
    let mut chars = label.chars();
    let first = chars.next().expect("label is non-empty");
    let last = label.chars().next_back().expect("label is non-empty");
    if !is_hostname_edge_char(first) || !is_hostname_edge_char(last) {
        return false;
    }
    chars.all(is_hostname_middle_char)
}

/// Middle hostname label: 1-63 chars, no leading/trailing dash.
fn is_valid_middle_label(label: &str) -> bool {
    let count = label.chars().count();
    if count == 0 || count > 63 {
        return false;
    }
    if label.starts_with('-') || label.ends_with('-') {
        return false;
    }
    label.chars().all(is_hostname_middle_char)
}

/// TLD: 2-63 `[a-z\u00a1-\uffff-]` (no digits) or punycode
/// `xn--[a-z0-9]{1,59}`, never starting/ending with a dash.
fn is_valid_tld(tld: &str) -> bool {
    if tld.starts_with('-') || tld.ends_with('-') {
        return false;
    }
    if tld.len() >= 4 && tld[..4].eq_ignore_ascii_case("xn--") {
        let rest = &tld[4..];
        return (1..=59).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_alphanumeric());
    }
    let count = tld.chars().count();
    if !(2..=63).contains(&count) {
        return false;
    }
    tld.chars()
        .all(|ch| ch == '-' || ch.is_ascii_alphabetic() || is_unicode_letter(ch))
}

/// `[a-z\u00a1-\uffff0-9]` (case-insensitive ASCII).
fn is_hostname_edge_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || is_unicode_letter(ch)
}

/// `[a-z\u00a1-\uffff0-9-]` (case-insensitive ASCII).
fn is_hostname_middle_char(ch: char) -> bool {
    ch == '-' || is_hostname_edge_char(ch)
}

/// The `\u00a1-\uffff` Unicode-letter range.
fn is_unicode_letter(ch: char) -> bool {
    ('\u{a1}'..='\u{ffff}').contains(&ch)
}

/// Strict IPv6 check on a bracketed netloc host
/// (`validate_ipv6_address` over the bracket contents).
fn check_ipv6_netloc(text: &str) -> bool {
    let authority = url_authority(text);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    if !host_port.starts_with('[') {
        return true;
    }
    let Some(end) = host_port.find(']') else {
        return false;
    };
    host_port[1..end].parse::<std::net::Ipv6Addr>().is_ok()
}

/// `urlsplit().hostname` present and at most 253 chars (lowercased,
/// brackets stripped, trailing dot kept).
fn check_hostname_length(text: &str) -> bool {
    let authority = url_authority(text);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = if host_port.starts_with('[') {
        match host_port.find(']') {
            Some(end) => &host_port[1..end],
            None => return false,
        }
    } else {
        match host_port.find(':') {
            Some(at) => &host_port[..at],
            None => host_port,
        }
    };
    if host.is_empty() {
        return false;
    }
    host.to_lowercase().chars().count() <= 253
}

/// IDN retry: ACE-encode (`idna`, UTS46 — the retry only runs when the
/// direct Unicode match already failed, where IDNA 2003 vs UTS46 agree
/// on every reachable input) the hostname labels and rebuild the URL.
fn idna_retry_url(text: &str) -> Option<String> {
    let scheme_end = text.find("://")? + 3;
    let (scheme, after_scheme) = text.split_at(scheme_end);
    let path_at = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let (authority, path) = after_scheme.split_at(path_at);
    let (userinfo, host_port) = match authority.rfind('@') {
        Some(at) => (&authority[..=at], &authority[at + 1..]),
        None => ("", authority),
    };
    if host_port.starts_with('[') {
        return None;
    }
    let (host, port) = match host_port.find(':') {
        Some(at) => (&host_port[..at], &host_port[at..]),
        None => (host_port, ""),
    };
    if host.is_empty() || host.is_ascii() {
        return None;
    }
    let ascii = idna::domain_to_ascii(host).ok()?;
    Some(format!("{scheme}{userinfo}{ascii}{port}{path}"))
}

/// DRF `ListField` input (`fields.py:1640-1720`) over an
/// `ArrayField`: non-lists fail `not_a_list` (CPython type names:
/// `int`, `str`, `dict`, `bool`, `float` — all probed live); children
/// run the child field and failures collect into a DICT keyed by
/// decimal index (`{"0": […], "2": […]}` — failed positions only);
/// then the `ArrayField(size=…)` max-length validator runs
/// (`ArrayMaxLengthValidator`, ngettext singular/plural, probed live).
/// Child validation is `child(value)`; `max_items` is the array size.
fn check_string_list(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    max_items: usize,
    child: &dyn Fn(&Value) -> Result<String, Vec<String>>,
) -> Presence<Vec<String>> {
    let list = match body.get(field) {
        None => return Presence::Missing,
        Some(Value::Null) => {
            push_error(errors, field, "This field may not be null.".to_owned());
            return Presence::Missing;
        }
        Some(Value::Array(list)) => list,
        Some(other) => {
            push_error(
                errors,
                field,
                format!(
                    "Expected a list of items but got type \"{}\".",
                    json_type_name(other),
                ),
            );
            return Presence::Missing;
        }
    };
    let mut out = Vec::with_capacity(list.len());
    let mut item_errors = Map::new();
    for (index, value) in list.iter().enumerate() {
        match child(value) {
            Ok(stored) => out.push(stored),
            Err(messages) => {
                item_errors.insert(
                    index.to_string(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if !item_errors.is_empty() {
        errors.insert(field.to_owned(), Value::Object(item_errors));
        return Presence::Missing;
    }
    if list.len() > max_items {
        let message = if list.len() == 1 {
            format!("List contains 1 item, it should contain no more than {max_items}.")
        } else {
            format!(
                "List contains {} items, it should contain no more than {max_items}.",
                list.len()
            )
        };
        push_error(errors, field, message);
        return Presence::Missing;
    }
    Presence::Value(out)
}

/// The `labels` child (`CharField(max_length=32)`): ints/floats coerce,
/// bools/containers/files invalid, blank gate, `max_length`, NUL gate.
fn validate_label_child(value: &Value) -> Result<String, Vec<String>> {
    let text = match value {
        Value::Null => return Err(vec!["This field may not be null.".to_owned()]),
        Value::Number(number) => py_str_value(&Value::Number(number.clone())),
        Value::String(text) => text.clone(),
        _ => return Err(vec!["Not a valid string.".to_owned()]),
    };
    let stripped = text.trim();
    if stripped.is_empty() {
        return Err(vec!["This field may not be blank.".to_owned()]);
    }
    let mut messages = Vec::new();
    if stripped.chars().count() > 32 {
        messages.push("Ensure this field has no more than 32 characters.".to_owned());
    }
    if stripped.contains('\0') {
        messages.push("Null characters are not allowed.".to_owned());
    }
    if messages.is_empty() {
        Ok(stripped.to_owned())
    } else {
        Err(messages)
    }
}

/// The `attachments` child (`URLField(max_length=200)`): null → the
/// null message; every other shape stringifies through CPython `str()`
/// and runs the child validators in order — `MaxLengthValidator`,
/// `ProhibitNullCharactersValidator`, `URLValidator` (overlong AND bad
/// URLs yield BOTH messages, probed live). The field overrides the
/// `invalid` message, so type failures also read "Enter a valid URL."
fn validate_attachment_child(value: &Value) -> Result<String, Vec<String>> {
    if matches!(value, Value::Null) {
        return Err(vec!["This field may not be null.".to_owned()]);
    }
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => py_str_value(&Value::Number(number.clone())),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
            return Err(vec!["Enter a valid URL.".to_owned()]);
        }
        Value::Null => unreachable!("null handled above"),
    };
    let stripped = text.trim();
    if stripped.is_empty() {
        return Err(vec!["This field may not be blank.".to_owned()]);
    }
    // DRF `run_validators` collects EVERY failure (never stops at
    // the first), so overlong + NUL + bad URLs yield up to three
    // messages in validator order.
    let mut messages = Vec::new();
    if stripped.chars().count() > 200 {
        messages.push("Ensure this field has no more than 200 characters.".to_owned());
    }
    if stripped.contains('\0') {
        messages.push("Null characters are not allowed.".to_owned());
    }
    if !is_valid_url(stripped) {
        messages.push("Enter a valid URL.".to_owned());
    }
    if messages.is_empty() {
        Ok(stripped.to_owned())
    } else {
        Err(messages)
    }
}

// ---------------------------------------------------------------------------
// Comments: validation + shapes + render
// ---------------------------------------------------------------------------

/// A decoded `issue_comments` row (positional or map — both land here).
#[derive(Debug, Clone)]
struct CommentBaseOwned {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    stripped: String,
    json: Value,
    html: String,
    description_id: Option<uuid::Uuid>,
    attachments: Vec<String>,
    labels: Vec<String>,
    issue_id: uuid::Uuid,
    actor_id: Option<uuid::Uuid>,
    access: String,
    external_source: Option<String>,
    external_id: Option<String>,
    speaker_type: String,
    speaker_label: String,
    speaker_run_id: Option<uuid::Uuid>,
    edited_at: Option<chrono::DateTime<chrono::Utc>>,
    parent_id: Option<uuid::Uuid>,
}

impl From<issue_comments::Model> for CommentBaseOwned {
    fn from(row: issue_comments::Model) -> Self {
        Self {
            id: row.id,
            created_at: utc(row.created_at),
            updated_at: utc(row.updated_at),
            created_by: row.created_by_id,
            updated_by: row.updated_by_id,
            deleted_at: row.deleted_at.map(utc),
            project_id: row.project_id,
            workspace_id: row.workspace_id,
            stripped: row.comment_stripped,
            json: row.comment_json,
            html: row.comment_html,
            description_id: row.description_id,
            attachments: row.attachments,
            labels: row.labels,
            issue_id: row.issue_id,
            actor_id: row.actor_id,
            access: row.access,
            external_source: row.external_source,
            external_id: row.external_id,
            speaker_type: row.speaker_type,
            speaker_label: row.speaker_label,
            speaker_run_id: row.speaker_agent_run_id,
            edited_at: row.edited_at.map(utc),
            parent_id: row.parent_id,
        }
    }
}

/// Validated `IssueCommentSerializer` input: `None` = absent (the caller
/// applies partial-skip / full defaults), `Some` = provided.
#[derive(Debug, Default)]
struct CommentValidated {
    deleted_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    stripped: Option<String>,
    json: Option<Value>,
    html: Option<String>,
    description: Option<Option<uuid::Uuid>>,
    attachments: Option<Vec<String>>,
    labels: Option<Vec<String>>,
    actor: Option<Option<uuid::Uuid>>,
    access: Option<String>,
    external_source: Option<Option<String>>,
    external_id: Option<Option<String>>,
    speaker_type: Option<String>,
    speaker_label: Option<String>,
    speaker_run: Option<Option<uuid::Uuid>>,
    edited_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    parent: Option<Option<uuid::Uuid>>,
}

/// `Presence` → `Some` when provided (null included), `None` when
/// missing or invalid.
fn presence_opt<T>(presence: Presence<T>) -> Option<Option<T>> {
    match presence {
        Presence::Missing => None,
        Presence::Null => Some(None),
        Presence::Value(value) => Some(Some(value)),
    }
}

/// `Presence` → value when a real value validated, else `None`.
fn presence_value<T>(presence: Presence<T>) -> Option<T> {
    match presence {
        Presence::Value(value) => Some(value),
        _ => None,
    }
}

/// `IssueCommentSerializer(data).is_valid()` (`issue.py:948-969`): every
/// writable field in live-DRF serializer order (relations trail the
/// concrete fields: `description`, `actor`, `parent` come after
/// `edited_at` — introspected live, since error-key order is
/// observable). Unknown keys are ignored; the sync-lock `validate()`
/// runs separately (it needs the instance).
#[allow(clippy::field_reassign_with_default)]
async fn validate_comment_input(
    pool: &sqlx::PgPool,
    body: &Map<String, Value>,
    is_html: bool,
) -> Result<CommentValidated, Denial> {
    let mut errors = Map::new();
    let mut out = CommentValidated::default();
    out.deleted_at = presence_opt(check_datetime(&mut errors, body, "deleted_at", true));
    out.stripped = presence_value(check_text(
        &mut errors,
        body,
        "comment_stripped",
        None,
        false,
        true,
    ));
    out.json = match body.get("comment_json") {
        None => None,
        Some(Value::Null) => {
            push_error(
                &mut errors,
                "comment_json",
                "This field may not be null.".to_owned(),
            );
            None
        }
        Some(value) => {
            if is_html && uploaded_file_name(value).is_some() {
                // HTML-input uploads fail the `json.dumps` round-trip
                // check (`TypeError` → invalid).
                push_error(
                    &mut errors,
                    "comment_json",
                    "Value must be valid JSON.".to_owned(),
                );
                None
            } else {
                Some(value.clone())
            }
        }
    };
    out.html = presence_value(check_text(
        &mut errors,
        body,
        "comment_html",
        None,
        false,
        true,
    ));
    out.attachments = presence_value(check_string_list(
        &mut errors,
        body,
        "attachments",
        10,
        &validate_attachment_child,
    ));
    out.labels = presence_value(check_string_list(
        &mut errors,
        body,
        "labels",
        8,
        &validate_label_child,
    ));
    out.access = presence_value(check_choice(
        &mut errors,
        body,
        "access",
        &["INTERNAL", "EXTERNAL"],
        is_html,
    ));
    out.external_source = presence_opt(check_text(
        &mut errors,
        body,
        "external_source",
        Some(255),
        true,
        true,
    ));
    out.external_id = presence_opt(check_text(
        &mut errors,
        body,
        "external_id",
        Some(255),
        true,
        true,
    ));
    out.speaker_type = presence_value(check_choice(
        &mut errors,
        body,
        "speaker_type",
        &["human", "agent", "system", "integration"],
        is_html,
    ));
    out.speaker_label = presence_value(check_text(
        &mut errors,
        body,
        "speaker_label",
        Some(128),
        false,
        true,
    ));
    out.speaker_run = presence_opt(check_uuid(&mut errors, body, "speaker_agent_run_id", true));
    out.edited_at = presence_opt(check_datetime(&mut errors, body, "edited_at", true));
    out.description = presence_opt(
        check_fk(
            pool,
            &mut errors,
            body,
            "description",
            "descriptions",
            true,
            is_html,
        )
        .await?,
    );
    out.actor =
        presence_opt(check_fk(pool, &mut errors, body, "actor", "users", true, is_html).await?);
    out.parent = presence_opt(
        check_fk(
            pool,
            &mut errors,
            body,
            "parent",
            "issue_comments",
            true,
            is_html,
        )
        .await?,
    );
    if !errors.is_empty() {
        return Err(Denial::BadJson(Value::Object(errors)));
    }
    Ok(out)
}

/// The sync-lock `validate()` (`issue.py:974-991`): on a synced
/// instance, touched-AND-changed locked fields refuse with the
/// per-field message.
fn comment_sync_lock_errors(
    stored: &CommentBaseOwned,
    attrs: &CommentValidated,
) -> Map<String, Value> {
    const MESSAGE: &str = "This comment is synced from a Git provider and is read-only. Unbind the project's repository to edit.";
    let mut errors = Map::new();
    if let Some(html) = &attrs.html {
        if *html != stored.html {
            push_error(&mut errors, "comment_html", MESSAGE.to_owned());
        }
    }
    if let Some(json) = &attrs.json {
        // Python `dict.__eq__` (cross-type numerics compare: `1 == 1.0`).
        if !py_json_eq(json, &stored.json) {
            push_error(&mut errors, "comment_json", MESSAGE.to_owned());
        }
    }
    if let Some(stripped) = &attrs.stripped {
        if *stripped != stored.stripped {
            push_error(&mut errors, "comment_stripped", MESSAGE.to_owned());
        }
    }
    errors
}

/// App `CommentReactionSerializer` (`issue.py:917-937`), in
/// `Meta.fields` order. `display_name` reads `actor.display_name`.
#[derive(Debug, serde::Serialize)]
struct CommentReactionView {
    id: String,
    actor: String,
    comment: String,
    reaction: String,
    display_name: String,
    deleted_at: Option<String>,
    workspace: String,
    project: String,
    created_at: String,
    updated_at: String,
    created_by: Option<String>,
    updated_by: Option<String>,
}

/// App `IssueCommentSerializer` (`issue.py:948-972`): `Meta.fields =
/// "__all__"` in live-DRF order (pk, declared, concrete, forward
/// relations). `is_member` is `Some` only on annotated-queryset rows
/// (list / retrieve / PUT); create / PATCH / history-comment omit it
/// (DRF `SkipField`).
#[derive(Debug, serde::Serialize)]
struct CommentView {
    id: String,
    actor_detail: Option<Value>,
    issue_detail: Value,
    project_detail: Value,
    workspace_detail: Value,
    comment_reactions: Vec<CommentReactionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_member: Option<bool>,
    is_synced: bool,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    comment_stripped: String,
    comment_json: Value,
    comment_html: String,
    attachments: Vec<String>,
    labels: Vec<String>,
    access: String,
    external_source: Option<String>,
    external_id: Option<String>,
    speaker_type: String,
    speaker_label: String,
    speaker_agent_run_id: Option<String>,
    edited_at: Option<String>,
    created_by: Option<String>,
    updated_by: Option<String>,
    project: String,
    workspace: String,
    description: Option<String>,
    issue: String,
    actor: Option<String>,
    parent: Option<String>,
}

/// `display_name` for reaction rows (`source="actor.display_name"`).
/// The `users` read is unguarded (no `deleted_at` column); a dangling
/// FK is a 500 like Python's `DoesNotExist` into the fallback.
async fn fetch_display_name(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<String, Denial> {
    users::Entity::find()
        .select_only()
        .column(users::Column::DisplayName)
        .filter(users::Column::Id.eq(*user_id))
        .into_tuple::<String>()
        .one(&orm(pool))
        .await
        .map_err(db_denial)?
        .ok_or(Denial::ServerError)
}

/// The live reaction rows for one comment, newest first (model
/// `ordering = ("-created_at",)`): the reverse-manager read the
/// serializer performs per row outside the history prefetch.
async fn render_comment_reactions_for(
    pool: &sqlx::PgPool,
    comment_id: &uuid::Uuid,
    tz: &Tz,
) -> Result<Vec<CommentReactionView>, Denial> {
    let rows = comment_reactions_for(&orm(pool), *comment_id)
        .await
        .map_err(db_denial)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let base = CommentReactionBaseOwned::from(row);
        out.push(comment_reaction_view(pool, &base, tz).await?);
    }
    Ok(out)
}

async fn comment_reactions_for(
    db: &impl sea_orm::ConnectionTrait,
    comment_id: uuid::Uuid,
) -> Result<Vec<comment_reactions::Model>, DbErr> {
    comment_reactions::Entity::find()
        .filter(comment_reactions::Column::DeletedAt.is_null())
        .filter(comment_reactions::Column::CommentId.eq(comment_id))
        .order_by_desc(comment_reactions::Column::CreatedAt)
        .all(db)
        .await
}

/// Assemble a [`CommentView`] from a decoded base, resolved nests, the
/// reaction list and the sync verdict.
#[allow(clippy::too_many_arguments)]
fn assemble_comment_view(
    base: &CommentBaseOwned,
    actor_detail: Option<Value>,
    issue_detail: Value,
    project_detail: Value,
    workspace_detail: Value,
    comment_reactions: Vec<CommentReactionView>,
    is_member: Option<bool>,
    is_synced: bool,
    tz: &Tz,
) -> CommentView {
    CommentView {
        id: base.id.to_string(),
        actor_detail,
        issue_detail,
        project_detail,
        workspace_detail,
        comment_reactions,
        is_member,
        is_synced,
        created_at: render_dt(&base.created_at, tz),
        updated_at: render_dt(&base.updated_at, tz),
        deleted_at: render_dt_opt(base.deleted_at, tz),
        comment_stripped: base.stripped.clone(),
        comment_json: base.json.clone(),
        comment_html: base.html.clone(),
        attachments: base.attachments.clone(),
        labels: base.labels.clone(),
        access: base.access.clone(),
        external_source: base.external_source.clone(),
        external_id: base.external_id.clone(),
        speaker_type: base.speaker_type.clone(),
        speaker_label: base.speaker_label.clone(),
        speaker_agent_run_id: base.speaker_run_id.map(|id| id.to_string()),
        edited_at: render_dt_opt(base.edited_at, tz),
        created_by: base.created_by.map(|id| id.to_string()),
        updated_by: base.updated_by.map(|id| id.to_string()),
        project: base.project_id.to_string(),
        workspace: base.workspace_id.to_string(),
        description: base.description_id.map(|id| id.to_string()),
        issue: base.issue_id.to_string(),
        actor: base.actor_id.map(|id| id.to_string()),
        parent: base.parent_id.map(|id| id.to_string()),
    }
}

/// Serialize the owned nests through the shared lite ports.
fn nest_values(
    actor: &Option<UserLiteOwned>,
    issue: &IssueFlatOwned,
    project: &ProjectLiteOwned,
    workspace: &WorkspaceLiteOwned,
) -> Result<(Option<Value>, Value, Value, Value), Denial> {
    let actor_detail = actor.as_ref().map(user_lite_value).transpose()?;
    let issue_detail = issue_flat_value(issue)?;
    let project_detail = project_lite_value(project)?;
    let workspace_detail = workspace_lite_value(workspace)?;
    Ok((actor_detail, issue_detail, project_detail, workspace_detail))
}

/// Render one history-branch comment: every nest comes from the joined
/// row; `is_member` stays absent; reactions come from the batched
/// prefetch map.
async fn render_history_comment(
    pool: &sqlx::PgPool,
    row: HistoryCommentRow,
    tz: &Tz,
    reactions: Vec<CommentReactionView>,
) -> Result<Value, Denial> {
    let base = CommentBaseOwned::from(row.comment);
    let (project, workspace, issue) = row.nests.into_owned(pool).await?;
    let actor = actor_owned(pool, row.actor).await?;
    let is_synced = comment_is_synced(pool, base.external_source.as_deref(), &base.id).await?;
    let (actor_detail, issue_detail, project_detail, workspace_detail) =
        nest_values(&actor, &issue, &project, &workspace)?;
    let view = assemble_comment_view(
        &base,
        actor_detail,
        issue_detail,
        project_detail,
        workspace_detail,
        reactions,
        None,
        is_synced,
        tz,
    );
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// Render one list-queryset comment: project / workspace / issue come
/// from the joined row, the actor loads lazily (no actor join on the
/// list statement), `is_member` comes from the annotation.
async fn render_list_comment(
    pool: &sqlx::PgPool,
    row: ListCommentRow,
    tz: &Tz,
) -> Result<Value, Denial> {
    let base = CommentBaseOwned::from(row.comment);
    let is_member = row.is_member;
    let (project, workspace, issue) = row.nests.into_owned(pool).await?;
    let actor = match base.actor_id {
        None => None,
        Some(id) => Some(fetch_user_lite(pool, &id).await?),
    };
    let reactions = render_comment_reactions_for(pool, &base.id, tz).await?;
    let is_synced = comment_is_synced(pool, base.external_source.as_deref(), &base.id).await?;
    let (actor_detail, issue_detail, project_detail, workspace_detail) =
        nest_values(&actor, &issue, &project, &workspace)?;
    let view = assemble_comment_view(
        &base,
        actor_detail,
        issue_detail,
        project_detail,
        workspace_detail,
        reactions,
        Some(is_member),
        is_synced,
        tz,
    );
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// Render a comment from a decoded base alone (create / update /
/// destroy-payload paths): every nest loads lazily, exactly like the
/// serializer over a fresh instance.
async fn render_base_comment(
    pool: &sqlx::PgPool,
    base: &CommentBaseOwned,
    tz: &Tz,
    is_member: Option<bool>,
) -> Result<Value, Denial> {
    let actor = match base.actor_id {
        None => None,
        Some(id) => Some(fetch_user_lite(pool, &id).await?),
    };
    let issue = fetch_issue_flat(pool, &base.issue_id).await?;
    let project = fetch_project_lite(pool, &base.project_id).await?;
    let workspace = fetch_workspace_lite(pool, &base.workspace_id).await?;
    let reactions = render_comment_reactions_for(pool, &base.id, tz).await?;
    let is_synced = comment_is_synced(pool, base.external_source.as_deref(), &base.id).await?;
    let (actor_detail, issue_detail, project_detail, workspace_detail) =
        nest_values(&actor, &issue, &project, &workspace)?;
    let view = assemble_comment_view(
        base,
        actor_detail,
        issue_detail,
        project_detail,
        workspace_detail,
        reactions,
        is_member,
        is_synced,
        tz,
    );
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Activities: shapes + render
// ---------------------------------------------------------------------------

/// The intake source triple behind `source_data`
/// (`activity.py:67-73`).
struct IntakeOwned {
    source: Option<String>,
    source_email: Option<String>,
    extra: Value,
}

/// Owned `IssueActivitySerializer` row: every nest plus the rendered
/// scalars the services shape borrows.
struct ActivityRender {
    id: String,
    actor: Option<UserLiteOwned>,
    issue: Option<IssueFlatOwned>,
    project: ProjectLiteOwned,
    workspace: WorkspaceLiteOwned,
    source: Option<IntakeOwned>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    verb: String,
    field: Option<String>,
    old_value: Option<String>,
    new_value: Option<String>,
    comment: String,
    attachments: Vec<String>,
    old_identifier: Option<String>,
    new_identifier: Option<String>,
    epoch: Option<f64>,
    created_by: Option<String>,
    updated_by: Option<String>,
    project_id: String,
    workspace_id: String,
    issue_id: Option<String>,
    issue_comment_id: Option<String>,
    actor_id: Option<String>,
}

/// Render one history-branch activity: every nest comes from the
/// joined row; `source_data` resolves from the intake prefetch map
/// (first element) or stays `None` — exactly the `:529` guard chain.
async fn render_history_activity(
    pool: &sqlx::PgPool,
    row: HistoryActivityRow,
    tz: &Tz,
    intake: Option<IntakeOwned>,
) -> Result<Value, Denial> {
    use pidash_services::app_issues::serializers_engage::{
        issue_activity_to_representation, ActivitySourceData, IssueActivityRow,
    };
    let (project, workspace, issue) = row.nests.into_owned(pool).await?;
    let actor = actor_owned(pool, row.actor).await?;
    let a = row.activity;
    let render = ActivityRender {
        id: a.id.to_string(),
        actor,
        // The history statement pins `issue_id` to the requested issue,
        // so the joined issue is always the activity's own.
        issue: Some(issue),
        project,
        workspace,
        source: intake,
        created_at: render_dt(&utc(a.created_at), tz),
        updated_at: render_dt(&utc(a.updated_at), tz),
        deleted_at: render_dt_opt(a.deleted_at.map(utc), tz),
        verb: a.verb,
        field: a.field,
        old_value: a.old_value,
        new_value: a.new_value,
        comment: a.comment,
        attachments: a.attachments,
        old_identifier: a.old_identifier.map(|id| id.to_string()),
        new_identifier: a.new_identifier.map(|id| id.to_string()),
        epoch: a.epoch,
        created_by: a.created_by_id.map(|id| id.to_string()),
        updated_by: a.updated_by_id.map(|id| id.to_string()),
        project_id: a.project_id.to_string(),
        workspace_id: a.workspace_id.to_string(),
        issue_id: a.issue_id.map(|id| id.to_string()),
        issue_comment_id: a.issue_comment_id.map(|id| id.to_string()),
        actor_id: a.actor_id.map(|id| id.to_string()),
    };
    use pidash_services::app_issues::serializers_engage::app_issue_flat_to_representation;
    use pidash_services::app_issues::serializers_engage::AppIssueFlatRow;
    use pidash_services::app_project::ser_member::{
        project_lite_to_representation, workspace_lite_to_representation, ProjectLiteRow,
        WorkspaceLiteRow,
    };
    use pidash_services::app_project::ser_shared::{user_lite_to_representation, UserLiteRow};
    let actor_row = render.actor.as_ref().map(|row| UserLiteRow {
        id: &row.id,
        first_name: &row.first_name,
        last_name: &row.last_name,
        avatar: &row.avatar,
        avatar_url: row.avatar_url.as_deref(),
        is_bot: row.is_bot,
        display_name: &row.display_name,
    });
    let actor_detail = actor_row.as_ref().map(user_lite_to_representation);
    let issue_row = render.issue.as_ref().map(|row| AppIssueFlatRow {
        id: &row.id,
        name: &row.name,
        description_json: &row.description_json,
        description_html: row.description_html.as_deref().unwrap_or(""),
        priority: &row.priority,
        complexity_score: row.complexity_score,
        start_date: row.start_date.as_deref(),
        target_date: row.target_date.as_deref(),
        sequence_id: row.sequence_id,
        sort_order: row.sort_order,
        is_draft: row.is_draft,
    });
    let issue_detail = issue_row.as_ref().map(app_issue_flat_to_representation);
    let project_row = ProjectLiteRow {
        id: &render.project.id,
        identifier: &render.project.identifier,
        name: &render.project.name,
        cover_image: render.project.cover_image.as_deref(),
        cover_image_url: render.project.cover_image_url.as_deref(),
        logo_props: &render.project.logo_props,
        description: &render.project.description,
        is_default: render.project.is_default,
    };
    let project_detail = project_lite_to_representation(&project_row);
    let workspace_row = WorkspaceLiteRow {
        name: &render.workspace.name,
        slug: &render.workspace.slug,
        id: &render.workspace.id,
        logo_url: render.workspace.logo_url.as_deref(),
    };
    let workspace_detail = workspace_lite_to_representation(&workspace_row);
    let source_data = render.source.as_ref().map(|intake| ActivitySourceData {
        source: intake.source.as_deref(),
        source_email: intake.source_email.as_deref(),
        extra: &intake.extra,
    });
    let attachments: Vec<&str> = render.attachments.iter().map(String::as_str).collect();
    let input = IssueActivityRow {
        id: &render.id,
        actor_detail,
        issue_detail,
        project_detail,
        workspace_detail,
        source_data,
        created_at: &render.created_at,
        updated_at: &render.updated_at,
        deleted_at: render.deleted_at.as_deref(),
        verb: &render.verb,
        field: render.field.as_deref(),
        old_value: render.old_value.as_deref(),
        new_value: render.new_value.as_deref(),
        comment: &render.comment,
        attachments,
        old_identifier: render.old_identifier.as_deref(),
        new_identifier: render.new_identifier.as_deref(),
        epoch: render.epoch,
        created_by: render.created_by.as_deref(),
        updated_by: render.updated_by.as_deref(),
        project: &render.project_id,
        workspace: &render.workspace_id,
        issue: render.issue_id.as_deref(),
        issue_comment: render.issue_comment_id.as_deref(),
        actor: render.actor_id.as_deref(),
    };
    let view = issue_activity_to_representation(&input);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Issue reactions: shapes + render
// ---------------------------------------------------------------------------

/// A decoded `issue_reactions` row.
#[derive(Debug, Clone)]
struct IssueReactionBaseOwned {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    reaction: String,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    actor_id: uuid::Uuid,
}

impl From<issue_reactions::Model> for IssueReactionBaseOwned {
    fn from(row: issue_reactions::Model) -> Self {
        Self {
            id: row.id,
            created_at: utc(row.created_at),
            updated_at: utc(row.updated_at),
            deleted_at: row.deleted_at.map(utc),
            reaction: row.reaction,
            created_by: row.created_by_id,
            updated_by: row.updated_by_id,
            project_id: row.project_id,
            workspace_id: row.workspace_id,
            issue_id: row.issue_id,
            actor_id: row.actor_id,
        }
    }
}

/// App `IssueReactionSerializer` (`issue.py:900-906`):
/// `Meta.fields = "__all__"` in live-DRF order (declared `id` +
/// `actor_detail`, concrete columns, forward relations in model order —
/// `actor` before `issue`, the declaration order at
/// `db/models/issue.py:727-731`).
#[derive(Debug, serde::Serialize)]
struct IssueReactionView {
    id: String,
    actor_detail: Value,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    reaction: String,
    created_by: Option<String>,
    updated_by: Option<String>,
    project: String,
    workspace: String,
    actor: String,
    issue: String,
}

/// Render one issue reaction: the actor nest loads lazily (no
/// `select_related` on the list queryset).
async fn render_issue_reaction(
    pool: &sqlx::PgPool,
    base: &IssueReactionBaseOwned,
    tz: &Tz,
) -> Result<Value, Denial> {
    let actor = fetch_user_lite(pool, &base.actor_id).await?;
    let actor_detail = user_lite_value(&actor)?;
    let view = IssueReactionView {
        id: base.id.to_string(),
        actor_detail,
        created_at: render_dt(&base.created_at, tz),
        updated_at: render_dt(&base.updated_at, tz),
        deleted_at: render_dt_opt(base.deleted_at, tz),
        reaction: base.reaction.clone(),
        created_by: base.created_by.map(|id| id.to_string()),
        updated_by: base.updated_by.map(|id| id.to_string()),
        project: base.project_id.to_string(),
        workspace: base.workspace_id.to_string(),
        actor: base.actor_id.to_string(),
        issue: base.issue_id.to_string(),
    };
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Subscribers: shapes + render
// ---------------------------------------------------------------------------

/// A decoded `issue_subscribers` row.
#[derive(Debug, Clone)]
struct SubscriberOwned {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    subscriber_id: uuid::Uuid,
}

/// Render one subscriber through the services
/// `IssueSubscriberSerializer` port.
fn render_subscriber(base: &SubscriberOwned, tz: &Tz) -> Result<Value, Denial> {
    use pidash_services::app_issues::serializers_engage::{
        issue_subscriber_to_representation, IssueSubscriberRow,
    };
    let id = base.id.to_string();
    let created_at = render_dt(&base.created_at, tz);
    let updated_at = render_dt(&base.updated_at, tz);
    let deleted_at = render_dt_opt(base.deleted_at, tz);
    let created_by = base.created_by.map(|id| id.to_string());
    let updated_by = base.updated_by.map(|id| id.to_string());
    let project = base.project_id.to_string();
    let workspace = base.workspace_id.to_string();
    let issue = base.issue_id.to_string();
    let subscriber = base.subscriber_id.to_string();
    let input = IssueSubscriberRow {
        id: &id,
        created_at: &created_at,
        updated_at: &updated_at,
        deleted_at: deleted_at.as_deref(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        project: &project,
        workspace: &workspace,
        issue: &issue,
        subscriber: &subscriber,
    };
    let view = issue_subscriber_to_representation(&input);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

/// `IssueActivityEndpoint.get` (`activity.py:30-86`).
async fn history(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    Query(query): Query<super::QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    // The `<uuid:>` converter rejects before auth (URL resolving runs
    // first), so the proxy check leads here like on every other path.
    let issue_id = match path_uuid(&issue_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    // Class permission first, decorator second — Django's order.
    if let Err(error) = check_entity(&membership, true) {
        return error.into_response();
    }
    if let Err(error) = check_allow(&membership, &[20, 15, 5]) {
        return error.into_response();
    }
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    // `?created_at__gt=` passthrough: unparseable input raises Django's
    // `ValidationError` into the invalid-detail 400. Valid input
    // normalizes to UTC (naive reads as UTC under `USE_TZ`), so the
    // text bind compares the same instant regardless of session TZ.
    let created_after = match super::query_last(&query, "created_at__gt") {
        None => None,
        Some(raw) => match parse_input_datetime(&raw) {
            Some(dt) => Some(dt.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)),
            None => {
                return Denial::BadError("Please provide valid detail".to_owned()).into_response();
            }
        },
    };
    match history_inner(
        &pool,
        &slug,
        &issue_id,
        &user_id,
        &tenant.timezone,
        created_after.as_deref(),
        super::query_last(&query, "activity_type").as_deref(),
    )
    .await
    {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(error) => error.into_response(),
    }
}

/// The merged read's sort key: Django subscripts each serialized dict
/// (`entry["created_at"]`), so a missing key raises into the generic
/// 500 — unreachable here (both renderers always emit it), mapped to
/// `ServerError` all the same.
fn rendered_created_at(value: &Value) -> Result<&str, Denial> {
    value
        .get("created_at")
        .and_then(Value::as_str)
        .ok_or(Denial::ServerError)
}

async fn history_inner(
    pool: &sqlx::PgPool,
    slug: &str,
    issue_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    tz: &Tz,
    created_after: Option<&str>,
    activity_type: Option<&str>,
) -> Result<String, Denial> {
    match activity_type {
        Some("issue-property") => {
            let (rows, intake) =
                history_activity_rows(pool, slug, issue_id, user_id, created_after).await?;
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                out.push(render_history_activity(pool, row, tz, clone_intake(&intake)).await?);
            }
            serde_json::to_string(&out).map_err(|_| Denial::ServerError)
        }
        Some("issue-comment") => {
            let (rows, mut grouped) =
                history_comment_rows(pool, slug, issue_id, user_id, created_after, tz).await?;
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                let reactions = grouped.remove(&row.comment.id).unwrap_or_default();
                out.push(render_history_comment(pool, row, tz, reactions).await?);
            }
            serde_json::to_string(&out).map_err(|_| Denial::ServerError)
        }
        _ => {
            // The merged read (`activity.py:88-103`): serialize each
            // side with its own renderer first, then merge the plain
            // values and sort stably by the rendered `created_at` —
            // ties keep chain order, activities first. The intake
            // prefetch rides every branch now, so the merged read
            // carries it too.
            let (activities, intake) =
                history_activity_rows(pool, slug, issue_id, user_id, created_after).await?;
            let (comments, mut grouped) =
                history_comment_rows(pool, slug, issue_id, user_id, created_after, tz).await?;
            // Chain order is activities first, then comments, and
            // `sort_by` is stable — equal `created_at` strings keep
            // that order, exactly `sorted(chain(...), key=...)`.
            let mut merged: Vec<(String, Value)> =
                Vec::with_capacity(activities.len() + comments.len());
            for row in activities {
                let rendered =
                    render_history_activity(pool, row, tz, clone_intake(&intake)).await?;
                merged.push((rendered_created_at(&rendered)?.to_owned(), rendered));
            }
            for row in comments {
                let reactions = grouped.remove(&row.comment.id).unwrap_or_default();
                let rendered = render_history_comment(pool, row, tz, reactions).await?;
                merged.push((rendered_created_at(&rendered)?.to_owned(), rendered));
            }
            merged.sort_by(|left, right| left.0.cmp(&right.0));
            let out: Vec<Value> = merged.into_iter().map(|(_, value)| value).collect();
            serde_json::to_string(&out).map_err(|_| Denial::ServerError)
        }
    }
}

/// The prefetch list is shared, so clone the small triple per row (same
/// `source_data` object Django attaches).
fn clone_intake(intake: &Option<IntakeOwned>) -> Option<IntakeOwned> {
    intake.as_ref().map(|owned| IntakeOwned {
        source: owned.source.clone(),
        source_email: owned.source_email.clone(),
        extra: owned.extra.clone(),
    })
}

/// The history activities plus the `issue__issue_intake` prefetch: one
/// issue, so the first (newest) intake row serves every activity — or
/// none. The prefetch runs only when there are activities.
async fn history_activity_rows(
    pool: &sqlx::PgPool,
    slug: &str,
    issue_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    created_after: Option<&str>,
) -> Result<(Vec<HistoryActivityRow>, Option<IntakeOwned>), Denial> {
    use super::queries_engage::{history_activities_select, history_intake_prefetch_select};
    let db = orm(pool);
    let rows = history_activities_select(slug, *issue_id, *user_id, created_after)
        .into_model::<HistoryActivityRow>()
        .all(&db)
        .await
        .map_err(db_denial)?;
    let mut intake = None;
    if !rows.is_empty() {
        let prefetched = history_intake_prefetch_select(&[*issue_id])
            .into_model::<IntakeSourceDb>()
            .all(&db)
            .await
            .map_err(db_denial)?;
        intake = prefetched.into_iter().next().map(|row| IntakeOwned {
            source: row.source,
            source_email: row.source_email,
            extra: row.extra,
        });
    }
    Ok((rows, intake))
}

/// The history comments plus the `comment_reactions` prefetch: one IN
/// query, grouped by parent in row order. The prefetch runs only when
/// there are comments.
async fn history_comment_rows(
    pool: &sqlx::PgPool,
    slug: &str,
    issue_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    created_after: Option<&str>,
    tz: &Tz,
) -> Result<
    (
        Vec<HistoryCommentRow>,
        std::collections::HashMap<uuid::Uuid, Vec<CommentReactionView>>,
    ),
    Denial,
> {
    use super::queries_engage::{history_comments_select, history_reactions_prefetch_select};
    let db = orm(pool);
    let rows = history_comments_select(slug, *issue_id, *user_id, created_after)
        .into_model::<HistoryCommentRow>()
        .all(&db)
        .await
        .map_err(db_denial)?;
    let mut grouped: std::collections::HashMap<uuid::Uuid, Vec<CommentReactionView>> =
        std::collections::HashMap::new();
    if !rows.is_empty() {
        let ids: Vec<uuid::Uuid> = rows.iter().map(|row| row.comment.id).collect();
        let prefetched = history_reactions_prefetch_select(&ids)
            .into_model::<PrefetchedReactionRow>()
            .all(&db)
            .await
            .map_err(db_denial)?;
        for row in prefetched {
            let comment_id = row.reaction.comment_id;
            grouped
                .entry(comment_id)
                .or_default()
                .push(render_prefetched_reaction(row, tz));
        }
    }
    Ok((rows, grouped))
}

/// Render one prefetched reaction: the actor nest comes from the joined
/// user columns (`select_related("actor")`); only `display_name` is
/// read, like the serializer's `source` traversal.
fn render_prefetched_reaction(row: PrefetchedReactionRow, tz: &Tz) -> CommentReactionView {
    let r = row.reaction;
    CommentReactionView {
        id: r.id.to_string(),
        actor: r.actor_id.to_string(),
        comment: r.comment_id.to_string(),
        reaction: r.reaction,
        display_name: row.display_name,
        deleted_at: render_dt_opt(r.deleted_at.map(utc), tz),
        workspace: r.workspace_id.to_string(),
        project: r.project_id.to_string(),
        created_at: render_dt(&utc(r.created_at), tz),
        updated_at: render_dt(&utc(r.updated_at), tz),
        created_by: r.created_by_id.map(|id| id.to_string()),
        updated_by: r.updated_by_id.map(|id| id.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Comments: writes
// ---------------------------------------------------------------------------

/// The `IssueComment.save()` update path (`issue.py:598-647`): recompute
/// `comment_stripped`, sync the `Description` triple for the changed
/// fields (or mint one when unlinked), stamp audit. Returns the saved
/// base for rendering.
#[allow(clippy::too_many_arguments)]
async fn save_comment_update(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    stored: &CommentBaseOwned,
    html: String,
    json: Value,
    attachments: Vec<String>,
    labels: Vec<String>,
    access: String,
    speaker_type: String,
    speaker_label: String,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    description: Option<Option<uuid::Uuid>>,
    actor: Option<Option<uuid::Uuid>>,
    external_source: Option<Option<String>>,
    external_id: Option<Option<String>>,
    speaker_run: Option<Option<uuid::Uuid>>,
    edited_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    parent: Option<Option<uuid::Uuid>>,
    user_id: &uuid::Uuid,
    now: &chrono::DateTime<chrono::Utc>,
) -> Result<CommentBaseOwned, Denial> {
    // `save()` recomputes the stripped text unconditionally — validated
    // `comment_stripped` input never survives (it still validates, and
    // still trips the sync lock).
    let stripped = if html.is_empty() {
        String::new()
    } else {
        ml_strip_tags(&html)
    };
    let description_id = description.unwrap_or(stored.description_id);
    // `_changes_on_save`: the post-recompute triple vs the stored row
    // (Python `==` — cross-type numerics compare on the JSON leg).
    let html_changed = html != stored.html;
    let json_changed = !py_json_eq(&json, &stored.json);
    let stripped_changed = stripped != stored.stripped;
    let description_id = if description_id.is_none() {
        // Unlinked (or cleared): mint a fresh Description over the new
        // triple. `Description.objects.create` runs `Description.save`,
        // which recomputes the stripped text with Django's own
        // `strip_tags` (entities verbatim, `None`-when-empty —
        // `description.py:23-29`), discarding the ML value the
        // comment save passes (PIDASHCONV-1581 item 1).
        let desc_stripped = description_stripped_for(Some(html.as_str()));
        let desc = comment_domain::DescriptionValues {
            id: uuid::Uuid::new_v4(),
            created_at: *now,
            updated_at: *now,
            created_by: stored.created_by,
            updated_by: Some(*user_id),
            workspace_id: stored.workspace_id,
            project_id: stored.project_id,
            json: &json,
            html: &html,
            stripped: desc_stripped.as_deref(),
        };
        comment_domain::insert_description_values(&pidash_db::orm_tx::TxConn::new(tx), &desc)
            .await
            .map_err(integrity_denial)?;
        Some(desc.id)
    } else {
        // Queryset `update` bypasses `Description.save`: the values
        // land verbatim (no `None`-when-empty recompute) — but only
        // the changed subset syncs (`issue.py:637-646`, PIDASHCONV-1581
        // item 2), so an unchanged field keeps the description row's
        // own value even after a re-link.
        if let Some(desc_id) = description_id {
            if html_changed || json_changed || stripped_changed {
                if html_changed {
                    comment_domain::sync_description_html(
                        &pidash_db::orm_tx::TxConn::new(tx),
                        desc_id,
                        &html,
                    )
                    .await
                    .map_err(|_| Denial::ServerError)?;
                }
                if stripped_changed {
                    comment_domain::sync_description_stripped(
                        &pidash_db::orm_tx::TxConn::new(tx),
                        desc_id,
                        &stripped,
                    )
                    .await
                    .map_err(|_| Denial::ServerError)?;
                }
                if json_changed {
                    comment_domain::sync_description_json(
                        &pidash_db::orm_tx::TxConn::new(tx),
                        desc_id,
                        &json,
                    )
                    .await
                    .map_err(|_| Denial::ServerError)?;
                }
                comment_domain::touch_synced_description(
                    &pidash_db::orm_tx::TxConn::new(tx),
                    desc_id,
                    Some(*user_id),
                    *now,
                )
                .await
                .map_err(|_| Denial::ServerError)?;
            }
        }
        description_id
    };
    let base = CommentBaseOwned {
        id: stored.id,
        created_at: stored.created_at,
        updated_at: *now,
        created_by: stored.created_by,
        updated_by: Some(*user_id),
        deleted_at,
        project_id: stored.project_id,
        workspace_id: stored.workspace_id,
        stripped,
        json,
        html,
        description_id,
        attachments,
        labels,
        issue_id: stored.issue_id,
        actor_id: actor.unwrap_or(stored.actor_id),
        access,
        external_source: external_source.unwrap_or_else(|| stored.external_source.clone()),
        external_id: external_id.unwrap_or_else(|| stored.external_id.clone()),
        speaker_type,
        speaker_label,
        speaker_run_id: speaker_run.unwrap_or(stored.speaker_run_id),
        edited_at: edited_at.unwrap_or(stored.edited_at),
        parent_id: parent.unwrap_or(stored.parent_id),
    };
    comment_domain::save_app_comment_update(
        &pidash_db::orm_tx::TxConn::new(tx),
        &comment_domain::AppCommentUpdate {
            id: base.id,
            updated_at: base.updated_at,
            updated_by: base.updated_by,
            deleted_at: base.deleted_at,
            stripped: &base.stripped,
            json: &base.json,
            html: &base.html,
            description_id: base.description_id,
            attachments: &base.attachments,
            labels: &base.labels,
            actor_id: base.actor_id,
            access: &base.access,
            external_source: base.external_source.as_deref(),
            external_id: base.external_id.as_deref(),
            speaker_type: &base.speaker_type,
            speaker_label: &base.speaker_label,
            speaker_agent_run_id: base.speaker_run_id,
            edited_at: base.edited_at,
            parent_id: base.parent_id,
        },
    )
    .await
    .map_err(integrity_denial)?;
    Ok(base)
}

// ---------------------------------------------------------------------------
// Comments: handlers
// ---------------------------------------------------------------------------

/// Shared preamble for the read paths (list / retrieve): session
/// auth, project rewrite, tenant. No membership gate — scoping lives
/// in the queryset.
struct ReadContext {
    pool: sqlx::PgPool,
    slug: String,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
    timezone: Tz,
}

/// Parse the common `(slug, project, issue)` preamble: session auth,
/// project rewrite, tenant. Callers parse their UUID path segments
/// first and proxy garbage ones (Django's `<uuid:>` converter would
/// never route them) before calling.
async fn read_context(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    issue_id: uuid::Uuid,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<ReadContext, Denial> {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    let tenant = tenant_context(&pool, &user_id).await?;
    Ok(ReadContext {
        pool,
        slug: slug.to_owned(),
        project_id,
        issue_id,
        user_id,
        timezone: tenant.timezone,
    })
}

/// `filterset_fields = ["issue__id", "workspace__id"]` (`comment.py:41`):
/// exact UUID matches narrowing the list queryset (and `get_object`,
/// which filters through the same backends). Garbage answers
/// django-filter's per-field `{"<param>": ["Enter a valid UUID."]}`
/// 400 — not the generic invalid-detail body.
fn filterset_predicates(
    query: &super::QueryMap,
) -> Result<Vec<(issue_comments::Column, uuid::Uuid)>, Denial> {
    let mut filters = Vec::new();
    let mut errors = Map::new();
    for (param, column) in [
        ("issue__id", issue_comments::Column::IssueId),
        ("workspace__id", issue_comments::Column::WorkspaceId),
    ] {
        if let Some(raw) = super::query_last(query, param) {
            // Django's `forms.UUIDField` takes every spelling
            // `uuid.UUID()` does (no converter involved here).
            match raw.parse::<uuid::Uuid>() {
                Ok(id) => filters.push((column, id)),
                Err(_) => push_error(&mut errors, param, "Enter a valid UUID.".to_owned()),
            }
        }
    }
    if errors.is_empty() {
        Ok(filters)
    } else {
        Err(Denial::BadJson(Value::Object(errors)))
    }
}

/// The comment-list queryset (`comment_list_select`) narrowed by the
/// filterset predicates and, for `get_object`, the pk.
async fn comment_list_rows(
    ctx: &ReadContext,
    filters: Vec<(issue_comments::Column, uuid::Uuid)>,
    pk: Option<uuid::Uuid>,
) -> Result<Vec<ListCommentRow>, Denial> {
    let mut select = super::queries_engage::comment_list_select(
        &ctx.slug,
        ctx.project_id,
        ctx.issue_id,
        ctx.user_id,
    );
    for (column, id) in filters {
        select = select.filter(column.eq(id));
    }
    if let Some(pk) = pk {
        select = select.filter(issue_comments::Column::Id.eq(pk));
    }
    select
        .into_model::<ListCommentRow>()
        .all(&orm(&ctx.pool))
        .await
        .map_err(db_denial)
}

/// `IssueCommentViewSet.list` (`comment.py:43-69`): DRF's default over
/// the annotated queryset, bare array, newest first.
async fn comment_list(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    Query(query): Query<super::QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match path_uuid(&issue_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let ctx = match read_context(&state, &slug, &project_raw, issue_id, extension).await {
        Ok(ctx) => ctx,
        Err(error) => return error.into_response(),
    };
    let filters = match filterset_predicates(&query) {
        Ok(filters) => filters,
        Err(error) => return error.into_response(),
    };
    // The path issue always scopes; the filterset only narrows further.
    let rows = match comment_list_rows(&ctx, filters, None).await {
        Ok(rows) => rows,
        Err(error) => return error.into_response(),
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        match render_list_comment(&ctx.pool, row, &ctx.timezone).await {
            Ok(value) => out.push(value),
            Err(error) => return error.into_response(),
        }
    }
    match serde_json::to_string(&out) {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(_) => Denial::ServerError.into_response(),
    }
}

/// `IssueCommentViewSet.retrieve`: `get_object` over the list queryset
/// (filterset included — `get_object` filters first); a miss is DRF's
/// `Http404` detail body.
async fn comment_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    Query(query): Query<super::QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let (issue_id, pk) = match (path_uuid(&issue_raw), path_uuid(&pk_raw)) {
        (Ok(issue_id), Ok(pk)) => (issue_id, pk),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let ctx = match read_context(&state, &slug, &project_raw, issue_id, extension).await {
        Ok(ctx) => ctx,
        Err(error) => return error.into_response(),
    };
    let filters = match filterset_predicates(&query) {
        Ok(filters) => filters,
        Err(error) => return error.into_response(),
    };
    let rows = match comment_list_rows(&ctx, filters, Some(pk)).await {
        Ok(rows) => rows,
        Err(error) => return error.into_response(),
    };
    let Some(row) = rows.into_iter().next() else {
        return Denial::NotFoundDetail.into_response();
    };
    match render_list_comment(&ctx.pool, row, &ctx.timezone).await {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(body) => json_response(StatusCode::OK, body),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `IssueCommentViewSet.create` (`comment.py:71-115`).
async fn comment_create(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match path_uuid(&issue_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_allow(&membership, &[20, 15, 5]) {
        return error.into_response();
    }
    // `Project.objects.get` / `Issue.objects.get`: default managers
    // (soft-deleted rows 404), unscoped by tenant (the gate scoped).
    let db = orm(&pool);
    let project = match super::access::project_tenant(&db, project_id).await {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some((workspace_id, guest_view_all)) = project else {
        return Denial::NotFound.into_response();
    };
    let issue: Option<Option<uuid::Uuid>> = match issues::Entity::find()
        .select_only()
        .column(issues::Column::CreatedById)
        .filter(issues::Column::Id.eq(issue_id))
        .filter(issues::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&db)
        .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(issue_created_by) = issue else {
        return Denial::NotFound.into_response();
    };
    // The guest-view rule refuses with 400, not 403.
    if membership.project_role == Some(5) && !guest_view_all && issue_created_by != Some(user_id) {
        return Denial::BadError("You are not allowed to comment on the issue".to_owned())
            .into_response();
    }
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody {
        map: mut body,
        is_html,
    } = request_body;
    let attrs = match validate_comment_input(&pool, &body, is_html).await {
        Ok(attrs) => attrs,
        Err(error) => return error.into_response(),
    };
    // Full-create defaults for the missing defaulted fields; input
    // `description` / `actor` / `comment_stripped` never survive the
    // save (overwritten below), but they validated above.
    let html = attrs.html.unwrap_or_else(|| "<p></p>".to_owned());
    let json = attrs.json.unwrap_or(Value::Object(Map::new()));
    let stripped = if html.is_empty() {
        String::new()
    } else {
        ml_strip_tags(&html)
    };
    let now = chrono::Utc::now();
    let mut base = CommentBaseOwned {
        id: uuid::Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        created_by: Some(user_id),
        updated_by: None,
        deleted_at: attrs.deleted_at.unwrap_or(None),
        project_id,
        workspace_id,
        stripped,
        json,
        html,
        description_id: None,
        attachments: attrs.attachments.unwrap_or_default(),
        labels: attrs.labels.unwrap_or_default(),
        issue_id,
        actor_id: Some(user_id),
        access: attrs.access.unwrap_or_else(|| "INTERNAL".to_owned()),
        external_source: attrs.external_source.unwrap_or(None),
        external_id: attrs.external_id.unwrap_or(None),
        speaker_type: attrs.speaker_type.unwrap_or_else(|| "human".to_owned()),
        speaker_label: attrs.speaker_label.unwrap_or_default(),
        speaker_run_id: attrs.speaker_run.unwrap_or(None),
        edited_at: attrs.edited_at.unwrap_or(None),
        parent_id: attrs.parent.unwrap_or(None),
    };
    // Django's order: comment insert, description insert, link update —
    // in one transaction (`IssueComment.save`).
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    // Domain twins, in Django order. The description stripped value
    // is the `Description.save` recompute (Django's own `strip_tags`,
    // entities verbatim, `None`-when-empty — `description.py:23-29`),
    // not the comment's ML text (PIDASHCONV-1581 item 1).
    // `deleted_at` binds validated client input, as before.
    let desc_stripped = description_stripped_for(Some(base.html.as_str()));
    let rows = comment_domain::CommentRows {
        id: base.id,
        description_id: uuid::Uuid::new_v4(),
        created_at: base.created_at,
        updated_at: base.updated_at,
        desc_created_at: now,
        desc_updated_at: now,
        workspace_id: base.workspace_id,
        project_id: base.project_id,
        issue_id: base.issue_id,
        actor_id: base.actor_id,
        created_by: base.created_by,
        updated_by: base.updated_by,
        deleted_at: base.deleted_at,
        html: &base.html,
        json: &base.json,
        stripped: &base.stripped,
        desc_stripped: desc_stripped.as_deref(),
        access: &base.access,
        speaker_type: &base.speaker_type,
        speaker_label: &base.speaker_label,
        speaker_agent_run_id: base.speaker_run_id,
        external_id: base.external_id.as_deref(),
        external_source: base.external_source.as_deref(),
        parent_id: base.parent_id,
        attachments: &base.attachments,
        labels: &base.labels,
        edited_at: base.edited_at,
    };
    if let Err(error) =
        comment_domain::insert_comment_row(&pidash_db::orm_tx::TxConn::new(&mut tx), &rows).await
    {
        return integrity_denial(error).into_response();
    }
    let desc = comment_domain::DescriptionValues {
        id: rows.description_id,
        created_at: now,
        updated_at: now,
        created_by: Some(user_id),
        updated_by: None,
        workspace_id,
        project_id,
        json: &base.json,
        html: &base.html,
        stripped: rows.desc_stripped,
    };
    if let Err(error) =
        comment_domain::insert_description_values(&pidash_db::orm_tx::TxConn::new(&mut tx), &desc)
            .await
    {
        return integrity_denial(error).into_response();
    }
    // `update_fields=["description_id"]` writes that column only: the
    // `auto_now` stamp is skipped (in memory AND in the row — Django's
    // well-known `update_fields` gotcha), while `BaseModel.save` still
    // assigns `updated_by` on the in-memory instance.
    if comment_domain::link_comment_description(
        &pidash_db::orm_tx::TxConn::new(&mut tx),
        base.id,
        rows.description_id,
    )
    .await
    .is_err()
    {
        return Denial::ServerError.into_response();
    }
    let description_id = rows.description_id;
    if tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }
    // In-memory post-save state: `updated_at` keeps the first save's
    // stamp; only `description_id` and (in memory only) `updated_by`
    // move.
    base.description_id = Some(description_id);
    base.updated_by = Some(user_id);
    let rendered = match render_base_comment(&pool, &base, &tenant.timezone, None).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let epoch = chrono::Utc::now().timestamp();
    let origin = app_origin(&state);
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("comment.activity.created".to_owned()),
    );
    activity.insert(
        "requested_data".to_owned(),
        Value::String(python_dumps(&rendered)),
    );
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert("current_instance".to_owned(), Value::Null);
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(origin.clone()));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    // Task bytes render the raw QueryDict (PIDASHCONV-763): form list
    // fields dump as last-wins scalars. In place — validation is done
    // and `body` moves into the payload below.
    if is_html {
        shared_body::project_list_scalars(&mut body, ENGAGE_BODY_SPEC.list_fields);
    }
    // Shared `model_activity` kernel (PIDASHCONV-1623): the same seven
    // keys in the same order (`current_instance` is `None` on create).
    let model = model_activity_kwargs(
        "issue_comment",
        &base.id.to_string(),
        Value::Object(body),
        None,
        &user_id.to_string(),
        &slug,
        Some(&origin),
    );
    enqueue_kwargs(&pool, MODEL_ACTIVITY_TASK, model).await;
    match serde_json::to_string(&rendered) {
        Ok(text) => json_response(StatusCode::CREATED, text),
        Err(_) => Denial::ServerError.into_response(),
    }
}

/// The scoped `.get()` behind `partial_update` / `destroy`
/// (`comment.py:128-130,165-167`): default manager (soft-deleted rows
/// miss), tenant-scoped, pk included.
async fn fetch_scoped_comment(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<Option<CommentBaseOwned>, Denial> {
    let row = issue_comments::Entity::find()
        .join(
            JoinType::InnerJoin,
            issue_comments::Relation::Workspaces.def(),
        )
        .filter(issue_comments::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issue_comments::Column::ProjectId.eq(*project_id))
        .filter(issue_comments::Column::IssueId.eq(*issue_id))
        .filter(issue_comments::Column::Id.eq(*pk))
        .one(&orm(pool))
        .await
        .map_err(db_denial)?;
    Ok(row.map(CommentBaseOwned::from))
}

/// The soft-deletable rows the engage destroys stamp.
#[derive(Debug, Clone, Copy)]
enum SoftDeleteTarget {
    IssueComment,
    CommentReaction,
    IssueReaction,
    IssueSubscriber,
    IssueWorklog,
}

/// `SoftDeleteModel.delete(soft=True)` (`db/mixins.py:57-78`): stamp
/// `deleted_at` (+ `updated_at`/`updated_by` through the full `save()`),
/// row stays. Comments write through `domain::comments`.
async fn soft_delete_row(
    pool: &sqlx::PgPool,
    target: SoftDeleteTarget,
    pk: &uuid::Uuid,
    user_id: &uuid::Uuid,
    now: &chrono::DateTime<chrono::Utc>,
) -> Result<(), Denial> {
    soft_delete_stamp(&orm(pool), target, *pk, *user_id, *now)
        .await
        .map_err(db_denial)
}

async fn soft_delete_stamp(
    db: &impl sea_orm::ConnectionTrait,
    target: SoftDeleteTarget,
    pk: uuid::Uuid,
    user_id: uuid::Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), DbErr> {
    macro_rules! stamp {
        ($table:ident) => {
            $table::Entity::update_many()
                .col_expr($table::Column::DeletedAt, Expr::value(now))
                .col_expr($table::Column::UpdatedAt, Expr::value(now))
                .col_expr($table::Column::UpdatedById, Expr::value(user_id))
                .filter($table::Column::Id.eq(pk))
                .exec(db)
                .await
                .map(|_| ())
        };
    }
    match target {
        SoftDeleteTarget::IssueComment => {
            comment_domain::soft_delete_app_comment(db, pk, user_id, now).await
        }
        SoftDeleteTarget::CommentReaction => stamp!(comment_reactions),
        SoftDeleteTarget::IssueReaction => stamp!(issue_reactions),
        SoftDeleteTarget::IssueSubscriber => stamp!(issue_subscribers),
        SoftDeleteTarget::IssueWorklog => stamp!(issue_worklogs),
    }
}

/// DRF's default `update` (`comment.py:184-195` maps PUT with no view
/// override): `get_object` (annotated queryset + filterset), full
/// validation with missing fields untouched (no required fields, no
/// serializer defaults), plain save — no decorator gate (class
/// `IsAuthenticated` only), no enqueues, no `edited_at` logic.
async fn comment_update(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    Query(query): Query<super::QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let (issue_id, pk) = match (path_uuid(&issue_raw), path_uuid(&pk_raw)) {
        (Ok(issue_id), Ok(pk)) => (issue_id, pk),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let ctx = match read_context(&state, &slug, &project_raw, issue_id, extension).await {
        Ok(ctx) => ctx,
        Err(error) => return error.into_response(),
    };
    let filters = match filterset_predicates(&query) {
        Ok(filters) => filters,
        Err(error) => return error.into_response(),
    };
    let rows = match comment_list_rows(&ctx, filters, Some(pk)).await {
        Ok(rows) => rows,
        Err(error) => return error.into_response(),
    };
    let Some(row) = rows.into_iter().next() else {
        return Denial::NotFoundDetail.into_response();
    };
    let is_member = row.is_member;
    let stored = CommentBaseOwned::from(row.comment);
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody { map: body, is_html } = request_body;
    let attrs = match validate_comment_input(&ctx.pool, &body, is_html).await {
        Ok(attrs) => attrs,
        Err(error) => return error.into_response(),
    };
    // The sync-lock `validate()` runs on updates (synced instances).
    match comment_is_synced(&ctx.pool, stored.external_source.as_deref(), &stored.id).await {
        Ok(true) => {
            let lock = comment_sync_lock_errors(&stored, &attrs);
            if !lock.is_empty() {
                return Denial::BadJson(Value::Object(lock)).into_response();
            }
        }
        Ok(false) => {}
        Err(error) => return error.into_response(),
    }
    // Full update with no required fields and no serializer defaults:
    // missing fields stay untouched (DRF `SkipField`), exactly like the
    // partial path — only the gate, the enqueues, and the `edited_at`
    // rule differ.
    let now = chrono::Utc::now();
    let mut tx = match ctx.pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let base = match save_comment_update(
        &mut tx,
        &stored,
        attrs.html.unwrap_or_else(|| stored.html.clone()),
        attrs.json.unwrap_or_else(|| stored.json.clone()),
        attrs
            .attachments
            .unwrap_or_else(|| stored.attachments.clone()),
        attrs.labels.unwrap_or_else(|| stored.labels.clone()),
        attrs.access.unwrap_or_else(|| stored.access.clone()),
        attrs
            .speaker_type
            .unwrap_or_else(|| stored.speaker_type.clone()),
        attrs
            .speaker_label
            .unwrap_or_else(|| stored.speaker_label.clone()),
        attrs.deleted_at.unwrap_or(stored.deleted_at),
        attrs.description,
        attrs.actor,
        attrs.external_source,
        attrs.external_id,
        attrs.speaker_run,
        attrs.edited_at,
        attrs.parent,
        &ctx.user_id,
        &now,
    )
    .await
    {
        Ok(base) => base,
        Err(error) => return error.into_response(),
    };
    if tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }
    // No enqueues on the default `update` — and the annotation from
    // `get_object` survives, so `is_member` renders.
    match render_base_comment(&ctx.pool, &base, &ctx.timezone, Some(is_member)).await {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(text) => json_response(StatusCode::OK, text),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `IssueCommentViewSet.partial_update` (`comment.py:118-155`): ADMIN +
/// creator gate, conditional `edited_at`, both enqueues.
async fn comment_partial_update(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let (issue_id, pk) = match (path_uuid(&issue_raw), path_uuid(&pk_raw)) {
        (Ok(issue_id), Ok(pk)) => (issue_id, pk),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_admin_creator(
        &pool,
        &slug,
        &project_id,
        &user_id,
        CreatorModel::IssueComment,
        &pk,
    )
    .await
    {
        return error.into_response();
    }
    let stored = match fetch_scoped_comment(&pool, &slug, &project_id, &issue_id, &pk).await {
        Ok(Some(base)) => base,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(error) => return error.into_response(),
    };
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody {
        map: mut body,
        is_html,
    } = request_body;
    // Task bytes render the raw QueryDict (PIDASHCONV-763): form list
    // fields dump as last-wins scalars. A projected COPY — validation
    // below keeps the arrays.
    let requested_data = if is_html {
        let mut projected = body.clone();
        shared_body::project_list_scalars(&mut projected, ENGAGE_BODY_SPEC.list_fields);
        python_dumps(&Value::Object(projected))
    } else {
        python_dumps(&Value::Object(body.clone()))
    };
    let current_rendered = match render_base_comment(&pool, &stored, &tenant.timezone, None).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let current_instance = python_dumps(&current_rendered);
    let attrs = match validate_comment_input(&pool, &body, is_html).await {
        Ok(attrs) => attrs,
        Err(error) => return error.into_response(),
    };
    match comment_is_synced(&pool, stored.external_source.as_deref(), &stored.id).await {
        Ok(true) => {
            let lock = comment_sync_lock_errors(&stored, &attrs);
            if !lock.is_empty() {
                return Denial::BadJson(Value::Object(lock)).into_response();
            }
        }
        Ok(false) => {}
        Err(error) => return error.into_response(),
    }
    // `edited_at` stamps when the RAW `comment_html` input arrived AND
    // differs (`comment.py:124` compares `request.data`, pre-strip:
    // whitespace-padded no-ops still stamp, and numbers never equal
    // the stored string) — overriding any input value.
    let html_touched_and_changed = match body.get("comment_html") {
        Some(Value::String(raw)) => *raw != stored.html,
        Some(Value::Number(_)) => true,
        _ => false,
    };
    let edited_at = if html_touched_and_changed {
        Some(Some(chrono::Utc::now()))
    } else {
        attrs.edited_at
    };
    // Partial: missing fields stay untouched (no defaults).
    let now = chrono::Utc::now();
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let base = match save_comment_update(
        &mut tx,
        &stored,
        attrs.html.unwrap_or_else(|| stored.html.clone()),
        attrs.json.unwrap_or_else(|| stored.json.clone()),
        attrs
            .attachments
            .unwrap_or_else(|| stored.attachments.clone()),
        attrs.labels.unwrap_or_else(|| stored.labels.clone()),
        attrs.access.unwrap_or_else(|| stored.access.clone()),
        attrs
            .speaker_type
            .unwrap_or_else(|| stored.speaker_type.clone()),
        attrs
            .speaker_label
            .unwrap_or_else(|| stored.speaker_label.clone()),
        attrs.deleted_at.unwrap_or(stored.deleted_at),
        attrs.description,
        attrs.actor,
        attrs.external_source,
        attrs.external_id,
        attrs.speaker_run,
        edited_at,
        attrs.parent,
        &user_id,
        &now,
    )
    .await
    {
        Ok(base) => base,
        Err(error) => return error.into_response(),
    };
    if tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }
    let epoch = chrono::Utc::now().timestamp();
    let origin = app_origin(&state);
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("comment.activity.updated".to_owned()),
    );
    activity.insert(
        "requested_data".to_owned(),
        Value::String(requested_data.clone()),
    );
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert(
        "current_instance".to_owned(),
        Value::String(current_instance.clone()),
    );
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(origin.clone()));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    // Same QueryDict projection as `requested_data` above (PIDASHCONV-763).
    // In place — validation is done and `body` moves below.
    if is_html {
        shared_body::project_list_scalars(&mut body, ENGAGE_BODY_SPEC.list_fields);
    }
    // Shared `model_activity` kernel (PIDASHCONV-1623): the same seven
    // keys in the same order (the before-image snapshot string rides
    // `current_instance` on update).
    let model = model_activity_kwargs(
        "issue_comment",
        &pk.to_string(),
        Value::Object(body),
        Some(&current_instance),
        &user_id.to_string(),
        &slug,
        Some(&origin),
    );
    enqueue_kwargs(&pool, MODEL_ACTIVITY_TASK, model).await;
    match render_base_comment(&pool, &base, &tenant.timezone, None).await {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(text) => json_response(StatusCode::OK, text),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `IssueCommentViewSet.destroy` (`comment.py:157-184`): ADMIN +
/// creator gate, sync 409, soft delete, `issue_activity` only.
async fn comment_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let (issue_id, pk) = match (path_uuid(&issue_raw), path_uuid(&pk_raw)) {
        (Ok(issue_id), Ok(pk)) => (issue_id, pk),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_admin_creator(
        &pool,
        &slug,
        &project_id,
        &user_id,
        CreatorModel::IssueComment,
        &pk,
    )
    .await
    {
        return error.into_response();
    }
    let stored = match fetch_scoped_comment(&pool, &slug, &project_id, &issue_id, &pk).await {
        Ok(Some(base)) => base,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(error) => return error.into_response(),
    };
    // The request body is never read on this path (not even for
    // errors); drop `req` without parsing.
    let _ = req;
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    match comment_has_sync_row(&pool, &stored.id).await {
        Ok(true) => {
            return json_response(
                StatusCode::CONFLICT,
                r#"{"error":"This comment is synced from a Git provider. Unbind the project's repository to delete."}"#.to_owned(),
            );
        }
        Ok(false) => {}
        Err(error) => return error.into_response(),
    }
    let current_rendered = match render_base_comment(&pool, &stored, &tenant.timezone, None).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    if let Err(error) = soft_delete_row(
        &pool,
        SoftDeleteTarget::IssueComment,
        &stored.id,
        &user_id,
        &now,
    )
    .await
    {
        return error.into_response();
    }
    let mut requested = Map::new();
    requested.insert("comment_id".to_owned(), Value::String(pk.to_string()));
    let epoch = chrono::Utc::now().timestamp();
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("comment.activity.deleted".to_owned()),
    );
    activity.insert(
        "requested_data".to_owned(),
        Value::String(python_dumps(&Value::Object(requested))),
    );
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert(
        "current_instance".to_owned(),
        Value::String(python_dumps(&current_rendered)),
    );
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(app_origin(&state)));
    // Order mirrors `comment.py:174-185`: `issue_comment.delete()` (which
    // fires `soft_delete_related_objects` from `db/mixins.py:77`) runs
    // BEFORE the `issue_activity.delay` — the reverse of the reaction
    // destroys, where the delay precedes `.delete()`.
    enqueue_soft_delete(&pool, "issuecomment", &stored.id).await;
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    empty_response(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Reactions
// ---------------------------------------------------------------------------

/// Required text input: missing → "This field is required." (other
/// failures already pushed their message).
fn require_text(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    allow_blank: bool,
) -> Option<String> {
    match check_text(errors, body, field, None, false, allow_blank) {
        Presence::Value(value) => Some(value),
        _ => {
            if !errors.contains_key(field) {
                push_error(errors, field, "This field is required.".to_owned());
            }
            None
        }
    }
}

/// The lazy forward-FK project read behind reaction / subscriber /
/// subscribe saves: `_base_manager` semantics — soft-deleted rows
/// count, only a truly absent row 404s.
async fn fetch_project_workspace_unguarded(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
) -> Result<uuid::Uuid, Denial> {
    projects::Entity::find()
        .select_only()
        .column(projects::Column::WorkspaceId)
        .filter(projects::Column::Id.eq(*project_id))
        .into_tuple::<uuid::Uuid>()
        .one(&orm(pool))
        .await
        .map_err(db_denial)?
        .ok_or(Denial::NotFound)
}

/// A decoded `comment_reactions` row (single-table maps).
#[derive(Debug, Clone)]
struct CommentReactionBaseOwned {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    comment_id: uuid::Uuid,
    actor_id: uuid::Uuid,
    reaction: String,
}

impl From<comment_reactions::Model> for CommentReactionBaseOwned {
    fn from(row: comment_reactions::Model) -> Self {
        Self {
            id: row.id,
            created_at: utc(row.created_at),
            updated_at: utc(row.updated_at),
            deleted_at: row.deleted_at.map(utc),
            created_by: row.created_by_id,
            updated_by: row.updated_by_id,
            project_id: row.project_id,
            workspace_id: row.workspace_id,
            comment_id: row.comment_id,
            actor_id: row.actor_id,
            reaction: row.reaction,
        }
    }
}

/// Render one comment reaction with its `display_name` leaf.
async fn render_comment_reaction(
    pool: &sqlx::PgPool,
    base: &CommentReactionBaseOwned,
    tz: &Tz,
) -> Result<Value, Denial> {
    let view = comment_reaction_view(pool, base, tz).await?;
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// [`render_comment_reaction`] before the JSON step.
async fn comment_reaction_view(
    pool: &sqlx::PgPool,
    base: &CommentReactionBaseOwned,
    tz: &Tz,
) -> Result<CommentReactionView, Denial> {
    let display_name = fetch_display_name(pool, &base.actor_id).await?;
    Ok(CommentReactionView {
        id: base.id.to_string(),
        actor: base.actor_id.to_string(),
        comment: base.comment_id.to_string(),
        reaction: base.reaction.clone(),
        display_name,
        deleted_at: render_dt_opt(base.deleted_at, tz),
        workspace: base.workspace_id.to_string(),
        project: base.project_id.to_string(),
        created_at: render_dt(&base.created_at, tz),
        updated_at: render_dt(&base.updated_at, tz),
        created_by: base.created_by.map(|id| id.to_string()),
        updated_by: base.updated_by.map(|id| id.to_string()),
    })
}

/// `CommentReactionViewSet.list` (`comment.py:186-200`): DRF's default,
/// bare array, newest first. No gate beyond `IsAuthenticated`.
async fn comment_reaction_list(
    State(state): State<AppState>,
    Path((slug, project_raw, comment_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let comment_id = match path_uuid(&comment_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let rows = match super::queries_engage::comment_reaction_list_select(
        &slug, project_id, comment_id, user_id,
    )
    .all(&orm(&pool))
    .await
    {
        Ok(rows) => rows,
        Err(error) => return db_denial(error).into_response(),
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let base = CommentReactionBaseOwned::from(row);
        match render_comment_reaction(&pool, &base, &tenant.timezone).await {
            Ok(value) => out.push(value),
            Err(error) => return error.into_response(),
        }
    }
    match serde_json::to_string(&out) {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(_) => Denial::ServerError.into_response(),
    }
}

/// The comment reaction `INSERT` (`BaseModel.save`: no `updated_by`, no
/// `deleted_at`).
async fn insert_comment_reaction(
    db: &impl sea_orm::ConnectionTrait,
    base: &CommentReactionBaseOwned,
) -> Result<(), DbErr> {
    use sea_orm::Set;
    let row = comment_reactions::ActiveModel {
        id: Set(base.id),
        created_at: Set(base.created_at.into()),
        updated_at: Set(base.updated_at.into()),
        created_by_id: Set(base.created_by),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        project_id: Set(base.project_id),
        workspace_id: Set(base.workspace_id),
        actor_id: Set(base.actor_id),
        comment_id: Set(base.comment_id),
        reaction: Set(base.reaction.clone()),
    };
    comment_reactions::Entity::insert(row)
        .exec_without_returning(db)
        .await
        .map(|_| ())
}

/// `CommentReactionViewSet.create` (`comment.py:202-212`): the only
/// writable field is `reaction`; any integrity failure (duplicate OR
/// dangling comment) answers the specific message.
async fn comment_reaction_create(
    State(state): State<AppState>,
    Path((slug, project_raw, comment_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let comment_id = match path_uuid(&comment_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_allow(&membership, &[20, 15, 5]) {
        return error.into_response();
    }
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody {
        map: mut body,
        is_html,
    } = request_body;
    let mut errors = Map::new();
    // Text-only validation takes no `is_html` (no choice, FK or
    // file-sensitive field); the flag still selects the task-bytes shape.
    let reaction = require_text(&mut errors, &body, "reaction", false);
    if !errors.is_empty() {
        return Denial::BadJson(Value::Object(errors)).into_response();
    }
    let reaction = match reaction {
        Some(reaction) => reaction,
        None => return Denial::ServerError.into_response(),
    };
    let workspace_id = match fetch_project_workspace_unguarded(&pool, &project_id).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    let base = CommentReactionBaseOwned {
        id: uuid::Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
        created_by: Some(user_id),
        updated_by: None,
        project_id,
        workspace_id,
        comment_id,
        actor_id: user_id,
        reaction,
    };
    if let Err(error) = insert_comment_reaction(&orm(&pool), &base).await {
        if is_integrity_violation(&error) {
            return Denial::BadError("Reaction already exists for the user".to_owned())
                .into_response();
        }
        return Denial::ServerError.into_response();
    }
    let epoch = chrono::Utc::now().timestamp();
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("comment_reaction.activity.created".to_owned()),
    );
    // Task bytes render the raw QueryDict (PIDASHCONV-763): form list
    // fields dump as last-wins scalars. In place — validation is done
    // and `body` moves into the dump below.
    if is_html {
        shared_body::project_list_scalars(&mut body, ENGAGE_BODY_SPEC.list_fields);
    }
    activity.insert(
        "requested_data".to_owned(),
        Value::String(python_dumps(&Value::Object(body))),
    );
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::Null);
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert("current_instance".to_owned(), Value::Null);
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(app_origin(&state)));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    match render_comment_reaction(&pool, &base, &tenant.timezone).await {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(text) => json_response(StatusCode::CREATED, text),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `CommentReactionViewSet.destroy` (`comment.py:230-254`).
async fn comment_reaction_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, comment_raw, reaction_code)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let comment_id = match path_uuid(&comment_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_allow(&membership, &[20, 15, 5]) {
        return error.into_response();
    }
    // The body is never read here; drop `req` without parsing.
    let _ = req;
    let row: Option<uuid::Uuid> = match comment_reactions::Entity::find()
        .select_only()
        .column(comment_reactions::Column::Id)
        .join(
            JoinType::InnerJoin,
            comment_reactions::Relation::Workspaces.def(),
        )
        .filter(comment_reactions::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(&slug))
        .filter(comment_reactions::Column::ProjectId.eq(project_id))
        .filter(comment_reactions::Column::CommentId.eq(comment_id))
        .filter(comment_reactions::Column::Reaction.eq(&reaction_code))
        .filter(comment_reactions::Column::ActorId.eq(user_id))
        .into_tuple()
        .one(&orm(&pool))
        .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(stored_uuid) = row else {
        return Denial::NotFound.into_response();
    };
    let stored_id = stored_uuid.to_string();
    let now = chrono::Utc::now();
    if let Err(error) = soft_delete_row(
        &pool,
        SoftDeleteTarget::CommentReaction,
        &stored_uuid,
        &user_id,
        &now,
    )
    .await
    {
        return error.into_response();
    }
    let mut current = Map::new();
    current.insert("reaction".to_owned(), Value::String(reaction_code));
    current.insert("identifier".to_owned(), Value::String(stored_id));
    current.insert(
        "comment_id".to_owned(),
        Value::String(comment_id.to_string()),
    );
    let epoch = chrono::Utc::now().timestamp();
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("comment_reaction.activity.deleted".to_owned()),
    );
    activity.insert("requested_data".to_owned(), Value::Null);
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::Null);
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert(
        "current_instance".to_owned(),
        Value::String(python_dumps(&Value::Object(current))),
    );
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(app_origin(&state)));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    enqueue_soft_delete(&pool, "commentreaction", &stored_uuid).await;
    empty_response(StatusCode::NO_CONTENT)
}

/// `IssueReactionViewSet.list` (`reaction.py:26-40`): DRF's default
/// over the scoped queryset, bare array, newest first. No gate beyond
/// `IsAuthenticated`. (No queries-layer builder covers this queryset —
/// the statement below follows the same Django shape.)
async fn issue_reaction_list(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match path_uuid(&issue_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let rows = match issue_reaction_list_select(&slug, project_id, issue_id, user_id)
        .all(&orm(&pool))
        .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let base = IssueReactionBaseOwned::from(row);
        match render_issue_reaction(&pool, &base, &tenant.timezone).await {
            Ok(value) => out.push(value),
            Err(error) => return error.into_response(),
        }
    }
    match serde_json::to_string(&out) {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(_) => Denial::ServerError.into_response(),
    }
}

/// The `IssueReactionViewSet.get_queryset` scope (`reaction.py:26-40`):
/// tenant + issue filters, the actor's active project membership (the
/// member join reads no tombstone filter, as coded), unarchived project,
/// `DISTINCT`, newest first.
fn issue_reaction_list_select(
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> sea_orm::Select<issue_reactions::Entity> {
    let members: sea_orm::RelationDef =
        issue_reactions::Entity::belongs_to(project_members::Entity)
            .from(issue_reactions::Column::ProjectId)
            .to(project_members::Column::ProjectId)
            .into();
    issue_reactions::Entity::find()
        .distinct()
        .join(
            JoinType::InnerJoin,
            issue_reactions::Relation::Workspaces.def(),
        )
        .join(
            JoinType::InnerJoin,
            issue_reactions::Relation::Projects.def(),
        )
        .join(JoinType::InnerJoin, members)
        .filter(issue_reactions::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issue_reactions::Column::ProjectId.eq(project_id))
        .filter(issue_reactions::Column::IssueId.eq(issue_id))
        .filter(projects::Column::ArchivedAt.is_null())
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(project_members::Column::IsActive.eq(true))
        .order_by_desc(issue_reactions::Column::CreatedAt)
}

/// `IssueReactionViewSet.create` (`reaction.py:42-62`): `reaction` is
/// the only meaningful input (`created_by` / `updated_by` validate but
/// lose to the save audit). Duplicates surface the GENERIC
/// `IntegrityError` body — the view catches nothing.
async fn issue_reaction_create(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match path_uuid(&issue_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_allow(&membership, &[20, 15, 5]) {
        return error.into_response();
    }
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody {
        map: mut body,
        is_html,
    } = request_body;
    let mut errors = Map::new();
    let reaction = require_text(&mut errors, &body, "reaction", false);
    // Writable-but-ignored audit FKs: garbage UUIDs are per-field
    // curly errors, missing users are field errors.
    if let Err(error) = check_fk(
        &pool,
        &mut errors,
        &body,
        "created_by",
        "users",
        true,
        is_html,
    )
    .await
    {
        return error.into_response();
    }
    if let Err(error) = check_fk(
        &pool,
        &mut errors,
        &body,
        "updated_by",
        "users",
        true,
        is_html,
    )
    .await
    {
        return error.into_response();
    }
    if !errors.is_empty() {
        return Denial::BadJson(Value::Object(errors)).into_response();
    }
    let reaction = match reaction {
        Some(reaction) => reaction,
        None => return Denial::ServerError.into_response(),
    };
    let workspace_id = match fetch_project_workspace_unguarded(&pool, &project_id).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    let base = IssueReactionBaseOwned {
        id: uuid::Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
        reaction,
        created_by: Some(user_id),
        updated_by: None,
        project_id,
        workspace_id,
        issue_id,
        actor_id: user_id,
    };
    let row = {
        use sea_orm::Set;
        issue_reactions::ActiveModel {
            id: Set(base.id),
            created_at: Set(base.created_at.into()),
            updated_at: Set(base.updated_at.into()),
            deleted_at: Set(None),
            reaction: Set(base.reaction.clone()),
            created_by_id: Set(base.created_by),
            updated_by_id: Set(None),
            project_id: Set(base.project_id),
            workspace_id: Set(base.workspace_id),
            issue_id: Set(base.issue_id),
            actor_id: Set(base.actor_id),
        }
    };
    if let Err(error) = issue_reactions::Entity::insert(row)
        .exec_without_returning(&orm(&pool))
        .await
    {
        return integrity_denial_orm(error).into_response();
    }
    let epoch = chrono::Utc::now().timestamp();
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("issue_reaction.activity.created".to_owned()),
    );
    // Task bytes render the raw QueryDict (PIDASHCONV-763): form list
    // fields dump as last-wins scalars. In place — validation is done
    // and `body` moves into the dump below.
    if is_html {
        shared_body::project_list_scalars(&mut body, ENGAGE_BODY_SPEC.list_fields);
    }
    activity.insert(
        "requested_data".to_owned(),
        Value::String(python_dumps(&Value::Object(body))),
    );
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert("current_instance".to_owned(), Value::Null);
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(app_origin(&state)));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    match render_issue_reaction(&pool, &base, &tenant.timezone).await {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(text) => json_response(StatusCode::CREATED, text),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `IssueReactionViewSet.destroy` (`reaction.py:64-85`).
async fn issue_reaction_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, reaction_code)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match path_uuid(&issue_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_allow(&membership, &[20, 15, 5]) {
        return error.into_response();
    }
    let _ = req;
    let row: Option<uuid::Uuid> = match issue_reactions::Entity::find()
        .select_only()
        .column(issue_reactions::Column::Id)
        .join(
            JoinType::InnerJoin,
            issue_reactions::Relation::Workspaces.def(),
        )
        .filter(issue_reactions::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(&slug))
        .filter(issue_reactions::Column::ProjectId.eq(project_id))
        .filter(issue_reactions::Column::IssueId.eq(issue_id))
        .filter(issue_reactions::Column::Reaction.eq(&reaction_code))
        .filter(issue_reactions::Column::ActorId.eq(user_id))
        .into_tuple()
        .one(&orm(&pool))
        .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(stored_uuid) = row else {
        return Denial::NotFound.into_response();
    };
    let stored_id = stored_uuid.to_string();
    let now = chrono::Utc::now();
    if let Err(error) = soft_delete_row(
        &pool,
        SoftDeleteTarget::IssueReaction,
        &stored_uuid,
        &user_id,
        &now,
    )
    .await
    {
        return error.into_response();
    }
    let mut current = Map::new();
    current.insert("reaction".to_owned(), Value::String(reaction_code));
    current.insert("identifier".to_owned(), Value::String(stored_id));
    let epoch = chrono::Utc::now().timestamp();
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("issue_reaction.activity.deleted".to_owned()),
    );
    activity.insert("requested_data".to_owned(), Value::Null);
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert(
        "current_instance".to_owned(),
        Value::String(python_dumps(&Value::Object(current))),
    );
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(app_origin(&state)));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    enqueue_soft_delete(&pool, "issuereaction", &stored_uuid).await;
    empty_response(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Subscribers
// ---------------------------------------------------------------------------

/// Required FK input: missing → "This field is required." (garbage
/// UUIDs are per-field curly errors out of `check_fk`).
async fn require_fk(
    pool: &sqlx::PgPool,
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    table: &str,
    is_html: bool,
) -> Result<Option<uuid::Uuid>, Denial> {
    match check_fk(pool, errors, body, field, table, false, is_html).await? {
        Presence::Value(id) => Ok(Some(id)),
        _ => {
            if !errors.contains_key(field) {
                push_error(errors, field, "This field is required.".to_owned());
            }
            Ok(None)
        }
    }
}

/// `IssueSubscriberViewSet.list` (`subscriber.py:52-57`): the ACTIVE
/// project-member roster (default-manager guard + `is_active`),
/// newest first, each through `ProjectMemberLiteSerializer` — with NO
/// `is_subscribed` annotation, so the key stays absent. The issue
/// itself is never read.
async fn subscriber_list(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    if path_uuid(&issue_raw).is_err() {
        return crate::edge::proxy(State(state), req).await;
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_entity(&membership, true) {
        return error.into_response();
    }
    let rows = match subscriber_roster_select(&slug, project_id)
        .into_model::<RosterRow>()
        .all(&orm(&pool))
        .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let id = row.id;
        let member = match row.member_id {
            None => None,
            Some(member_id) => {
                // The LEFT JOIN hit a user row: its NOT NULL columns are set.
                let (
                    Some(first_name),
                    Some(last_name),
                    Some(avatar),
                    Some(is_bot),
                    Some(display_name),
                ) = (
                    row.first_name,
                    row.last_name,
                    row.avatar,
                    row.is_bot,
                    row.display_name,
                )
                else {
                    return Denial::ServerError.into_response();
                };
                let avatar_url =
                    match logo_or_cover_url(&pool, Some(avatar.clone()), row.avatar_asset_id).await
                    {
                        Ok(url) => url,
                        Err(error) => return error.into_response(),
                    };
                Some(UserLiteOwned {
                    id: member_id.to_string(),
                    first_name,
                    last_name,
                    avatar,
                    avatar_url,
                    is_bot,
                    display_name,
                })
            }
        };
        let id_string = id.to_string();
        let member_row =
            member.as_ref().map(
                |row| pidash_services::app_project::ser_shared::UserLiteRow {
                    id: &row.id,
                    first_name: &row.first_name,
                    last_name: &row.last_name,
                    avatar: &row.avatar,
                    avatar_url: row.avatar_url.as_deref(),
                    is_bot: row.is_bot,
                    display_name: &row.display_name,
                },
            );
        let input = pidash_services::app_project::ser_shared::ProjectMemberLiteRow {
            id: &id_string,
            member: member_row,
            is_subscribed: None,
        };
        let view =
            pidash_services::app_project::ser_shared::project_member_lite_to_representation(&input);
        match serde_json::to_value(&view) {
            Ok(value) => out.push(value),
            Err(_) => return Denial::ServerError.into_response(),
        }
    }
    match serde_json::to_string(&out) {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(_) => Denial::ServerError.into_response(),
    }
}

/// One roster row: the membership id plus the member's lite columns,
/// all null when the LEFT JOIN misses.
#[derive(Debug, FromQueryResult)]
struct RosterRow {
    id: uuid::Uuid,
    member_id: Option<uuid::Uuid>,
    first_name: Option<String>,
    last_name: Option<String>,
    avatar: Option<String>,
    avatar_asset_id: Option<uuid::Uuid>,
    is_bot: Option<bool>,
    display_name: Option<String>,
}

/// The `subscriber.py:52-57` roster: live, active project members in the
/// slug's workspace (INNER JOIN workspaces, LEFT JOIN users), newest
/// first.
fn subscriber_roster_select(
    slug: &str,
    project_id: uuid::Uuid,
) -> sea_orm::Select<project_members::Entity> {
    let member: sea_orm::RelationDef = project_members::Entity::belongs_to(users::Entity)
        .from(project_members::Column::MemberId)
        .to(users::Column::Id)
        .into();
    project_members::Entity::find()
        .select_only()
        .column(project_members::Column::Id)
        .column_as(users::Column::Id, "member_id")
        .column_as(users::Column::FirstName, "first_name")
        .column_as(users::Column::LastName, "last_name")
        .column_as(users::Column::Avatar, "avatar")
        .column_as(users::Column::AvatarAssetId, "avatar_asset_id")
        .column_as(users::Column::IsBot, "is_bot")
        .column_as(users::Column::DisplayName, "display_name")
        .join(
            JoinType::InnerJoin,
            project_members::Relation::Workspaces.def(),
        )
        .join(JoinType::LeftJoin, member)
        .filter(project_members::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::IsActive.eq(true))
        .order_by_desc(project_members::Column::CreatedAt)
}

/// The `issue_subscribers` `INSERT` (`BaseModel.save`: no `updated_by`).
async fn insert_subscriber(
    db: &impl sea_orm::ConnectionTrait,
    base: &SubscriberOwned,
) -> Result<(), DbErr> {
    use sea_orm::Set;
    let row = issue_subscribers::ActiveModel {
        id: Set(base.id),
        created_at: Set(base.created_at.into()),
        updated_at: Set(base.updated_at.into()),
        deleted_at: Set(base.deleted_at.map(Into::into)),
        created_by_id: Set(base.created_by),
        updated_by_id: Set(None),
        project_id: Set(base.project_id),
        workspace_id: Set(base.workspace_id),
        issue_id: Set(base.issue_id),
        subscriber_id: Set(base.subscriber_id),
    };
    issue_subscribers::Entity::insert(row)
        .exec_without_returning(db)
        .await
        .map(|_| ())
}

/// `IssueSubscriberViewSet.create`: DRF's default
/// (`subscriber.py:16-51`). `deleted_at` is REQUIRED input (the
/// `unique_together` uniqueness extras), `subscriber` is a required
/// user FK, audit inputs validate-then-lose to the save.
async fn subscriber_create(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match path_uuid(&issue_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_entity(&membership, false) {
        return error.into_response();
    }
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody { map: body, is_html } = request_body;
    let mut errors = Map::new();
    // Error order is DRF field order (`deleted_at` before `subscriber`).
    let deleted_at = match check_datetime(&mut errors, &body, "deleted_at", true) {
        Presence::Missing => {
            if !errors.contains_key("deleted_at") {
                push_error(
                    &mut errors,
                    "deleted_at",
                    "This field is required.".to_owned(),
                );
            }
            None
        }
        Presence::Null => Some(None),
        Presence::Value(dt) => Some(Some(dt)),
    };
    if let Err(error) = check_fk(
        &pool,
        &mut errors,
        &body,
        "created_by",
        "users",
        true,
        is_html,
    )
    .await
    {
        return error.into_response();
    }
    if let Err(error) = check_fk(
        &pool,
        &mut errors,
        &body,
        "updated_by",
        "users",
        true,
        is_html,
    )
    .await
    {
        return error.into_response();
    }
    let subscriber =
        match require_fk(&pool, &mut errors, &body, "subscriber", "users", is_html).await {
            Ok(subscriber) => subscriber,
            Err(error) => return error.into_response(),
        };
    if !errors.is_empty() {
        return Denial::BadJson(Value::Object(errors)).into_response();
    }
    let (Some(subscriber_id), Some(deleted_at)) = (subscriber, deleted_at) else {
        return Denial::ServerError.into_response();
    };
    let workspace_id = match fetch_project_workspace_unguarded(&pool, &project_id).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    let base = SubscriberOwned {
        id: uuid::Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        deleted_at,
        created_by: Some(user_id),
        updated_by: None,
        project_id,
        workspace_id,
        issue_id,
        subscriber_id,
    };
    if let Err(error) = insert_subscriber(&orm(&pool), &base).await {
        return integrity_denial_orm(error).into_response();
    }
    match render_subscriber(&base, &tenant.timezone) {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(text) => json_response(StatusCode::CREATED, text),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// The subscription-row `.get()` behind `destroy` / `unsubscribe`:
/// default manager, tenant-scoped, keyed by subscriber user id.
async fn fetch_subscription(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    subscriber_id: &uuid::Uuid,
) -> Result<Option<uuid::Uuid>, Denial> {
    issue_subscribers::Entity::find()
        .select_only()
        .column(issue_subscribers::Column::Id)
        .join(
            JoinType::InnerJoin,
            issue_subscribers::Relation::Workspaces.def(),
        )
        .filter(issue_subscribers::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issue_subscribers::Column::ProjectId.eq(*project_id))
        .filter(issue_subscribers::Column::IssueId.eq(*issue_id))
        .filter(issue_subscribers::Column::SubscriberId.eq(*subscriber_id))
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(db_denial)
}

/// `IssueSubscriberViewSet.destroy` (`subscriber.py:59-67`): the kwarg
/// is the subscriber's USER id.
async fn subscriber_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, subscriber_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let (issue_id, subscriber_id) = match (path_uuid(&issue_raw), path_uuid(&subscriber_raw)) {
        (Ok(issue_id), Ok(subscriber_id)) => (issue_id, subscriber_id),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_entity(&membership, false) {
        return error.into_response();
    }
    let _ = req;
    let pk = match fetch_subscription(&pool, &slug, &project_id, &issue_id, &subscriber_id).await {
        Ok(Some(pk)) => pk,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    if let Err(error) = soft_delete_row(
        &pool,
        SoftDeleteTarget::IssueSubscriber,
        &pk,
        &user_id,
        &now,
    )
    .await
    {
        return error.into_response();
    }
    enqueue_soft_delete(&pool, "issuesubscriber", &pk).await;
    empty_response(StatusCode::NO_CONTENT)
}

/// `subscribe` (`subscriber.py:70-84`): lite gate, duplicate 400,
/// `workspace` backfilled from the project at save.
async fn subscribe(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match path_uuid(&issue_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_lite(&membership) {
        return error.into_response();
    }
    // The body is never read here; drop `req` without parsing.
    let _ = req;
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    match fetch_subscription(&pool, &slug, &project_id, &issue_id, &user_id).await {
        Ok(Some(_)) => {
            return Denial::BadMessage("User already subscribed to the issue.".to_owned())
                .into_response();
        }
        Ok(None) => {}
        Err(error) => return error.into_response(),
    }
    let workspace_id = match fetch_project_workspace_unguarded(&pool, &project_id).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    let base = SubscriberOwned {
        id: uuid::Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
        created_by: Some(user_id),
        updated_by: None,
        project_id,
        workspace_id,
        issue_id,
        subscriber_id: user_id,
    };
    if let Err(error) = insert_subscriber(&orm(&pool), &base).await {
        return integrity_denial_orm(error).into_response();
    }
    match render_subscriber(&base, &tenant.timezone) {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(text) => json_response(StatusCode::CREATED, text),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `unsubscribe` (`subscriber.py:87-95`).
async fn unsubscribe(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match path_uuid(&issue_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_lite(&membership) {
        return error.into_response();
    }
    let _ = req;
    let pk = match fetch_subscription(&pool, &slug, &project_id, &issue_id, &user_id).await {
        Ok(Some(pk)) => pk,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    if let Err(error) = soft_delete_row(
        &pool,
        SoftDeleteTarget::IssueSubscriber,
        &pk,
        &user_id,
        &now,
    )
    .await
    {
        return error.into_response();
    }
    enqueue_soft_delete(&pool, "issuesubscriber", &pk).await;
    empty_response(StatusCode::NO_CONTENT)
}

/// `subscription_status` (`subscriber.py:98-104`).
async fn subscription_status(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match path_uuid(&issue_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_lite(&membership) {
        return error.into_response();
    }
    let _ = req;
    match fetch_subscription(&pool, &slug, &project_id, &issue_id, &user_id).await {
        Ok(subscribed) => json_response(
            StatusCode::OK,
            format!("{{\"subscribed\":{}}}", subscribed.is_some()),
        ),
        Err(error) => error.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Worklogs
// ---------------------------------------------------------------------------

/// An owned `issue_worklogs` row: every column the
/// `IssueWorklogSerializer` reads (`serializers/worklog.py:11-28` —
/// `__all__` plus the `actor_detail` nest).
struct WorklogBaseOwned {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    minutes: i32,
    logged_on: chrono::NaiveDate,
    note: String,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    actor_id: Option<uuid::Uuid>,
}

impl From<issue_worklogs::Model> for WorklogBaseOwned {
    fn from(row: issue_worklogs::Model) -> Self {
        Self {
            id: row.id,
            created_at: utc(row.created_at),
            updated_at: utc(row.updated_at),
            deleted_at: row.deleted_at.map(utc),
            minutes: row.minutes,
            logged_on: row.logged_on,
            note: row.note,
            created_by: row.created_by_id,
            updated_by: row.updated_by_id,
            project_id: row.project_id,
            workspace_id: row.workspace_id,
            issue_id: row.issue_id,
            actor_id: row.actor_id,
        }
    }
}

/// The `IssueWorklogSerializer` shape in field order: the two declared
/// fields (`id`, `actor_detail`) first, then the model columns
/// (`worklog.py:11-28`, order verified against live Django).
#[derive(Debug, serde::Serialize)]
struct WorklogView {
    id: String,
    actor_detail: Option<Value>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    minutes: i32,
    logged_on: String,
    note: String,
    created_by: Option<String>,
    updated_by: Option<String>,
    project: String,
    workspace: String,
    issue: String,
    actor: Option<String>,
}

fn assemble_worklog_view(
    base: &WorklogBaseOwned,
    actor_detail: Option<Value>,
    tz: &Tz,
) -> WorklogView {
    WorklogView {
        id: base.id.to_string(),
        actor_detail,
        created_at: render_dt(&base.created_at, tz),
        updated_at: render_dt(&base.updated_at, tz),
        deleted_at: render_dt_opt(base.deleted_at, tz),
        minutes: base.minutes,
        logged_on: render_date(&base.logged_on),
        note: base.note.clone(),
        created_by: base.created_by.map(|id| id.to_string()),
        updated_by: base.updated_by.map(|id| id.to_string()),
        project: base.project_id.to_string(),
        workspace: base.workspace_id.to_string(),
        issue: base.issue_id.to_string(),
        actor: base.actor_id.map(|id| id.to_string()),
    }
}

/// Render one entry: the actor nest loads lazily (no actor join on any
/// worklog statement), exactly like the serializer over a fresh
/// instance. A null actor renders `"actor_detail": null` (DRF renders
/// the nested serializer `None`).
async fn render_worklog(
    pool: &sqlx::PgPool,
    base: &WorklogBaseOwned,
    tz: &Tz,
) -> Result<Value, Denial> {
    let actor_detail = match base.actor_id {
        None => None,
        Some(id) => Some(
            fetch_user_lite(pool, &id)
                .await
                .map(|row| user_lite_value(&row))??,
        ),
    };
    let view = assemble_worklog_view(base, actor_detail, tz);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// Python `int(text, 10)` over ASCII digits: surrounding `str.strip()`,
/// an optional sign, digits with single `_` separators between digits
/// only. Saturates past `i64` (the min/max arms below keep their
/// verdicts — any saturating magnitude is out of range either way).
/// Non-ASCII decimal digits are rejected (Tier 3: `int()` accepts
/// them; no Pi Dash client sends them).
fn parse_worklog_int(text: &str) -> Option<i64> {
    let text = text.trim_matches(is_py_strip_ws);
    let (negative, digits) = match text.strip_prefix(['+', '-']) {
        Some(rest) => (text.starts_with('-'), rest),
        None => (false, text),
    };
    if digits.is_empty() {
        return None;
    }
    let mut cleaned = String::with_capacity(digits.len());
    let mut prev_underscore = true;
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            continue;
        }
        if !ch.is_ascii_digit() {
            return None;
        }
        prev_underscore = false;
        cleaned.push(ch);
    }
    if prev_underscore {
        return None;
    }
    // Past i128 the verdict is already fixed: positive overflows the
    // max arm, negative overflows the min arm.
    let magnitude: i128 = match cleaned.parse() {
        Ok(magnitude) => magnitude,
        Err(_) => return Some(if negative { i64::MIN } else { i64::MAX }),
    };
    let signed = if negative { -magnitude } else { magnitude };
    Some(signed.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}

/// Python `re` `\s` (for the `re_decimal` tail below): Rust
/// `White_Space` plus U+001C-U+001F, like [`is_py_strip_ws`] (the
/// remaining exotic Unicode spaces are Tier 3).
fn is_re_space(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// DRF `IntegerField` input for `minutes` (`fields.py:910-918`):
/// `int(re_decimal.sub('', str(data)))` where `re_decimal` strips one
/// trailing `.0*` + whitespace suffix — then the model
/// `MinValueValidator(1)` and the field max 2147483647. Every arm below
/// is probed against live Django: `" 90 "` / `90.0` / `"90.0"` /
/// `"90."` / `"+90"` / `"1_0"` pass; `1.5` / `"9.5"` / `true` / `[1]` /
/// `""` / `"_1"` / `"1__0"` fail invalid; `0` / `-30` / `-2**40` fail
/// with only the min-1 message (no lower field bound rides along);
/// `2**40` fails with only the max message; >1000-char strings fail
/// "String value too large."
fn check_worklog_minutes(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
) -> Presence<i32> {
    const MAX_STRING_LENGTH: usize = 1000;
    let value = match body.get("minutes") {
        None => return Presence::Missing,
        Some(Value::Null) => {
            push_error(errors, "minutes", "This field may not be null.".to_owned());
            return Presence::Missing;
        }
        Some(value) => value,
    };
    let text: String = match value {
        Value::String(text) => {
            // `len(data)` counts code points, like Python (`fields.py:911`).
            if text.chars().count() > MAX_STRING_LENGTH {
                push_error(errors, "minutes", "String value too large.".to_owned());
                return Presence::Missing;
            }
            text.clone()
        }
        Value::Number(number) => py_str_value(&Value::Number(number.clone())),
        _ => {
            push_error(errors, "minutes", "A valid integer is required.".to_owned());
            return Presence::Missing;
        }
    };
    // `re_decimal = re.compile(r'\.0*\s*$')`: only a trailing `.0*`
    // suffix strips (the `$` anchors; only the last dot can match to
    // the end).
    let mut core = text.as_str();
    if let Some(dot) = core.rfind('.') {
        let (head, tail) = core.split_at(dot);
        let zeros = tail[1..].trim_start_matches('0');
        if zeros.trim_matches(is_re_space).is_empty() {
            core = head;
        }
    }
    let number = match parse_worklog_int(core) {
        Some(number) => number,
        None => {
            push_error(errors, "minutes", "A valid integer is required.".to_owned());
            return Presence::Missing;
        }
    };
    if number < 1 {
        push_error(
            errors,
            "minutes",
            "Ensure this value is greater than or equal to 1.".to_owned(),
        );
        return Presence::Missing;
    }
    if number > 2_147_483_647 {
        push_error(
            errors,
            "minutes",
            "Ensure this value is less than or equal to 2147483647.".to_owned(),
        );
        return Presence::Missing;
    }
    Presence::Value(number as i32)
}

/// Python 3.12 `date.fromisoformat` as Django's `parse_date` meets it
/// (`dateparse.py:67-78`): extended `YYYY-MM-DD`, basic `YYYYMMDD` and
/// week `YYYY-Www-D` / `YYYYWwwD` — all strict, all verified against
/// live Django, which rejects ordinal dates, padded input and year 0
/// (`datetime.date` starts at year 1; chrono would accept year 0, so
/// every arm below guards it explicitly).
fn worklog_fromisodate(text: &str) -> Option<chrono::NaiveDate> {
    let bytes = text.as_bytes();
    // Extended `YYYY-MM-DD`.
    if bytes.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-' {
        let year: i32 = text[0..4].parse().ok()?;
        let month: u32 = text[5..7].parse().ok()?;
        let day: u32 = text[8..10].parse().ok()?;
        if year < 1 {
            return None;
        }
        return chrono::NaiveDate::from_ymd_opt(year, month, day);
    }
    // Week `YYYY-Www-D`.
    if bytes.len() == 10 && bytes[4] == b'-' && bytes[5] == b'W' && bytes[8] == b'-' {
        let year: i32 = text[0..4].parse().ok()?;
        let week: u32 = text[6..8].parse().ok()?;
        let weekday: u32 = text[9..10].parse().ok()?;
        if year < 1 {
            return None;
        }
        let weekday = u8::try_from(weekday)
            .ok()?
            .checked_sub(1)
            .and_then(|w| chrono::Weekday::try_from(w).ok())?;
        return chrono::NaiveDate::from_isoywd_opt(year, week, weekday);
    }
    // Basic `YYYYMMDD` / week `YYYYWwwD`.
    if bytes.len() == 8 && bytes.iter().all(|b| b.is_ascii_digit()) {
        let year: i32 = text[0..4].parse().ok()?;
        let month: u32 = text[4..6].parse().ok()?;
        let day: u32 = text[6..8].parse().ok()?;
        if year < 1 {
            return None;
        }
        return chrono::NaiveDate::from_ymd_opt(year, month, day);
    }
    if bytes.len() == 8 && bytes[4] == b'W' {
        let year: i32 = text[0..4].parse().ok()?;
        let week: u32 = text[5..7].parse().ok()?;
        let weekday: u32 = text[7..8].parse().ok()?;
        if !text[0..4].bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if year < 1 {
            return None;
        }
        let weekday = u8::try_from(weekday)
            .ok()?
            .checked_sub(1)
            .and_then(|w| chrono::Weekday::try_from(w).ok())?;
        return chrono::NaiveDate::from_isoywd_opt(year, week, weekday);
    }
    None
}

/// Django `date_re` fallback (`dateparse.py:13,76-78`):
/// `(\d{4})-(\d{1,2})-(\d{1,2})$`, where `$` also matches just before
/// ONE trailing newline (probed: `"2026-10-20\n"` passes,
/// `"2026-10-20\r"` / `"\n2026-10-20"` fail). Well formatted but
/// impossible dates fail too.
fn worklog_date_re(text: &str) -> Option<chrono::NaiveDate> {
    let core = text.strip_suffix('\n').unwrap_or(text);
    let mut parts = core.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    if year.len() != 4 || month.is_empty() || month.len() > 2 || day.is_empty() || day.len() > 2 {
        return None;
    }
    if !year.bytes().all(|b| b.is_ascii_digit())
        || !month.bytes().all(|b| b.is_ascii_digit())
        || !day.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let year: i32 = year.parse().ok()?;
    if year < 1 {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(year, month.parse().ok()?, day.parse().ok()?)
}

/// DRF `DateField` input for `logged_on` (`fields.py:1231-1258` over
/// `DATE_INPUT_FORMATS = [iso-8601]`): Django `parse_date` or the
/// invalid message. Non-strings always miss; the parsed date renders
/// back zero-padded (`"2026-1-5"` stores as `2026-01-05`).
fn check_worklog_date(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
) -> Presence<chrono::NaiveDate> {
    const INVALID: &str = "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.";
    match body.get("logged_on") {
        None => Presence::Missing,
        Some(Value::Null) => {
            push_error(
                errors,
                "logged_on",
                "This field may not be null.".to_owned(),
            );
            Presence::Missing
        }
        Some(Value::String(text)) => {
            match worklog_fromisodate(text).or_else(|| worklog_date_re(text)) {
                Some(date) => Presence::Value(date),
                None => {
                    push_error(errors, "logged_on", INVALID.to_owned());
                    Presence::Missing
                }
            }
        }
        _ => {
            push_error(errors, "logged_on", INVALID.to_owned());
            Presence::Missing
        }
    }
}

/// The validated worklog write triple: `minutes` + `logged_on`
/// (required on create, skipped-when-missing on partial) and `note`
/// (blankable, defaulting to `""` on create).
#[derive(Debug, PartialEq)]
struct WorklogAttrs {
    minutes: Option<i32>,
    logged_on: Option<chrono::NaiveDate>,
    note: Option<String>,
}

/// Validate one worklog write body in serializer field order
/// (`minutes`, `logged_on`, `note` — the error dict keeps it): every
/// other input key is read-only or unknown, both ignored by DRF.
/// `partial` selects create (missing → required errors + `""` note
/// default) vs partial-update (missing → untouched) semantics.
fn validate_worklog_input(body: &Map<String, Value>, partial: bool) -> Result<WorklogAttrs, Value> {
    let mut errors = Map::new();
    let minutes = match check_worklog_minutes(&mut errors, body) {
        Presence::Value(minutes) => Some(minutes),
        _ => {
            if !partial && !body.contains_key("minutes") {
                push_error(&mut errors, "minutes", "This field is required.".to_owned());
            }
            None
        }
    };
    let logged_on = match check_worklog_date(&mut errors, body) {
        Presence::Value(date) => Some(date),
        _ => {
            if !partial && !body.contains_key("logged_on") {
                push_error(
                    &mut errors,
                    "logged_on",
                    "This field is required.".to_owned(),
                );
            }
            None
        }
    };
    let note = match check_text(&mut errors, body, "note", None, false, true) {
        Presence::Value(note) => Some(note),
        Presence::Null => None,
        Presence::Missing => {
            if partial || !errors.is_empty() {
                None
            } else {
                Some(String::new())
            }
        }
    };
    if errors.is_empty() {
        Ok(WorklogAttrs {
            minutes,
            logged_on,
            note,
        })
    } else {
        Err(Value::Object(errors))
    }
}

/// The `IssueWorklogViewSet.get_queryset` scope (`worklog.py:45-63`):
/// tenant + issue filters, live project membership, unarchived
/// project, `DISTINCT`, newest first. Only `issue_worklogs` columns are
/// selected (the serializer reads no joined nest but the lazily-loaded
/// actor). The member join excludes soft-deleted rows (the reverse-FK
/// traversal uses the default manager).
fn worklog_list_select(
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> sea_orm::Select<issue_worklogs::Entity> {
    let members: sea_orm::RelationDef = projects::Entity::belongs_to(project_members::Entity)
        .from(projects::Column::Id)
        .to(project_members::Column::ProjectId)
        .into();
    issue_worklogs::Entity::find()
        .distinct()
        .join(
            JoinType::InnerJoin,
            issue_worklogs::Relation::Workspaces.def(),
        )
        .join(
            JoinType::InnerJoin,
            issue_worklogs::Relation::Projects.def(),
        )
        .join(JoinType::InnerJoin, members)
        .filter(issue_worklogs::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issue_worklogs::Column::ProjectId.eq(project_id))
        .filter(issue_worklogs::Column::IssueId.eq(issue_id))
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .filter(projects::Column::ArchivedAt.is_null())
        .order_by_desc(issue_worklogs::Column::CreatedAt)
}

/// `IssueWorklogViewSet.list` (`worklog.py:45-63` over DRF's default):
/// class `IsAuthenticated` only — outsiders list `[]`, every active
/// project member reads, bare array, newest first.
async fn worklog_list(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match path_uuid(&issue_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let ctx = match read_context(&state, &slug, &project_raw, issue_id, extension).await {
        Ok(ctx) => ctx,
        Err(error) => return error.into_response(),
    };
    let rows = match worklog_list_select(&ctx.slug, ctx.project_id, ctx.issue_id, ctx.user_id)
        .all(&orm(&ctx.pool))
        .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let base = WorklogBaseOwned::from(row);
        match render_worklog(&ctx.pool, &base, &ctx.timezone).await {
            Ok(value) => out.push(value),
            Err(error) => return error.into_response(),
        }
    }
    match serde_json::to_string(&out) {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(_) => Denial::ServerError.into_response(),
    }
}

/// `IssueWorklogViewSet.retrieve`: `get_object` over the list queryset;
/// a miss is the model-naming `Http404` detail body.
async fn worklog_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let (issue_id, pk) = match (path_uuid(&issue_raw), path_uuid(&pk_raw)) {
        (Ok(issue_id), Ok(pk)) => (issue_id, pk),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let ctx = match read_context(&state, &slug, &project_raw, issue_id, extension).await {
        Ok(ctx) => ctx,
        Err(error) => return error.into_response(),
    };
    let rows = match worklog_list_select(&ctx.slug, ctx.project_id, ctx.issue_id, ctx.user_id)
        .filter(issue_worklogs::Column::Id.eq(pk))
        .all(&orm(&ctx.pool))
        .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(row) = rows.into_iter().next() else {
        return Denial::NotFoundWorklog.into_response();
    };
    let base = WorklogBaseOwned::from(row);
    match render_worklog(&ctx.pool, &base, &ctx.timezone).await {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(body) => json_response(StatusCode::OK, body),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `IssueWorklogViewSet._can_log` (`worklog.py:65-93`): any live
/// project membership exists, and the caller is a project ADMIN, a
/// workspace ADMIN, or a live assignee of the issue.
async fn worklog_can_log(
    pool: &sqlx::PgPool,
    membership: &Membership,
    issue_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    if membership.project_role.is_none() {
        return Ok(false);
    }
    if membership.project_role == Some(20) || membership.workspace_role == Some(20) {
        return Ok(true);
    }
    let assigned: Option<uuid::Uuid> = issue_assignees::Entity::find()
        .select_only()
        .column(issue_assignees::Column::Id)
        .filter(issue_assignees::Column::IssueId.eq(*issue_id))
        .filter(issue_assignees::Column::AssigneeId.eq(*user_id))
        .filter(issue_assignees::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(db_denial)?;
    Ok(assigned.is_some())
}

/// The scoped `.get()` behind `partial_update` / `destroy`
/// (`worklog.py:132,154`): default manager (soft-deleted rows miss),
/// tenant-scoped, pk included.
async fn fetch_scoped_worklog(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<Option<WorklogBaseOwned>, Denial> {
    let row = issue_worklogs::Entity::find()
        .join(
            JoinType::InnerJoin,
            issue_worklogs::Relation::Workspaces.def(),
        )
        .filter(issue_worklogs::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issue_worklogs::Column::ProjectId.eq(*project_id))
        .filter(issue_worklogs::Column::IssueId.eq(*issue_id))
        .filter(issue_worklogs::Column::Id.eq(*pk))
        .one(&orm(pool))
        .await
        .map_err(db_denial)?;
    Ok(row.map(WorklogBaseOwned::from))
}

/// `IssueWorklogViewSet.create` (`worklog.py:95-128`): ADMIN + MEMBER
/// gate, live-issue lookup, intake refusal, `_can_log`, validation,
/// save (workspace backfilled from the project, actor forced to the
/// caller), one `worklog.activity.created` enqueue — and no
/// `model_activity` (the view imports only `issue_activity`).
async fn worklog_create(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match path_uuid(&issue_raw) {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_allow(&membership, &[20, 15]) {
        return error.into_response();
    }
    // `Issue.objects.get(pk=issue_id, project_id=project_id)`: default
    // manager (soft-deleted rows miss), unscoped by workspace slug.
    let db = orm(&pool);
    let issue: Option<uuid::Uuid> = match issues::Entity::find()
        .select_only()
        .column(issues::Column::Id)
        .filter(issues::Column::Id.eq(issue_id))
        .filter(issues::Column::ProjectId.eq(project_id))
        .filter(issues::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&db)
        .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if issue.is_none() {
        return Denial::NotFound.into_response();
    }
    // The intake refusal precedes `_can_log` (`worklog.py:98-112`).
    let intake: Option<uuid::Uuid> = match intake_issues::Entity::find()
        .select_only()
        .column(intake_issues::Column::Id)
        .join(
            JoinType::InnerJoin,
            intake_issues::Relation::Workspaces.def(),
        )
        .filter(intake_issues::Column::IssueId.eq(issue_id))
        .filter(intake_issues::Column::Status.is_in([-2, 0]))
        .filter(workspaces::Column::Slug.eq(&slug))
        .filter(intake_issues::Column::ProjectId.eq(project_id))
        .filter(intake_issues::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&db)
        .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if intake.is_some() {
        return Denial::BadError("Time cannot be logged on an intake item.".to_owned())
            .into_response();
    }
    match worklog_can_log(&pool, &membership, &issue_id, &user_id).await {
        Ok(true) => {}
        Ok(false) => {
            return Denial::ForbiddenError(
                "Only admins and assignees can log time on this work item.".to_owned(),
            )
            .into_response();
        }
        Err(error) => return error.into_response(),
    }
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody { map: body, .. } = request_body;
    let attrs = match validate_worklog_input(&body, false) {
        Ok(attrs) => attrs,
        Err(errors) => return Denial::BadJson(errors).into_response(),
    };
    // `serializer.save(project_id=..., issue_id=..., actor=user)`:
    // `ProjectBaseModel.save` backfills the workspace from the project
    // (lazy forward-FK read — soft-deleted projects count) and
    // `BaseModel.save` stamps `created_by`, leaving `updated_by` null.
    let workspace_id = match fetch_project_workspace_unguarded(&pool, &project_id).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    // Full create: validation guarantees both (missing pushed
    // required errors above).
    let (Some(minutes), Some(logged_on)) = (attrs.minutes, attrs.logged_on) else {
        return Denial::ServerError.into_response();
    };
    let now = chrono::Utc::now();
    let base = WorklogBaseOwned {
        id: uuid::Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
        minutes,
        logged_on,
        note: attrs.note.unwrap_or_default(),
        created_by: Some(user_id),
        updated_by: None,
        project_id,
        workspace_id,
        issue_id,
        actor_id: Some(user_id),
    };
    let row = {
        use sea_orm::Set;
        issue_worklogs::ActiveModel {
            id: Set(base.id),
            created_at: Set(base.created_at.into()),
            updated_at: Set(base.updated_at.into()),
            deleted_at: Set(None),
            minutes: Set(base.minutes),
            logged_on: Set(base.logged_on),
            note: Set(base.note.clone()),
            created_by_id: Set(base.created_by),
            updated_by_id: Set(None),
            project_id: Set(base.project_id),
            workspace_id: Set(base.workspace_id),
            issue_id: Set(base.issue_id),
            actor_id: Set(base.actor_id),
        }
    };
    if let Err(error) = issue_worklogs::Entity::insert(row)
        .exec_without_returning(&db)
        .await
    {
        return integrity_denial_orm(error).into_response();
    }
    let rendered = match render_worklog(&pool, &base, &tenant.timezone).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let epoch = chrono::Utc::now().timestamp();
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("worklog.activity.created".to_owned()),
    );
    // `requested_data` is the full serialized response
    // (`json.dumps(serializer.data)`), `current_instance` is null.
    activity.insert(
        "requested_data".to_owned(),
        Value::String(python_dumps(&rendered)),
    );
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert("current_instance".to_owned(), Value::Null);
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(app_origin(&state)));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    match serde_json::to_string(&rendered) {
        Ok(text) => json_response(StatusCode::CREATED, text),
        Err(_) => Denial::ServerError.into_response(),
    }
}

/// `IssueWorklogViewSet.partial_update` (`worklog.py:130-150`): ADMIN +
/// creator gate, scoped lookup, partial validation (missing fields stay
/// untouched, an empty body still stamps `updated_at`/`updated_by`
/// through the save), one `worklog.activity.updated` enqueue carrying
/// the raw body and the before-image render.
async fn worklog_partial_update(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let (issue_id, pk) = match (path_uuid(&issue_raw), path_uuid(&pk_raw)) {
        (Ok(issue_id), Ok(pk)) => (issue_id, pk),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_admin_creator(
        &pool,
        &slug,
        &project_id,
        &user_id,
        CreatorModel::IssueWorklog,
        &pk,
    )
    .await
    {
        return error.into_response();
    }
    let stored = match fetch_scoped_worklog(&pool, &slug, &project_id, &issue_id, &pk).await {
        Ok(Some(base)) => base,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(error) => return error.into_response(),
    };
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody { map: body, is_html } = request_body;
    // Task bytes render the raw QueryDict (PIDASHCONV-763): form list
    // fields dump as last-wins scalars. A projected COPY — validation
    // below keeps the arrays.
    let requested_data = if is_html {
        let mut projected = body.clone();
        shared_body::project_list_scalars(&mut projected, ENGAGE_BODY_SPEC.list_fields);
        python_dumps(&Value::Object(projected))
    } else {
        python_dumps(&Value::Object(body.clone()))
    };
    let current_rendered = match render_worklog(&pool, &stored, &tenant.timezone).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let current_instance = python_dumps(&current_rendered);
    let attrs = match validate_worklog_input(&body, true) {
        Ok(attrs) => attrs,
        Err(errors) => return Denial::BadJson(errors).into_response(),
    };
    let now = chrono::Utc::now();
    let base = WorklogBaseOwned {
        minutes: attrs.minutes.unwrap_or(stored.minutes),
        logged_on: attrs.logged_on.unwrap_or(stored.logged_on),
        note: attrs.note.unwrap_or_else(|| stored.note.clone()),
        updated_at: now,
        updated_by: Some(user_id),
        ..stored
    };
    if let Err(error) = issue_worklogs::Entity::update_many()
        .col_expr(issue_worklogs::Column::Minutes, Expr::value(base.minutes))
        .col_expr(
            issue_worklogs::Column::LoggedOn,
            Expr::value(base.logged_on),
        )
        .col_expr(issue_worklogs::Column::Note, Expr::value(base.note.clone()))
        .col_expr(
            issue_worklogs::Column::UpdatedAt,
            Expr::value(base.updated_at),
        )
        .col_expr(
            issue_worklogs::Column::UpdatedById,
            Expr::value(base.updated_by),
        )
        .filter(issue_worklogs::Column::Id.eq(base.id))
        .exec(&orm(&pool))
        .await
    {
        return integrity_denial_orm(error).into_response();
    }
    let epoch = chrono::Utc::now().timestamp();
    let origin = app_origin(&state);
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("worklog.activity.updated".to_owned()),
    );
    activity.insert(
        "requested_data".to_owned(),
        Value::String(requested_data.clone()),
    );
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert(
        "current_instance".to_owned(),
        Value::String(current_instance.clone()),
    );
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(origin));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    match render_worklog(&pool, &base, &tenant.timezone).await {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(text) => json_response(StatusCode::OK, text),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `IssueWorklogViewSet.destroy` (`worklog.py:152-168`): ADMIN +
/// creator gate, soft delete (the `SoftDeleteModel` default — the row
/// stays with `deleted_at` stamped, firing
/// `soft_delete_related_objects` before the `worklog.activity.deleted`
/// enqueue, which carries `{"worklog_id": pk}` and the before-image).
async fn worklog_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let (issue_id, pk) = match (path_uuid(&issue_raw), path_uuid(&pk_raw)) {
        (Ok(issue_id), Ok(pk)) => (issue_id, pk),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_admin_creator(
        &pool,
        &slug,
        &project_id,
        &user_id,
        CreatorModel::IssueWorklog,
        &pk,
    )
    .await
    {
        return error.into_response();
    }
    let stored = match fetch_scoped_worklog(&pool, &slug, &project_id, &issue_id, &pk).await {
        Ok(Some(base)) => base,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(error) => return error.into_response(),
    };
    // The request body is never read on this path (not even for
    // errors); drop `req` without parsing.
    let _ = req;
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let current_rendered = match render_worklog(&pool, &stored, &tenant.timezone).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    if let Err(error) = soft_delete_row(
        &pool,
        SoftDeleteTarget::IssueWorklog,
        &stored.id,
        &user_id,
        &now,
    )
    .await
    {
        return error.into_response();
    }
    let mut requested = Map::new();
    requested.insert("worklog_id".to_owned(), Value::String(pk.to_string()));
    let epoch = chrono::Utc::now().timestamp();
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("worklog.activity.deleted".to_owned()),
    );
    activity.insert(
        "requested_data".to_owned(),
        Value::String(python_dumps(&Value::Object(requested))),
    );
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert(
        "current_instance".to_owned(),
        Value::String(python_dumps(&current_rendered)),
    );
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(app_origin(&state)));
    // Order mirrors `worklog.py:156-167`: `worklog.delete()` (which
    // fires `soft_delete_related_objects` from `db/mixins.py:78`) runs
    // BEFORE the `issue_activity.delay`.
    enqueue_soft_delete(&pool, "issueworklog", &stored.id).await;
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    empty_response(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn origin_state(app_base_url: Option<&str>, web_url: Option<&str>) -> AppState {
        let mut settings = pidash_db::config::Settings::test_defaults();
        settings.urls.app_base_url = app_base_url.map(str::to_owned);
        settings.urls.web_url = web_url.map(str::to_owned);
        AppState::with_settings("test", settings)
    }

    /// `base_host(request, is_app=True)` treats `""` as missing
    /// (PIDASHCONV-1587): an empty `APP_BASE_URL` falls through to
    /// `WEB_URL`. (The localhost fallback for unset-everywhere is a
    /// separate, deliberate-looking deviation — out of scope.)
    #[test]
    fn app_origin_falls_through_empty_strings() {
        let web = "https://web.example.invalid";
        let state = origin_state(Some(""), Some(web));
        assert_eq!(app_origin(&state), web);
        let state = origin_state(Some("https://app.example.invalid"), Some(web));
        assert_eq!(app_origin(&state), "https://app.example.invalid");
        let state = origin_state(Some(""), Some(""));
        assert_eq!(app_origin(&state), "http://localhost");
    }

    #[test]
    fn strip_tags_matches_django() {
        // The shared `MLStripper` port (`html_processor.strip_tags`,
        // vectors verified against the live helper).
        assert_eq!(ml_strip_tags("<p>cycle note</p>"), "cycle note");
        assert_eq!(ml_strip_tags(""), "");
        assert_eq!(ml_strip_tags("plain"), "plain");
        assert_eq!(ml_strip_tags("<p>a<br/>b</p>"), "ab");
        assert_eq!(ml_strip_tags("a < b"), "a < b");
        assert_eq!(ml_strip_tags("<p>x</p><!-- c -->tail"), "xtail");
        assert_eq!(ml_strip_tags("&lt;p&gt;"), "<p>");
    }

    #[test]
    fn parse_body_shapes_match_drf() {
        assert!(parse_body(b"").unwrap().is_empty());
        let map = parse_body(br#"{"reaction": "+1"}"#).unwrap();
        assert_eq!(map["reaction"], json!("+1"));
        match parse_body(b"[1]").unwrap_err() {
            Denial::BadJson(value) => assert_eq!(
                value,
                json!({"non_field_errors": ["Invalid data. Expected a dictionary, but got list."]})
            ),
            other => panic!("unexpected {other:?}"),
        }
        match parse_body(b"null").unwrap_err() {
            Denial::BadJson(value) => {
                assert_eq!(value, json!({"non_field_errors": ["No data provided"]}))
            }
            other => panic!("unexpected {other:?}"),
        }
        match parse_body(b"{oops").unwrap_err() {
            Denial::BadDetail(message) => assert!(message.starts_with("JSON parse error - ")),
            other => panic!("unexpected {other:?}"),
        }
    }

    // The `str`/`repr` asserts moved to `pidash_compat::py` with
    // `py_str_value` (PIDASHCONV-1623); the equality asserts stay.
    #[test]
    fn py_json_eq_matches_cpython() {
        assert!(py_json_eq(&json!(true), &json!(1)));
        assert!(py_json_eq(&json!(1), &json!(1.0)));
        assert!(py_json_eq(
            &json!({"a": 1, "b": [1, 2]}),
            &json!({"b": [1, 2], "a": 1.0})
        ));
        assert!(!py_json_eq(&json!(1), &json!("1")));
        assert!(!py_json_eq(&json!([1, 2]), &json!([2, 1])));
    }

    #[test]
    fn char_field_coercion_matches_drf() {
        let mut errors = Map::new();
        let body: Map<String, Value> =
            serde_json::from_value(json!({"r": 77, "f": 4.5, "s": "  x  "})).unwrap();
        assert!(matches!(
            check_text(&mut errors, &body, "r", None, false, false),
            Presence::Value(text) if text == "77"
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "f", None, false, false),
            Presence::Value(text) if text == "4.5"
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "s", None, false, false),
            Presence::Value(text) if text == "x"
        ));
        let body: Map<String, Value> =
            serde_json::from_value(json!({"b": true, "sp": "   ", "n": Value::Null})).unwrap();
        assert!(matches!(
            check_text(&mut errors, &body, "b", None, false, false),
            Presence::Missing
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "sp", None, false, false),
            Presence::Missing
        ));
        assert_eq!(
            Value::Object(errors),
            json!({
                "b": ["Not a valid string."],
                "sp": ["This field may not be blank."],
            })
        );
    }

    #[test]
    fn url_verdicts_match_django_probes() {
        for ok in [
            "https://x.example/a.png",
            "HTTP://X.EXAMPLE/A",
            "http://localhost/",
            "http://127.0.0.1:8080/x",
            "http://[::1]/",
            "http://example.com.",
            "http://user:pw@h.example/x",
            "http://münchen.de/",
            "http://xn--e28h.example/",
            "http://😀.example/",
            "http://user%zz@h.example/",
            "ftp://f.example:21/x",
            "https://x.co/?q=1&r=2",
            "http://[::ffff:1.2.3.4]/",
            "http://a.bc/",
        ] {
            assert!(is_valid_url(ok), "{ok}");
        }
        for bad in [
            "a.b",
            "http://a.b",
            "http://a",
            "http:///x",
            "http://",
            "gopher://x.example/",
            "javascript:alert(1)",
            "https://x.example/a b.png",
            "http://example.com:/x",
            "http://[::1",
            "http://a-.com/",
            "http://-a.com/",
            "http://example.123/",
            "http://foo.bar1/",
            "http://1.2.3.4.5/",
            "http://01.2.3.4/",
            "http://[fe80::1%25eth0]/",
            "http://us＠er@h.example/",
            "http://h／.example/",
        ] {
            assert!(!is_valid_url(bad), "{bad}");
        }
    }

    #[test]
    fn input_datetime_parsing_matches_drf() {
        // Offset-aware shapes normalize to UTC.
        let dt = parse_input_datetime("2024-05-01T12:30:00+02:00").unwrap();
        assert_eq!(
            dt.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
            "2024-05-01T10:30:00Z"
        );
        // `Z` and space separators pass.
        assert!(parse_input_datetime("2024-05-01T10:30:00Z").is_some());
        assert!(parse_input_datetime("2024-05-01 10:30:00").is_some());
        // Naive reads as UTC.
        let naive = parse_input_datetime("2024-05-01T10:30:00").unwrap();
        assert_eq!(
            naive.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
            "2024-05-01T10:30:00Z"
        );
        // `fromisoformat` extras (all probed live against Django).
        assert_eq!(
            parse_input_datetime("2024-01-01")
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
            "2024-01-01T00:00:00Z"
        );
        assert!(parse_input_datetime("2024-1-1T1:1").is_some());
        assert!(parse_input_datetime("20240101T000000").is_some());
        assert!(parse_input_datetime("2024-W01-1").is_some());
        assert!(parse_input_datetime("2024-01-01X00:00:00").is_some());
        assert!(parse_input_datetime("2024-01-01T00:00:00,5").is_some());
        assert_eq!(
            parse_input_datetime("2024-01-01T00:00:00.1234567")
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
            "2024-01-01T00:00:00.123456Z"
        );
        assert!(parse_input_datetime("2024-01-01T00:00:00+0000").is_some());
        assert!(parse_input_datetime("2024-01-01T00:00:00+00").is_some());
        // Rejections (all probed live).
        for bad in [
            "garbage",
            "",
            "2024-01-01t00:00:00z",
            "2024-01-01T00:00:00z",
            "2024-001",
            "2024001",
            "2024-w01-1",
            "2024-W011",
            "2024-01-01Z",
            " 2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00+0",
            "2024-01-01T24:00:00",
            "2024-01-01T00:00:60",
            "2024-01-01T00:00:00.",
            "2024-02-30T00:00:00",
            "2023-W53-7",
            "2024-01-01T0",
            "2024-01-01T00:00:00+24:00",
        ] {
            assert!(parse_input_datetime(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn check_datetime_iso8601_literal_fallback() {
        // PIDASHCONV-773: DRF `to_internal_value` falls through to
        // `strptime(value, 'iso-8601')` when `parse_datetime` returns None;
        // the literal matches case-insensitively and yields naive
        // 1900-01-01, read as UTC (probed live both backends).
        for text in [
            "iso-8601", "ISO-8601", "Iso-8601", "iSo-8601", "isO-8601", "ISo-8601", "IsO-8601",
            "iSO-8601",
        ] {
            let mut errors = Map::new();
            let body: Map<String, Value> =
                serde_json::from_value(json!({"deleted_at": text})).unwrap();
            let Presence::Value(dt) = check_datetime(&mut errors, &body, "deleted_at", true) else {
                panic!("{text:?} rejected: {errors:?}");
            };
            assert_eq!(
                dt.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
                "1900-01-01T00:00:00Z",
                "{text:?}"
            );
            assert!(errors.is_empty(), "{text:?}: {errors:?}");
        }
        // Near-misses stay invalid (exact match, both sides probed).
        for text in [
            "iso8601",
            "xiso-8601",
            "iso-8601x",
            " iso-8601",
            "iso-8601 ",
            "iso-8601\n",
            "\tiso-8601",
        ] {
            let mut errors = Map::new();
            let body: Map<String, Value> =
                serde_json::from_value(json!({"deleted_at": text})).unwrap();
            assert!(
                matches!(
                    check_datetime(&mut errors, &body, "deleted_at", true),
                    Presence::Missing
                ),
                "{text:?}"
            );
        }
        // The shared parser also serves the `?created_at__gt=` query path
        // (model `to_python`, no `strptime` step): the literal stays
        // invalid there.
        assert!(parse_input_datetime("iso-8601").is_none());
        assert!(parse_input_datetime("ISO-8601").is_none());
    }

    #[test]
    fn text_validation_messages_match_drf() {
        let mut errors = Map::new();
        let body: Map<String, Value> =
            serde_json::from_value(json!({"a": true, "b": Value::Null, "c": "", "d": "toolong"}))
                .unwrap();
        assert!(matches!(
            check_text(&mut errors, &body, "a", None, false, true),
            Presence::Missing
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "b", None, false, true),
            Presence::Missing
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "c", None, false, false),
            Presence::Missing
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "d", Some(3), false, true),
            Presence::Missing
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "zzz", None, false, true),
            Presence::Missing
        ));
        assert_eq!(
            Value::Object(errors),
            json!({
                "a": ["Not a valid string."],
                "b": ["This field may not be null."],
                "c": ["This field may not be blank."],
                "d": ["Ensure this field has no more than 3 characters."],
            })
        );
    }

    #[test]
    fn choice_and_uuid_messages_match_drf() {
        let mut errors = Map::new();
        let body: Map<String, Value> =
            serde_json::from_value(json!({"access": "bogus", "run": "nope"})).unwrap();
        assert!(matches!(
            check_choice(
                &mut errors,
                &body,
                "access",
                &["INTERNAL", "EXTERNAL"],
                false,
            ),
            Presence::Missing
        ));
        assert!(matches!(
            check_uuid(&mut errors, &body, "run", true),
            Presence::Missing
        ));
        assert_eq!(
            Value::Object(errors),
            json!({
                "access": ["\"bogus\" is not a valid choice."],
                "run": ["Must be a valid UUID."],
            })
        );
    }

    #[test]
    fn list_field_errors_match_drf_shape() {
        let mut errors = Map::new();
        let body: Map<String, Value> =
            serde_json::from_value(json!({"attachments": ["https://ok.example/a", "nope", 7]}))
                .unwrap();
        assert!(matches!(
            check_string_list(
                &mut errors,
                &body,
                "attachments",
                10,
                &validate_attachment_child
            ),
            Presence::Missing
        ));
        assert_eq!(
            Value::Object(errors),
            json!({"attachments": {"1": ["Enter a valid URL."], "2": ["Enter a valid URL."]}})
        );
        let mut errors = Map::new();
        let body: Map<String, Value> = serde_json::from_value(json!({"labels": "nope"})).unwrap();
        assert!(matches!(
            check_string_list(&mut errors, &body, "labels", 8, &validate_label_child),
            Presence::Missing
        ));
        assert_eq!(
            Value::Object(errors),
            json!({"labels": ["Expected a list of items but got type \"str\"."]})
        );
    }

    #[test]
    fn url_check_accepts_realistic_attachments() {
        assert!(is_valid_url("https://files.example.com/a.png"));
        assert!(is_valid_url("http://localhost:8000/a"));
        assert!(is_valid_url("ftp://files.example.com/a"));
        assert!(!is_valid_url("not-a-url"));
        assert!(!is_valid_url("gopher://example.com/a"));
        assert!(!is_valid_url("https://"));
    }

    #[test]
    fn denial_bodies_match_drf() {
        let (status, body) = Denial::Unauthorized.status_and_body();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        let (status, body) = Denial::Forbidden.status_and_body();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            r#"{"error":"You don't have the required permissions."}"#
        );
        let (status, body) = Denial::ForbiddenDetail.status_and_body();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, VIEWSET_FORBIDDEN_BODY);
        let (status, body) = Denial::NotFoundDetail.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            r#"{"detail":"No IssueComment matches the given query."}"#
        );
        let (status, body) = Denial::ProjectNotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, r#"{"detail":"Project not found"}"#);
        let (status, body) =
            Denial::BadError("The payload is not valid".to_owned()).status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, r#"{"error":"The payload is not valid"}"#);
        let (status, body) = Denial::BadMessage("User already subscribed to the issue.".to_owned())
            .status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            r#"{"message":"User already subscribed to the issue."}"#
        );
        // DRF's `APIException` envelope is lowercase `detail`
        // (`rest_framework/views.py:96`); the 400 parse error and the 415
        // share it.
        let (status, body) =
            Denial::BadDetail("JSON parse error - oops".to_owned()).status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, r#"{"detail":"JSON parse error - oops"}"#);
        let (status, body) = Denial::UnsupportedMediaType(
            "Unsupported media type \"text/plain\" in request.".to_owned(),
        )
        .status_and_body();
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            body,
            r#"{"detail":"Unsupported media type \"text/plain\" in request."}"#
        );
    }

    #[test]
    fn sync_lock_only_fires_on_touched_and_changed() {
        let id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now();
        let stored = CommentBaseOwned {
            id,
            created_at: now,
            updated_at: now,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project_id: uuid::Uuid::new_v4(),
            workspace_id: uuid::Uuid::new_v4(),
            stripped: "note".to_owned(),
            json: json!({}),
            html: "<p>note</p>".to_owned(),
            description_id: None,
            attachments: vec![],
            labels: vec![],
            issue_id: uuid::Uuid::new_v4(),
            actor_id: None,
            access: "INTERNAL".to_owned(),
            external_source: None,
            external_id: None,
            speaker_type: "human".to_owned(),
            speaker_label: String::new(),
            speaker_run_id: None,
            edited_at: None,
            parent_id: None,
        };
        // Untouched triple: no errors.
        let attrs = CommentValidated {
            access: Some("EXTERNAL".to_owned()),
            ..CommentValidated::default()
        };
        assert!(comment_sync_lock_errors(&stored, &attrs).is_empty());
        // Touched-but-equal: no errors.
        let attrs = CommentValidated {
            html: Some("<p>note</p>".to_owned()),
            ..CommentValidated::default()
        };
        assert!(comment_sync_lock_errors(&stored, &attrs).is_empty());
        // Touched-and-changed: per-field errors.
        let attrs = CommentValidated {
            html: Some("<p>edited</p>".to_owned()),
            json: Some(json!({"a": 1})),
            ..CommentValidated::default()
        };
        let errors = comment_sync_lock_errors(&stored, &attrs);
        assert_eq!(errors.len(), 2);
        assert!(errors.contains_key("comment_html"));
        assert!(errors.contains_key("comment_json"));
    }

    #[test]
    fn comment_view_key_order_matches_drf() {
        let view = CommentView {
            id: "id".to_owned(),
            actor_detail: None,
            issue_detail: json!({}),
            project_detail: json!({}),
            workspace_detail: json!({}),
            comment_reactions: vec![],
            is_member: Some(true),
            is_synced: false,
            created_at: "c".to_owned(),
            updated_at: "u".to_owned(),
            deleted_at: None,
            comment_stripped: "s".to_owned(),
            comment_json: json!({}),
            comment_html: "h".to_owned(),
            attachments: vec![],
            labels: vec![],
            access: "INTERNAL".to_owned(),
            external_source: None,
            external_id: None,
            speaker_type: "human".to_owned(),
            speaker_label: String::new(),
            speaker_agent_run_id: None,
            edited_at: None,
            created_by: None,
            updated_by: None,
            project: "p".to_owned(),
            workspace: "w".to_owned(),
            description: None,
            issue: "i".to_owned(),
            actor: None,
            parent: None,
        };
        let value = serde_json::to_value(&view).unwrap();
        let keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            vec![
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
                "deleted_at",
                "comment_stripped",
                "comment_json",
                "comment_html",
                "attachments",
                "labels",
                "access",
                "external_source",
                "external_id",
                "speaker_type",
                "speaker_label",
                "speaker_agent_run_id",
                "edited_at",
                "created_by",
                "updated_by",
                "project",
                "workspace",
                "description",
                "issue",
                "actor",
                "parent",
            ]
        );
    }

    #[test]
    fn reaction_view_key_orders_match_drf() {
        let reaction = CommentReactionView {
            id: "id".to_owned(),
            actor: "a".to_owned(),
            comment: "c".to_owned(),
            reaction: "r".to_owned(),
            display_name: "d".to_owned(),
            deleted_at: None,
            workspace: "w".to_owned(),
            project: "p".to_owned(),
            created_at: "c".to_owned(),
            updated_at: "u".to_owned(),
            created_by: None,
            updated_by: None,
        };
        let reaction_value = serde_json::to_value(&reaction).unwrap();
        let keys: Vec<String> = reaction_value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            keys,
            vec![
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
        );
        let issue = IssueReactionView {
            id: "id".to_owned(),
            actor_detail: json!({}),
            created_at: "c".to_owned(),
            updated_at: "u".to_owned(),
            deleted_at: None,
            reaction: "r".to_owned(),
            created_by: None,
            updated_by: None,
            project: "p".to_owned(),
            workspace: "w".to_owned(),
            actor: "a".to_owned(),
            issue: "i".to_owned(),
        };
        let issue_value = serde_json::to_value(&issue).unwrap();
        let keys: Vec<String> = issue_value.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "actor_detail",
                "created_at",
                "updated_at",
                "deleted_at",
                "reaction",
                "created_by",
                "updated_by",
                "project",
                "workspace",
                "actor",
                "issue",
            ]
        );
    }

    #[test]
    fn path_uuid_matches_django_converter() {
        let canonical = "12345678-1234-1234-1234-1234567890ab";
        assert!(path_uuid(canonical).is_ok());
        // Django's `<uuid:>` regex is lowercase-hex hyphenated only;
        // every other spelling fails URL resolving (→ proxy here).
        for raw in [
            "12345678-1234-1234-1234-1234567890AB",
            "123456781234123412341234567890ab",
            "{12345678-1234-1234-1234-1234567890ab}",
            "urn:uuid:12345678-1234-1234-1234-1234567890ab",
            "not-a-uuid",
            "",
            "12345678-1234-1234-1234-1234567890abc",
            "12345678_1234_1234_1234_1234567890ab",
        ] {
            assert!(path_uuid(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn filterset_errors_match_django_filter() {
        use std::collections::HashMap;
        // Valid values narrow; absent values are ignored.
        let query: super::super::QueryMap = HashMap::new();
        assert!(filterset_predicates(&query).unwrap().is_empty());
        let id = "12345678-1234-1234-1234-1234567890ab";
        let mut query: super::super::QueryMap = HashMap::new();
        query.insert(
            "issue__id".to_owned(),
            super::super::OneOrMany::Many(vec!["junk".to_owned(), id.to_owned()]),
        );
        let filters = filterset_predicates(&query).unwrap();
        assert_eq!(filters.len(), 1);
        assert!(matches!(filters[0].0, issue_comments::Column::IssueId));
        // Garbage answers the per-field django-filter body, in
        // `filterset_fields` order — not the invalid-detail 400.
        let mut query: super::super::QueryMap = HashMap::new();
        query.insert(
            "workspace__id".to_owned(),
            super::super::OneOrMany::One("junk".to_owned()),
        );
        query.insert(
            "issue__id".to_owned(),
            super::super::OneOrMany::One("junk".to_owned()),
        );
        match filterset_predicates(&query) {
            Err(Denial::BadJson(Value::Object(errors))) => {
                let keys: Vec<String> = errors.keys().cloned().collect();
                assert_eq!(keys, vec!["issue__id", "workspace__id"]);
                assert_eq!(
                    errors["issue__id"],
                    serde_json::json!(["Enter a valid UUID."])
                );
            }
            other => panic!("expected field errors, got {other:?}"),
        }
    }

    #[test]
    fn asset_url_matches_model_property() {
        assert_eq!(
            asset_url("abc", Some("USER_AVATAR"), None, None, None).unwrap(),
            Some("/api/assets/v2/static/abc/".to_owned())
        );
        assert_eq!(
            asset_url(
                "abc",
                Some("ISSUE_ATTACHMENT"),
                Some("ws"),
                Some("p"),
                Some("i")
            )
            .unwrap(),
            Some("/api/assets/v2/workspaces/ws/projects/p/issues/i/attachments/abc/".to_owned())
        );
        assert_eq!(
            asset_url("abc", Some("BOGUS"), None, None, None).unwrap(),
            None
        );
        // A null workspace is Django's `None.slug` `AttributeError` →
        // the generic 500, not a quiet null.
        assert!(asset_url("abc", Some("ISSUE_ATTACHMENT"), None, Some("p"), Some("i")).is_err());
    }

    fn file_part(filename: &str) -> shared_body::FilePart {
        shared_body::FilePart {
            filename: filename.to_owned(),
            content_type: String::new(),
            bytes: Vec::new(),
            in_memory: true,
        }
    }

    /// `""` on an FK coerces to null for HTML input only (`Field.get_value`);
    /// JSON `""` runs `to_internal_value` → the curly error (no DB read on
    /// either path, so a lazy pool never connects).
    #[tokio::test]
    async fn fk_empty_string_is_html_gated() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/pidash_test_unused").unwrap();
        let mut errors = Map::new();
        let body: Map<String, Value> = serde_json::from_value(json!({"actor": ""})).unwrap();
        assert!(matches!(
            check_fk(&pool, &mut errors, &body, "actor", "users", true, false).await,
            Ok(Presence::Missing)
        ));
        assert_eq!(
            errors.get("actor").unwrap(),
            &json!(["\u{201c}\u{201d} is not a valid UUID."])
        );
        let mut errors = Map::new();
        assert!(matches!(
            check_fk(&pool, &mut errors, &body, "actor", "users", true, true).await,
            Ok(Presence::Null)
        ));
        assert!(errors.is_empty());
        // The required-FK fallthrough: HTML `""` without allow_null runs
        // `to_internal_value` (`get_value` falls through when required).
        let mut errors = Map::new();
        assert!(matches!(
            check_fk(&pool, &mut errors, &body, "actor", "users", false, true).await,
            Ok(Presence::Missing)
        ));
        assert_eq!(
            errors.get("actor").unwrap(),
            &json!(["\u{201c}\u{201d} is not a valid UUID."])
        );
    }

    /// A JSON `{"__file__": …}` dict is ordinary data (py-str rendering in
    /// the curly error); the HTML-input sentinel echoes its filename.
    #[tokio::test]
    async fn fk_file_sentinel_is_html_gated() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/pidash_test_unused").unwrap();
        let body: Map<String, Value> =
            serde_json::from_value(json!({"actor": {"__file__": "pic.png"}})).unwrap();
        let mut errors = Map::new();
        assert!(matches!(
            check_fk(&pool, &mut errors, &body, "actor", "users", true, false).await,
            Ok(Presence::Missing)
        ));
        assert_eq!(
            errors.get("actor").unwrap(),
            &json!(["\u{201c}{'__file__': 'pic.png'}\u{201d} is not a valid UUID."])
        );
        let mut errors = Map::new();
        assert!(matches!(
            check_fk(&pool, &mut errors, &body, "actor", "users", true, true).await,
            Ok(Presence::Missing)
        ));
        assert_eq!(
            errors.get("actor").unwrap(),
            &json!(["\u{201c}pic.png\u{201d} is not a valid UUID."])
        );
    }

    /// Same gate on the choice path: JSON sentinel dicts render py-str,
    /// HTML-input uploads echo the filename into the membership test.
    #[test]
    fn choice_file_sentinel_is_html_gated() {
        let body: Map<String, Value> =
            serde_json::from_value(json!({"access": {"__file__": "INTERNAL"}})).unwrap();
        let mut errors = Map::new();
        assert!(matches!(
            check_choice(
                &mut errors,
                &body,
                "access",
                &["INTERNAL", "EXTERNAL"],
                false
            ),
            Presence::Missing
        ));
        assert_eq!(
            errors.get("access").unwrap(),
            &json!(["\"{'__file__': 'INTERNAL'}\" is not a valid choice."])
        );
        let mut errors = Map::new();
        assert!(matches!(
            check_choice(
                &mut errors,
                &body,
                "access",
                &["INTERNAL", "EXTERNAL"],
                true
            ),
            Presence::Value(_)
        ));
        assert!(errors.is_empty());
    }

    /// Indexed list assemblies resolve positionally: Null placeholders
    /// become the next upload's sentinel, dict-form inner nulls pop for
    /// alignment (no leftover append), exact-key uploads append after
    /// texts.
    #[test]
    fn form_list_indexed_placeholders_resolve_positionally() {
        let map: Map<String, Value> =
            serde_json::from_value(json!({"labels": ["a", null]})).unwrap();
        let mut files = shared_body::FilesMap::new();
        files.insert("labels".to_owned(), vec![file_part("f.txt")]);
        let merged = form_body_map(map, files).unwrap();
        assert_eq!(
            merged.get("labels").unwrap(),
            &json!(["a", {"__file__": "f.txt"}])
        );

        let map: Map<String, Value> =
            serde_json::from_value(json!({"labels": [[{"s": [null]}]]})).unwrap();
        let mut files = shared_body::FilesMap::new();
        files.insert("labels".to_owned(), vec![file_part("g.txt")]);
        let merged = form_body_map(map, files).unwrap();
        assert_eq!(merged.get("labels").unwrap(), &json!([[ {"s": [null]} ]]));

        let map: Map<String, Value> = serde_json::from_value(json!({"labels": ["a"]})).unwrap();
        let mut files = shared_body::FilesMap::new();
        files.insert(
            "labels".to_owned(),
            vec![file_part("p1.txt"), file_part("p2.txt")],
        );
        let merged = form_body_map(map, files).unwrap();
        assert_eq!(
            merged.get("labels").unwrap(),
            &json!(["a", {"__file__": "p1.txt"}, {"__file__": "p2.txt"}])
        );
    }

    /// `comment_json` uploads stringify to the filename before `json.loads`:
    /// a file named `123` yields the number, a prosaic name is invalid.
    #[test]
    fn form_comment_json_filename_parses() {
        let mut files = shared_body::FilesMap::new();
        files.insert("comment_json".to_owned(), vec![file_part("123")]);
        let merged = form_body_map(Map::new(), files).unwrap();
        assert_eq!(merged.get("comment_json").unwrap(), &json!(123));

        let mut files = shared_body::FilesMap::new();
        files.insert("comment_json".to_owned(), vec![file_part("notes.txt")]);
        match form_body_map(Map::new(), files) {
            Err(Denial::BadJson(value)) => assert_eq!(
                value,
                json!({"comment_json": ["Value must be valid JSON."]})
            ),
            other => panic!("expected invalid JSON error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn form_arrays_dump_as_querydict_scalars() {
        // Task payloads render the raw QueryDict (PIDASHCONV-763): the
        // engage comment/reaction dumps project form `attachments` /
        // `labels` arrays to last-wins scalars; validation keeps arrays.
        let raw = b"reaction=hooray&labels=a&labels=b&attachments=f1&attachments=f2";
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("http://test/x")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("content-length", raw.len().to_string())
            .body(axum::body::Body::from(raw.to_vec()))
            .expect("request");
        let RequestBody { mut map, is_html } = read_body(req).await.expect("parses");
        assert!(is_html);
        assert_eq!(map["labels"], json!(["a", "b"]));
        assert_eq!(map["attachments"], json!(["f1", "f2"]));
        shared_body::project_list_scalars(&mut map, ENGAGE_BODY_SPEC.list_fields);
        assert_eq!(map["labels"], json!("b"));
        assert_eq!(map["attachments"], json!("f2"));
        assert_eq!(map["reaction"], json!("hooray"));
    }
}

#[cfg(test)]
mod worklog_tests {
    use super::{
        assemble_worklog_view, check_worklog_date, check_worklog_minutes, validate_worklog_input,
        Presence, WorklogBaseOwned, WORKLOG_NOT_FOUND_BODY,
    };
    use chrono_tz::Tz;
    use serde_json::{json, Map, Value};

    fn body(value: Value) -> Map<String, Value> {
        value.as_object().expect("object").clone()
    }

    fn minutes_of(payload: Value) -> Result<Option<i32>, Value> {
        let body = body(payload);
        let mut errors = Map::new();
        let out = match check_worklog_minutes(&mut errors, &body) {
            Presence::Value(minutes) => Some(minutes),
            _ => None,
        };
        if errors.is_empty() {
            Ok(out)
        } else {
            Err(Value::Object(errors))
        }
    }

    // Every arm probed against live Django (PIDASHCONV-1676).
    #[test]
    fn minutes_grammar_matches_drf() {
        for (input, want) in [
            (json!(90), 90),
            (json!("90"), 90),
            (json!(" 90 "), 90),
            (json!(90.0), 90),
            (json!("90.0"), 90),
            (json!("90."), 90),
            (json!("+90"), 90),
            (json!("1_0"), 10),
        ] {
            assert_eq!(
                minutes_of(json!({"minutes": input.clone(), "logged_on": "2026-10-20"})),
                Ok(Some(want)),
                "input {input}"
            );
        }
        for input in [
            json!(1.5),
            json!("9.5"),
            json!(true),
            json!([1]),
            json!(""),
            json!("   "),
            json!("_1"),
            json!("1__0"),
            json!("0x10"),
            json!({"n": 1}),
        ] {
            assert_eq!(
                minutes_of(json!({"minutes": input.clone(), "logged_on": "2026-10-20"})),
                Err(json!({"minutes": ["A valid integer is required."]})),
                "input {input}"
            );
        }
        assert_eq!(
            minutes_of(json!({"minutes": Value::Null, "logged_on": "2026-10-20"})),
            Err(json!({"minutes": ["This field may not be null."]}))
        );
        assert_eq!(minutes_of(json!({"logged_on": "2026-10-20"})), Ok(None));
        assert_eq!(
            minutes_of(json!({"minutes": "9".repeat(1001), "logged_on": "2026-10-20"})),
            Err(json!({"minutes": ["String value too large."]}))
        );
        // `len(data)` counts code points, not bytes (`fields.py:911`):
        // 600 non-ASCII chars (1200 bytes) fall through to `int()`,
        // which fails them as invalid — only past 1000 *chars* does
        // the length guard fire.
        assert_eq!(
            minutes_of(json!({"minutes": "é".repeat(600), "logged_on": "2026-10-20"})),
            Err(json!({"minutes": ["A valid integer is required."]}))
        );
        assert_eq!(
            minutes_of(json!({"minutes": "é".repeat(1000), "logged_on": "2026-10-20"})),
            Err(json!({"minutes": ["A valid integer is required."]}))
        );
        assert_eq!(
            minutes_of(json!({"minutes": "é".repeat(1001), "logged_on": "2026-10-20"})),
            Err(json!({"minutes": ["String value too large."]}))
        );
    }

    #[test]
    fn minutes_bounds_match_drf() {
        for input in [json!(0), json!(-30), json!(-(2_i64.pow(40))), json!("-0")] {
            assert_eq!(
                minutes_of(json!({"minutes": input.clone(), "logged_on": "2026-10-20"})),
                Err(json!({"minutes": ["Ensure this value is greater than or equal to 1."]})),
                "input {input}"
            );
        }
        for input in [json!(2_i64.pow(40)), json!("9".repeat(1000))] {
            assert_eq!(
                minutes_of(json!({"minutes": input.clone(), "logged_on": "2026-10-20"})),
                Err(json!({"minutes": ["Ensure this value is less than or equal to 2147483647."]})),
                "input {input}"
            );
        }
        assert_eq!(
            minutes_of(json!({"minutes": 2_147_483_647, "logged_on": "2026-10-20"})),
            Ok(Some(2_147_483_647))
        );
        assert_eq!(
            minutes_of(json!({"minutes": 1, "logged_on": "2026-10-20"})),
            Ok(Some(1))
        );
    }

    fn date_of(payload: Value) -> Result<Option<String>, Value> {
        let body = body(payload);
        let mut errors = Map::new();
        let out = match check_worklog_date(&mut errors, &body) {
            Presence::Value(date) => Some(date.format("%Y-%m-%d").to_string()),
            _ => None,
        };
        if errors.is_empty() {
            Ok(out)
        } else {
            Err(Value::Object(errors))
        }
    }

    #[test]
    fn date_grammar_matches_django_parse_date() {
        for (input, want) in [
            ("2026-10-20", "2026-10-20"),
            ("2026-1-5", "2026-01-05"),
            ("20261020", "2026-10-20"),
            ("2026-W42-3", "2026-10-14"),
            ("2026W423", "2026-10-14"),
            ("2026-10-20\n", "2026-10-20"),
        ] {
            assert_eq!(
                date_of(json!({"minutes": 30, "logged_on": input})),
                Ok(Some(want.to_owned())),
                "input {input:?}"
            );
        }
        let invalid = json!({"logged_on": ["Date has wrong format. Use one of these formats instead: YYYY-MM-DD."]});
        for input in [
            json!("soon"),
            json!("2026-13-01"),
            json!("2026-02-30"),
            json!("10/20/2026"),
            json!("2026-10-20T00:00:00Z"),
            json!("2026-10-20T00:00"),
            json!(20261020),
            json!(true),
            json!("2026-293"),
            json!("2026293"),
            json!("0000-01-01"),
            json!(" 2026-10-20"),
            json!("2026-10-20 "),
            json!("2026-10-20\r"),
        ] {
            assert_eq!(
                date_of(json!({"minutes": 30, "logged_on": input.clone()})),
                Err(invalid.clone()),
                "input {input}"
            );
        }
        assert_eq!(
            date_of(json!({"minutes": 30, "logged_on": Value::Null})),
            Err(json!({"logged_on": ["This field may not be null."]}))
        );
        assert_eq!(date_of(json!({"minutes": 30})), Ok(None));
    }

    #[test]
    fn worklog_input_create_requires_and_defaults() {
        // `{}` fails both required fields in serializer order.
        assert_eq!(
            validate_worklog_input(&body(json!({})), false),
            Err(json!({
                "minutes": ["This field is required."],
                "logged_on": ["This field is required."],
            }))
        );
        // Read-only and unknown keys are ignored; the note defaults.
        let attrs = validate_worklog_input(
            &body(json!({"minutes": 30, "logged_on": "2026-10-20"})),
            false,
        )
        .expect("valid");
        assert_eq!(attrs.minutes, Some(30));
        assert_eq!(
            attrs.logged_on,
            Some(chrono::NaiveDate::from_ymd_opt(2026, 10, 20).expect("date"))
        );
        assert_eq!(attrs.note, Some(String::new()));
        let attrs = validate_worklog_input(
            &body(json!({
                "minutes": 30,
                "logged_on": "2026-10-20",
                "note": 5,
                "actor": "00000000-0000-4000-8000-000000000000",
                "id": "00000000-0000-4000-8000-000000000001",
                "zzz": 1,
            })),
            false,
        )
        .expect("valid");
        assert_eq!(attrs.note, Some("5".to_owned()));
    }

    #[test]
    fn worklog_input_partial_skips_missing() {
        let attrs = validate_worklog_input(&body(json!({})), true).expect("valid");
        assert_eq!(attrs.minutes, None);
        assert_eq!(attrs.logged_on, None);
        assert_eq!(attrs.note, None);
        let attrs = validate_worklog_input(&body(json!({"note": "x"})), true).expect("valid");
        assert_eq!(attrs.note, Some("x".to_owned()));
        assert!(validate_worklog_input(&body(json!({"minutes": 0})), true).is_err());
    }

    fn sample_base() -> WorklogBaseOwned {
        WorklogBaseOwned {
            id: "11111111-1111-4111-8111-111111111111"
                .parse()
                .expect("uuid"),
            created_at: chrono::DateTime::parse_from_rfc3339("2026-10-20T00:00:00Z")
                .expect("dt")
                .with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339("2026-10-21T00:00:00Z")
                .expect("dt")
                .with_timezone(&chrono::Utc),
            deleted_at: None,
            minutes: 90,
            logged_on: chrono::NaiveDate::from_ymd_opt(2026, 10, 20).expect("date"),
            note: "Deep work".to_owned(),
            created_by: Some(
                "22222222-2222-4222-8222-222222222222"
                    .parse()
                    .expect("uuid"),
            ),
            updated_by: None,
            project_id: "33333333-3333-4333-8333-333333333333"
                .parse()
                .expect("uuid"),
            workspace_id: "44444444-4444-4444-8444-444444444444"
                .parse()
                .expect("uuid"),
            issue_id: "55555555-5555-4555-8555-555555555555"
                .parse()
                .expect("uuid"),
            actor_id: Some(
                "22222222-2222-4222-8222-222222222222"
                    .parse()
                    .expect("uuid"),
            ),
        }
    }

    #[test]
    fn worklog_view_key_order_matches_drf() {
        let tz: Tz = "UTC".parse().expect("tz");
        let view = assemble_worklog_view(&sample_base(), None, &tz);
        let value = serde_json::to_value(&view).expect("value");
        let keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "id",
                "actor_detail",
                "created_at",
                "updated_at",
                "deleted_at",
                "minutes",
                "logged_on",
                "note",
                "created_by",
                "updated_by",
                "project",
                "workspace",
                "issue",
                "actor",
            ]
        );
        assert_eq!(value["minutes"], json!(90));
        assert_eq!(value["logged_on"], json!("2026-10-20"));
        assert_eq!(value["actor_detail"], Value::Null);
        assert_eq!(value["created_at"], json!("2026-10-20T00:00:00Z"));
    }

    #[test]
    fn worklog_not_found_body_names_the_model() {
        assert_eq!(
            WORKLOG_NOT_FOUND_BODY,
            r#"{"detail":"No IssueWorklog matches the given query."}"#
        );
    }
}

#[cfg(test)]
mod pidashconv_736_tests {
    use super::normalize_resolve_identifier;

    #[test]
    fn resolve_identifier_strips_py_whitespace() {
        assert_eq!(normalize_resolve_identifier("  eng "), "ENG");
        // Python `str.strip()` also strips U+001C-U+001F (PIDASHCONV-736):
        // `%1C`-padded identifiers must resolve, not 404.
        for sep in ['\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}'] {
            let padded = format!("{sep}eng{sep}");
            assert_eq!(
                normalize_resolve_identifier(&padded),
                "ENG",
                "U+{:04X} padding must strip like Python",
                sep as u32
            );
        }
        // TAB and U+0085 padding already matched Django; pin the behavior.
        assert_eq!(normalize_resolve_identifier("\teng\t"), "ENG");
        assert_eq!(normalize_resolve_identifier("\u{85}eng\u{85}"), "ENG");
    }
}

#[cfg(test)]
mod orm_tests {
    //! The engage reads and writes on SeaORM: statement-shape pins (no
    //! database) and one live run against constraint-free temp tables.
    use super::*;
    use sea_orm::{DbBackend, QueryTrait};

    fn sql_of<E: EntityTrait>(select: sea_orm::Select<E>) -> String {
        select.build(DbBackend::Postgres).to_string()
    }

    #[test]
    fn roster_select_keeps_joins_scope_and_unique_names() {
        let sql = sql_of(subscriber_roster_select("acme", uuid::Uuid::nil()));
        assert!(
            sql.contains(
                r#"INNER JOIN "workspaces" ON "project_members"."workspace_id" = "workspaces"."id""#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#"LEFT JOIN "users" ON "project_members"."member_id" = "users"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""project_members"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""project_members"."is_active" = TRUE"#),
            "{sql}"
        );
        assert!(
            sql.ends_with(r#"ORDER BY "project_members"."created_at" DESC"#),
            "{sql}"
        );
        for alias in [
            r#""users"."id" AS "member_id""#,
            r#""users"."first_name" AS "first_name""#,
            r#""users"."display_name" AS "display_name""#,
        ] {
            assert!(sql.contains(alias), "{alias} in {sql}");
        }
    }

    #[test]
    fn reaction_and_worklog_lists_are_distinct_and_member_scoped() {
        let id = uuid::Uuid::nil();
        let reactions = sql_of(issue_reaction_list_select("acme", id, id, id));
        assert!(reactions.starts_with("SELECT DISTINCT "), "{reactions}");
        assert!(
            reactions.contains(r#"INNER JOIN "project_members" ON "issue_reactions"."project_id" = "project_members"."project_id""#),
            "{reactions}"
        );
        assert!(
            reactions.contains(r#""projects"."archived_at" IS NULL"#),
            "{reactions}"
        );
        // As coded: the reaction scope reads no member tombstone filter.
        assert!(
            !reactions.contains(r#""project_members"."deleted_at""#),
            "{reactions}"
        );
        let worklogs = sql_of(worklog_list_select("acme", id, id, id));
        assert!(worklogs.starts_with("SELECT DISTINCT "), "{worklogs}");
        assert!(
            worklogs.contains(r#"INNER JOIN "project_members" ON "projects"."id" = "project_members"."project_id""#),
            "{worklogs}"
        );
        assert!(
            worklogs.contains(r#""project_members"."deleted_at" IS NULL"#),
            "{worklogs}"
        );
        assert!(
            worklogs.ends_with(r#"ORDER BY "issue_worklogs"."created_at" DESC"#),
            "{worklogs}"
        );
    }

    #[test]
    fn asset_url_select_left_joins_the_workspace_slug() {
        let sql = sql_of(asset_url_select(uuid::Uuid::nil()));
        assert!(
            sql.contains(
                r#"LEFT JOIN "workspaces" ON "file_assets"."workspace_id" = "workspaces"."id""#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#""workspaces"."slug" AS "workspace_slug""#),
            "{sql}"
        );
        assert!(
            !sql.contains("deleted_at"),
            "unguarded _base_manager read: {sql}"
        );
    }

    async fn exec(pool: &sqlx::PgPool, sql: &str) {
        sqlx::query(sql).execute(pool).await.expect(sql);
    }

    /// Every new read and write on a one-connection pool over temp
    /// copies of the tables (no constraints, so each NOT NULL column the
    /// entity models decode is seeded by hand).
    #[tokio::test]
    async fn live_engage_queries() {
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
            "file_assets",
            "workspaces",
            "projects",
            "issues",
            "project_members",
            "workspace_members",
            "issue_comments",
            "descriptions",
            "comment_reactions",
            "issue_reactions",
            "issue_subscribers",
            "issue_worklogs",
            "issue_assignees",
            "intake_issues",
            "git_comment_syncs",
            "issue_activities",
        ] {
            exec(
                &pool,
                &format!("CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"),
            )
            .await;
        }
        let id = uuid::Uuid::new_v4;
        let (ws, project, archived, issue) = (id(), id(), id(), id());
        let (member, other, ghost) = (id(), id(), id());
        let (avatar_asset, attach_asset) = (id(), id());
        let (comment, gone_comment, desc, gone_desc) = (id(), id(), id(), id());
        let (old_reaction, new_reaction, dead_reaction) = (id(), id(), id());
        let (issue_reaction, archived_reaction) = (id(), id());
        let (worklog, dead_worklog, subscription) = (id(), id(), id());
        let (pm_old, pm_new, pm_ghost, pm_inactive, pm_dup) = (id(), id(), id(), id(), id());
        let (act_early, act_late, act_comment) = (id(), id(), id());
        for sql in [
            format!(
                "INSERT INTO issue_activities (id, created_at, updated_at, verb, field, old_value, new_value, comment, attachments, issue_id, project_id, workspace_id, actor_id, epoch, deleted_at) VALUES \
                 ('{act_early}', '2026-01-01T00:00:00Z', now(), 'updated', 'state', 'a', 'b', 'moved', '{{}}', '{issue}', '{project}', '{ws}', '{member}', 1.5, NULL), \
                 ('{act_late}', '2026-02-01T00:00:00Z', now(), 'created', NULL, NULL, NULL, '', '{{x}}', '{issue}', '{project}', '{ws}', NULL, NULL, NULL), \
                 ('{act_comment}', '2026-01-10T00:00:00Z', now(), 'created', 'comment', NULL, NULL, '', '{{}}', '{issue}', '{project}', '{ws}', NULL, NULL, NULL), \
                 (gen_random_uuid(), '2026-01-11T00:00:00Z', now(), 'deleted', 'state', NULL, NULL, '', '{{}}', '{issue}', '{project}', '{ws}', NULL, NULL, now())"
            ),
            format!(
                "INSERT INTO users (id, first_name, last_name, avatar, avatar_asset_id, is_bot, display_name, user_timezone) VALUES \
                 ('{member}', 'Ada', 'L', 'raw.png', '{avatar_asset}', false, 'ada', 'Asia/Tokyo'), \
                 ('{other}', 'Bob', 'K', '', NULL, true, 'bob', 'UTC')"
            ),
            format!(
                "INSERT INTO file_assets (id, entity_type, workspace_id, project_id, issue_id, deleted_at) VALUES \
                 ('{avatar_asset}', 'USER_AVATAR', NULL, NULL, NULL, now()), \
                 ('{attach_asset}', 'ISSUE_ATTACHMENT', '{ws}', '{project}', '{issue}', NULL)"
            ),
            format!(
                "INSERT INTO workspaces (id, name, slug, logo, logo_asset_id) VALUES ('{ws}', 'Acme', 'acme', 'https://x/logo', NULL)"
            ),
            format!(
                "INSERT INTO projects (id, identifier, name, cover_image, cover_image_asset_id, logo_props, description, is_default, workspace_id, guest_view_all_features, archived_at, deleted_at) VALUES \
                 ('{project}', 'ENG', 'Eng', NULL, '{attach_asset}', '{{\"a\":1}}', 'd', true, '{ws}', false, NULL, now()), \
                 ('{archived}', 'OLD', 'Old', '', NULL, '{{}}', '', false, '{ws}', false, now(), NULL)"
            ),
            format!(
                "INSERT INTO issues (id, name, description_json, description_html, priority, complexity_score, start_date, target_date, sequence_id, sort_order, is_draft, project_id, created_by_id, deleted_at) VALUES \
                 ('{issue}', 'Fix', '{{\"t\":[]}}', '<p>x</p>', 'high', 3, '2026-01-02', NULL, 7, 65535.5, false, '{project}', '{member}', now())"
            ),
            format!(
                "INSERT INTO project_members (id, project_id, workspace_id, member_id, role, is_active, deleted_at, created_at) VALUES \
                 ('{pm_old}', '{project}', '{ws}', '{member}', 20, true, NULL, now() - interval '2 hours'), \
                 ('{pm_new}', '{project}', '{ws}', '{other}', 15, true, NULL, now() - interval '1 hour'), \
                 ('{pm_ghost}', '{project}', '{ws}', '{ghost}', 5, true, NULL, now()), \
                 ('{pm_inactive}', '{project}', '{ws}', '{other}', 15, false, NULL, now()), \
                 ('{pm_dup}', '{project}', '{ws}', '{member}', 20, true, now(), now()), \
                 (gen_random_uuid(), '{archived}', '{ws}', '{member}', 20, true, NULL, now())"
            ),
            format!(
                "INSERT INTO workspace_members (id, workspace_id, member_id, role, is_active, deleted_at) VALUES \
                 (gen_random_uuid(), '{ws}', '{member}', 20, true, NULL)"
            ),
            format!(
                "INSERT INTO issue_comments (id, created_at, updated_at, comment_stripped, attachments, created_by_id, issue_id, project_id, updated_by_id, workspace_id, actor_id, comment_html, comment_json, access, external_id, external_source, deleted_at, edited_at, description_id, parent_id, speaker_type, speaker_label, speaker_agent_run_id, labels) VALUES \
                 ('{comment}', now(), now(), 'hi', '{{a.png}}', '{member}', '{issue}', '{project}', NULL, '{ws}', '{member}', '<p>hi</p>', '{{\"k\":1}}', 'INTERNAL', NULL, 'github', NULL, NULL, '{desc}', NULL, 'human', '', NULL, '{{}}'), \
                 ('{gone_comment}', now(), now(), '', '{{}}', '{member}', '{issue}', '{project}', NULL, '{ws}', NULL, '', '{{}}', 'INTERNAL', NULL, NULL, now(), NULL, NULL, NULL, 'human', '', NULL, '{{}}')"
            ),
            format!(
                "INSERT INTO descriptions (id, deleted_at) VALUES ('{desc}', NULL), ('{gone_desc}', now())"
            ),
            format!(
                "INSERT INTO comment_reactions (id, created_at, updated_at, reaction, actor_id, comment_id, created_by_id, project_id, updated_by_id, workspace_id, deleted_at) VALUES \
                 ('{old_reaction}', now() - interval '1 hour', now(), '+1', '{member}', '{comment}', '{member}', '{project}', NULL, '{ws}', NULL), \
                 ('{new_reaction}', now(), now(), 'heart', '{other}', '{comment}', NULL, '{project}', NULL, '{ws}', NULL), \
                 ('{dead_reaction}', now(), now(), 'x', '{member}', '{comment}', NULL, '{project}', NULL, '{ws}', now())"
            ),
            format!(
                "INSERT INTO issue_reactions (id, created_at, updated_at, reaction, actor_id, created_by_id, issue_id, project_id, updated_by_id, workspace_id, deleted_at) VALUES \
                 ('{issue_reaction}', now(), now(), '+1', '{member}', '{member}', '{issue}', '{project}', NULL, '{ws}', NULL), \
                 ('{archived_reaction}', now(), now(), '+1', '{member}', NULL, '{issue}', '{archived}', NULL, '{ws}', NULL)"
            ),
            format!(
                "INSERT INTO issue_worklogs (id, created_at, updated_at, deleted_at, minutes, logged_on, note, actor_id, created_by_id, issue_id, project_id, updated_by_id, workspace_id) VALUES \
                 ('{worklog}', now(), now(), NULL, 30, '2026-01-03', 'n', '{member}', '{member}', '{issue}', '{project}', NULL, '{ws}'), \
                 ('{dead_worklog}', now(), now(), now(), 5, '2026-01-03', '', NULL, NULL, '{issue}', '{project}', NULL, '{ws}')"
            ),
            format!(
                "INSERT INTO issue_subscribers (id, deleted_at, issue_id, project_id, subscriber_id, workspace_id) VALUES \
                 ('{subscription}', NULL, '{issue}', '{project}', '{member}', '{ws}')"
            ),
            format!(
                "INSERT INTO issue_assignees (id, issue_id, assignee_id, deleted_at) VALUES \
                 (gen_random_uuid(), '{issue}', '{other}', now())"
            ),
            format!(
                "INSERT INTO intake_issues (id, issue_id, status, project_id, workspace_id, deleted_at, created_at, extra) VALUES \
                 (gen_random_uuid(), '{issue}', 1, '{project}', '{ws}', NULL, now(), '{{\"k\":2}}')"
            ),
            format!(
                "INSERT INTO git_comment_syncs (id, comment_id, deleted_at) VALUES (gen_random_uuid(), '{comment}', now())"
            ),
            format!(
                "INSERT INTO git_comment_syncs (id, comment_id, deleted_at) VALUES (gen_random_uuid(), '{comment}', NULL)"
            ),
        ] {
            exec(&pool, &sql).await;
        }
        let db = orm(&pool);
        let tz: Tz = "UTC".parse().unwrap();

        // Plumbing reads.
        assert_eq!(
            super::super::access::user_timezone(&db, member)
                .await
                .unwrap()
                .as_deref(),
            Some("Asia/Tokyo")
        );
        let membership = fetch_membership(&pool, "acme", &project, &member)
            .await
            .unwrap();
        assert_eq!(
            (membership.project_role, membership.workspace_role),
            (Some(20), Some(20))
        );
        assert!(
            created_by_probe(&db, CreatorModel::IssueComment, comment, member)
                .await
                .unwrap()
        );
        assert!(
            !created_by_probe(&db, CreatorModel::IssueComment, gone_comment, member)
                .await
                .unwrap()
        );
        assert!(
            created_by_probe(&db, CreatorModel::IssueWorklog, worklog, member)
                .await
                .unwrap()
        );
        assert!(
            !created_by_probe(&db, CreatorModel::IssueWorklog, worklog, other)
                .await
                .unwrap()
        );
        assert_eq!(
            fk_target_exists(&db, "users", other).await.unwrap(),
            Some(true)
        );
        assert_eq!(
            fk_target_exists(&db, "users", ghost).await.unwrap(),
            Some(false)
        );
        assert_eq!(
            fk_target_exists(&db, "descriptions", desc).await.unwrap(),
            Some(true)
        );
        assert_eq!(
            fk_target_exists(&db, "descriptions", gone_desc)
                .await
                .unwrap(),
            Some(false)
        );
        assert_eq!(
            fk_target_exists(&db, "issue_comments", gone_comment)
                .await
                .unwrap(),
            Some(false)
        );
        assert_eq!(fk_target_exists(&db, "states", desc).await.unwrap(), None);
        // `_base_manager`: the soft-deleted project still answers.
        assert_eq!(
            fetch_project_workspace_unguarded(&pool, &project)
                .await
                .unwrap(),
            ws
        );
        assert_eq!(fetch_display_name(&pool, &other).await.unwrap(), "bob");

        // Lite nests: a soft-deleted avatar asset still renders its URL; the
        // attachment URL interpolates the LEFT-JOINed workspace slug.
        let user = fetch_user_lite(&pool, &member).await.unwrap();
        assert_eq!(
            user.avatar_url,
            Some(format!("/api/assets/v2/static/{avatar_asset}/"))
        );
        assert_eq!((user.avatar.as_str(), user.is_bot), ("raw.png", false));
        let project_lite = fetch_project_lite(&pool, &project).await.unwrap();
        assert_eq!(
            project_lite.cover_image_url,
            Some(format!(
                "/api/assets/v2/workspaces/acme/projects/{project}/issues/{issue}/attachments/{attach_asset}/"
            ))
        );
        assert_eq!(project_lite.logo_props, serde_json::json!({"a": 1}));
        let workspace = fetch_workspace_lite(&pool, &ws).await.unwrap();
        assert_eq!(workspace.logo_url.as_deref(), Some("https://x/logo"));
        let flat = fetch_issue_flat(&pool, &issue).await.unwrap();
        assert_eq!(flat.start_date.as_deref(), Some("2026-01-02"));
        assert_eq!(flat.target_date, None);
        assert_eq!((flat.sequence_id, flat.sort_order), (7, 65535.5));
        assert_eq!(flat.description_html.as_deref(), Some("<p>x</p>"));

        // Comments: the scoped get, sync probes (git-only after PDASHOSS01-354).
        let stored = fetch_scoped_comment(&pool, "acme", &project, &issue, &comment)
            .await
            .unwrap()
            .expect("live comment");
        assert_eq!(stored.attachments, vec!["a.png".to_owned()]);
        assert_eq!(stored.description_id, Some(desc));
        assert!(
            fetch_scoped_comment(&pool, "other", &project, &issue, &comment)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            fetch_scoped_comment(&pool, "acme", &project, &issue, &gone_comment)
                .await
                .unwrap()
                .is_none()
        );
        assert!(comment_is_synced(&pool, Some("github"), &comment)
            .await
            .unwrap());
        assert!(!comment_is_synced(&pool, Some(""), &comment).await.unwrap());
        assert!(!comment_has_sync_row(&pool, &gone_comment).await.unwrap());

        // Comment reactions: live rows newest first, then an insert.
        let reactions = render_comment_reactions_for(&pool, &comment, &tz)
            .await
            .unwrap();
        assert_eq!(
            reactions.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            vec![new_reaction.to_string(), old_reaction.to_string()]
        );
        let now = chrono::Utc::now();
        let fresh = CommentReactionBaseOwned {
            id: id(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
            created_by: Some(other),
            updated_by: None,
            project_id: project,
            workspace_id: ws,
            comment_id: comment,
            actor_id: other,
            reaction: "eyes".to_owned(),
        };
        insert_comment_reaction(&db, &fresh).await.unwrap();
        let stored_reaction = comment_reactions::Entity::find_by_id(fresh.id)
            .one(&db)
            .await
            .unwrap()
            .expect("inserted");
        assert_eq!(
            (
                stored_reaction.reaction.as_str(),
                stored_reaction.updated_by_id
            ),
            ("eyes", None)
        );

        // Issue reactions: one row despite two memberships; the archived
        // project's reaction drops out.
        let listed = issue_reaction_list_select("acme", project, issue, member)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(
            listed.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![issue_reaction]
        );
        assert!(issue_reaction_list_select("acme", archived, issue, member)
            .all(&db)
            .await
            .unwrap()
            .is_empty());

        // Worklogs: the tombstoned membership does not count, DISTINCT holds.
        let logs = worklog_list_select("acme", project, issue, member)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(logs.iter().map(|w| w.id).collect::<Vec<_>>(), vec![worklog]);
        assert_eq!(WorklogBaseOwned::from(logs[0].clone()).minutes, 30);
        assert!(
            fetch_scoped_worklog(&pool, "acme", &project, &issue, &dead_worklog)
                .await
                .unwrap()
                .is_none()
        );
        assert!(!worklog_can_log(
            &pool,
            &Membership {
                project_role: Some(15),
                workspace_role: Some(15)
            },
            &issue,
            &other
        )
        .await
        .unwrap());

        // Subscribers: the active roster newest first, a ghost member as a
        // LEFT JOIN miss; the subscription lookup.
        let roster = subscriber_roster_select("acme", project)
            .into_model::<RosterRow>()
            .all(&db)
            .await
            .unwrap();
        assert_eq!(
            roster.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![pm_ghost, pm_new, pm_old]
        );
        assert_eq!(roster[0].member_id, None);
        assert_eq!(roster[1].display_name.as_deref(), Some("bob"));
        assert_eq!(
            fetch_subscription(&pool, "acme", &project, &issue, &member)
                .await
                .unwrap(),
            Some(subscription)
        );
        let sub = SubscriberOwned {
            id: id(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
            created_by: Some(other),
            updated_by: None,
            project_id: project,
            workspace_id: ws,
            issue_id: issue,
            subscriber_id: other,
        };
        insert_subscriber(&db, &sub).await.unwrap();
        assert_eq!(
            fetch_subscription(&pool, "acme", &project, &issue, &other)
                .await
                .unwrap(),
            Some(sub.id)
        );

        // Batch 2h: the history / comment-list selects, decoded by name.
        // The soft-deleted `pm_dup` membership still joins (bug 4), so the
        // DISTINCT-less history paths return every row twice.
        use super::super::queries_engage as qe;
        let activities = qe::history_activities_select("acme", issue, member, None)
            .into_model::<HistoryActivityRow>()
            .all(&db)
            .await
            .unwrap();
        assert_eq!(
            activities.iter().map(|r| r.activity.id).collect::<Vec<_>>(),
            vec![act_early, act_early, act_late, act_late]
        );
        let early = &activities[0];
        assert_eq!(early.activity.epoch, Some(1.5));
        assert_eq!(
            early.actor.as_ref().map(|a| a.display_name.as_str()),
            Some("ada")
        );
        assert_eq!(early.nests.project.identifier, "ENG");
        assert_eq!(early.nests.workspace.slug, "acme");
        assert_eq!(early.nests.issue.sequence_id, 7);
        assert!(activities[2].actor.is_none());
        assert_eq!(activities[2].activity.attachments, vec!["x".to_owned()]);
        let later =
            qe::history_activities_select("acme", issue, member, Some("2026-01-15T00:00:00+00:00"))
                .into_model::<HistoryActivityRow>()
                .all(&db)
                .await
                .unwrap();
        assert_eq!(
            later.iter().map(|r| r.activity.id).collect::<Vec<_>>(),
            vec![act_late, act_late]
        );
        assert!(
            qe::history_activities_select("other-ws", issue, member, None)
                .into_model::<HistoryActivityRow>()
                .all(&db)
                .await
                .unwrap()
                .is_empty()
        );
        let history_comments = qe::history_comments_select("acme", issue, member, None)
            .into_model::<HistoryCommentRow>()
            .all(&db)
            .await
            .unwrap();
        assert_eq!(
            history_comments
                .iter()
                .map(|r| r.comment.id)
                .collect::<Vec<_>>(),
            vec![comment, comment]
        );
        assert_eq!(
            history_comments[0]
                .actor
                .as_ref()
                .map(|a| a.first_name.as_str()),
            Some("Ada")
        );
        let prefetched = qe::history_reactions_prefetch_select(&[comment])
            .into_model::<PrefetchedReactionRow>()
            .all(&db)
            .await
            .unwrap();
        // Live reactions newest first (the earlier write steps added one).
        let live_reactions: Vec<uuid::Uuid> = comment_reactions::Entity::find()
            .filter(comment_reactions::Column::CommentId.eq(comment))
            .filter(comment_reactions::Column::DeletedAt.is_null())
            .order_by_desc(comment_reactions::Column::CreatedAt)
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(
            prefetched.iter().map(|r| r.reaction.id).collect::<Vec<_>>(),
            live_reactions
        );
        assert!(!live_reactions.contains(&dead_reaction));
        let names: std::collections::HashMap<_, _> = prefetched
            .iter()
            .map(|r| (r.reaction.id, r.display_name.as_str()))
            .collect();
        assert_eq!(names[&new_reaction], "bob");
        assert_eq!(names[&old_reaction], "ada");
        let intake = qe::history_intake_prefetch_select(&[issue])
            .into_model::<IntakeSourceDb>()
            .all(&db)
            .await
            .unwrap();
        assert_eq!(intake.len(), 1);
        assert_eq!(intake[0].source, None);
        // The rendered branches: property, comment, merged.
        let render = |kind: Option<&'static str>| {
            let pool = pool.clone();
            async move {
                let body = history_inner(&pool, "acme", &issue, &member, &tz, None, kind)
                    .await
                    .unwrap();
                serde_json::from_str::<Vec<Value>>(&body).unwrap()
            }
        };
        let property = render(Some("issue-property")).await;
        assert_eq!(property.len(), 4);
        assert_eq!(property[0]["id"], serde_json::json!(act_early.to_string()));
        assert_eq!(property[0]["actor_detail"]["display_name"], "ada");
        assert_eq!(property[0]["project_detail"]["identifier"], "ENG");
        assert_eq!(property[2]["actor_detail"], Value::Null);
        let comments_view = render(Some("issue-comment")).await;
        assert_eq!(comments_view.len(), 2);
        assert_eq!(
            comments_view[0]["comment_reactions"]
                .as_array()
                .map(Vec::len),
            Some(live_reactions.len())
        );
        assert_eq!(comments_view[0]["workspace_detail"]["slug"], "acme");
        assert_eq!(render(None).await.len(), 6);
        // Comment list: DISTINCT folds the duplicate membership.
        let listed = qe::comment_list_select("acme", project, issue, member)
            .into_model::<ListCommentRow>()
            .all(&db)
            .await
            .unwrap();
        assert_eq!(
            listed.iter().map(|r| r.comment.id).collect::<Vec<_>>(),
            vec![comment]
        );
        assert!(listed[0].is_member);
        assert_eq!(listed[0].nests.issue.priority, "high");
        let narrowed = qe::comment_list_select("acme", project, issue, member)
            .filter(issue_comments::Column::WorkspaceId.eq(project))
            .all(&db)
            .await
            .unwrap();
        assert!(narrowed.is_empty());
        let rendered = render_list_comment(&pool, listed.into_iter().next().unwrap(), &tz)
            .await
            .unwrap();
        assert_eq!(rendered["is_member"], Value::Bool(true));
        assert_eq!(rendered["issue_detail"]["sequence_id"], 7);
        let reactions = qe::comment_reaction_list_select("acme", project, comment, member)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(
            reactions.iter().map(|r| r.id).collect::<Vec<_>>(),
            live_reactions
        );
        assert_eq!(
            qe::comment_reaction_list_select("acme", project, comment, ghost)
                .all(&db)
                .await
                .unwrap()
                .len(),
            live_reactions.len()
        );
        assert!(
            qe::comment_reaction_list_select("other-ws", project, comment, member)
                .all(&db)
                .await
                .unwrap()
                .is_empty()
        );

        // Soft deletes stamp deleted_at / updated_at / updated_by.
        for (target, pk) in [
            (SoftDeleteTarget::IssueComment, comment),
            (SoftDeleteTarget::CommentReaction, old_reaction),
            (SoftDeleteTarget::IssueReaction, issue_reaction),
            (SoftDeleteTarget::IssueSubscriber, subscription),
            (SoftDeleteTarget::IssueWorklog, worklog),
        ] {
            soft_delete_row(&pool, target, &pk, &other, &now)
                .await
                .unwrap();
        }
        let comment_row = issue_comments::Entity::find_by_id(comment)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(comment_row.updated_by_id, Some(other));
        assert_eq!(
            comment_row.deleted_at.map(utc),
            Some(now).map(|n| {
                // Postgres keeps microseconds.
                chrono::DateTime::from_timestamp_micros(n.timestamp_micros()).unwrap()
            })
        );
        assert!(fetch_subscription(&pool, "acme", &project, &issue, &member)
            .await
            .unwrap()
            .is_none());
        assert!(worklog_list_select("acme", project, issue, member)
            .all(&db)
            .await
            .unwrap()
            .is_empty());
        let stamped = issue_reactions::Entity::find_by_id(issue_reaction)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stamped.updated_by_id, Some(other));
        assert!(stamped.deleted_at.is_some());
    }
}
