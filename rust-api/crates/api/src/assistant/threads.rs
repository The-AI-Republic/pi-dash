//! Assistant thread list/create/detail handlers (D-06, stage 5).
//!
//! Port of `apps/api/pi_dash/assistant/views/threads.py:1-111`
//! (`AssistantThreadListCreateEndpoint`, `AssistantThreadDetailEndpoint`,
//! `urls.py:38-43`):
//!
//! * `GET threads/` — [`list_threads`]: reap abandoned empty threads, then
//!   the chat-only list (`threads.py:50-63`).
//! * `POST threads/` — [`create_thread`]: workspace lookup, title capped at
//!   255 chars, 201 (`threads.py:65-79`).
//! * `PATCH threads/<id>/` — [`patch_thread`]: title / `is_archived`
//!   partial update, always re-saving `updated_at` (`threads.py:83-95`).
//! * `DELETE threads/<id>/` — [`delete_thread`]: cancel any active turn
//!   first, then delete (`threads.py:97-111`).
//!
//! Registration is the cutover granularity (the `app_issues` / `license`
//! rule): owned methods serve from Rust, every other method on these paths
//! proxies to Django through the edge fallback (its 405s and metadata
//! responses live there).
//!
//! Fixture ids F-A6-01 (thread shape), F-A6-07 (member gate).

// Every handler returns a fully-rendered `Response` by design (like the
// intake `parse_body` precedent, which carries per-function allows for the
// same lint).
#![allow(clippy::result_large_err)]

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, patch};
use axum::Router;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::common::{
    json_response, or_empty_str, owned_thread, parse_json_body, pool_ref, py_bool, request_actor,
    require_member, role_for, server_error, thread_json, thread_not_found, truncate_chars,
    unauthenticated, workspace_id_for_slug, HandlerResult, ThreadRow,
};
use super::redis::signal_cancel;
use crate::middleware::SessionHandle;
use crate::state::AppState;

/// Register the owned thread routes. Unowned methods fall through to Django
/// through the edge fallback (never a Rust 405).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/ai-assistant/threads/",
            get(list_threads)
                .post(create_thread)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/ai-assistant/threads/{thread_id}/",
            patch(patch_thread)
                .delete(delete_thread)
                .get(crate::edge::proxy)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

/// `AssistantThreadListCreateEndpoint.get` (`threads.py:50-63`): the member
/// gate, abandonment reaping, then chat threads for this user in this
/// workspace (`is_archived=False`, `kind=chat`, `-updated_at`, 50 rows).
async fn list_threads(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<Extension<SessionHandle>>,
) -> HandlerResult<Response> {
    let pool = pool_ref(&state)?;
    let Some(actor) = request_actor(pool, extension).await? else {
        return Err(unauthenticated());
    };
    let role = role_for(pool, &actor.id, &slug).await?;
    require_member(role)?;
    reap_empty_threads(pool, &actor.id, &slug).await?;
    let rows = sqlx::query_as::<_, (Uuid, Uuid, Uuid, String, String, bool, Option<Uuid>, DateTime<Utc>, DateTime<Utc>)>(
        "SELECT \"assistant_thread\".\"id\", \"assistant_thread\".\"workspace_id\", \
         \"assistant_thread\".\"user_id\", \"assistant_thread\".\"title\", \
         \"assistant_thread\".\"kind\", \"assistant_thread\".\"is_archived\", \
         \"assistant_thread\".\"active_turn_id\", \"assistant_thread\".\"created_at\", \
         \"assistant_thread\".\"updated_at\" \
         FROM \"assistant_thread\" \
         INNER JOIN \"workspaces\" ON (\"assistant_thread\".\"workspace_id\" = \"workspaces\".\"id\") \
         WHERE (\"assistant_thread\".\"user_id\" = $1 \
         AND \"workspaces\".\"slug\" = $2 \
         AND \"assistant_thread\".\"is_archived\" = false \
         AND \"assistant_thread\".\"kind\" = 'chat') \
         ORDER BY \"assistant_thread\".\"updated_at\" DESC LIMIT 50",
    )
    .bind(actor.id)
    .bind(&slug)
    .fetch_all(pool)
    .await
    .map_err(|_| server_error())?;
    let mut bodies = Vec::with_capacity(rows.len());
    for (
        id,
        workspace_id,
        user_id,
        title,
        kind,
        is_archived,
        active_turn_id,
        created_at,
        updated_at,
    ) in rows
    {
        bodies.push(thread_json(
            &ThreadRow {
                id,
                workspace_id,
                user_id,
                title,
                kind,
                is_archived,
                active_turn_id,
                created_at,
                updated_at,
            },
            &actor.timezone,
        ));
    }
    Ok(json_response(
        StatusCode::OK,
        format!("[{}]", bodies.join(",")),
    ))
}

