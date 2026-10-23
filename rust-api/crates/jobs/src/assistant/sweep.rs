//! Stale-turn sweep (D-06, stage 5).
//!
//! Port of `sweep_stale_turns` (`apps/api/pi_dash/assistant/tasks.py:511-526`,
//! PIDASHCONV-254, fixture id F-A6-10).
//!
//! A turn whose worker died stays `RUNNING` forever and wedges its thread
//! (the message POST refuses new turns while `active_turn` is set), so the
//! beat fails every stale turn: `RUNNING` rows with
//! `started_at < now - (hard + 60)` where `hard` is the configured
//! `ASSISTANT_TURN_HARD_LIMIT` (`tasks.py:524`; 390s at defaults), one
//! `_load_context` + `_fail_turn("turn_timeout", …)` per row, skipping rows
//! whose context no longer loads (`tasks.py:520-523`). Returns the failed
//! count.
//!
//! The beat entry already exists in [`crate::schedule`] (`celery.py:100-103`
//! → `assistant-sweep-stale-turns`, every 30s); [`SWEEP_BEAT_NAME`] /
//! [`SWEEP_INTERVAL_SECS`] pin that transcription, like the ticker scanners
//! pin theirs.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use sea_query::{Expr, Order, PostgresQueryBuilder, Query};
use sqlx::PgPool;
use uuid::Uuid;

use super::run_turn::{
    context_eligible, SeamError, TurnContext, CODE_TURN_TIMEOUT, MSG_FAILED, TABLE_TURN,
};
use crate::queue::JobRow;
use crate::worker::{HandlerError, Registry, Verdict};
use pidash_db::assistant::event_queries::{publish_frame, serialize_event_json, EventParts};

/// Celery task name (`tasks.py:511`, `@shared_task(name=...)`).
pub const SWEEP_TASK: &str = "assistant.sweep_stale_turns";

/// Beat entry name (`celery.py:100`).
pub const SWEEP_BEAT_NAME: &str = "assistant-sweep-stale-turns";

/// Beat cadence, seconds (`celery.py:102`, `timedelta(seconds=30)`).
pub const SWEEP_INTERVAL_SECS: u64 = 30;

/// Timeout detail (`tasks.py:524`).
pub const SWEEP_TIMEOUT_DETAIL: &str = "The assistant turn did not finish (worker lost).";

/// Staleness cutoff (`tasks.py:524`): `now - (hard + 60)`.
///
/// `hard_limit_secs` is the configured `ASSISTANT_TURN_HARD_LIMIT`
/// (`settings.assistant.turn_hard_limit`), not the
/// [`TURN_HARD_LIMIT_SECS`][super::run_turn::TURN_HARD_LIMIT_SECS] default:
/// Django reads the same setting at `tasks.py:46`, so a hard-coded 330 here
/// would sweep healthy turns for any operator who raised the limit
/// (PIDASHCONV-1248). 390s at defaults.
pub fn sweep_cutoff(now: &DateTime<Utc>, hard_limit_secs: i64) -> DateTime<Utc> {
    *now - Duration::seconds(hard_limit_secs + super::run_turn::SWEEP_CUTOFF_SLACK_SECS)
}

/// Stale-turn select (`tasks.py:514-518`): `RUNNING` ids with
/// `started_at < cutoff`, `ORDER BY created_at` (the `Meta.ordering`).
pub fn stale_turns_select() -> String {
    Query::select()
        .from(TABLE_TURN)
        .column("id")
        .and_where(Expr::col("status").eq(super::run_turn::TURN_RUNNING))
        .and_where(Expr::cust("started_at < $1"))
        .order_by("created_at", Order::Asc)
        .to_string(PostgresQueryBuilder)
}

/// Sweep context select: turn + thread identity for the fail path.
///
/// The full `_load_context` (`tasks.py:73-96`) also derives display strings
/// (user text, display name, workspace role) for the agent run — but the
/// sweep never runs the agent: it only needs turn/thread ids for `_fail_turn`
/// (`tasks.py:182-201`, which touches `ctx.turn`/`ctx.thread` alone). The
/// display fields stay empty here, and [`TurnContext`] documents which paths
/// need them.
pub fn sweep_context_select() -> String {
    "SELECT t.id, t.status, th.id, th.kind, th.workspace_id, th.user_id \
     FROM assistant_turn t INNER JOIN assistant_thread th ON t.thread_id = th.id \
     WHERE t.id = $1"
        .to_owned()
}

