//! Assistant message list/create + cancel handlers (D-06, stage 5).
//!
//! Port of `apps/api/pi_dash/assistant/views/messages.py:1-128`
//! (`AssistantMessageListCreateEndpoint`, `AssistantCancelEndpoint`,
//! `urls.py:45-58`):
//!
//! * `GET threads/<id>/messages/` — [`list_messages`]: the `after`/`limit`
//!   cursor over the transcript (`messages.py:44-61`).
//! * `POST threads/<id>/messages/` — [`post_message`]: validation gates,
//!   the LLM-config gate, the atomic turn+message creation, the
//!   commit-hook enqueue, and the 202 envelope (`messages.py:63-111`).
//! * `POST threads/<id>/cancel/` — [`cancel_turn`]: the cancel signal when
//!   a turn is in flight (`messages.py:114-128`).
//!
//! Registration is the cutover granularity (the `app_issues` / `license`
//! rule): owned methods serve from Rust, every other method on these paths
//! proxies to Django through the edge fallback.
//!
//! Fixture ids F-A6-06 (message envelopes + seq), F-A6-07 (member gate +
//! message throttle).

// Every handler returns a fully-rendered `Response` by design (like the
// intake `parse_body` precedent, which carries per-function allows for the
// same lint).
#![allow(clippy::result_large_err)]

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use pidash_db::assistant::event_queries::{message_envelope_json, MessageParts};
use pidash_db::config::AssistantSettings;
use uuid::Uuid;

use super::common::{
    body_get, json_response, or_empty_body, owned_thread, parse_after, parse_body, parse_limit,
    pool_ref, py_iso, query_last, request_actor, request_content_type, require_member, role_for,
    server_error, thread_not_found, title_from, unauthenticated, HandlerResult, ParsedBody,
};
use super::redis::{check_message_throttle, signal_cancel, ThrottleVerdict};
use crate::middleware::SessionHandle;
use crate::state::AppState;

