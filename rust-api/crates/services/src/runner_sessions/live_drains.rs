#![forbid(unsafe_code)]

//! Live matcher-drain executors (PIDASHCONV-862).
//!
//! Public [`drain_for_runner_by_id`] + [`drain_pod_by_id`] over
//! (`pool`, `redis client`, [`RunnerSettings`]), unifying the five
//! private api copies (`runner_sessions/runner_open.rs`,
//! `runner_sessions/runner_poll.rs`, `runner_enroll/teardown.rs`,
//! `app_issues/handlers_versions_move.rs`,
//! `v1_work_items/handlers_move_search.rs`): lookup-miss → `Ok` with
//! no transaction; else one transaction over lock → claim → assign
//! ([`ASSIGN_RUN_UPDATE_SQL`][drain_kernel::ASSIGN_RUN_UPDATE_SQL]
//! with a fresh microsecond `now` per assignment); commit; then one
//! post-commit `assign` dispatch per claim through
//! [`send_to_runner`](pubsub_kernel::send_to_runner) in assignment
//! order; the matcher info lines. Matches
//! `matcher.py:194-303` semantics.
//!
//! [`PoolPubsubStore`] is the public [`PubsubStore`] implementation
//! the drains send through (the api `PoolPubsubStore` precedent with
//! explicit fields instead of `&AppState`): each statement
//! autocommits, like the source paths that run outside any `atomic`.
//! The bin's `SweepsRunsOutbox::send_to_runner` can reuse it instead
//! of carrying its own.
//!
//! Return values follow Python: the runner drain answers whether a
//! run was assigned, the pod drain answers the assignment count. The
//! jobs `RunnerRunsSeams` provider maps both to `()`.
//!
//! # Translation notes
//!
//! * `visibility` reads as `i16` (the column is
//!   `PositiveSmallIntegerField` → `smallint`); three of the five
//!   private copies agree (the two move copies read `i32`).
//! * `run_config` reads as `Value` (the column is `NOT NULL` with a
//!   `default=dict`); a non-object answers [`DrainError::Config`],
//!   like the private copies' `Err`.
//! * Timestamps truncate to microseconds (Django datetimes are
//!   microsecond-exact; Postgres rounds stored nanos, so a raw
//!   `Utc::now()` can land 1µs off Python). The threshold
//!   comparison is unaffected either way (heartbeat stamps are
//!   microsecond-exact, so truncation never flips it).
//! * A lock/claim miss commits the empty transaction (the
//!   `runner_open.rs` shape) rather than dropping it (the
//!   `runner_poll.rs` shape) — same rows, no writes either way.
//! * Dispatch failures propagate: the offline error is `Err` (the
//!   row stays `ASSIGNED` while the caller observes it, fixture
//!   `drain_for_runner_sessionless`); swallowed failures come back
//!   as warning lines the executor logs.

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_db::config::RunnerSettings;
use pidash_db::runner_sessions::machine_outbox::MachineOutboxError;
use pidash_db::runner_sessions::models::runner_session::runner_session_from_row;
use pidash_db::runner_sessions::outbox::{self as session_outbox, OutboxError};
use pidash_db::runner_sessions::RunnerSession;

use super::drain as drain_kernel;
use super::guards::alive_threshold;
use super::pubsub::{self as pubsub_kernel, PubsubStore};

/// `timezone.now()` truncated to microseconds (Django datetimes are
/// microsecond-exact; Postgres would round stored nanos).
fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).expect("micros in range")
}

// ---------------------------------------------------------------------------
// Pool-backed `PubsubStore`
// ---------------------------------------------------------------------------

/// Pool-backed [`PubsubStore`] over explicit inputs (no `AppState`).
/// Construct with [`PoolPubsubStore::new`] — no I/O.
#[derive(Debug, Clone)]
pub struct PoolPubsubStore {
    pool: PgPool,
    redis: Option<redis::Client>,
    runner: RunnerSettings,
}

impl PoolPubsubStore {
    /// Build the store. No I/O: the pool, client and settings are
    /// stored for the per-call effects.
    pub fn new(pool: PgPool, redis: Option<redis::Client>, runner: RunnerSettings) -> Self {
        Self {
            pool,
            redis,
            runner,
        }
    }
}