/// How one stale row settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleOutcome {
    /// Context gone (terminal already, or row vanished) — or a concurrent
    /// terminal write won the guarded fail: skipped, uncounted
    /// (`tasks.py:521-523`, `continue`).
    Skipped,
    /// Failed with `turn_timeout` (`tasks.py:524-525`): counted.
    Failed,
}

/// Store boundary for the sweep: list stale ids, load each context, fail it.
/// The fail reports whether its guarded write won the row.
pub trait SweepStore: Send + Sync {
    fn stale_turn_ids(
        &self,
        cutoff: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<Vec<Uuid>, SeamError>> + Send;
    fn load_context(
        &self,
        turn_id: Uuid,
    ) -> impl std::future::Future<Output = Result<Option<TurnContext>, SeamError>> + Send;
    fn fail_turn(
        &self,
        ctx: &TurnContext,
        code: &str,
        detail: &str,
    ) -> impl std::future::Future<Output = Result<bool, SeamError>> + Send;
}

/// Fail every stale turn (`tasks.py:511-526`): per-row load, skip when the
/// context is gone, else `_fail_turn("turn_timeout", …)`. A lost guard race
/// (the turn went terminal concurrently) skips like a gone context.
/// Returns the failed count. `hard_limit_secs` is the configured
/// `ASSISTANT_TURN_HARD_LIMIT` (see [`sweep_cutoff`]).
pub async fn drive_sweep<S: SweepStore>(
    store: &S,
    now: &DateTime<Utc>,
    hard_limit_secs: i64,
) -> Result<(usize, Vec<(Uuid, StaleOutcome)>), SeamError> {
    let cutoff = sweep_cutoff(now, hard_limit_secs);
    let ids = store.stale_turn_ids(cutoff).await?;
    let mut count = 0;
    let mut outcomes = Vec::with_capacity(ids.len());
    for turn_id in ids {
        match store.load_context(turn_id).await? {
            None => outcomes.push((turn_id, StaleOutcome::Skipped)),
            Some(ctx) => {
                if store
                    .fail_turn(&ctx, CODE_TURN_TIMEOUT, SWEEP_TIMEOUT_DETAIL)
                    .await?
                {
                    count += 1;
                    outcomes.push((turn_id, StaleOutcome::Failed));
                } else {
                    outcomes.push((turn_id, StaleOutcome::Skipped));
                }
            }
        }
    }
    Ok((count, outcomes))
}

/// Live store: the two SQL shapes above plus the shared fail-turn write.
/// Eligibility reuses [`context_eligible`] so a turn that went terminal
/// between the select and the load is skipped, not failed. The optional
/// Redis client feeds the post-commit publish; `None` (no `REDIS_URL`)
/// skips it like Django's `None` client.
pub struct LiveSweepStore {
    pool: PgPool,
    redis: Option<redis::Client>,
}

impl LiveSweepStore {
    pub fn new(pool: PgPool, redis: Option<redis::Client>) -> Self {
        Self { pool, redis }
    }
}

/// The committed event row for the post-commit publish (`events.py:67-70`,
/// `_publish_by_id` re-reads by id). `$1` binds the event id; the payload
/// crosses as the column's `::text`.
fn published_event_lookup_sql() -> String {
    format!(
        "SELECT \"id\", \"thread_id\", \"message_id\", \"seq\", \"kind\", \
         \"payload\"::text, \"created_at\" FROM \"{}\" WHERE \"id\" = $1",
        super::run_turn::TABLE_EVENT
    )
}

/// `datetime.isoformat()` for an aware stamp (`events.py:49`): seconds plus
/// six micro digits iff nonzero, suffixed `+00:00`. The same rendering the
/// SSE replay uses for this column (this crate cannot name that helper).
fn py_iso(moment: &DateTime<Utc>) -> String {
    let base = moment.format("%Y-%m-%dT%H:%M:%S").to_string();
    let micros = moment.timestamp_subsec_micros();
    if micros == 0 {
        format!("{base}+00:00")
    } else {
        format!("{base}.{micros:06}+00:00")
    }
}

/// PUBLISH one serialized event on its thread channel (`events.py:53-64`).
/// A missing client is a no-op (`redis_instance()` returning `None`);
/// every failure surfaces to the caller, which swallows it with a log line
/// — the source's `except Exception: logger.exception(...)` never fails
/// the sweep. Same shape as the runner sweeps' chat publish.
async fn publish_event(
    client: Option<&redis::Client>,
    channel: &str,
    payload: &str,
) -> Result<(), redis::RedisError> {
    let Some(client) = client else {
        return Ok(());
    };
    use redis::AsyncCommands;
    let mut connection = client.get_multiplexed_async_connection().await?;
    let _: () = connection.publish(channel, payload).await?;
    Ok(())
}

/// `_publish_by_id` (`events.py:67-70`): re-read the committed event row (a
/// miss publishes nothing) and PUBLISH the serialized frame. A lookup
/// database failure propagates — only the Redis half is swallowed.
async fn publish_turn_failed(
    pool: &PgPool,
    redis: Option<&redis::Client>,
    event_id: i64,
) -> Result<(), SeamError> {
    let row = sqlx::query_as::<_, (i64, Uuid, Option<Uuid>, i64, String, String, DateTime<Utc>)>(
        &published_event_lookup_sql(),
    )
    .bind(event_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| SeamError(e.to_string()))?;
    let Some((id, thread_id, message_id, seq, kind, payload_text, created_at)) = row else {
        return Ok(());
    };
    let thread_str = thread_id.to_string();
    let message_str = message_id.map(|id| id.to_string());
    let created = py_iso(&created_at);
    // The payload relays the column's `::text` verbatim: `jsonb` text uses
    // the same `": "`/`", "` separators and short escapes as `json.dumps`,
    // so for this ASCII payload these are the same bytes the SSE replay
    // renders for the row (it re-renders the same text through `py_dumps`).
    let body = serialize_event_json(&EventParts {
        id,
        thread_id: &thread_str,
        message_id: message_str.as_deref(),
        seq,
        kind: &kind,
        payload_json: &payload_text,
        created_at: &created,
    });
    let (channel, body) = publish_frame(&thread_str, &body);
    if let Err(error) = publish_event(redis, &channel, &body).await {
        tracing::error!(%error, "publish turn_failed failed for thread {thread_str}");
    }
    Ok(())
}

impl SweepStore for LiveSweepStore {
    async fn stale_turn_ids(&self, cutoff: DateTime<Utc>) -> Result<Vec<Uuid>, SeamError> {
        sqlx::query_scalar::<_, Uuid>(&stale_turns_select())
            .bind(cutoff)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| SeamError(e.to_string()))
    }