/// Register the owned message + cancel routes. Unowned methods fall through
/// to Django through the edge fallback (never a Rust 405).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/ai-assistant/threads/{thread_id}/messages/",
            get(list_messages)
                .post(post_message)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/ai-assistant/threads/{thread_id}/cancel/",
            post(cancel_turn)
                .get(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

/// One transcript row as the list and the POST response read it.
#[derive(sqlx::FromRow)]
struct MessageRow {
    id: Uuid,
    kind: String,
    display_content: String,
    status: String,
    seq: i64,
    turn_id: Option<Uuid>,
    payload_text: String,
    created_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

/// Wire envelope (`runtime/events.py:128-140`) rendered through
/// [`message_envelope_json`]: kind→role, display_content→content, exact
/// Python key order. The payload arrives as the column's `::text` and is
/// re-parsed (order-preserving) into compact DRF form — the owning-layer
/// compaction the `MessageParts` docs require. Stamps use [`py_iso`]
/// (`.isoformat()`, never the DRF `Z` path).
fn envelope(row: &MessageRow) -> String {
    let payload: serde_json::Value =
        serde_json::from_str(&row.payload_text).unwrap_or(serde_json::Value::Null);
    let payload_json = serde_json::to_string(&payload).expect("value serializes");
    let created = py_iso(&row.created_at);
    let completed = row.completed_at.as_ref().map(py_iso);
    message_envelope_json(&MessageParts {
        id: &row.id.to_string(),
        kind: &row.kind,
        display_content: &row.display_content,
        status: &row.status,
        seq: row.seq,
        turn_id: row.turn_id.as_ref().map(Uuid::to_string).as_deref(),
        payload_json: &payload_json,
        created_at: &created,
        completed_at: completed.as_deref(),
    })
}

/// `AssistantMessageListCreateEndpoint.get` (`messages.py:44-61`): the
/// member gate, the owned-thread scope (404), then
/// `filter(thread, seq__gt=after).order_by("seq")[:limit]` with the lenient
/// cursor parsing (`after` → 0, `limit` → 100, clamped to `[1, 200]`).
async fn list_messages(
    State(state): State<AppState>,
    Path((slug, thread_id)): Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
    extension: Option<Extension<SessionHandle>>,
) -> HandlerResult<Response> {
    let pool = pool_ref(&state)?;
    let Some(actor) = request_actor(&state, pool, extension).await? else {
        return Err(unauthenticated());
    };
    let role = role_for(pool, &actor.id, &slug).await?;
    require_member(role)?;
    // A non-UUID segment never matches Django's `<uuid:thread_id>`
    // converter (its resolver 404s); the owned scope below would also miss
    // it, so an invalid id answers the same scope-miss 404.
    let thread_id: Uuid = thread_id.parse().map_err(|_| thread_not_found())?;
    let Some(_thread) = owned_thread(pool, &thread_id, &actor.id, &slug).await? else {
        return Err(thread_not_found());
    };
    let after = parse_after(query_last(&params, "after").as_deref());
    let limit = parse_limit(query_last(&params, "limit").as_deref());
    let rows = sqlx::query_as::<_, MessageRow>(
        "SELECT \"id\", \"kind\", \"display_content\", \"status\", \"seq\", \"turn_id\", \
         \"payload\"::text AS \"payload_text\", \"created_at\", \"completed_at\" \
         FROM \"assistant_message\" \
         WHERE (\"thread_id\" = $1 AND \"seq\" > $2) \
         ORDER BY \"seq\" ASC LIMIT $3",
    )
    .bind(thread_id)
    .bind(after)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|_| server_error())?;
    let mut bodies = Vec::with_capacity(rows.len());
    for row in &rows {
        bodies.push(envelope(row));
    }
    Ok(json_response(
        StatusCode::OK,
        format!("[{}]", bodies.join(",")),
    ))
}

/// Exact bytes of the message validation denials (`messages.py:72-81`).
const EMPTY_MESSAGE_BODY: &str = r#"{"error":"empty_message"}"#;
const MESSAGE_TOO_LONG_BODY: &str = r#"{"error":"message_too_long"}"#;
const LLM_CONFIG_MISSING_BODY: &str =
    r#"{"error":"llm_config_missing","detail":"Configure your AI provider in Settings."}"#;
/// `messages.py:86,88-91`: the in-flight-turn and thread-cap brakes.
const TURN_ACTIVE_BODY: &str = r#"{"error":"turn_active"}"#;
const THREAD_FULL_BODY: &str = r#"{"error":"thread_full","detail":"Start a new thread."}"#;

/// `AssistantMessageListCreateEndpoint.post` (`messages.py:63-111`).
///
/// Gate order, preserved: POST-only throttle → member gate → owned-thread
/// scope → content validation → LLM-config gate → the atomic creation.
/// DRF `initial()` runs `check_throttles` before the handler body, so a
/// throttled caller answers 429 even when a gate would also deny it; the
/// throttle follows the UUID parse because Django's `<uuid:thread_id>`
/// route converter 404s garbage ids before any view code runs.
/// The creation runs in one transaction under a `SELECT ... FOR UPDATE`
/// thread lock: reject an in-flight turn (409), reject a full thread (409),
/// insert the queued turn, insert the user message (`seq = MAX+1`,
/// `COMPLETED`), link `turn.user_message`, claim `thread.active_turn` and
/// auto-title an untitled thread. `transaction.on_commit(...)` becomes an
/// enqueue into `rust_job_queue` after commit (the intake precedent: without
/// it the response still stands).
async fn post_message(
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
    let thread_id: Uuid = thread_id.parse().map_err(|_| thread_not_found())?;
    // POST-only throttle (`get_throttles`, `messages.py:39-42`): DRF runs
    // `check_throttles` before the handler body, so denied POSTs count and
    // a throttled caller answers 429 even when a gate would also deny it
    // (see `redis::check_message_throttle`).
    if check_message_throttle(&state, &actor.id).await == ThrottleVerdict::Deny {
        return Err(crate::assistant::throttles::Throttled.into_response());
    }
    let role = role_for(pool, &actor.id, &slug).await?;
    require_member(role)?;
    let Some(_thread) = owned_thread(pool, &thread_id, &actor.id, &slug).await? else {
        return Err(thread_not_found());
    };
    let data = match parse_body(&body, request_content_type(&headers)) {
        Ok(ParsedBody::Object(map)) => map,
        // A non-object body has no `.get`: `AttributeError` → generic
        // 500. (JSON `null` takes the same path: `None.get` raises too.)
        Ok(_) => return Err(server_error()),
        Err(failure) => return Err(failure.into_response()),
    };
    let content = or_empty_body(body_get(&data, "content"))?.trim().to_owned();
    if content.is_empty() {
        return Err(json_response(
            StatusCode::BAD_REQUEST,
            EMPTY_MESSAGE_BODY.to_owned(),
        ));
    }
    // `len(content) > MAX_MESSAGE_CHARS` (`errors.py:107`, 32_000):
    // Python counts code points.
    if content.chars().count() > 32_000 {
        return Err(json_response(
            StatusCode::BAD_REQUEST,
            MESSAGE_TOO_LONG_BODY.to_owned(),
        ));
    }
    if !llm_gate_allows(&state, pool, &actor.id).await? {
        return Err(json_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            LLM_CONFIG_MISSING_BODY.to_owned(),
        ));
    }

    let mut tx = pool.begin().await.map_err(|_| server_error())?;
    // `select_for_update().get(pk)` (`messages.py:84`): the lock that
    // serializes concurrent posts. A thread deleted between the scope check
    // and the lock raises `DoesNotExist`, which `handle_exception` maps to
    // the `ObjectDoesNotExist` 404 (`app/views/base.py:79-83`) — a
    // different body from the scope-miss 404 above.
    let locked: Option<(Uuid, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT \"id\", \"active_turn_id\", \"title\" FROM \"assistant_thread\" \
         WHERE \"id\" = $1 FOR UPDATE",
    )
    .bind(thread_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| server_error())?;
    let Some((_id, active_turn_id, title)) = locked else {
        return Err(json_response(
            StatusCode::NOT_FOUND,
            super::common::OBJECT_NOT_FOUND_BODY.to_owned(),
        ));
    };
    if active_turn_id.is_some() {
        return Err(json_response(
            StatusCode::CONFLICT,
            TURN_ACTIVE_BODY.to_owned(),
        ));
    }
    // `AssistantMessage.objects.filter(thread=locked).count() >=
    // MAX_THREAD_MESSAGES` (`errors.py:106`, 200; `messages.py:87-91`).
    let (message_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM \"assistant_message\" WHERE \"thread_id\" = $1")
            .bind(thread_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| server_error())?;
    if message_count >= 200 {
        return Err(json_response(
            StatusCode::CONFLICT,
            THREAD_FULL_BODY.to_owned(),
        ));
    }
    // `AssistantTurn.objects.create(thread=locked, status=QUEUED)`
    // (`messages.py:92`): the ORM fills the remaining fields from their
    // model defaults (`model_used=""`, `error_code=""`, `error_detail=""`,
    // `models.py:88-94`) — the columns are `NOT NULL` with no database
    // default, so the insert must carry the defaults explicitly.
    let (turn_id, turn_status): (Uuid, String) = sqlx::query_as(
        "INSERT INTO \"assistant_turn\" \
         (\"id\", \"thread_id\", \"status\", \"model_used\", \"error_code\", \
         \"error_detail\", \"created_at\") \
         VALUES (gen_random_uuid(), $1, 'queued', '', '', '', now()) \
         RETURNING \"id\", \"status\"",
    )
    .bind(thread_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| server_error())?;
    // `events.create_message(locked, USER, turn, content, COMPLETED)`
    // (`messages.py:93-95`): `seq = MAX+1` under the thread lock.
    let (next_seq,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(MAX(\"seq\"), 0) + 1 FROM \"assistant_message\" WHERE \"thread_id\" = $1",
    )
    .bind(thread_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| server_error())?;
    let created = pidash_services::domain::assistant::create_user_message(
        &mut *tx, thread_id, turn_id, next_seq, &content,
    )
    .await
    .map_err(|_| server_error())?;
    let message = MessageRow {
        id: created.id,
        kind: created.kind,
        display_content: created.display_content,
        status: created.status,
        seq: created.seq,
        turn_id: created.turn_id,
        payload_text: created.payload_text,
        created_at: created.created_at,
        completed_at: created.completed_at,
    };
    // `turn.user_message = user_msg; turn.save(...)` (`messages.py:96-97`).
    sqlx::query("UPDATE \"assistant_turn\" SET \"user_message_id\" = $1 WHERE \"id\" = $2")
        .bind(message.id)
        .bind(turn_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| server_error())?;
    // `locked.active_turn = turn`, auto-title when empty, always bump the
    // stamp (`messages.py:98-101`). A non-empty title is written back
    // verbatim (no re-truncation — `CharField.max_length` is not a database
    // constraint, so a legacy over-long title survives this path in Python
    // too).
    let title = if title.is_empty() {
        title_from(&content)
    } else {
        title
    };
    sqlx::query(
        "UPDATE \"assistant_thread\" SET \"active_turn_id\" = $1, \"title\" = $2, \
         \"updated_at\" = now() WHERE \"id\" = $3",
    )
    .bind(turn_id)
    .bind(&title)
    .bind(thread_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| server_error())?;
    tx.commit().await.map_err(|_| server_error())?;

    // `transaction.on_commit(lambda: run_assistant_turn.delay(tid))`
    // (`messages.py:103`): enqueue `assistant.run_turn` with
    // `args=[str(turn_id)]` into `rust_job_queue` (the intake precedent;
    // the worker forwards the Python-owned name to the broker).
    enqueue_run_turn(pool, &state.settings().assistant, &turn_id).await;

    let body = format!(
        "{{\"turn\":{{\"id\":{},\"status\":{}}},\"message\":{}}}",
        serde_json::to_string(&turn_id.to_string()).expect("string serializes"),
        serde_json::to_string(&turn_status).expect("string serializes"),
        envelope(&message),
    );
    Ok(json_response(StatusCode::ACCEPTED, body))
}

/// The LLM-config gate (`messages.py:77`): the OSS key-bit read,
/// resolved through the installed
/// [`AssistantModelSeam`](pidash_services::extensions::AssistantModelSeam).
/// CE answers the bit itself (the trait default delegates to the ported
/// seam logic), so CE behavior is identical; the overlay additionally
/// accepts its platform credentials. The gate must not build a model or
/// decrypt anything.
async fn llm_gate_allows(
    state: &AppState,
    pool: &sqlx::PgPool,
    user_id: &Uuid,
) -> HandlerResult<bool> {
    let has_key = has_oss_api_key(pool, user_id).await?;
    Ok(state
        .assistant_seam()
        .has_usable_llm_config(*user_id, has_key)
        .await)
}

/// The OSS half of the gate: a `UserLLMConfig` row whose
/// `api_key_encrypted` is set (`has_api_key`, `models.py:260-262`).
async fn has_oss_api_key(pool: &sqlx::PgPool, user_id: &Uuid) -> HandlerResult<bool> {
    let row: Option<(Option<Vec<u8>>,)> = sqlx::query_as(
        "SELECT \"api_key_encrypted\" FROM \"assistant_user_llm_config\" WHERE \"user_id\" = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    Ok(matches!(row, Some((Some(key),)) if !key.is_empty()))
}

/// The queue `run_turn` jobs land on: `ASSISTANT_TURN_QUEUE` when the
/// operator sets it to a non-empty name, else the default `celery` queue.
/// The contract harness steers Django through celery `task_routes` from
/// the same variable, so serve honors it here — the queue name is the
/// only channel distinguishing TURN-oracle turns from APPSOFF-oracle
/// turns.
fn run_turn_queue(settings: &AssistantSettings) -> &str {
    match settings.turn_queue.as_deref() {
        Some(queue) if !queue.is_empty() => queue,
        _ => pidash_jobs::queue::DEFAULT_QUEUE,
    }
}

/// Best-effort post-commit enqueue (the intake `enqueue_message`
/// precedent): without it the 202 response still stands.
async fn enqueue_run_turn(pool: &sqlx::PgPool, settings: &AssistantSettings, turn_id: &Uuid) {
    let mut job = pidash_jobs::queue::NewJob::new(
        pidash_jobs::assistant::RUN_TURN_TASK,
        serde_json::json!([turn_id.to_string()]),
        serde_json::json!({}),
    );
    job.queue = run_turn_queue(settings).to_owned();
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, turn = turn_id.to_string(), "assistant.messages: turn enqueue failed; response stands");
    }
}

/// Exact bytes of the cancel denials (`messages.py:122-123`).
const NO_ACTIVE_TURN_BODY: &str = r#"{"error":"no_active_turn"}"#;

/// `AssistantCancelEndpoint.post` (`messages.py:114-128`): the member gate,
/// the owned-thread scope (404), 409 when no turn is in flight, else the
/// cancel signal (Redis `SET`, every failure swallowed; see
/// `redis::signal_cancel`) and 204.
async fn cancel_turn(
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
    let thread_id: Uuid = thread_id.parse().map_err(|_| thread_not_found())?;
    let Some(thread) = owned_thread(pool, &thread_id, &actor.id, &slug).await? else {
        return Err(thread_not_found());
    };
    let Some(turn_id) = thread.active_turn_id else {
        return Err(json_response(
            StatusCode::CONFLICT,
            NO_ACTIVE_TURN_BODY.to_owned(),
        ));
    };
    signal_cancel(&state, &turn_id).await;
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty 204 builds"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use pidash_services::extensions::{AssistantModelSeam, SeamFuture, TitleError, TitleRequest};

    /// Overlay-shaped seam: a keyless user still counts as configured.
    #[derive(Debug)]
    struct GatewaySeam;

    impl AssistantModelSeam for GatewaySeam {
        fn has_usable_llm_config(
            &self,
            user_id: uuid::Uuid,
            has_api_key: bool,
        ) -> SeamFuture<'_, bool> {
            let _ = (user_id, has_api_key);
            Box::pin(async move { true })
        }

        fn generate_title(
            &self,
            request: TitleRequest,
        ) -> SeamFuture<'_, Result<String, TitleError>> {
            let _ = request;
            Box::pin(async move { Ok("seam title".to_owned()) })
        }
    }

    async fn live_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::postgres::PgPoolOptions::new()
                    .max_connections(5)
                    .connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    fn live_state(seam: Option<Arc<dyn AssistantModelSeam>>) -> AppState {
        let settings = pidash_db::config::Settings::test_defaults();
        let mut state = AppState::with_settings("test", settings);
        if let Some(seam) = seam {
            state = state.with_assistant_seam(seam);
        }
        state
    }

    /// Seed one active user; returns its id. Column list mirrors the
    /// contract harness `create_user` (the `handlers_members` precedent).
    async fn seed_user(pool: &sqlx::PgPool) -> uuid::Uuid {
        let id = uuid::Uuid::new_v4();
        let email = format!("gate-{}@example.com", id.simple());
        sqlx::query(
            "INSERT INTO users (id, password, username, email, first_name, last_name,
                avatar, date_joined, created_at, updated_at, last_location,
                created_location, is_superuser, is_managed, is_password_expired,
                is_active, is_staff, is_email_verified, is_password_autoset, token,
                user_timezone, last_login_ip, last_logout_ip, last_login_medium,
                last_login_uagent, is_bot, display_name, is_email_valid,
                is_password_reset_required)
             VALUES ($1, '!', $2, $2, 'Gate', 'User',
                '', now(), now(), now(), '',
                '', false, false, false,
                true, false, true, false, '',
                'UTC', '', '', '',
                '', false, 'Gate User', true,
                false)",
        )
        .bind(id)
        .bind(&email)
        .execute(pool)
        .await
        .expect("seed user");
        id
    }

    /// Seed a BYOK config row carrying a stored key for `user_id`.
    /// Column list mirrors the save path (`llm_config.rs`).
    async fn seed_llm_key(pool: &sqlx::PgPool, user_id: &uuid::Uuid) {
        sqlx::query(
            "INSERT INTO assistant_user_llm_config \
             (user_id, provider_kind, base_url, model_name, api_key_encrypted, \
             last_verified_at, created_at, updated_at) \
             VALUES ($1, 'openai_compatible', 'https://api.example.com/v1', \
             'byok-model', $2, NULL, now(), now())",
        )
        .bind(user_id)
        .bind(b"stored-key-blob".to_vec())
        .execute(pool)
        .await
        .expect("seed llm key");
    }

    #[tokio::test]
    async fn gate_uses_the_installed_seam() {
        let Some(pool) = live_pool().await else {
            return;
        };
        // A keyless user (no config row at all): only the installed
        // seam's verdict can open the gate.
        let user_id = seed_user(&pool).await;
        let state = live_state(Some(Arc::new(GatewaySeam)));
        assert!(
            llm_gate_allows(&state, &pool, &user_id)
                .await
                .expect("gate reads"),
            "a seam that accepts the user must open the gate without an OSS key"
        );
    }

    #[tokio::test]
    async fn gate_keeps_the_ce_default() {
        let Some(pool) = live_pool().await else {
            return;
        };
        // No `with_assistant_seam`: the default CE seam answers the OSS
        // bit exactly like the pre-seam check did.
        let state = live_state(None);
        let keyless = seed_user(&pool).await;
        assert!(
            !llm_gate_allows(&state, &pool, &keyless)
                .await
                .expect("gate reads"),
            "CE still denies a keyless user (the 422 path)"
        );
        let keyed = seed_user(&pool).await;
        seed_llm_key(&pool, &keyed).await;
        assert!(
            llm_gate_allows(&state, &pool, &keyed)
                .await
                .expect("gate reads"),
            "CE still allows a user with a stored key"
        );
    }

    #[test]
    fn run_turn_queue_honors_the_setting() {
        let mut settings = pidash_db::config::Settings::test_defaults().assistant;
        assert_eq!(run_turn_queue(&settings), "celery");
        settings.turn_queue = Some("turn972c02".to_owned());
        assert_eq!(run_turn_queue(&settings), "turn972c02");
    }

    #[test]
    fn run_turn_queue_treats_empty_as_unset() {
        let mut settings = pidash_db::config::Settings::test_defaults().assistant;
        settings.turn_queue = Some(String::new());
        assert_eq!(run_turn_queue(&settings), "celery");
    }

    /// `ensure_schema`, tolerant of the parallel-test race: two tests
    /// creating `rust_job_queue` at once collide on the catalog entry
    /// (23505), so the loser retries into the winner's finished table.
    async fn ensure_queue_schema(pool: &sqlx::PgPool) {
        for _ in 0..5 {
            match pidash_jobs::queue::ensure_schema(pool).await {
                Ok(()) => return,
                Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => continue,
                Err(other) => panic!("queue schema: {other}"),
            }
        }
        panic!("queue schema: still racing after retries");
    }

    #[tokio::test]
    async fn enqueue_lands_on_the_steered_queue() {
        let Some(pool) = live_pool().await else {
            return;
        };
        ensure_queue_schema(&pool).await;
        let mut settings = pidash_db::config::Settings::test_defaults().assistant;
        settings.turn_queue = Some("turn1026probe".to_owned());
        let turn_id = uuid::Uuid::new_v4();
        enqueue_run_turn(&pool, &settings, &turn_id).await;
        let (queue,): (String,) =
            sqlx::query_as("SELECT queue FROM rust_job_queue WHERE args = $1")
                .bind(serde_json::json!([turn_id.to_string()]))
                .fetch_one(&pool)
                .await
                .expect("steered row lands");
        assert_eq!(queue, "turn1026probe");
    }

    #[tokio::test]
    async fn enqueue_keeps_the_default_queue() {
        let Some(pool) = live_pool().await else {
            return;
        };
        ensure_queue_schema(&pool).await;
        let settings = pidash_db::config::Settings::test_defaults().assistant;
        let turn_id = uuid::Uuid::new_v4();
        enqueue_run_turn(&pool, &settings, &turn_id).await;
        let (queue,): (String,) =
            sqlx::query_as("SELECT queue FROM rust_job_queue WHERE args = $1")
                .bind(serde_json::json!([turn_id.to_string()]))
                .fetch_one(&pool)
                .await
                .expect("default row lands");
        assert_eq!(queue, "celery");
    }

    #[test]
    fn error_bodies_are_byte_identical() {
        assert_eq!(EMPTY_MESSAGE_BODY, r#"{"error":"empty_message"}"#);
        assert_eq!(MESSAGE_TOO_LONG_BODY, r#"{"error":"message_too_long"}"#);
        assert_eq!(
            LLM_CONFIG_MISSING_BODY,
            r#"{"error":"llm_config_missing","detail":"Configure your AI provider in Settings."}"#
        );
        assert_eq!(TURN_ACTIVE_BODY, r#"{"error":"turn_active"}"#);
        assert_eq!(
            THREAD_FULL_BODY,
            r#"{"error":"thread_full","detail":"Start a new thread."}"#
        );
        assert_eq!(NO_ACTIVE_TURN_BODY, r#"{"error":"no_active_turn"}"#);
    }
}
