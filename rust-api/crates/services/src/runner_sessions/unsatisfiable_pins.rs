#![forbid(unsafe_code)]

//! Unsatisfiable-pin recovery planners (PDASHOSS01-233).
//!
//! Port of `matcher.reconcile_unsatisfiable_pins`
//! (`apps/api/pi_dash/runner/services/matcher.py`, PR #541). A QUEUED
//! run's `pinned_runner` is served as soon as that runner frees up —
//! but a runner that wedged BUSY, went offline or died silently never
//! takes it, and the run queues forever. Two remedies, mildest first:
//!
//! 1. Pinned runner BUSY with a fresh heartbeat but no busy-status run:
//!    release it ([`release_runner_if_idle_stmt`]) and keep the pin, so
//!    the runner serves its own pinned run on the next drain. A dead
//!    wedged runner (stale heartbeat) is left to the BUSY-but-dead
//!    sweep, which flips it OFFLINE instead of resurrecting it.
//! 2. Pin held past `RUNNER_PIN_AUTO_RELEASE_SECS` (from `created_at`)
//!    while the pinned runner is neither assignable (ONLINE + fresh
//!    heartbeat + not operator-paused) nor legitimately working (BUSY +
//!    fresh heartbeat + a busy-status run): clear the pin and the
//!    parent's `thread_id` (the manual release-pin endpoint's
//!    resume-hint drop). `local_runner` runs with a locality pin only:
//!    a dedicated pin is owned by the issue and never released to the
//!    pod (§12.1), like the `offline_recovery` release sweeps.
//!
//! Pure planners only (SeaORM statement builders with the same
//! predicates, order and locks as the Django 4.2.30 queries); the
//! executing driver lives in
//! `pidash_jobs::runner_runs::unsatisfiable_pins`.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * The wedged list carries the `DISTINCT`-pair quirk: `created_at`
//!   rides along so the default `ORDER BY` survives `DISTINCT`, which
//!   can repeat a `(runner, pod)` pair. The driver releases each row
//!   in order; a repeat is a no-op `UPDATE` (zero rows), like Django.
//! * The stale-pin lock is a joined `FOR UPDATE SKIP LOCKED` with no
//!   `OF`, so it locks the pinned runner row too: a runner a poll is
//!   holding is skipped this pass and retried on the next.
//! * The "assignable" carve-out is the three scalar clauses the source
//!   spells out (status, heartbeat, `paused_at`), not the whole of
//!   `RunnerQuerySet.assignable()`: the engine-pause exclusion is not
//!   part of it on either side.
//!
//! [`release_runner_if_idle_stmt`]: super::drain::release_runner_if_idle_stmt

use chrono::{DateTime, Utc};
use pidash_db::entity::{agent_run as run, runner};
use pidash_types::agent_runs::{AgentExecutorKind, MACHINE_EXECUTORS};
use pidash_types::runner_runs::AgentRunStatus;
use sea_orm::sea_query::{Alias, Condition, Expr, LockBehavior, LockType, Query, SimpleExpr};
use sea_orm::{ColumnTrait, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect};
use uuid::Uuid;

use super::guards::BUSY_STATUSES;

/// `RUNNER_PIN_AUTO_RELEASE_SECS` default (`settings/common.py`).
pub const DEFAULT_PIN_AUTO_RELEASE_SECS: i64 = 600;

/// `RunnerStatus.ONLINE` / `RunnerStatus.BUSY` column values.
const RUNNER_ONLINE: &str = "online";
const RUNNER_BUSY: &str = "busy";

/// One [`wedged_pinned_runners_select`] row: `(pinned_runner_id, pod_id,
/// created_at)` — Django's `DISTINCT` triple.
pub type WedgedPinRow = (Uuid, Uuid, DateTime<Utc>);

/// One [`stale_pins_lock_select`] row: `(id, pinned_runner_id, pod_id)`.
pub type StalePinRow = (Uuid, Uuid, Uuid);

/// `agent_run.pinned_runner_id -> runner.id` (trimmed from the entity).
fn pinned_runner_rel() -> sea_orm::RelationDef {
    run::Entity::belongs_to(runner::Entity)
        .from(run::Column::PinnedRunnerId)
        .to(runner::Column::Id)
        .into()
}