impl PubsubStore for PoolPubsubStore {
    async fn enqueue_for_runner(
        &self,
        runner_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, OutboxError> {
        session_outbox::enqueue_for_runner(
            self.redis.as_ref(),
            &self.pool,
            &self.runner,
            runner_id,
            message,
        )
        .await
    }

    async fn enqueue_for_machine(
        &self,
        dev_machine_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, MachineOutboxError> {
        pidash_db::runner_sessions::machine_outbox::enqueue_for_machine(
            self.redis.as_ref(),
            &self.pool,
            &self.runner,
            dev_machine_id,
            message,
        )
        .await
    }

    async fn active_runner_sessions(
        &self,
        runner_id: Uuid,
    ) -> Result<Vec<RunnerSession>, OutboxError> {
        let rows = sqlx::query(pubsub_kernel::CLOSE_ACTIVE_SESSIONS_SQL)
            .bind(runner_id)
            .fetch_all(&self.pool)
            .await
            .map_err(OutboxError::Db)?;
        rows.iter()
            .map(runner_session_from_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(OutboxError::Db)
    }

    async fn revoke_runner_session(
        &self,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), OutboxError> {
        crate::domain::tokens::revoke_runner_session(&self.pool, session_id, now_micros(), reason)
            .await
            .map_err(OutboxError::Db)?;
        Ok(())
    }

    async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), OutboxError> {
        session_outbox::clear_session_marker(self.redis.as_ref(), &session_id.to_string()).await
    }

    async fn publish_session_eviction(
        &self,
        runner_id: Uuid,
        old_session_id: Uuid,
        new_session_id: &str,
    ) -> Result<(), OutboxError> {
        session_outbox::publish_session_eviction(
            self.redis.as_ref(),
            &runner_id.to_string(),
            Some(&old_session_id.to_string()),
            new_session_id,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure a live drain reports. The bin maps it to the seam
/// error string via `Display`.
#[derive(Debug, thiserror::Error)]
pub enum DrainError {
    /// A database failure (`sqlx::Error`).
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    /// A post-commit dispatch failure. Only the offline error reaches
    /// here ([`pubsub_kernel::send_to_runner`] swallows the rest into
    /// warnings); the assignment rows are already committed.
    #[error("outbox error: {0}")]
    Outbox(#[from] OutboxError),
    /// A `run_config` that is not an object (unreachable on the
    /// `NOT NULL default=dict` column; the private copies `Err` too).
    #[error("{0}")]
    Config(String),
}

// ---------------------------------------------------------------------------
// Claim (`next_assignable_for_runner`, PDASHOSS01-272)
// ---------------------------------------------------------------------------

/// `next_assignable_for_runner(runner, pod)`: the next run `runner`
/// should take, breaking an over-budget pin when that is the run that
/// wins. Every drain executor claims through this one function (the
/// public drains below and the private api copies), so the pin wait
/// budget cannot drift between them.
///
/// One selection, not a primary plus a fallback: the runner's own
/// pinned work still comes first (rank 0), and the unpinned pod queue
/// plus any pin that has outlived the pod's budget compete on
/// `created_at` (rank 1). The pin is cleared — together with the
/// parent's stale `thread_id`, exactly as the release-pin hatches do —
/// only for the run that is actually claimed.
///
/// Runs on the caller's transaction connection; the returned row is
/// locked `FOR UPDATE SKIP LOCKED` and carries the
/// [`next_for_runner_sql`](drain_kernel::next_for_runner_sql)
/// projection (its `pinned_runner_id` is the pre-release value).
/// `Ok(None)` for a non-private runner without issuing a query (the
/// `qs.none()` arm), or when nothing is claimable.
#[allow(clippy::too_many_arguments)]
pub async fn claim_next_assignable(
    conn: &mut sqlx::PgConnection,
    pod_id: Uuid,
    runner_id: Uuid,
    owner_id: Uuid,
    provisioning: &str,
    visibility: i32,
    instance_budget_secs: i64,
) -> Result<Option<sqlx::postgres::PgRow>, sqlx::Error> {
    let Some(plain_sql) = drain_kernel::next_for_runner_sql(provisioning, visibility) else {
        return Ok(None);
    };
    let mut releasable: Vec<Uuid> = Vec::new();
    let mut budget_secs = 0;
    if let Some(candidates_sql) =
        drain_kernel::releasable_overbudget_pin_ids_sql(provisioning, visibility)
    {
        let pod_override: Option<Option<i32>> =
            sqlx::query_scalar(drain_kernel::POD_PIN_WAIT_BUDGET_SQL)
                .bind(pod_id)
                .fetch_optional(&mut *conn)
                .await?;
        budget_secs = drain_kernel::effective_pin_wait_budget_secs(
            pod_override.flatten().map(i64::from),
            instance_budget_secs,
        );
        if budget_secs > 0 {
            let now = now_micros();
            releasable = sqlx::query_scalar(&candidates_sql)
                .bind(pod_id)
                .bind(runner_id)
                .bind(owner_id)
                .bind(now - chrono::Duration::seconds(budget_secs))
                .bind(alive_threshold(now))
                .fetch_all(&mut *conn)
                .await?;
        }
    }
    let run = if releasable.is_empty() {
        sqlx::query(&plain_sql)
            .bind(pod_id)
            .bind(runner_id)
            .bind(owner_id)
            .fetch_optional(&mut *conn)
            .await?
    } else {
        let widened_sql =
            drain_kernel::next_for_runner_with_releasable_sql(provisioning, visibility)
                .expect("same visibility gate as next_for_runner_sql");
        sqlx::query(&widened_sql)
            .bind(pod_id)
            .bind(runner_id)
            .bind(owner_id)
            .bind(&releasable)
            .fetch_optional(&mut *conn)
            .await?
    };
    let Some(run) = run else {
        return Ok(None);
    };
    let pinned_runner_id: Option<Uuid> = run.try_get("pinned_runner_id")?;
    if let (true, Some(previous_runner_id)) = (
        drain_kernel::is_overbudget_takeover(pinned_runner_id, runner_id),
        pinned_runner_id,
    ) {
        let run_id: Uuid = run.try_get("id")?;
        let parent_run_id: Option<Uuid> = run.try_get("parent_run_id")?;
        let created_at: DateTime<Utc> = run.try_get("created_at")?;
        if let Some(parent_run_id) = parent_run_id {
            sqlx::query(drain_kernel::CLEAR_PARENT_THREAD_ID_SQL)
                .bind(parent_run_id)
                .execute(&mut *conn)
                .await?;
        }
        sqlx::query(&super::offline_recovery::clear_pin_sql())
            .bind(run_id)
            .execute(&mut *conn)
            .await?;
        let waited_secs = (now_micros() - created_at).num_seconds();
        tracing::info!(
            "{}",
            drain_kernel::released_pin_log(
                &run_id,
                &previous_runner_id,
                &runner_id,
                budget_secs,
                waited_secs
            )
        );
    }
    Ok(Some(run))
}

// ---------------------------------------------------------------------------
// Executors
// ---------------------------------------------------------------------------

/// `drain_for_runner_by_id` (`matcher.py:298-304`): runner lookup
/// (miss ⇒ `Ok(false)`, no transaction), else the single-assignment
/// drain. Answers whether a run was assigned.
pub async fn drain_for_runner_by_id(
    pool: &PgPool,
    redis: Option<&redis::Client>,
    runner: &RunnerSettings,
    runner_id: Uuid,
) -> Result<bool, DrainError> {
    let row = sqlx::query(drain_kernel::DRAIN_FOR_RUNNER_BY_ID_LOOKUP_SQL)
        .bind(runner_id)
        .fetch_optional(pool)
        .await?;
    if row.is_none() {
        return Ok(false);
    }
    drain_for_runner(pool, redis, runner, runner_id).await
}

/// `drain_for_runner` (`matcher.py:253-296`): re-select the runner
/// under `SKIP LOCKED` (miss ⇒ `Ok(false)`), take the next queued
/// run (miss ⇒ `Ok(false)`), write the assignment, commit, then
/// dispatch the frame and log.
async fn drain_for_runner(
    pool: &PgPool,
    redis: Option<&redis::Client>,
    runner: &RunnerSettings,
    runner_id: Uuid,
) -> Result<bool, DrainError> {
    let mut tx = pool.begin().await?;
    let threshold = alive_threshold(now_micros());
    let locked = sqlx::query(drain_kernel::DRAIN_FOR_RUNNER_LOCK_SQL)
        .bind(threshold)
        .bind(runner_id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(locked) = locked else {
        tx.commit().await?;
        return Ok(false);
    };
    let owner_id: Uuid = locked.try_get("owner_id")?;
    let pod_id: Uuid = locked.try_get("pod_id")?;
    let provisioning: String = locked.try_get("provisioning")?;
    let visibility: i16 = locked.try_get("visibility")?;
    // Non-private runners issue no query (the `qs.none()` arm).
    let run = claim_next_assignable(
        &mut tx,
        pod_id,
        runner_id,
        owner_id,
        &provisioning,
        i32::from(visibility),
        runner.pin_wait_budget_secs,
    )
    .await?;
    let Some(run) = run else {
        tx.commit().await?;
        return Ok(false);
    };
    let run_id: Uuid = run.try_get("id")?;
    let work_item_id: Option<Uuid> = run.try_get("work_item_id")?;
    let prompt: String = run.try_get("prompt")?;
    let run_config: Value = run.try_get("run_config")?;
    let Value::Object(run_config) = run_config else {
        return Err(DrainError::Config(
            "drain_for_runner: run_config is not an object".to_owned(),
        ));
    };
    // `$3` is a fresh `timezone.now()` per assignment (`:285-289`).
    sqlx::query(drain_kernel::ASSIGN_RUN_UPDATE_SQL)
        .bind(owner_id)
        .bind(runner_id)
        .bind(now_micros())
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
    let plan = drain_kernel::plan_assignment(&drain_kernel::AssignmentFacts {
        run_id,
        runner_id,
        owner_id,
        work_item_id,
        prompt,
        run_config,
    });
    tx.commit().await?;
    let drain_kernel::DrainEffect::SendAssign { runner_id, message } = &plan.after_commit;
    let store = PoolPubsubStore::new(pool.clone(), redis.cloned(), runner.clone());
    let outcome = pubsub_kernel::send_to_runner(&store, *runner_id, message).await?;
    for warning in &outcome.warnings {
        tracing::warn!("{warning}");
    }
    tracing::info!("{}", drain_kernel::drain_for_runner_log(runner_id, &run_id));
    Ok(true)
}

/// `drain_pod_by_id` (`matcher.py:242-251`): pod lookup (miss ⇒
/// `Ok(0)`, no transaction), else the drain loop. Answers the
/// assignment count.
pub async fn drain_pod_by_id(
    pool: &PgPool,
    redis: Option<&redis::Client>,
    runner: &RunnerSettings,
    pod_id: Uuid,
) -> Result<usize, DrainError> {
    let pod = sqlx::query(drain_kernel::DRAIN_POD_BY_ID_LOOKUP_SQL)
        .bind(pod_id)
        .fetch_optional(pool)
        .await?;
    if pod.is_none() {
        return Ok(0);
    }
    drain_pod(pool, redis, runner, pod_id).await
}

/// `drain_pod` (`matcher.py:194-240`): lock the idle list, then per
/// runner in list order take the next queued run, write the
/// assignment, and plan the frame; commit once; then dispatch each
/// frame in assignment order and log when non-empty.
async fn drain_pod(
    pool: &PgPool,
    redis: Option<&redis::Client>,
    runner: &RunnerSettings,
    pod_id: Uuid,
) -> Result<usize, DrainError> {
    let mut tx = pool.begin().await?;
    let threshold = alive_threshold(now_micros());
    let runners = sqlx::query(drain_kernel::DRAIN_POD_IDLE_RUNNERS_SQL)
        .bind(threshold)
        .bind(pod_id)
        .fetch_all(&mut *tx)
        .await?;
    let mut plans: Vec<drain_kernel::AssignmentPlan> = Vec::new();
    for runner_row in &runners {
        let runner_id: Uuid = runner_row.try_get("id")?;
        let owner_id: Uuid = runner_row.try_get("owner_id")?;
        let provisioning: String = runner_row.try_get("provisioning")?;
        let visibility: i16 = runner_row.try_get("visibility")?;
        let run = claim_next_assignable(
            &mut tx,
            pod_id,
            runner_id,
            owner_id,
            &provisioning,
            i32::from(visibility),
            runner.pin_wait_budget_secs,
        )
        .await?;
        let Some(run) = run else {
            continue;
        };
        let run_id: Uuid = run.try_get("id")?;
        let work_item_id: Option<Uuid> = run.try_get("work_item_id")?;
        let prompt: String = run.try_get("prompt")?;
        let run_config: Value = run.try_get("run_config")?;
        let Value::Object(run_config) = run_config else {
            return Err(DrainError::Config(
                "drain_pod: run_config is not an object".to_owned(),
            ));
        };
        sqlx::query(drain_kernel::ASSIGN_RUN_UPDATE_SQL)
            .bind(owner_id)
            .bind(runner_id)
            .bind(now_micros())
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        plans.push(drain_kernel::plan_assignment(
            &drain_kernel::AssignmentFacts {
                run_id,
                runner_id,
                owner_id,
                work_item_id,
                prompt,
                run_config,
            },
        ));
    }
    tx.commit().await?;
    let store = PoolPubsubStore::new(pool.clone(), redis.cloned(), runner.clone());
    for plan in &plans {
        let drain_kernel::DrainEffect::SendAssign { runner_id, message } = &plan.after_commit;
        let outcome = pubsub_kernel::send_to_runner(&store, *runner_id, message).await?;
        for warning in &outcome.warnings {
            tracing::warn!("{warning}");
        }
    }
    // After the sends (`matcher.py:235-238`): a dispatch failure
    // propagates with no info line, exactly as Python.
    if !plans.is_empty() {
        tracing::info!("{}", drain_kernel::drain_pod_log(&pod_id, plans.len()));
    }
    Ok(plans.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runner_settings() -> RunnerSettings {
        RunnerSettings {
            long_poll_interval_secs: 25,
            access_token_ttl_secs: 3600,
            offline_threshold_secs: 90,
            offline_pin_release_secs: 300,
            offline_run_fail_secs: 3600,
            offline_stream_ttl_secs: 86400,
            offline_stream_maxlen: 1000,
            stream_min_retention_secs: 3600,
            event_batch_max_age_ms: 1000,
            event_batch_max_bytes: 65536,
            run_message_dedupe_ttl_secs: 86400,
            latest_runner_version: None,
            min_runner_version: None,
            agent_stall_threshold_secs: 360,
            agent_observability_stale_secs: 90,
            pin_wait_budget_secs: 600,
        }
    }

    /// The store constructs with no I/O (the 859 providers hold one
    /// per call shape; construction must not touch the network).
    #[tokio::test]
    async fn pool_pubsub_store_constructs_without_io() {
        let pool = PgPool::connect_lazy("postgres://localhost:1/drains_no_io")
            .expect("lazy pool builds without connecting");
        let store = PoolPubsubStore::new(pool, None, runner_settings());
        assert!(store.redis.is_none());
        let cloned = store.clone();
        assert!(cloned.redis.is_none());
    }

    /// Error rendering the bin maps to the seam string.
    #[test]
    fn drain_error_config_renders_verbatim() {
        let error = DrainError::Config("drain_pod: run_config is not an object".to_owned());
        assert_eq!(error.to_string(), "drain_pod: run_config is not an object");
    }

    /// The executors run the kernel SQL consts verbatim (never a
    /// fork): the by-id lookups carry the same soft-delete scope and
    /// ordering the kernel pins against FX-RSES-07.
    #[test]
    fn executors_use_kernel_sql_consts() {
        assert!(drain_kernel::DRAIN_FOR_RUNNER_BY_ID_LOOKUP_SQL.contains("FROM \"runner\""));
        assert!(drain_kernel::DRAIN_POD_BY_ID_LOOKUP_SQL.contains("\"pod\".\"deleted_at\" IS NULL"));
        assert!(drain_kernel::DRAIN_FOR_RUNNER_LOCK_SQL.contains("FOR UPDATE SKIP LOCKED"));
        assert!(drain_kernel::DRAIN_POD_IDLE_RUNNERS_SQL.contains("FOR UPDATE SKIP LOCKED"));
    }

    async fn pool() -> PgPool {
        let url = std::env::var("DATABASE_URL").expect("export DATABASE_URL for live drain tests");
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("pool connects")
    }

    /// Lookup-miss answers `Ok(false)` with no transaction (an
    /// unknown runner id touches only the by-id SELECT).
    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_drain_for_runner_by_id_miss_is_ok_false() {
        let pool = pool().await;
        let settings = runner_settings();
        let assigned = drain_for_runner_by_id(&pool, None, &settings, Uuid::new_v4())
            .await
            .expect("miss is Ok");
        assert!(!assigned);
    }

    /// Lookup-miss answers `Ok(0)` with no transaction.
    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_drain_pod_by_id_miss_is_ok_zero() {
        let pool = pool().await;
        let settings = runner_settings();
        let assigned = drain_pod_by_id(&pool, None, &settings, Uuid::new_v4())
            .await
            .expect("miss is Ok");
        assert_eq!(assigned, 0);
    }
}
