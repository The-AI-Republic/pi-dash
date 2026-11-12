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
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch};
use axum::Router;
use chrono::Utc;
use uuid::Uuid;

use super::common::{
    body_get, json_response, or_empty_body, owned_thread, parse_body, pool_ref, py_bool,
    request_actor, request_content_type, require_member, role_for, server_error, thread_json,
    thread_not_found, truncate_chars, unauthenticated, workspace_id_for_slug, BodyField,
    HandlerResult, ParsedBody, ThreadRow,
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
    let Some(actor) = request_actor(&state, pool, extension).await? else {
        return Err(unauthenticated());
    };
    let role = role_for(pool, &actor.id, &slug).await?;
    require_member(role)?;
    reap_empty_threads(pool, &actor.id, &slug).await?;
    let rows = {
        use sea_orm::{
            ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait,
        };
        use sea_query::JoinType;
        let db = pidash_db::pool::sea_orm_connection(pool);
        pidash_db::entity::assistant_thread::Entity::find()
            .join(
                JoinType::InnerJoin,
                pidash_db::entity::assistant_thread::Relation::Workspaces.def(),
            )
            .filter(pidash_db::entity::assistant_thread::Column::UserId.eq(actor.id))
            .filter(pidash_db::entity::workspaces::Column::Slug.eq(slug.clone()))
            .filter(pidash_db::entity::assistant_thread::Column::IsArchived.eq(false))
            .filter(pidash_db::entity::assistant_thread::Column::Kind.eq("chat"))
            .order_by_desc(pidash_db::entity::assistant_thread::Column::UpdatedAt)
            .limit(50)
            .all(&db)
            .await
            .map_err(|_| server_error())?
    };
    let mut bodies = Vec::with_capacity(rows.len());
    for row in rows {
        bodies.push(thread_json(
            &ThreadRow {
                id: row.id,
                workspace_id: row.workspace_id,
                user_id: row.user_id,
                title: row.title,
                kind: row.kind,
                is_archived: row.is_archived,
                active_turn_id: row.active_turn_id,
                created_at: row.created_at.with_timezone(&Utc),
                updated_at: row.updated_at.with_timezone(&Utc),
            },
            &actor.timezone,
        ));
    }
    Ok(json_response(
        StatusCode::OK,
        format!("[{}]", bodies.join(",")),
    ))
}

/// Delete one thread with Django's ORM-collector semantics
/// (`threads.py:111`, `thread.delete()`): the `on_delete=CASCADE` rows
/// (turns, messages, events — `assistant/models.py:74-75,127-128,156-157`)
/// go, and the `on_delete=SET_NULL` sides (`loop_targets.thread_id` /
/// `last_run_id` — `db/models/loop.py:101-117`) are nulled. Django 4.2
/// creates every one of these FKs with no `ON DELETE` clause
/// (`DEFERRABLE INITIALLY DEFERRED`), so the collector — and this helper —
/// does it all in one transaction; a bare `DELETE FROM "assistant_thread"`
/// fails its deferred checks on any thread that has children. The
/// in-family `SET_NULL` sides (`thread.active_turn_id`,
/// `turn.user_message_id`) need no explicit nulling: their own rows are
/// deleted in the same transaction. Leaf-first order (events, messages,
/// turns, thread) mirrors the collector.
async fn delete_thread_rows(conn: &mut sqlx::PgConnection, thread_id: &Uuid) -> HandlerResult<()> {
    {
        use sea_orm::sea_query::Expr;
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let tx = pidash_db::orm_tx::TxConn::new(&mut *conn);
        // SET_NULL sides first (loop targets), then leaf CASCADE rows.
        pidash_db::entity::loop_targets::Entity::update_many()
            .col_expr(
                pidash_db::entity::loop_targets::Column::ThreadId,
                Expr::value(None::<Uuid>),
            )
            .filter(pidash_db::entity::loop_targets::Column::ThreadId.eq(*thread_id))
            .exec(&tx)
            .await
            .map_err(|_| server_error())?;
        // `last_run_id IN (SELECT id FROM assistant_turn WHERE thread_id)`:
        // set NULL on targets whose last run belongs to this thread — one
        // statement with a subquery, exactly as before.
        {
            use sea_orm::sea_query::{Alias, Expr, Query};
            use sea_orm::{ConnectionTrait, DbBackend};
            let mut stmt = Query::update()
                .table(Alias::new("loop_targets"))
                .value(Alias::new("last_run_id"), Expr::value(None::<Uuid>))
                .and_where(
                    Expr::col((Alias::new("loop_targets"), Alias::new("last_run_id"))).in_subquery(
                        Query::select()
                            .column(Alias::new("id"))
                            .from(Alias::new("assistant_turn"))
                            .and_where(Expr::col(Alias::new("thread_id")).eq(*thread_id))
                            .to_owned(),
                    ),
                )
                .to_owned();
            // Ensure the subquery is rendered as a real subselect.
            let built = DbBackend::Postgres.build(&stmt);
            tx.execute(built).await.map_err(|_| server_error())?;
            let _ = &mut stmt;
        }
        pidash_db::entity::assistant_event::Entity::delete_many()
            .filter(pidash_db::entity::assistant_event::Column::ThreadId.eq(*thread_id))
            .exec(&tx)
            .await
            .map_err(|_| server_error())?;
    }
    // Message leg (leaf-first order preserved — messages after events,
    // before turns).
    pidash_services::domain::assistant::delete_thread_messages(
        &pidash_db::orm_tx::TxConn::new(&mut *conn),
        *thread_id,
    )
    .await
    .map_err(|_| server_error())?;
    {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let tx = pidash_db::orm_tx::TxConn::new(&mut *conn);
        pidash_db::entity::assistant_turn::Entity::delete_many()
            .filter(pidash_db::entity::assistant_turn::Column::ThreadId.eq(*thread_id))
            .exec(&tx)
            .await
            .map_err(|_| server_error())?;
        pidash_db::entity::assistant_thread::Entity::delete_many()
            .filter(pidash_db::entity::assistant_thread::Column::Id.eq(*thread_id))
            .exec(&tx)
            .await
            .map_err(|_| server_error())?;
    }
    Ok(())
}