    async fn load_context(&self, turn_id: Uuid) -> Result<Option<TurnContext>, SeamError> {
        let row: Option<(Uuid, String, Uuid, String, Uuid, Uuid)> =
            sqlx::query_as(&sweep_context_select())
                .bind(turn_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| SeamError(e.to_string()))?;
        Ok(row.and_then(
            |(turn_id, status, thread_id, thread_kind, workspace_id, user_id)| {
                if !context_eligible(&status) {
                    return None;
                }
                Some(TurnContext {
                    turn_id,
                    turn_status: status,
                    thread_id,
                    thread_kind,
                    workspace_id,
                    workspace_slug: String::new(),
                    workspace_name: String::new(),
                    workspace_role: 0,
                    user_id,
                    user_display: String::new(),
                    user_text: String::new(),
                })
            },
        ))
    }

    async fn fail_turn(
        &self,
        ctx: &TurnContext,
        code: &str,
        detail: &str,
    ) -> Result<bool, SeamError> {
        use super::run_turn as rt;
        let code = rt::truncate_code(code).to_owned();
        let detail = rt::truncate_detail(detail).to_owned();
        // `created_at` on the two rows below is `auto_now_add` in Django:
        // stamped at insert, supplied explicitly here (`events.create_message`
        // / `events.append_event`).
        let now = Utc::now();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SeamError(e.to_string()))?;
        // The ORM's `transaction.atomic` arm (`tasks.py:182-190`): lock,
        // fail write, close streaming rows, clear the in-flight pointer.
        sqlx::query(&rt::lock_turn_select())
            .bind(ctx.turn_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| SeamError(e.to_string()))?;
        let won = sqlx::query(&rt::fail_turn_update())
            .bind(ctx.turn_id)
            .bind(&code)
            .bind(&detail)
            .execute(&mut *tx)
            .await
            .map_err(|e| SeamError(e.to_string()))?
            .rows_affected();
        if won == 0 {
            // The turn left `running` between the sweep's load and its lock
            // (a concurrent terminal write won): roll back and emit nothing,
            // so the sweep never writes a second terminal event.
            return Ok(false);
        }
        sqlx::query(&rt::finalize_open_rows_update())
            .bind(ctx.turn_id)
            .bind(MSG_FAILED)
            .execute(&mut *tx)
            .await
            .map_err(|e| SeamError(e.to_string()))?;
        sqlx::query(&rt::clear_active_turn_update())
            .bind(ctx.thread_id)
            .bind(ctx.turn_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| SeamError(e.to_string()))?;
        // The error message row + `turn_failed` event (`tasks.py:191-200`):
        // thread lock first (seq allocation), then both inserts, then delta
        // prune (`tasks.py:201`).
        sqlx::query(&rt::thread_lock_select())
            .bind(ctx.thread_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| SeamError(e.to_string()))?;
        let message_id = Uuid::new_v4();
        let content = if detail.is_empty() {
            code.clone()
        } else {
            detail.clone()
        };
        sqlx::query(&rt::error_message_insert())
            .bind(message_id)
            .bind(ctx.thread_id)
            .bind(ctx.turn_id)
            .bind(&content)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(|e| SeamError(e.to_string()))?;
        let payload = serde_json::json!({
            "turn_id": ctx.turn_id.to_string(),
            "error_code": code,
            "detail": detail,
        });
        let event_id: i64 = sqlx::query_scalar(&rt::turn_failed_event_insert())
            .bind(ctx.thread_id)
            .bind(ctx.turn_id)
            .bind(message_id)
            .bind(payload)
            .bind(now)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| SeamError(e.to_string()))?;
        sqlx::query(&rt::prune_turn_deltas_delete())
            .bind(ctx.turn_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| SeamError(e.to_string()))?;
        tx.commit().await.map_err(|e| SeamError(e.to_string()))?;
        // Post-commit publish (`events.append_event`'s `on_commit`): the
        // event above reaches the thread channel like Django's.
        publish_turn_failed(&self.pool, self.redis.as_ref(), event_id).await?;
        Ok(true)
    }
}