/// Abandoned-empty-thread reaping (`threads.py:27-46`): delete this user's
/// chat threads in this workspace with no title, no turns, and no in-flight
/// turn, older than the one-hour grace (`EMPTY_THREAD_GRACE`,
/// `threads.py:24`). The `Exists(turns)` annotation becomes `NOT EXISTS`;
/// the cutoff is `now() - interval '1 hour'` evaluated in Postgres (the
/// Python side binds `timezone.now() - grace`; sub-second skew against a
/// one-hour window is unobservable).
async fn reap_empty_threads(pool: &sqlx::PgPool, user_id: &Uuid, slug: &str) -> HandlerResult<()> {
    sqlx::query(
        "DELETE FROM \"assistant_thread\" USING \"workspaces\" \
         WHERE \"assistant_thread\".\"workspace_id\" = \"workspaces\".\"id\" \
         AND \"assistant_thread\".\"user_id\" = $1 \
         AND \"workspaces\".\"slug\" = $2 \
         AND \"assistant_thread\".\"kind\" = 'chat' \
         AND \"assistant_thread\".\"title\" = '' \
         AND \"assistant_thread\".\"active_turn_id\" IS NULL \
         AND \"assistant_thread\".\"created_at\" < now() - interval '1 hour' \
         AND NOT EXISTS (SELECT 1 FROM \"assistant_turn\" \
         WHERE \"assistant_turn\".\"thread_id\" = \"assistant_thread\".\"id\")",
    )
    .bind(user_id)
    .bind(slug)
    .execute(pool)
    .await
    .map_err(|_| server_error())?;
    Ok(())
}

/// `AssistantThreadListCreateEndpoint.post` (`threads.py:65-79`): the member
/// gate, the workspace lookup (404 `not_found` when missing), then
/// `AssistantThread.objects.create(workspace, user, title[:255])` — `kind`
/// and `is_archived` take their model defaults (`chat`, false) — answered
/// 201 with the serializer.
async fn create_thread(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<Extension<SessionHandle>>,
    body: Bytes,
) -> HandlerResult<Response> {
    let pool = pool_ref(&state)?;
    let Some(actor) = request_actor(pool, extension).await? else {
        return Err(unauthenticated());
    };
    let role = role_for(pool, &actor.id, &slug).await?;
    require_member(role)?;
    let data = parse_json_body(&body)?;
    let title = truncate_chars(&or_empty_str(data.get("title"))?, 255);
    let Some(workspace_id) = workspace_id_for_slug(pool, &slug).await? else {
        return Err(thread_not_found());
    };
    let row = sqlx::query_as::<
        _,
        (
            Uuid,
            Uuid,
            Uuid,
            String,
            String,
            bool,
            Option<Uuid>,
            DateTime<Utc>,
            DateTime<Utc>,
        ),
    >(
        "INSERT INTO \"assistant_thread\" \
         (\"id\", \"workspace_id\", \"user_id\", \"title\", \"kind\", \"is_archived\", \
         \"active_turn_id\", \"created_at\", \"updated_at\") \
         VALUES (gen_random_uuid(), $1, $2, $3, 'chat', false, NULL, now(), now()) \
         RETURNING \"id\", \"workspace_id\", \"user_id\", \"title\", \"kind\", \
         \"is_archived\", \"active_turn_id\", \"created_at\", \"updated_at\"",
    )
    .bind(workspace_id)
    .bind(actor.id)
    .bind(&title)
    .fetch_one(pool)
    .await
    .map_err(|_| server_error())?;
    let (
        id,
        workspace_id,
        user_id,
        title,
        kind,
        is_archived,
        active_turn_id,
        created_at,
        updated_at,
    ) = row;
    Ok(json_response(
        StatusCode::CREATED,
        thread_json(
            &ThreadRow {
                id,
                workspace_id,
                user_id,
                title,
                kind,
                is_archived,
                active_turn_id,
                created_at,
                updated_at,
            },
            &actor.timezone,
        ),
    ))
}

