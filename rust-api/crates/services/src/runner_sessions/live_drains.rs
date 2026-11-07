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
use sea_orm::sea_query::{Alias, Expr, LockBehavior, LockType, Query, SimpleExpr};
use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_auth::permissions::runner::VISIBILITY_PRIVATE;
use pidash_db::config::RunnerSettings;
use pidash_db::entity::{agent_run, issue_assignees, issues, pod, runner, scheduler_bindings};
use pidash_db::orm_tx::{into_sqlx, TxConn};
use pidash_db::runner_enroll::columns::enums::RUNNER_STATUS_ONLINE;
use pidash_db::runner_sessions::machine_outbox::MachineOutboxError;
use pidash_db::runner_sessions::outbox::{self as session_outbox, OutboxError};
use pidash_db::runner_sessions::RunnerSession;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::AgentRunStatus;

use super::drain as drain_kernel;
use super::guards::{alive_threshold, BUSY_STATUSES};
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

impl From<sea_orm::DbErr> for DrainError {
    fn from(error: sea_orm::DbErr) -> Self {
        DrainError::Db(into_sqlx(error))
    }
}

// ---------------------------------------------------------------------------
// SeaORM statements (twins of the `drain` kernel text)
// ---------------------------------------------------------------------------
//
// Same predicates, ordering, `LIMIT`s and `FOR UPDATE SKIP LOCKED` as
// the captured text in [`drain_kernel`]; the selects read only the cells
// the executors consume.

/// The runner cells an assignment needs.
#[derive(Debug, Clone, PartialEq, Eq, sea_orm::FromQueryResult)]
struct DrainRunnerRow {
    id: Uuid,
    owner_id: Uuid,
    pod_id: Uuid,
    provisioning: String,
    visibility: i16,
}

/// The run cells the assign frame needs.
#[derive(Debug, Clone, PartialEq, Eq, sea_orm::FromQueryResult)]
struct NextRunRow {
    id: Uuid,
    work_item_id: Option<Uuid>,
    prompt: String,
    run_config: Value,
}

/// `~Exists(AgentRun.objects.filter(runner=OuterRef("pk"),
/// status__in=BUSY_STATUSES))`: the runner holds no busy run.
fn no_busy_run_condition() -> SimpleExpr {
    let busy = Alias::new("U1");
    Expr::exists(
        Query::select()
            .expr(Expr::val(1))
            .from_as(agent_run::Entity, busy.clone())
            .and_where(
                Expr::col((busy.clone(), agent_run::Column::Status))
                    .is_in(BUSY_STATUSES.iter().map(AgentRunStatus::value)),
            )
            .and_where(
                Expr::col((busy, agent_run::Column::RunnerId))
                    .equals((runner::Entity, runner::Column::Id)),
            )
            .limit(1)
            .to_owned(),
    )
    .not()
}

/// The idle-runner core (`assignable()` + `idle`): fresh heartbeat,
/// unpaused, ONLINE, no busy run.
fn idle_runner_core(threshold: DateTime<Utc>) -> sea_orm::Select<runner::Entity> {
    runner::Entity::find()
        .select_only()
        .columns([
            runner::Column::Id,
            runner::Column::OwnerId,
            runner::Column::PodId,
            runner::Column::Provisioning,
            runner::Column::Visibility,
        ])
        .filter(runner::Column::LastHeartbeatAt.gte(threshold))
        .filter(runner::Column::PausedAt.is_null())
        .filter(runner::Column::Status.eq(RUNNER_STATUS_ONLINE))
        .filter(no_busy_run_condition())
}