/// Register the local `assistant.sweep_stale_turns` handler.
///
/// Unlike the turn task, the sweep keeps the default retry budget: a DB
/// outage parks-or-retries through the worker's settlement (`tasks.py` sets
/// no `max_retries` on the sweep, so Celery defaults apply). The Redis
/// client feeds the post-commit `turn_failed` publish; `None` (no
/// `REDIS_URL`) skips it like Django's `None` client.
/// `hard_limit_secs` is the configured `ASSISTANT_TURN_HARD_LIMIT`
/// (`settings.assistant.turn_hard_limit`); every run sweeps against
/// `now - (hard + 60)` like Django (`tasks.py:524`).
pub fn register_sweep_handler(
    registry: &mut Registry,
    pool: PgPool,
    redis: Option<redis::Client>,
    hard_limit_secs: i64,
) {
    let store = Arc::new(LiveSweepStore::new(pool, redis));
    registry.register(
        SWEEP_TASK,
        Arc::new(move |_job: JobRow| {
            let store = store.clone();
            let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                Box::pin(async move {
                    let now = Utc::now();
                    drive_sweep(store.as_ref(), &now, hard_limit_secs)
                        .await
                        .map(|_| Verdict::Ack)
                        .map_err(|error| error.to_string())
                });
            fut
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::super::run_turn::{finalize_open_rows_update, lock_turn_select, CODE_TURN_TIMEOUT};
    use super::*;
    use std::sync::Mutex;

    struct FakeStore {
        stale: Vec<Uuid>,
        contexts: Mutex<std::collections::HashMap<Uuid, TurnContext>>,
        failed: Mutex<Vec<(Uuid, String, String)>>,
        win: bool,
    }

    fn ctx(id: Uuid) -> TurnContext {
        TurnContext {
            turn_id: id,
            turn_status: super::super::run_turn::TURN_RUNNING.to_owned(),
            thread_id: Uuid::new_v4(),
            thread_kind: "chat".to_owned(),
            workspace_id: Uuid::new_v4(),
            workspace_slug: "ws".to_owned(),
            workspace_name: "WS".to_owned(),
            workspace_role: 0,
            user_id: Uuid::new_v4(),
            user_display: String::new(),
            user_text: String::new(),
        }
    }

    impl SweepStore for FakeStore {
        async fn stale_turn_ids(&self, _c: DateTime<Utc>) -> Result<Vec<Uuid>, SeamError> {
            Ok(self.stale.clone())
        }
        async fn load_context(&self, id: Uuid) -> Result<Option<TurnContext>, SeamError> {
            Ok(self.contexts.lock().unwrap().get(&id).cloned())
        }
        async fn fail_turn(
            &self,
            c: &TurnContext,
            code: &str,
            detail: &str,
        ) -> Result<bool, SeamError> {
            self.failed
                .lock()
                .unwrap()
                .push((c.turn_id, code.to_owned(), detail.to_owned()));
            Ok(self.win)
        }
    }

    #[test]
    fn cutoff_and_select() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 9, 29, 12, 0, 0).unwrap();
        // Default limit (330): the fixture's flat 390s horizon.
        assert_eq!(
            sweep_cutoff(&now, super::super::run_turn::TURN_HARD_LIMIT_SECS as i64),
            Utc.with_ymd_and_hms(2026, 9, 29, 11, 53, 30).unwrap()
        );
        assert_eq!(SWEEP_TASK, "assistant.sweep_stale_turns");
        assert_eq!(SWEEP_BEAT_NAME, "assistant-sweep-stale-turns");
        assert_eq!(SWEEP_INTERVAL_SECS, 30);

        let sql = stale_turns_select();
        assert!(sql.contains("assistant_turn"));
        assert!(sql.contains("status"), "status predicate: {sql}");
        assert!(sql.contains("started_at < $1"), "cutoff predicate: {sql}");
        assert!(sql.contains("ORDER BY"), "created_at ordering: {sql}");
        assert!(lock_turn_select().contains("FOR UPDATE"));
        assert!(finalize_open_rows_update().contains("streaming"));
        // The fail path reuses the shared turn-task writes.
        assert_eq!(CODE_TURN_TIMEOUT, "turn_timeout");
        assert_eq!(
            SWEEP_TIMEOUT_DETAIL,
            "The assistant turn did not finish (worker lost)."
        );
    }

    /// PIDASHCONV-1248: the cutoff honors the configured
    /// `ASSISTANT_TURN_HARD_LIMIT`, not the 330 default — a 400s-old turn is
    /// stale under `now - 390s` but fresh under `now - 960s`.
    #[test]
    fn cutoff_honors_configured_hard_limit() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 9, 29, 12, 0, 0).unwrap();
        assert_eq!(
            sweep_cutoff(&now, 900),
            Utc.with_ymd_and_hms(2026, 9, 29, 11, 44, 0).unwrap()
        );
    }

    /// The configured limit flows through `drive_sweep` into the stale
    /// select: the store sees `now - (900 + 60)`.
    #[tokio::test]
    async fn drive_sweep_passes_configured_cutoff_to_store() {
        struct CaptureStore {
            seen: Mutex<Option<DateTime<Utc>>>,
        }
        impl SweepStore for CaptureStore {
            async fn stale_turn_ids(&self, c: DateTime<Utc>) -> Result<Vec<Uuid>, SeamError> {
                *self.seen.lock().unwrap() = Some(c);
                Ok(Vec::new())
            }
            async fn load_context(&self, _id: Uuid) -> Result<Option<TurnContext>, SeamError> {
                Ok(None)
            }
            async fn fail_turn(
                &self,
                _c: &TurnContext,
                _code: &str,
                _detail: &str,
            ) -> Result<bool, SeamError> {
                Ok(true)
            }
        }
        let store = CaptureStore {
            seen: Mutex::new(None),
        };
        let now = Utc::now();
        let (count, outcomes) = drive_sweep(&store, &now, 900).await.unwrap();
        assert_eq!(count, 0);
        assert!(outcomes.is_empty());
        assert_eq!(
            store.seen.lock().unwrap().unwrap(),
            now - Duration::seconds(960)
        );
    }

    #[tokio::test]
    async fn sweep_fails_stale_skips_gone() {
        let live = Uuid::new_v4();
        let gone = Uuid::new_v4();
        let store = FakeStore {
            stale: vec![live, gone],
            contexts: Mutex::new([(live, ctx(live))].into_iter().collect()),
            failed: Mutex::new(Vec::new()),
            win: true,
        };
        let now = Utc::now();
        let (count, outcomes) = drive_sweep(
            &store,
            &now,
            super::super::run_turn::TURN_HARD_LIMIT_SECS as i64,
        )
        .await
        .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            outcomes,
            vec![(live, StaleOutcome::Failed), (gone, StaleOutcome::Skipped),]
        );
        assert_eq!(
            store.failed.lock().unwrap().as_slice(),
            [(
                live,
                CODE_TURN_TIMEOUT.to_owned(),
                SWEEP_TIMEOUT_DETAIL.to_owned()
            )]
        );
    }

    /// A lost guard race (the turn went terminal between the sweep's load
    /// and its fail) skips like a gone context: uncounted, no failure.
    #[tokio::test]
    async fn sweep_lost_guard_race_skips_uncounted() {
        let raced = Uuid::new_v4();
        let store = FakeStore {
            stale: vec![raced],
            contexts: Mutex::new([(raced, ctx(raced))].into_iter().collect()),
            failed: Mutex::new(Vec::new()),
            win: false,
        };
        let now = Utc::now();
        let (count, outcomes) = drive_sweep(
            &store,
            &now,
            super::super::run_turn::TURN_HARD_LIMIT_SECS as i64,
        )
        .await
        .unwrap();
        assert_eq!(count, 0);
        assert_eq!(outcomes, vec![(raced, StaleOutcome::Skipped)]);
    }

    #[test]
    fn beat_entry_transcription() {
        // `celery.py:100-103` → schedule.rs: the entry keeps its name, task
        // and 30s cadence.
        let entries = crate::schedule::beat_schedule();
        let entry = entries
            .iter()
            .find(|e| e.name == SWEEP_BEAT_NAME)
            .expect("assistant-sweep-stale-turns transcribed");
        assert_eq!(entry.task, SWEEP_TASK);
        assert_eq!(
            entry.cadence,
            crate::schedule::Cadence::IntervalSecs(SWEEP_INTERVAL_SECS)
        );
    }

    #[test]
    fn publish_lookup_selects_event_row_by_id() {
        let sql = published_event_lookup_sql();
        assert!(sql.contains("assistant_event"), "event table: {sql}");
        assert!(sql.contains("\"payload\"::text"), "payload text: {sql}");
        assert!(sql.contains("WHERE \"id\" = $1"), "by-id lookup: {sql}");
    }

    #[test]
    fn py_iso_renders_python_isoformat_bytes() {
        use chrono::TimeZone;
        let whole = Utc.with_ymd_and_hms(2026, 9, 29, 13, 50, 51).unwrap();
        assert_eq!(py_iso(&whole), "2026-09-29T13:50:51+00:00");
        let micros = whole + chrono::Duration::microseconds(378_615);
        assert_eq!(py_iso(&micros), "2026-09-29T13:50:51.378615+00:00");
        // Trailing-zero micros keep six digits (where chrono AutoSi would
        // trim), like the SSE replay's `py_iso`.
        let millis = whole + chrono::Duration::milliseconds(120);
        assert_eq!(py_iso(&millis), "2026-09-29T13:50:51.120000+00:00");
    }

    #[test]
    fn publish_frame_targets_thread_channel() {
        // Representative committed row: the payload relays the column's
        // `::text` verbatim (`jsonb` key order), like the SSE replay renders.
        let parts = EventParts {
            id: 7,
            thread_id: "tid-1",
            message_id: Some("mid-9"),
            seq: 3,
            kind: "turn_failed",
            payload_json: "{\"Detail\": \"x\", \"turn_id\": \"t\", \"error_code\": \"c\"}",
            created_at: "2026-09-29T13:50:51+00:00",
        };
        let (channel, body) = publish_frame(parts.thread_id, &serialize_event_json(&parts));
        assert_eq!(channel, "assistant:thread:tid-1");
        assert_eq!(
            body,
            "{\"id\": 7, \"thread\": \"tid-1\", \"message\": \"mid-9\", \"seq\": 3, \
             \"kind\": \"turn_failed\", \"payload\": {\"Detail\": \"x\", \"turn_id\": \"t\", \
             \"error_code\": \"c\"}, \"created_at\": \"2026-09-29T13:50:51+00:00\"}"
        );
    }

    /// A missing Redis client is a no-op publish (`redis_instance()`
    /// returning `None`): the sweep keeps working with no `REDIS_URL`.
    #[tokio::test]
    async fn publish_without_client_is_noop() {
        publish_event(None, "assistant:thread:tid-1", "{}")
            .await
            .expect("None client publishes nothing, fails nothing");
    }
}