/// Abandoned-empty-thread reaping (`threads.py:27-46`): delete this user's
/// chat threads in this workspace with no title, no turns, and no in-flight
/// turn, older than the one-hour grace (`EMPTY_THREAD_GRACE`,
/// `threads.py:24`). The `Exists(turns)` annotation becomes `NOT EXISTS`;
/// the cutoff is `now() - interval '1 hour'` evaluated in Postgres (the
/// Python side binds `timezone.now() - grace`; sub-second skew against a
/// one-hour window is unobservable). The queryset `.delete()` runs the full
/// collector, so each reaped id goes through [`delete_thread_rows`] in one
/// transaction — a "no turns" thread can still own messages, events, or a
/// loop-target edge.
async fn reap_empty_threads(pool: &sqlx::PgPool, user_id: &Uuid, slug: &str) -> HandlerResult<()> {
    let ids: Vec<Uuid> = {
        use sea_orm::{
            ColumnTrait, Condition, EntityTrait, QueryFilter, QuerySelect, RelationTrait,
        };
        use sea_query::{Alias, Expr, JoinType, Query};
        let db = pidash_db::pool::sea_orm_connection(pool);
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(1);
        let has_turn = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("assistant_turn"))
            .and_where(
                Expr::col((Alias::new("assistant_turn"), Alias::new("thread_id")))
                    .equals((Alias::new("assistant_thread"), Alias::new("id"))),
            )
            .to_owned();
        pidash_db::entity::assistant_thread::Entity::find()
            .join(
                JoinType::InnerJoin,
                pidash_db::entity::assistant_thread::Relation::Workspaces.def(),
            )
            .filter(pidash_db::entity::assistant_thread::Column::UserId.eq(*user_id))
            .filter(pidash_db::entity::workspaces::Column::Slug.eq(slug.to_owned()))
            .filter(pidash_db::entity::assistant_thread::Column::Kind.eq("chat"))
            .filter(pidash_db::entity::assistant_thread::Column::Title.eq(""))
            .filter(pidash_db::entity::assistant_thread::Column::ActiveTurnId.is_null())
            .filter(pidash_db::entity::assistant_thread::Column::CreatedAt.lt(cutoff))
            .filter(Condition::all().add(Expr::exists(has_turn).not()))
            .select_only()
            .column(pidash_db::entity::assistant_thread::Column::Id)
            .into_tuple::<Uuid>()
            .all(&db)
            .await
            .map_err(|_| server_error())?
    };
    if ids.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await.map_err(|_| server_error())?;
    for id in &ids {
        delete_thread_rows(&mut tx, id).await?;
    }
    tx.commit().await.map_err(|_| server_error())?;
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
    headers: HeaderMap,
    body: Bytes,
) -> HandlerResult<Response> {
    let pool = pool_ref(&state)?;
    let Some(actor) = request_actor(&state, pool, extension).await? else {
        return Err(unauthenticated());
    };
    let role = role_for(pool, &actor.id, &slug).await?;
    require_member(role)?;
    let data = match parse_body(&body, request_content_type(&headers)) {
        Ok(ParsedBody::Object(map)) => map,
        // A non-object body has no `.get`: `AttributeError` → generic
        // 500. (JSON `null` takes the same path: `None.get` raises too.)
        Ok(_) => return Err(server_error()),
        Err(failure) => return Err(failure.into_response()),
    };
    let title = truncate_chars(&or_empty_body(body_get(&data, "title"))?, 255);
    let Some(workspace_id) = workspace_id_for_slug(pool, &slug).await? else {
        return Err(thread_not_found());
    };
    let row = {
        use sea_orm::{ActiveModelTrait, Set};
        let db = pidash_db::pool::sea_orm_connection(pool);
        let now = chrono::Utc::now();
        let model = pidash_db::entity::assistant_thread::ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            workspace_id: Set(workspace_id),
            user_id: Set(actor.id),
            title: Set(title.clone()),
            kind: Set("chat".to_owned()),
            is_archived: Set(false),
            active_turn_id: Set(None),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        };
        model.insert(&db).await.map_err(|_| server_error())?
    };
    Ok(json_response(
        StatusCode::CREATED,
        thread_json(
            &ThreadRow {
                id: row.id,
                workspace_id: row.workspace_id,
                user_id: row.user_id,
                title: row.title,
                kind: row.kind,
                is_archived: row.is_archived,
                active_turn_id: row.active_turn_id,
                created_at: row.created_at.with_timezone(&Utc),
                updated_at: row.updated_at.with_timezone(&Utc),
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
    headers: HeaderMap,
    body: Bytes,
) -> HandlerResult<Response> {
    let pool = pool_ref(&state)?;
    let Some(actor) = request_actor(&state, pool, extension).await? else {
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
    let data = match parse_body(&body, request_content_type(&headers)) {
        Ok(ParsedBody::Object(map)) => map,
        // A non-object body supports neither `in` nor `.get`:
        // `TypeError`/`AttributeError` → generic 500. (JSON `null`
        // takes the same path.)
        Ok(_) => return Err(server_error()),
        Err(failure) => return Err(failure.into_response()),
    };
    let title = match body_get(&data, "title") {
        Some(field) => truncate_chars(&or_empty_body(Some(field))?, 255),
        None => thread.title,
    };
    let is_archived = match body_get(&data, "is_archived") {
        // `bool(UploadedFile)` is True: objects are truthy.
        Some(BodyField::File { .. }) => true,
        Some(BodyField::Json(value)) => py_bool(value),
        None => thread.is_archived,
    };
    let row = {
        use sea_orm::{ActiveModelTrait, NotSet, Set};
        let db = pidash_db::pool::sea_orm_connection(pool);
        let now = chrono::Utc::now();
        let model = pidash_db::entity::assistant_thread::ActiveModel {
            id: Set(thread_id),
            title: Set(title.clone()),
            is_archived: Set(is_archived),
            updated_at: Set(now.into()),
            workspace_id: NotSet,
            user_id: NotSet,
            kind: NotSet,
            active_turn_id: NotSet,
            created_at: NotSet,
        };
        model.update(&db).await.map_err(|_| server_error())?
    };
    Ok(json_response(
        StatusCode::OK,
        thread_json(
            &ThreadRow {
                id: row.id,
                workspace_id: row.workspace_id,
                user_id: row.user_id,
                title: row.title,
                kind: row.kind,
                is_archived: row.is_archived,
                active_turn_id: row.active_turn_id,
                created_at: row.created_at.with_timezone(&Utc),
                updated_at: row.updated_at.with_timezone(&Utc),
            },
            &actor.timezone,
        ),
    ))
}

/// `AssistantThreadDetailEndpoint.delete` (`threads.py:97-111`): the member
/// gate, the owned-thread scope (404), the cancel signal when a turn is in
/// flight (Redis `SET`, every failure swallowed), then the delete — answered
/// 204. The delete is [`delete_thread_rows`] in one transaction: the
/// queryset `.delete()` runs the ORM collector (Django's FKs carry no
/// `ON DELETE` clause), never a bare row delete.
async fn delete_thread(
    State(state): State<AppState>,
    Path((slug, thread_id)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
) -> HandlerResult<Response> {
    let pool = pool_ref(&state)?;
    let Some(actor) = request_actor(&state, pool, extension).await? else {
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
    let mut tx = pool.begin().await.map_err(|_| server_error())?;
    delete_thread_rows(&mut tx, &thread_id).await?;
    tx.commit().await.map_err(|_| server_error())?;
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty 204 builds"))
}

#[cfg(test)]
mod tests_assistant_orm {
    #[test]
    fn list_threads_seaorm_shape_pins_join_and_filters() {
        use sea_orm::{
            ColumnTrait, EntityTrait, QueryFilter, QuerySelect, QueryTrait, RelationTrait,
        };
        use sea_query::JoinType;
        let sql = pidash_db::entity::assistant_thread::Entity::find()
            .join(
                JoinType::InnerJoin,
                pidash_db::entity::assistant_thread::Relation::Workspaces.def(),
            )
            .filter(pidash_db::entity::assistant_thread::Column::UserId.eq(uuid::Uuid::nil()))
            .filter(pidash_db::entity::workspaces::Column::Slug.eq("ws".to_owned()))
            .filter(pidash_db::entity::assistant_thread::Column::IsArchived.eq(false))
            .filter(pidash_db::entity::assistant_thread::Column::Kind.eq("chat"))
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("INNER JOIN"), "join: {sql}");
        assert!(sql.contains("workspaces"), "join table: {sql}");
        assert!(sql.contains("is_archived"), "archived filter: {sql}");
    }
}
