//! Stale-turn sweep (D-06, stage 5).
//!
//! Port of `sweep_stale_turns` (`apps/api/pi_dash/assistant/tasks.py:511-526`,
//! PIDASHCONV-254, fixture id F-A6-10).
//!
//! A turn whose worker died stays `RUNNING` forever and wedges its thread
//! (the message POST refuses new turns while `active_turn` is set), so the
//! beat fails every stale turn: `RUNNING` rows with
//! `started_at < now - (TURN_HARD_LIMIT + 60)` (`tasks.py:513`), one
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

/// Celery task name (`tasks.py:511`, `@shared_task(name=...)`).
pub const SWEEP_TASK: &str = "assistant.sweep_stale_turns";

/// Beat entry name (`celery.py:100`).
pub const SWEEP_BEAT_NAME: &str = "assistant-sweep-stale-turns";

/// Beat cadence, seconds (`celery.py:102`, `timedelta(seconds=30)`).
pub const SWEEP_INTERVAL_SECS: u64 = 30;

/// Staleness horizon, seconds: `TURN_HARD_LIMIT + 60` (`tasks.py:513`).
/// Unlike the fixture's flat `390`, this derives from the limit const so the
/// two cannot drift apart.
pub const SWEEP_CUTOFF_SECS: i64 =
    super::run_turn::TURN_HARD_LIMIT_SECS as i64 + super::run_turn::SWEEP_CUTOFF_SLACK_SECS;

/// Timeout detail (`tasks.py:524`).
pub const SWEEP_TIMEOUT_DETAIL: &str = "The assistant turn did not finish (worker lost).";

/// Staleness cutoff (`tasks.py:513`): `now - (hard + 60)`.
pub fn sweep_cutoff(now: &DateTime<Utc>) -> DateTime<Utc> {
    *now - Duration::seconds(SWEEP_CUTOFF_SECS)
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
    /// Context gone (terminal already, or row vanished): skipped, uncounted
    /// (`tasks.py:521-523`, `continue`).
    Skipped,
    /// Failed with `turn_timeout` (`tasks.py:524-525`): counted.
    Failed,
}

/// Store boundary for the sweep: list stale ids, load each context, fail it.
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
    ) -> impl std::future::Future<Output = Result<(), SeamError>> + Send;
}

/// Fail every stale turn (`tasks.py:511-526`): per-row load, skip when the
/// context is gone, else `_fail_turn("turn_timeout", …)`. Returns the failed
/// count.
pub async fn drive_sweep<S: SweepStore>(
    store: &S,
    now: &DateTime<Utc>,
) -> Result<(usize, Vec<(Uuid, StaleOutcome)>), SeamError> {
    let cutoff = sweep_cutoff(now);
    let ids = store.stale_turn_ids(cutoff).await?;
    let mut count = 0;
    let mut outcomes = Vec::with_capacity(ids.len());
    for turn_id in ids {
        match store.load_context(turn_id).await? {
            None => outcomes.push((turn_id, StaleOutcome::Skipped)),
            Some(ctx) => {
                store
                    .fail_turn(&ctx, CODE_TURN_TIMEOUT, SWEEP_TIMEOUT_DETAIL)
                    .await?;
                count += 1;
                outcomes.push((turn_id, StaleOutcome::Failed));
            }
        }
    }
    Ok((count, outcomes))
}

/// Live store: the two SQL shapes above plus the shared fail-turn write.
/// Eligibility reuses [`context_eligible`] so a turn that went terminal
/// between the select and the load is skipped, not failed.
pub struct LiveSweepStore {
    pool: PgPool,
}

impl LiveSweepStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
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
    ) -> Result<(), SeamError> {
        use super::run_turn as rt;
        let code = rt::truncate_code(code).to_owned();
        let detail = rt::truncate_detail(detail).to_owned();
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
        sqlx::query(&rt::fail_turn_update())
            .bind(ctx.turn_id)
            .bind(&code)
            .bind(&detail)
            .execute(&mut *tx)
            .await
            .map_err(|e| SeamError(e.to_string()))?;
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
            .execute(&mut *tx)
            .await
            .map_err(|e| SeamError(e.to_string()))?;
        let payload = serde_json::json!({
            "turn_id": ctx.turn_id.to_string(),
            "error_code": code,
            "detail": detail,
        });
        sqlx::query(&rt::turn_failed_event_insert())
            .bind(ctx.thread_id)
            .bind(ctx.turn_id)
            .bind(message_id)
            .bind(payload)
            .execute(&mut *tx)
            .await
            .map_err(|e| SeamError(e.to_string()))?;
        sqlx::query(&rt::prune_turn_deltas_delete())
            .bind(ctx.turn_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| SeamError(e.to_string()))?;
        tx.commit().await.map_err(|e| SeamError(e.to_string()))?;
        Ok(())
    }
}

/// Register the local `assistant.sweep_stale_turns` handler.
///
/// Unlike the turn task, the sweep keeps the default retry budget: a DB
/// outage parks-or-retries through the worker's settlement (`tasks.py` sets
/// no `max_retries` on the sweep, so Celery defaults apply).
pub fn register_sweep_handler(registry: &mut Registry, pool: PgPool) {
    let store = Arc::new(LiveSweepStore::new(pool));
    registry.register(
        SWEEP_TASK,
        Arc::new(move |_job: JobRow| {
            let store = store.clone();
            let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                Box::pin(async move {
                    let now = Utc::now();
                    drive_sweep(store.as_ref(), &now)
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
        ) -> Result<(), SeamError> {
            self.failed
                .lock()
                .unwrap()
                .push((c.turn_id, code.to_owned(), detail.to_owned()));
            Ok(())
        }
    }

    #[test]
    fn cutoff_and_select() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 9, 29, 12, 0, 0).unwrap();
        assert_eq!(
            sweep_cutoff(&now),
            Utc.with_ymd_and_hms(2026, 9, 29, 11, 53, 30).unwrap()
        );
        assert_eq!(SWEEP_CUTOFF_SECS, 390);
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

    #[tokio::test]
    async fn sweep_fails_stale_skips_gone() {
        let live = Uuid::new_v4();
        let gone = Uuid::new_v4();
        let store = FakeStore {
            stale: vec![live, gone],
            contexts: Mutex::new([(live, ctx(live))].into_iter().collect()),
            failed: Mutex::new(Vec::new()),
        };
        let now = Utc::now();
        let (count, outcomes) = drive_sweep(&store, &now).await.unwrap();
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
}
