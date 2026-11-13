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
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, QueryFilter, QueryOrder, TransactionTrait,
};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::config::RunnerSettings;
use pidash_db::entity::{agent_run, dev_machine, pod, runner};
use pidash_db::orm_tx::into_sqlx;
use pidash_db::runner_sessions::machine_outbox::MachineOutboxError;
use pidash_db::runner_sessions::outbox::{self as session_outbox, OutboxError};
use pidash_db::runner_sessions::RunnerSession;

use super::drain::{self as drain_kernel, DispatchFacts};
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
        pubsub_kernel::active_runner_sessions(
            &pidash_db::pool::sea_orm_connection(&self.pool),
            runner_id,
        )
        .await
        .map_err(|error| OutboxError::Db(into_sqlx(error)))
    }

    async fn revoke_runner_session(
        &self,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), OutboxError> {
        crate::domain::tokens::revoke_runner_session(
            &pidash_db::pool::sea_orm_connection(&self.pool),
            session_id,
            now_micros(),
            reason,
        )
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
    /// A SeaORM failure (the kernel statements run through it).
    #[error("database error: {0}")]
    Sea(#[from] sea_orm::DbErr),
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
/// wins. Every drain executor claims through this one function, so the
/// pin wait budget cannot drift between them.
/// All eight arguments are genuinely distinct inputs; there is no
/// natural bundle, so the arg-count lint is allowed here.
#[allow(clippy::too_many_arguments)]
pub async fn claim_next_assignable<C: ConnectionTrait>(
    db: &C,
    pod_id: Uuid,
    runner_id: Uuid,
    owner_id: Uuid,
    visibility: i32,
    facts: &DispatchFacts,
    provisioning: &str,
    instance_budget_secs: i64,
) -> Result<Option<sea_orm::prelude::QueryResult>, DrainError> {
    // Desktop-bundled runners never take a released pin.
    let is_desktop = drain_kernel::is_desktop_provisioning(provisioning);
    let mut releasable: Vec<Uuid> = Vec::new();
    let mut budget_secs = 0i64;
    if !is_desktop && visibility == pidash_auth::permissions::runner::VISIBILITY_PRIVATE {
        let takes_shared = if facts.is_engine {
            facts.pod_member && !facts.paused
        } else {
            facts.pod_member
        };
        if takes_shared {
            // Pod override via SeaORM query builder (no raw SQL).
            use sea_orm::sea_query::{Alias, Expr, Query};
            let pod_stmt = Query::select()
                .column((Alias::new("pod"), "pin_wait_budget_secs"))
                .from(Alias::new("pod"))
                .and_where(Expr::col((Alias::new("pod"), "id")).eq(pod_id))
                .to_owned();
            let pod_row = db.query_one(DbBackend::Postgres.build(&pod_stmt)).await?;
            let pod_override: Option<i32> = match pod_row {
                Some(row) => row.try_get("", "pin_wait_budget_secs").unwrap_or(None),
                None => None,
            };
            budget_secs = drain_kernel::effective_pin_wait_budget_secs(
                pod_override.map(i64::from),
                instance_budget_secs,
            );
            if budget_secs > 0 {
                let now = now_micros();
                let cutoff = now - chrono::Duration::seconds(budget_secs);
                let threshold = alive_threshold(now);
                if let Some(stmt) = drain_kernel::releasable_overbudget_pin_ids_stmt(
                    visibility, pod_id, runner_id, owner_id, cutoff, threshold, facts,
                ) {
                    let rows = db.query_all(DbBackend::Postgres.build(&stmt)).await?;
                    for row in rows {
                        let id: Uuid = row.try_get("", "id")?;
                        releasable.push(id);
                    }
                }
            }
        }
    }
    let next_stmt_opt = if releasable.is_empty() {
        drain_kernel::next_for_runner_stmt(visibility, pod_id, runner_id, owner_id, facts)
    } else {
        drain_kernel::next_for_runner_stmt_with_releasable(
            visibility,
            pod_id,
            runner_id,
            owner_id,
            facts,
            &releasable,
        )
    };
    let Some(next_stmt) = next_stmt_opt else {
        return Ok(None);
    };
    let run = db.query_one(DbBackend::Postgres.build(&next_stmt)).await?;
    let Some(run) = run else {
        return Ok(None);
    };
    let pinned_runner_id: Option<Uuid> = run.try_get("", "pinned_runner_id")?;
    if drain_kernel::is_overbudget_takeover(pinned_runner_id, runner_id) {
        let run_id: Uuid = run.try_get("", "id")?;
        let parent_run_id: Option<Uuid> = run.try_get("", "parent_run_id")?;
        let created_at: DateTime<Utc> = run.try_get("", "created_at")?;
        if let Some(parent_id) = parent_run_id {
            // Clear parent thread_id (Python: only when non-empty).
            use sea_orm::sea_query::Expr as SeaExpr;
            let _ = agent_run::Entity::update_many()
                .col_expr(agent_run::Column::ThreadId, SeaExpr::value(""))
                .filter(agent_run::Column::Id.eq(parent_id))
                .filter(agent_run::Column::ThreadId.ne(""))
                .exec(db)
                .await?;
        }
        // Clear the pin itself.
        agent_run::Entity::update_many()
            .col_expr(
                agent_run::Column::PinnedRunnerId,
                sea_orm::sea_query::Expr::value(Option::<Uuid>::None),
            )
            .filter(agent_run::Column::Id.eq(run_id))
            .exec(db)
            .await?;
        if let Some(previous) = pinned_runner_id {
            let waited_secs = (now_micros() - created_at).num_seconds();
            tracing::info!(
                "{}",
                drain_kernel::released_pin_log(
                    &run_id,
                    &previous,
                    &runner_id,
                    budget_secs,
                    waited_secs
                )
            );
        }
    }
    Ok(Some(run))
}

// ---------------------------------------------------------------------------
// Executors
// ---------------------------------------------------------------------------

/// §10.2 `last_dedicated`: true when the runner's most recent run by
/// `assigned_at` (NULLs excluded) has a pin with
/// `pin_kind = dedicated`. Derived, not stored — one indexed
/// lookup; a runner with no assigned runs counts as false.
async fn last_dedicated_for(
    db: &impl ConnectionTrait,
    runner_id: Uuid,
) -> Result<bool, DrainError> {
    let last = agent_run::Entity::find()
        .filter(agent_run::Column::RunnerId.eq(runner_id))
        .filter(agent_run::Column::AssignedAt.is_not_null())
        .order_by_desc(agent_run::Column::AssignedAt)
        .one(db)
        .await?;
    Ok(matches!(last, Some(run) if run.pinned_runner_id.is_some() && run.pin_kind == "dedicated"))
}

/// The runner's [`DispatchFacts`]: own-agent runners need no machine
/// read; engines read their machine's pause/team fields. A missing
/// machine answers unpaused/no-team (matching
/// `engine_team_runs_enabled` and `engine_paused`).
async fn dispatch_facts_of(
    db: &impl ConnectionTrait,
    runner_id: Uuid,
    provisioning: &str,
    pod_dispatch: &str,
    machine_id: Option<Uuid>,
) -> Result<DispatchFacts, DrainError> {
    let last_dedicated = last_dedicated_for(db, runner_id).await?;
    let is_engine = provisioning == "desktop_bundled";
    let pod_member = pod_dispatch == "pod_member";
    if !is_engine {
        return Ok(DispatchFacts {
            is_engine: false,
            pod_member,
            paused: false,
            team_runs: true,
            last_dedicated,
        });
    }
    let (paused, team_runs) = match machine_id {
        Some(id) => {
            let machine = dev_machine::Entity::find_by_id(id).one(db).await?;
            match machine {
                Some(machine) => (
                    machine.engine_paused_by_owner || !machine.engine_auto_pause_reason.is_empty(),
                    machine.engine_team_runs,
                ),
                None => (false, false),
            }
        }
        None => (false, false),
    };
    Ok(DispatchFacts {
        is_engine: true,
        pod_member,
        paused,
        team_runs,
        last_dedicated,
    })
}

async fn write_assignment(
    db: &impl ConnectionTrait,
    owner_id: Uuid,
    runner_id: Uuid,
    run_id: Uuid,
) -> Result<(), DrainError> {
    // A fresh `timezone.now()` per assignment (`matcher.py:285-289`).
    use sea_orm::sea_query::Expr;
    agent_run::Entity::update_many()
        .col_expr(agent_run::Column::OwnerId, Expr::value(owner_id))
        .col_expr(agent_run::Column::RunnerId, Expr::value(runner_id))
        .col_expr(agent_run::Column::Status, Expr::value("assigned"))
        .col_expr(agent_run::Column::AssignedAt, Expr::value(now_micros()))
        .filter(agent_run::Column::Id.eq(run_id))
        .exec(db)
        .await?;
    Ok(())
}

/// `release_runner_if_idle` (`matcher.py:306-328`, PDASHOSS01-231):
/// flip a `busy` runner with no busy-status run back to `online`.
/// Returns whether a row changed (the Python `bool(...)`). Runs on the
/// caller's connection, so session open keeps it inside its transaction.
pub async fn release_runner_if_idle<C: ConnectionTrait>(
    db: &C,
    runner_id: Uuid,
) -> Result<bool, sea_orm::DbErr> {
    let result = db
        .execute(DbBackend::Postgres.build(&drain_kernel::release_runner_if_idle_stmt(runner_id)))
        .await?;
    Ok(result.rows_affected() > 0)
}

/// `drain_for_runner_by_id` (`matcher.py:298-304`): runner lookup
/// (miss ⇒ `Ok(false)`, no transaction), else the single-assignment
/// drain. Answers whether a run was assigned.
pub async fn drain_for_runner_by_id(
    pool: &PgPool,
    redis: Option<&redis::Client>,
    runner: &RunnerSettings,
    runner_id: Uuid,
) -> Result<bool, DrainError> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = runner::Entity::find_by_id(runner_id).one(&db).await?;
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let txn = db.begin().await?;
    let threshold = alive_threshold(now_micros());
    let locked = txn
        .query_one(
            DbBackend::Postgres.build(&drain_kernel::drain_for_runner_lock_stmt(
                threshold, runner_id,
            )),
        )
        .await?;
    let Some(locked) = locked else {
        txn.commit().await?;
        return Ok(false);
    };
    let owner_id: Uuid = locked.try_get("", "owner_id")?;
    let pod_id: Uuid = locked.try_get("", "pod_id")?;
    let provisioning: String = locked.try_get("", "provisioning")?;
    let pod_dispatch: String = locked.try_get("", "pod_dispatch")?;
    let machine_id: Option<Uuid> = locked.try_get("", "dev_machine_id")?;
    let visibility: i16 = locked.try_get("", "visibility")?;
    let facts =
        dispatch_facts_of(&txn, runner_id, &provisioning, &pod_dispatch, machine_id).await?;
    // Claims through the budgeted helper (PDASHOSS01-272).
    let run_opt = claim_next_assignable(
        &txn,
        pod_id,
        runner_id,
        owner_id,
        i32::from(visibility),
        &facts,
        &provisioning,
        runner.pin_wait_budget_secs,
    )
    .await?;
    let Some(run) = run_opt else {
        txn.commit().await?;
        return Ok(false);
    };
    let run_id: Uuid = run.try_get("", "id")?;
    let work_item_id: Option<Uuid> = run.try_get("", "work_item_id")?;
    let prompt: String = run.try_get("", "prompt")?;
    let run_config: Value = run.try_get("", "run_config")?;
    let Value::Object(run_config) = run_config else {
        return Err(DrainError::Config(
            "drain_for_runner: run_config is not an object".to_owned(),
        ));
    };
    write_assignment(&txn, owner_id, runner_id, run_id).await?;
    let plan = drain_kernel::plan_assignment(&drain_kernel::AssignmentFacts {
        run_id,
        runner_id,
        owner_id,
        work_item_id,
        prompt,
        run_config,
    });
    txn.commit().await?;
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let pod = pod::Entity::find()
        .filter(pod::Column::Id.eq(pod_id))
        .filter(pod::Column::DeletedAt.is_null())
        .order_by(pod::Column::IsDefault, sea_orm::Order::Desc)
        .order_by(pod::Column::CreatedAt, sea_orm::Order::Asc)
        .one(&db)
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let txn = db.begin().await?;
    let threshold = alive_threshold(now_micros());
    let runners = txn
        .query_all(
            DbBackend::Postgres.build(&drain_kernel::drain_pod_idle_list_stmt(threshold, pod_id)),
        )
        .await?;
    let mut plans: Vec<drain_kernel::AssignmentPlan> = Vec::new();
    for runner_row in &runners {
        let runner_id: Uuid = runner_row.try_get("", "id")?;
        let owner_id: Uuid = runner_row.try_get("", "owner_id")?;
        let provisioning: String = runner_row.try_get("", "provisioning")?;
        let pod_dispatch: String = runner_row.try_get("", "pod_dispatch")?;
        let machine_id: Option<Uuid> = runner_row.try_get("", "dev_machine_id")?;
        let visibility: i16 = runner_row.try_get("", "visibility")?;
        let facts =
            dispatch_facts_of(&txn, runner_id, &provisioning, &pod_dispatch, machine_id).await?;
        // Budgeted claim per runner (PDASHOSS01-272).
        let run_opt = claim_next_assignable(
            &txn,
            pod_id,
            runner_id,
            owner_id,
            i32::from(visibility),
            &facts,
            &provisioning,
            runner.pin_wait_budget_secs,
        )
        .await?;
        let Some(run) = run_opt else {
            continue;
        };
        let run_id: Uuid = run.try_get("", "id")?;
        let work_item_id: Option<Uuid> = run.try_get("", "work_item_id")?;
        let prompt: String = run.try_get("", "prompt")?;
        let run_config: Value = run.try_get("", "run_config")?;
        let Value::Object(run_config) = run_config else {
            return Err(DrainError::Config(
                "drain_pod: run_config is not an object".to_owned(),
            ));
        };
        write_assignment(&txn, owner_id, runner_id, run_id).await?;
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
    txn.commit().await?;
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

    /// The executors run the kernel builders (never a fork): the
    /// lock and idle statements carry the row lock.
    #[test]
    fn executors_use_kernel_builders() {
        use sea_query::PostgresQueryBuilder;
        use uuid::Uuid;

        let threshold = chrono::Utc::now();
        let lock = drain_kernel::drain_for_runner_lock_stmt(threshold, Uuid::new_v4());
        assert!(lock.to_string(PostgresQueryBuilder).ends_with(
            "ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 1 FOR UPDATE SKIP LOCKED"
        ));
        let idle = drain_kernel::drain_pod_idle_list_stmt(threshold, Uuid::new_v4());
        assert!(idle
            .to_string(PostgresQueryBuilder)
            .contains("FOR UPDATE SKIP LOCKED"));
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