/// `AssistantThreadDetailEndpoint.patch` (`threads.py:83-95`): the member
/// gate, the owned-thread scope (404), then the partial update — `title`
/// (coerced, capped) and `is_archived` (Python `bool()`) — always saved
/// with `updated_at` (`update_fields=["title", "is_archived",
/// "updated_at"]`, so even an empty PATCH bumps the stamp and reorders the
/// list).
async fn patch_thread(
    State(state): State<AppState>,
    Path((slug, thread_id)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    body: Bytes,
) -> HandlerResult<Response> {
    let pool = pool_ref(&state)?;
    let Some(actor) = request_actor(pool, extension).await? else {
        return Err(unauthenticated());
    };
    let role = role_for(pool, &actor.id, &slug).await?;
    require_member(role)?;
    // A non-UUID segment never matches Django's `<uuid:thread_id>` converter
    // (its resolver 404s); the owned scope below would also miss it, so an
    // invalid id answers the same scope-miss 404 with the same status.
    let thread_id: Uuid = thread_id.parse().map_err(|_| thread_not_found())?;
    let Some(thread) = owned_thread(pool, &thread_id, &actor.id, &slug).await? else {
        return Err(thread_not_found());
    };
    let data = parse_json_body(&body)?;
    let title = match data.get("title") {
        Some(value) => truncate_chars(&or_empty_str(Some(value))?, 255),
        None => thread.title,
    };
    let is_archived = match data.get("is_archived") {
        Some(value) => py_bool(value),
        None => thread.is_archived,
    };
    let row = sqlx::query_as::<
        _,
        (
            Uuid,
            Uuid,
            Uuid,
            String,
            String,
            bool,
            Option<Uuid>,
            DateTime<Utc>,
            DateTime<Utc>,
        ),
    >(
        "UPDATE \"assistant_thread\" SET \"title\" = $1, \"is_archived\" = $2, \
         \"updated_at\" = now() WHERE \"id\" = $3 \
         RETURNING \"id\", \"workspace_id\", \"user_id\", \"title\", \"kind\", \
         \"is_archived\", \"active_turn_id\", \"created_at\", \"updated_at\"",
    )
    .bind(&title)
    .bind(is_archived)
    .bind(thread_id)
    .fetch_one(pool)
    .await
    .map_err(|_| server_error())?;
    let (
        id,
        workspace_id,
        user_id,
        title,
        kind,
        is_archived,
        active_turn_id,
        created_at,
        updated_at,
    ) = row;
    Ok(json_response(
        StatusCode::OK,
        thread_json(
            &ThreadRow {
                id,
                workspace_id,
                user_id,
                title,
                kind,
                is_archived,
                active_turn_id,
                created_at,
                updated_at,
            },
            &actor.timezone,
        ),
    ))
}

/// `AssistantThreadDetailEndpoint.delete` (`threads.py:97-111`): the member
/// gate, the owned-thread scope (404), the cancel signal when a turn is in
/// flight (Redis `SET`, every failure swallowed), then the delete — answered
/// 204. Row cascades (`on_delete=CASCADE` on turns, messages, events) remove
/// the thread's rows at the database level, exactly like the queryset
/// `.delete()`.
async fn delete_thread(
    State(state): State<AppState>,
    Path((slug, thread_id)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
) -> HandlerResult<Response> {
    let pool = pool_ref(&state)?;
    let Some(actor) = request_actor(pool, extension).await? else {
        return Err(unauthenticated());
    };
    let role = role_for(pool, &actor.id, &slug).await?;
    require_member(role)?;
    // A non-UUID segment never matches Django's `<uuid:thread_id>` converter
    // (its resolver 404s); the owned scope below would also miss it, so an
    // invalid id answers the same scope-miss 404 with the same status.
    let thread_id: Uuid = thread_id.parse().map_err(|_| thread_not_found())?;
    let Some(thread) = owned_thread(pool, &thread_id, &actor.id, &slug).await? else {
        return Err(thread_not_found());
    };
    if let Some(turn_id) = thread.active_turn_id {
        signal_cancel(&state, &turn_id).await;
    }
    sqlx::query("DELETE FROM \"assistant_thread\" WHERE \"id\" = $1")
        .bind(thread_id)
        .execute(pool)
        .await
        .map_err(|_| server_error())?;
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty 204 builds"))
}