/// Wedged-busy pinned runners (remedy 1): QUEUED machine-executor runs
/// whose pinned runner is `busy` and heartbeat-fresh (`alive` is
/// `now - HEARTBEAT_GRACE`), `ORDER BY created_at DESC`. The inner join
/// makes the pin non-null.
pub fn wedged_pinned_runners_select(
    alive: DateTime<Utc>,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<WedgedPinRow>> {
    run::Entity::find()
        .select_only()
        .distinct()
        .columns([
            run::Column::PinnedRunnerId,
            run::Column::PodId,
            run::Column::CreatedAt,
        ])
        .join(JoinType::InnerJoin, pinned_runner_rel())
        .filter(run::Column::ExecutorKind.is_in(MACHINE_EXECUTORS.iter().map(|kind| kind.value())))
        .filter(runner::Column::LastHeartbeatAt.gte(alive))
        .filter(runner::Column::Status.eq(RUNNER_BUSY))
        .filter(run::Column::Status.eq(AgentRunStatus::Queued.value()))
        .order_by_desc(run::Column::CreatedAt)
        .into_tuple()
}

/// `pinned_runner__agent_runs__status__in=BUSY_STATUSES` inside the
/// "legitimately working" exclude: `EXISTS` a serving run on the pinned
/// runner (alias `U2`, values in [`BUSY_STATUSES`] order).
fn pinned_runner_serving() -> SimpleExpr {
    let serving = Alias::new("U2");
    let sub = Query::select()
        .expr_as(Expr::val(1), Alias::new("a"))
        .from_as(run::Entity, serving.clone())
        .and_where(
            Expr::col((serving.clone(), run::Column::Status))
                .is_in(BUSY_STATUSES.iter().map(AgentRunStatus::value)),
        )
        .and_where(
            Expr::col((serving, run::Column::RunnerId))
                .equals((run::Entity, run::Column::PinnedRunnerId)),
        )
        .limit(1)
        .to_owned();
    Expr::exists(sub)
}

/// Stale pins past the bound (remedy 2), under `FOR UPDATE SKIP
/// LOCKED`. `cutoff` is `now - RUNNER_PIN_AUTO_RELEASE_SECS` (against
/// `created_at`); `alive` is the heartbeat threshold of both carve-outs.
/// The `IS NOT NULL` inside each `NOT` keeps a never-heartbeated runner
/// out of the carve-outs, so its pins are released.
pub fn stale_pins_lock_select(
    cutoff: DateTime<Utc>,
    alive: DateTime<Utc>,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<StalePinRow>> {
    // Assignable: the pin will be served as soon as the runner picks
    // work up. A paused runner is not assignable.
    let assignable = Condition::all()
        .add(runner::Column::LastHeartbeatAt.gte(alive))
        .add(runner::Column::LastHeartbeatAt.is_not_null())
        .add(runner::Column::PausedAt.is_null())
        .add(runner::Column::Status.eq(RUNNER_ONLINE));
    // Legitimately working: an alive BUSY runner still serving a
    // busy-status run frees itself on finalization.
    let working = Condition::all()
        .add(pinned_runner_serving())
        .add(runner::Column::LastHeartbeatAt.gte(alive))
        .add(runner::Column::LastHeartbeatAt.is_not_null())
        .add(runner::Column::Status.eq(RUNNER_BUSY));
    run::Entity::find()
        .select_only()
        .columns([
            run::Column::Id,
            run::Column::PinnedRunnerId,
            run::Column::PodId,
        ])
        .join(JoinType::InnerJoin, pinned_runner_rel())
        .filter(run::Column::CreatedAt.lt(cutoff))
        .filter(run::Column::ExecutorKind.eq(AgentExecutorKind::LocalRunner.value()))
        .filter(run::Column::PinnedRunnerId.is_not_null())
        .filter(run::Column::Status.eq(AgentRunStatus::Queued.value()))
        .filter(run::Column::PinKind.ne("dedicated"))
        .filter(assignable.not())
        .filter(working.not())
        .order_by_desc(run::Column::CreatedAt)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
        .into_tuple()
}

/// Bulk pin clear: `AgentRun.objects.filter(id__in=stale_ids)
/// .update(pinned_runner=None)`. Call only with a non-empty list (the
/// source guards with `if stale:`).
pub fn clear_pins_statement(run_ids: &[Uuid]) -> sea_orm::UpdateMany<run::Entity> {
    debug_assert!(!run_ids.is_empty(), "pin clear needs stale run ids");
    run::Entity::update_many()
        .col_expr(
            run::Column::PinnedRunnerId,
            Expr::value(Option::<Uuid>::None),
        )
        .filter(run::Column::Id.is_in(run_ids.iter().copied()))
}

/// Parent resume-hint drop: `AgentRun.objects.filter(id__in=
/// AgentRun.objects.filter(id__in=stale_ids).values("parent_run_id"))
/// .exclude(thread_id="").update(thread_id="")`. Same non-empty
/// precondition as [`clear_pins_statement`].
pub fn clear_parent_threads_statement(run_ids: &[Uuid]) -> sea_orm::UpdateMany<run::Entity> {
    debug_assert!(!run_ids.is_empty(), "parent clear needs stale run ids");
    let child = Alias::new("U0");
    let parents = Query::select()
        .column((child.clone(), run::Column::ParentRunId))
        .from_as(run::Entity, child.clone())
        .and_where(Expr::col((child, run::Column::Id)).is_in(run_ids.iter().copied()))
        .to_owned();
    run::Entity::update_many()
        .col_expr(run::Column::ThreadId, Expr::value(String::new()))
        .filter(run::Column::Id.in_subquery(parents))
        .filter(run::Column::ThreadId.ne(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use sea_orm::{DbBackend, QueryTrait};

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap()
    }

    fn bound() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 2, 2, 54, 5).unwrap()
    }

    fn pg_selector<S: sea_orm::SelectorTrait>(selector: sea_orm::Selector<S>) -> String {
        selector.into_statement(DbBackend::Postgres).to_string()
    }

    fn pg_update(update: sea_orm::UpdateMany<run::Entity>) -> String {
        update.build(DbBackend::Postgres).to_string()
    }

    const AT: &str = "'2026-01-02 03:04:05.000000 +00:00'";
    const BOUND: &str = "'2026-01-02 02:54:05.000000 +00:00'";
    const BUSY_LIST: &str = "'assigned', 'waiting_for_worktree', 'running', 'cancel_requested', \
                             'awaiting_approval', 'awaiting_reauth'";

    /// Same projection, join, predicates and order as Django's wedged
    /// list (PR #541 capture).
    #[test]
    fn wedged_select_matches_django() {
        assert_eq!(
            pg_selector(wedged_pinned_runners_select(at())),
            format!(
                "SELECT DISTINCT \"agent_run\".\"pinned_runner_id\", \"agent_run\".\"pod_id\", \
                 \"agent_run\".\"created_at\" FROM \"agent_run\" INNER JOIN \"runner\" ON \
                 \"agent_run\".\"pinned_runner_id\" = \"runner\".\"id\" WHERE \
                 \"agent_run\".\"executor_kind\" IN ('local_runner', 'managed_runner') AND \
                 \"runner\".\"last_heartbeat_at\" >= {AT} AND \"runner\".\"status\" = 'busy' AND \
                 \"agent_run\".\"status\" = 'queued' ORDER BY \"agent_run\".\"created_at\" DESC"
            )
        );
    }

    /// Django's stale-pin lock (PR #541 capture) plus the dedicated-pin
    /// exclusion: both carve-outs keep the `IS NOT NULL` guard, the
    /// assignable one carries `paused_at IS NULL`, and the lock has no
    /// `OF`.
    #[test]
    fn stale_lock_matches_django() {
        assert_eq!(
            pg_selector(stale_pins_lock_select(bound(), at())),
            format!(
                "SELECT \"agent_run\".\"id\", \"agent_run\".\"pinned_runner_id\", \
                 \"agent_run\".\"pod_id\" FROM \"agent_run\" INNER JOIN \"runner\" ON \
                 \"agent_run\".\"pinned_runner_id\" = \"runner\".\"id\" WHERE \
                 \"agent_run\".\"created_at\" < {BOUND} AND \
                 \"agent_run\".\"executor_kind\" = 'local_runner' AND \
                 \"agent_run\".\"pinned_runner_id\" IS NOT NULL AND \
                 \"agent_run\".\"status\" = 'queued' AND \
                 \"agent_run\".\"pin_kind\" <> 'dedicated' AND \
                 (NOT (\"runner\".\"last_heartbeat_at\" >= {AT} AND \
                 \"runner\".\"last_heartbeat_at\" IS NOT NULL AND \
                 \"runner\".\"paused_at\" IS NULL AND \"runner\".\"status\" = 'online')) AND \
                 (NOT (EXISTS(SELECT 1 AS \"a\" FROM \"agent_run\" AS \"U2\" WHERE \
                 \"U2\".\"status\" IN ({BUSY_LIST}) AND \
                 \"U2\".\"runner_id\" = \"agent_run\".\"pinned_runner_id\" LIMIT 1) AND \
                 \"runner\".\"last_heartbeat_at\" >= {AT} AND \
                 \"runner\".\"last_heartbeat_at\" IS NOT NULL AND \
                 \"runner\".\"status\" = 'busy')) \
                 ORDER BY \"agent_run\".\"created_at\" DESC FOR UPDATE SKIP LOCKED"
            )
        );
    }

    #[test]
    fn bulk_writes_match_django() {
        let one = Uuid::nil();
        let two = Uuid::from_u128(2);
        assert_eq!(
            pg_update(clear_pins_statement(&[one])),
            "UPDATE \"agent_run\" SET \"pinned_runner_id\" = NULL WHERE \"agent_run\".\"id\" IN \
             ('00000000-0000-0000-0000-000000000000')"
        );
        assert_eq!(
            pg_update(clear_parent_threads_statement(&[one, two])),
            "UPDATE \"agent_run\" SET \"thread_id\" = '' WHERE \"agent_run\".\"id\" IN \
             (SELECT \"U0\".\"parent_run_id\" FROM \"agent_run\" AS \"U0\" WHERE \"U0\".\"id\" IN \
             ('00000000-0000-0000-0000-000000000000', '00000000-0000-0000-0000-000000000002')) \
             AND \"agent_run\".\"thread_id\" <> ''"
        );
    }

    #[test]
    fn default_bound_matches_django() {
        assert_eq!(DEFAULT_PIN_AUTO_RELEASE_SECS, 600);
    }
}