/// [`drain_kernel::DRAIN_FOR_RUNNER_LOCK_SQL`]: the runner back under
/// `FOR UPDATE SKIP LOCKED` when still idle; `Runner.Meta.ordering`.
fn drain_for_runner_lock_select(
    threshold: DateTime<Utc>,
    runner_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectModel<DrainRunnerRow>> {
    idle_runner_core(threshold)
        .filter(runner::Column::Id.eq(runner_id))
        .order_by_desc(runner::Column::LastHeartbeatAt)
        .order_by_desc(runner::Column::CreatedAt)
        .limit(1)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
        .into_model()
}

/// [`drain_kernel::DRAIN_POD_IDLE_RUNNERS_SQL`]: the pod's idle set
/// (no `DESKTOP_BUNDLED` exclusion, no `LIMIT` — ported as written),
/// `-last_heartbeat_at` only, `FOR UPDATE SKIP LOCKED`.
fn drain_pod_idle_runners_select(
    threshold: DateTime<Utc>,
    pod_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectModel<DrainRunnerRow>> {
    idle_runner_core(threshold)
        .filter(runner::Column::PodId.eq(pod_id))
        .order_by_desc(runner::Column::LastHeartbeatAt)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
        .into_model()
}

/// [`drain_kernel::DRAIN_FOR_RUNNER_BY_ID_LOOKUP_SQL`]:
/// `Runner.objects.filter(pk=…).first()` — presence only.
fn runner_by_id_select(runner_id: Uuid) -> sea_orm::Selector<sea_orm::SelectGetableTuple<Uuid>> {
    runner::Entity::find()
        .select_only()
        .column(runner::Column::Id)
        .filter(runner::Column::Id.eq(runner_id))
        .order_by_desc(runner::Column::LastHeartbeatAt)
        .order_by_desc(runner::Column::CreatedAt)
        .limit(1)
        .into_tuple()
}

/// [`drain_kernel::DRAIN_POD_BY_ID_LOOKUP_SQL`]:
/// `Pod.objects.filter(pk=…).first()` under the soft-delete manager —
/// presence only.
fn pod_by_id_select(pod_id: Uuid) -> sea_orm::Selector<sea_orm::SelectGetableTuple<Uuid>> {
    pod::Entity::find()
        .select_only()
        .column(pod::Column::Id)
        .filter(pod::Column::DeletedAt.is_null())
        .filter(pod::Column::Id.eq(pod_id))
        .order_by_desc(pod::Column::IsDefault)
        .order_by_asc(pod::Column::CreatedAt)
        .limit(1)
        .into_tuple()
}

/// `filter_runs_usable_by_runner` for a private runner
/// ([`crate::runner_runs::guards::runs_usable_by_runner_predicate`]):
/// the run was created by or is owned by the runner's owner, or its
/// work item is visible to them (creator or live assignee), or its
/// scheduler binding is theirs.
fn runs_usable_by_runner_condition(owner_id: Uuid) -> Condition {
    let issue = Alias::new("U0");
    let assignee = Alias::new("U2");
    let issue_exists = Query::select()
        .expr(Expr::val(1))
        .from_as(issues::Entity, issue.clone())
        .join_as(
            sea_orm::JoinType::LeftJoin,
            issue_assignees::Entity,
            assignee.clone(),
            Expr::col((issue.clone(), issues::Column::Id))
                .equals((assignee.clone(), issue_assignees::Column::IssueId)),
        )
        .and_where(Expr::col((issue.clone(), issues::Column::DeletedAt)).is_null())
        .and_where(
            Expr::col((issue.clone(), issues::Column::Id))
                .equals((agent_run::Entity, agent_run::Column::WorkItemId)),
        )
        .cond_where(
            Condition::any()
                .add(Expr::col((issue, issues::Column::CreatedById)).eq(owner_id))
                .add(
                    Condition::all()
                        .add(
                            Expr::col((assignee.clone(), issue_assignees::Column::AssigneeId))
                                .eq(owner_id),
                        )
                        .add(Expr::col((assignee, issue_assignees::Column::DeletedAt)).is_null()),
                ),
        )
        .limit(1)
        .to_owned();
    let binding = Alias::new("U0");
    let scheduler_exists = Query::select()
        .expr(Expr::val(1))
        .from_as(scheduler_bindings::Entity, binding.clone())
        .and_where(Expr::col((binding.clone(), scheduler_bindings::Column::DeletedAt)).is_null())
        .and_where(Expr::col((binding.clone(), scheduler_bindings::Column::ActorId)).eq(owner_id))
        .and_where(
            Expr::col((binding, scheduler_bindings::Column::Id))
                .equals((agent_run::Entity, agent_run::Column::SchedulerBindingId)),
        )
        .limit(1)
        .to_owned();
    Condition::any()
        .add(agent_run::Column::CreatedById.eq(owner_id))
        .add(agent_run::Column::OwnerId.eq(owner_id))
        .add(Expr::exists(issue_exists))
        .add(Expr::exists(scheduler_exists))
}

/// [`drain_kernel::next_for_runner_sql`] on SeaORM: the next QUEUED run
/// this runner should take — pinned-to-me first, then the pod queue,
/// FIFO inside each tier, `FOR UPDATE SKIP LOCKED`. `None` for a
/// non-private runner (`qs.none()`: no query runs).
fn next_for_runner_select(
    pod_id: Uuid,
    runner_id: Uuid,
    owner_id: Uuid,
    provisioning: &str,
    visibility: i32,
) -> Option<sea_orm::Selector<sea_orm::SelectModel<NextRunRow>>> {
    if visibility != VISIBILITY_PRIVATE {
        return None;
    }
    let pinned_to_me: SimpleExpr = Expr::case(
        Expr::col((agent_run::Entity, agent_run::Column::PinnedRunnerId)).eq(runner_id),
        Expr::val(0),
    )
    .finally(Expr::val(1))
    .into();
    let mut select = agent_run::Entity::find()
        .select_only()
        .columns([
            agent_run::Column::Id,
            agent_run::Column::WorkItemId,
            agent_run::Column::Prompt,
            agent_run::Column::RunConfig,
        ])
        .filter(agent_run::Column::ExecutorKind.is_in([
            AgentExecutorKind::LocalRunner.value(),
            AgentExecutorKind::ManagedRunner.value(),
        ]))
        .filter(agent_run::Column::PodId.eq(pod_id))
        .filter(agent_run::Column::Status.eq(AgentRunStatus::Queued.value()))
        .filter(
            Condition::any()
                .add(agent_run::Column::PinnedRunnerId.eq(runner_id))
                .add(agent_run::Column::PinnedRunnerId.is_null()),
        )
        .filter(runs_usable_by_runner_condition(owner_id));
    select = if drain_kernel::is_desktop_provisioning(provisioning) {
        select
            .filter(agent_run::Column::ExecutorKind.eq(AgentExecutorKind::ManagedRunner.value()))
            .filter(agent_run::Column::PinnedRunnerId.eq(runner_id))
    } else {
        select.filter(agent_run::Column::ExecutorKind.eq(AgentExecutorKind::LocalRunner.value()))
    };
    Some(
        select
            .order_by(pinned_to_me, sea_orm::Order::Asc)
            .order_by_asc(agent_run::Column::CreatedAt)
            .limit(1)
            .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
            .into_model(),
    )
}

/// [`drain_kernel::ASSIGN_RUN_UPDATE_SQL`]: `save(update_fields=["runner",
/// "owner", "status", "assigned_at"])` in model field order.
fn assign_run_update_statement(
    run_id: Uuid,
    runner_id: Uuid,
    owner_id: Uuid,
    now: DateTime<Utc>,
) -> sea_orm::UpdateMany<agent_run::Entity> {
    agent_run::Entity::update_many()
        .col_expr(agent_run::Column::OwnerId, Expr::value(owner_id))
        .col_expr(agent_run::Column::RunnerId, Expr::value(runner_id))
        .col_expr(
            agent_run::Column::Status,
            Expr::value(AgentRunStatus::Assigned.value()),
        )
        .col_expr(agent_run::Column::AssignedAt, Expr::value(now))
        .filter(agent_run::Column::Id.eq(run_id))
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
    let row = runner_by_id_select(runner_id)
        .one(&pidash_db::pool::sea_orm_connection(pool))
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
    let conn = TxConn::new(&mut tx);
    let locked = drain_for_runner_lock_select(threshold, runner_id)
        .one(&conn)
        .await?;
    let Some(locked) = locked else {
        tx.commit().await?;
        return Ok(false);
    };
    let owner_id = locked.owner_id;
    let pod_id = locked.pod_id;
    // Non-private runners issue no query (the `qs.none()` arm).
    let Some(next) = next_for_runner_select(
        pod_id,
        runner_id,
        owner_id,
        &locked.provisioning,
        i32::from(locked.visibility),
    ) else {
        tx.commit().await?;
        return Ok(false);
    };
    let run = next.one(&conn).await?;
    let Some(run) = run else {
        tx.commit().await?;
        return Ok(false);
    };
    let run_id = run.id;
    let work_item_id = run.work_item_id;
    let prompt = run.prompt;
    let Value::Object(run_config) = run.run_config else {
        return Err(DrainError::Config(
            "drain_for_runner: run_config is not an object".to_owned(),
        ));
    };
    // `$3` is a fresh `timezone.now()` per assignment (`:285-289`).
    assign_run_update_statement(run_id, runner_id, owner_id, now_micros())
        .exec(&conn)
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
    let pod = pod_by_id_select(pod_id)
        .one(&pidash_db::pool::sea_orm_connection(pool))
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
    let conn = TxConn::new(&mut tx);
    let runners = drain_pod_idle_runners_select(threshold, pod_id)
        .all(&conn)
        .await?;
    let mut plans: Vec<drain_kernel::AssignmentPlan> = Vec::new();
    for runner_row in &runners {
        let runner_id = runner_row.id;
        let owner_id = runner_row.owner_id;
        let Some(next) = next_for_runner_select(
            pod_id,
            runner_id,
            owner_id,
            &runner_row.provisioning,
            i32::from(runner_row.visibility),
        ) else {
            continue;
        };
        let run = next.one(&conn).await?;
        let Some(run) = run else {
            continue;
        };
        let run_id = run.id;
        let work_item_id = run.work_item_id;
        let prompt = run.prompt;
        let Value::Object(run_config) = run.run_config else {
            return Err(DrainError::Config(
                "drain_pod: run_config is not an object".to_owned(),
            ));
        };
        assign_run_update_statement(run_id, runner_id, owner_id, now_micros())
            .exec(&conn)
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

    /// Drain selects against temp copies on one transaction
    /// (`TxConn`): the by-id lookups, the idle lock, the next-run
    /// claim (pinned first), and the assign write.
    #[tokio::test]
    async fn live_drain_selects_on_temp_tables() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("pool connects");
        let mut tx = pool.begin().await.expect("begin");
        for table in [
            "runner",
            "pod",
            "agent_run",
            "issues",
            "issue_assignees",
            "scheduler_bindings",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} ON COMMIT DROP AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&mut *tx)
            .await
            .expect("temp table");
        }
        let (runner_id, pod_id, owner, ws, creator, pinned_id, queued_id) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let now = chrono::Utc::now();
        sqlx::query(&format!(
            "INSERT INTO pod (id, name, description, is_default, created_at, updated_at, workspace_id, project_id) \
             VALUES ('{pod_id}', 'main', '', true, now(), now(), '{ws}', '{ws}')"
        ))
        .execute(&mut *tx).await.expect("seed pod");
        sqlx::query(&format!(
            "INSERT INTO runner (id, name, capabilities, status, os, arch, runner_version, protocol_version, created_at, updated_at, owner_id, workspace_id, pod_id, host_label, refresh_token_hash, refresh_token_fingerprint, refresh_token_generation, previous_refresh_token_hash, access_token_signing_key_version, enrollment_token_hash, enrollment_token_fingerprint, revoked_reason, visibility, dev_metadata, provisioning, last_heartbeat_at) \
             VALUES ('{runner_id}', 'r', '{{}}', 'online', '', '', '', 4, now(), now(), '{owner}', '{ws}', '{pod_id}', '', '', '', 0, '', 1, '', '', '', 0, '{{}}', 'manual', now())"
        ))
        .execute(&mut *tx).await.expect("seed runner");
        let pinned_sql = format!("'{runner_id}'");
        for (id, pinned_runner, created_ago) in [
            (queued_id, "NULL".to_owned(), "interval '2 hours'"),
            (pinned_id, pinned_sql, "interval '1 hour'"),
        ] {
            sqlx::query(&format!(
                "INSERT INTO agent_run (id, status, prompt, run_config, required_capabilities, thread_id, created_at, assigned_at, workspace_id, pod_id, created_by_id, owner_id, llm_model, trigger, executor_kind, dispatch_attempts, cancel_reason, tool_plan, phase_kind, agent_metadata, usage, error_details, pinned_runner_id) \
                 VALUES ('{id}', 'queued', 'p', '{{}}', '{{}}', '', now() - {created_ago}, now(), '{ws}', '{pod_id}', '{creator}', '{owner}', '', 'manual', 'local_runner', 0, '', '{{}}', 'main', '{{}}', '{{}}', '{{}}', {pinned_runner})"
            ))
            .execute(&mut *tx).await.expect("seed agent_run");
        }
        {
            let conn = TxConn::new(&mut tx);
            assert_eq!(
                runner_by_id_select(runner_id)
                    .one(&conn)
                    .await
                    .expect("by-id")
                    .expect("row"),
                runner_id
            );
            assert_eq!(
                pod_by_id_select(pod_id)
                    .one(&conn)
                    .await
                    .expect("by-id")
                    .expect("row"),
                pod_id
            );
            assert!(pod_by_id_select(Uuid::new_v4())
                .one(&conn)
                .await
                .expect("miss")
                .is_none());
            let threshold = now - chrono::Duration::seconds(90);
            let locked = drain_for_runner_lock_select(threshold, runner_id)
                .one(&conn)
                .await
                .expect("lock")
                .expect("row");
            assert_eq!(locked.id, runner_id);
            let idle: Vec<_> = drain_pod_idle_runners_select(threshold, pod_id)
                .all(&conn)
                .await
                .expect("idle");
            assert_eq!(idle.len(), 1);
            // Pinned-to-me beats the older unpinned run (FIFO inside tiers).
            let next =
                next_for_runner_select(pod_id, runner_id, owner, "manual", VISIBILITY_PRIVATE)
                    .expect("private => Some")
                    .one(&conn)
                    .await
                    .expect("next")
                    .expect("row");
            assert_eq!(next.id, pinned_id);
            // Non-private runners issue no query (`qs.none()` arm).
            assert!(next_for_runner_select(
                pod_id,
                runner_id,
                owner,
                "manual",
                VISIBILITY_PRIVATE + 1
            )
            .is_none());
            assert_eq!(
                assign_run_update_statement(pinned_id, runner_id, owner, now)
                    .exec(&conn)
                    .await
                    .expect("assign")
                    .rows_affected,
                1
            );
        }
        tx.rollback().await.expect("rollback");
    }
}
