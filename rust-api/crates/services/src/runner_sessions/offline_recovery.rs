#![forbid(unsafe_code)]

//! Offline-runner recovery sweeps (PIDASHCONV-1063, stage 8 drift).
//!
//! Port of the `70d684af` ("Recover agent runs stranded on offline
//! runners", PR #1191) task bodies in `apps/api/pi_dash/runner/tasks.py`
//! plus the shared `_finish_unclaimed_runs` half of
//! `runner/services/session_service.py` that the fail sweep calls with
//! `stamp_cancelled=True`.
//!
//! * `release_pins_on_offline_runners`: QUEUED `local_runner` runs
//!   pinned to a runner silent past `RUNNER_OFFLINE_PIN_RELEASE_SECS`
//!   (default 300) — or operator-paused (`09738d222`, which keeps
//!   heartbeating but takes no work) — and created before the cutoff
//!   go back to the pod queue (pin cleared, parent `thread_id`
//!   cleared), 500 per pass, then each affected pod drains.
//! * `release_pins_for_paused_runner` (`09738d222`, called from the
//!   pause endpoint's commit hook): every QUEUED `local_runner` run
//!   pinned to the just-paused runner goes back to the pod queue
//!   immediately, then each affected pod drains.
//! * `fail_runs_on_offline_runners`: BUSY `local_runner` runs on a
//!   runner silent past `RUNNER_OFFLINE_RUN_FAIL_SECS` (default 3600)
//!   are terminated through `_finish_unclaimed_runs` with error code
//!   [`RUNNER_OFFLINE_ERROR_CODE`](super::session_service::RUNNER_OFFLINE_ERROR_CODE)
//!   and CANCELLED rows stamped.
//!
//! Either sweep is disabled when its setting is `<= 0`.
//!
//! This module is pure planners only: SeaORM statement builders for the
//! sweep drivers in `pidash_jobs::runner_runs::offline_recovery`, plus
//! the SQL-text twins the paused-runner release in `api::runner_enroll`
//! still binds (pinned against Django 4.2.30 output, see
//! `.probe1063-sql.py` in the issue workspace).
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * The fail sweep's runner list carries the `DISTINCT`-pair quirk:
//!   `created_at`/`last_heartbeat_at` ride along so the `ORDER BY`
//!   survives `DISTINCT`, which can repeat a runner id; the driver
//!   dedupes exactly like the source's `set` (first-seen order).
//! * The release lock is `FOR UPDATE OF "agent_run"`: the pinned-runner
//!   join must not lock the runner row.
//! * The stamped cancel barrier binds `error_code` after `ended_at`
//!   (the `.update()` call order); the unstamped barrier is the
//!   existing [`cancel_barrier_update_sql`](super::session_service::cancel_barrier_update_sql).

use chrono::{DateTime, Utc};
use pidash_db::entity::{agent_run as run, dev_machine, runner};
use pidash_db::runner_runs::agent_run;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::AgentRunStatus;
use sea_orm::sea_query::extension::postgres::PgBinOper;
use sea_orm::sea_query::{Alias, Condition, Expr, LockType, Query};
use sea_orm::{
    ColumnTrait, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect, QueryTrait,
    RelationTrait,
};
use uuid::Uuid;

use super::guards::BUSY_STATUSES;

/// `"agent_run"."a", "agent_run"."b", …` for a column list.
fn qualified_columns(table: &str, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|column| format!("\"{table}\".\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `T3."a", T3."b", …`: Django renders a same-table self-join alias
/// bare (folded to lowercase by Postgres), never double-quoted —
/// `"T3"` would be a different, undeclared alias.
fn aliased_columns(alias: &str, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|column| format!("{alias}.\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Release-pins sweep (`release_pins_on_offline_runners`)
// ---------------------------------------------------------------------------

/// Stuck-pin candidate ids: QUEUED `local_runner` runs with a pin,
/// created before the cutoff, whose pinned runner is silent past the
/// cutoff, never heartbeated, or operator-paused (`_pinned_runner_gone_q`,
/// `09738d222`). Binds: `$1` cutoff (`created_at`),
/// `$2` `local_runner`, `$3` `queued`, `$4` cutoff (heartbeat).
pub fn stuck_queued_run_ids_select(
    cutoff: DateTime<Utc>,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<Uuid>> {
    run::Entity::find()
        .select_only()
        .column(run::Column::Id)
        .join(JoinType::InnerJoin, pinned_runner_rel())
        .filter(run::Column::CreatedAt.lt(cutoff))
        .filter(run::Column::ExecutorKind.eq(AgentExecutorKind::LocalRunner.value()))
        .filter(run::Column::PinnedRunnerId.is_not_null())
        .filter(run::Column::Status.eq(AgentRunStatus::Queued.value()))
        .filter(runner_gone(cutoff))
        .order_by_desc(run::Column::CreatedAt)
        .limit(500)
        .into_tuple::<Uuid>()
}

/// Per-run re-check lock: the candidate back under `FOR UPDATE OF
/// "agent_run"` with the gone predicate re-applied (the runner may
/// have come back or been resumed), plus the parent row via
/// `select_related`. Binds: `$1` run id, `$2` `queued`, `$3` cutoff.
pub fn release_pin_lock_select(
    run_id: Uuid,
    cutoff: DateTime<Utc>,
) -> sea_orm::Selector<sea_orm::SelectModel<ReleasePinRow>> {
    let mut select = run::Entity::find()
        .select_only()
        .columns([run::Column::PodId, run::Column::ParentRunId])
        .expr_as(
            Expr::col((Alias::new(PARENT_ALIAS), run::Column::ThreadId)),
            "parent_thread_id",
        )
        .join(JoinType::InnerJoin, pinned_runner_rel())
        .join_as(
            JoinType::LeftJoin,
            run::Entity::belongs_to(run::Entity)
                .from(run::Column::ParentRunId)
                .to(run::Column::Id)
                .into(),
            Alias::new(PARENT_ALIAS),
        )
        .filter(run::Column::Id.eq(run_id))
        .filter(run::Column::PinnedRunnerId.is_not_null())
        .filter(run::Column::Status.eq(AgentRunStatus::Queued.value()))
        .filter(runner_gone(cutoff))
        .order_by_desc(run::Column::CreatedAt)
        .limit(1);
    QueryTrait::query(&mut select).lock_with_tables(LockType::Update, [run::Entity]);
    select.into_model::<ReleasePinRow>()
}

/// The [`release_pin_lock_select`] row: the run's pod and parent, and the
/// parent's `thread_id` (`None` without a parent: LEFT JOIN).
#[derive(Debug, Clone, PartialEq, Eq, sea_orm::FromQueryResult)]
pub struct ReleasePinRow {
    pub pod_id: Uuid,
    pub parent_run_id: Option<Uuid>,
    pub parent_thread_id: Option<String>,
}

/// Self-join alias for the parent run (Django's `T3`).
const PARENT_ALIAS: &str = "T3";

/// `agent_run.pinned_runner_id -> runner.id` (trimmed from the entity).
fn pinned_runner_rel() -> sea_orm::RelationDef {
    run::Entity::belongs_to(runner::Entity)
        .from(run::Column::PinnedRunnerId)
        .to(runner::Column::Id)
        .into()
}

/// `_pinned_runner_gone_q`: silent past the cutoff, never heartbeated, or
/// operator-paused.
fn runner_gone(cutoff: DateTime<Utc>) -> Condition {
    // `engine_paused_q` (`models.py`): `IN (subquery)`, never a join —
    // callers add `FOR UPDATE`. The subquery's guard column is
    // `engine_paused_by_owner OR engine_auto_pause_reason > ''`
    // (Postgres `>` matches Django's `OR non-empty` for this text type).
    let paused_machines = Query::select()
        .column(dev_machine::Column::Id)
        .from(dev_machine::Entity)
        .cond_where(
            Condition::any()
                .add(dev_machine::Column::EnginePausedByOwner.eq(true))
                .add(dev_machine::Column::EngineAutoPauseReason.gt("")),
        )
        .to_owned();
    Condition::any()
        .add(runner::Column::LastHeartbeatAt.lt(cutoff))
        .add(runner::Column::LastHeartbeatAt.is_null())
        .add(runner::Column::PausedAt.is_not_null())
        .add(
            Condition::all()
                .add(runner::Column::DevMachineId.in_subquery(paused_machines))
                .add(runner::Column::Provisioning.eq("desktop_bundled")),
        )
        // `pod_dispatch='pinned_only'` engine pins also release
        // (`_pinned_runner_gone_q`, #2903).
        .add(
            Condition::all()
                .add(runner::Column::PodDispatch.eq("pinned_only"))
                .add(runner::Column::Provisioning.eq("desktop_bundled")),
        )
}

/// Paused-runner candidate ids (`release_pins_for_paused_runner`,
/// `09738d222`): every QUEUED `local_runner` run pinned to the
/// just-paused runner, whatever its age. No runner join (the pin
/// column carries the id) and no `LIMIT`: the pause owns the set.
/// Binds: `$1` `local_runner`, `$2` `queued`, `$3` runner id.
pub fn paused_runner_pin_ids_select() -> String {
    "SELECT \"agent_run\".\"id\" FROM \"agent_run\" WHERE \
     (\"agent_run\".\"executor_kind\" = $1 AND \"agent_run\".\"pinned_runner_id\" IS NOT NULL AND \
     \"agent_run\".\"status\" = $2 AND \"agent_run\".\"pinned_runner_id\" = $3) \
     ORDER BY \"agent_run\".\"created_at\" DESC"
        .to_owned()
}

/// Paused-runner re-check lock: the candidate back under `FOR UPDATE
/// OF "agent_run"` with the still-paused predicate re-applied (the
/// runner may have been resumed since the candidate list was read),
/// plus the parent row via `select_related`. Binds: `$1` run id,
/// `$2` `queued`.
pub fn paused_runner_pin_lock_select() -> String {
    format!(
        "SELECT {}, {} FROM \"agent_run\" INNER JOIN \"runner\" ON \
         (\"agent_run\".\"pinned_runner_id\" = \"runner\".\"id\") \
         LEFT OUTER JOIN \"agent_run\" T3 ON (\"agent_run\".\"parent_run_id\" = T3.\"id\") \
         WHERE (\"agent_run\".\"id\" = $1 AND \"agent_run\".\"pinned_runner_id\" IS NOT NULL AND \
         \"agent_run\".\"status\" = $2 AND \"runner\".\"paused_at\" IS NOT NULL) \
         ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1 FOR UPDATE OF \"agent_run\"",
        qualified_columns("agent_run", agent_run::COLUMNS),
        aliased_columns("T3", agent_run::COLUMNS),
    )
}

/// Parent `thread_id` clear (`parent.save(update_fields=["thread_id"])`
/// after stamping `""`). Binds: `$1` the empty string, `$2` parent id.
/// `agent_run` carries no `updated_at`, so exactly one column writes.
pub fn clear_parent_thread_sql() -> String {
    "UPDATE \"agent_run\" SET \"thread_id\" = $1 WHERE \"agent_run\".\"id\" = $2".to_owned()
}

/// Pin clear (`run.save(update_fields=["pinned_runner"])` after
/// stamping `None`). Binds: `$1` run id.
pub fn clear_pin_sql() -> String {
    "UPDATE \"agent_run\" SET \"pinned_runner_id\" = NULL WHERE \"agent_run\".\"id\" = $1"
        .to_owned()
}

// ---------------------------------------------------------------------------
// Fail-runs sweep (`fail_runs_on_offline_runners`)
// ---------------------------------------------------------------------------

/// Runners holding BUSY `local_runner` runs assigned before the cutoff
/// while themselves silent past it (or never heartbeated). The
/// `DISTINCT` triple keeps the runner ordering (`-last_heartbeat_at`,
/// `-created_at`); the driver dedupes ids in first-seen order.
/// Binds: `$1` cutoff (`assigned_at`), `$2` `local_runner`, `$3`
/// cutoff (heartbeat).
pub fn silent_runner_ids_select(
    cutoff: DateTime<Utc>,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<SilentRunnerRow>> {
    runner::Entity::find()
        .select_only()
        .distinct()
        .columns([
            runner::Column::Id,
            runner::Column::LastHeartbeatAt,
            runner::Column::CreatedAt,
        ])
        .join(JoinType::InnerJoin, run::Relation::Runner.def().rev())
        .filter(run::Column::AssignedAt.lt(cutoff))
        .filter(run::Column::ExecutorKind.eq(AgentExecutorKind::LocalRunner.value()))
        .filter(run::Column::Status.is_in(BUSY_STATUSES.iter().map(AgentRunStatus::value)))
        .filter(runner_silent(cutoff))
        .order_by_desc(runner::Column::LastHeartbeatAt)
        .order_by_desc(runner::Column::CreatedAt)
        .limit(500)
        .into_tuple()
}

/// One [`silent_runner_ids_select`] row: `(runner id, last_heartbeat_at,
/// created_at)` — Django's `DISTINCT` triple.
pub type SilentRunnerRow = (Uuid, Option<DateTime<Utc>>, DateTime<Utc>);

/// Silent past the cutoff, or never heartbeated.
fn runner_silent(cutoff: DateTime<Utc>) -> Condition {
    Condition::any()
        .add(runner::Column::LastHeartbeatAt.lt(cutoff))
        .add(runner::Column::LastHeartbeatAt.is_null())
}

/// Per-runner re-check lock: the runner row back under plain `FOR
/// UPDATE` with the silence predicate re-applied (a runner that polled
/// since the list was read keeps its runs — the poll takes the same
/// row lock before writing the heartbeat). Binds: `$1` runner id, `$2`
/// cutoff.
pub fn runner_silent_lock_select(
    runner_id: Uuid,
    cutoff: DateTime<Utc>,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<Uuid>> {
    runner::Entity::find()
        .select_only()
        .column(runner::Column::Id)
        .filter(runner::Column::Id.eq(runner_id))
        .filter(runner_silent(cutoff))
        .order_by_desc(runner::Column::LastHeartbeatAt)
        .order_by_desc(runner::Column::CreatedAt)
        .limit(1)
        .lock_exclusive()
        .into_tuple::<Uuid>()
}

/// Shared sweep-stale `WHERE` core: `assigned_at < $1 AND
/// executor_kind = $2 AND runner_id = $3 AND status IN (…BUSY…)` —
/// the `_finish_unclaimed_runs` `stale` queryset. Django sorts the
/// `filter()` kwargs (`assigned_at`, `executor_kind`, `runner_id`,
/// `status`).
fn sweep_stale(cutoff: DateTime<Utc>, runner_id: Uuid) -> sea_orm::Select<run::Entity> {
    run::Entity::find()
        .select_only()
        .filter(run::Column::AssignedAt.lt(cutoff))
        .filter(run::Column::ExecutorKind.eq(AgentExecutorKind::LocalRunner.value()))
        .filter(run::Column::RunnerId.eq(runner_id))
        .filter(run::Column::Status.is_in(BUSY_STATUSES.iter().map(AgentRunStatus::value)))
}

/// Stopped-cancel barrier id scan: the stale core plus the redundant
/// `AND status = 'cancel_requested'` (kept verbatim).
pub fn sweep_stale_cancel_ids_sql(
    cutoff: DateTime<Utc>,
    runner_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<Uuid>> {
    sweep_stale(cutoff, runner_id)
        .column(run::Column::Id)
        .filter(run::Column::Status.eq(AgentRunStatus::CancelRequested.value()))
        .order_by_desc(run::Column::CreatedAt)
        .into_tuple::<Uuid>()
}

/// Stopped-cancel barrier pod-id scan: same `WHERE`, `pod_id` projection.
pub fn sweep_stale_cancel_pods_sql(
    cutoff: DateTime<Utc>,
    runner_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<Uuid>> {
    sweep_stale(cutoff, runner_id)
        .column(run::Column::PodId)
        .filter(run::Column::Status.eq(AgentRunStatus::CancelRequested.value()))
        .order_by_desc(run::Column::CreatedAt)
        .into_tuple::<Uuid>()
}

/// Sweep reap candidate scan: `SELECT id, pod_id` over the stale core,
/// plus `AND NOT (id IN (…))` once the barrier has moved stopped
/// cancels out. `excluded_ids` is the barrier id count (0 renders no
/// term); the `IN` params bind `$4…`.
pub fn sweep_stale_pairs_sql(
    cutoff: DateTime<Utc>,
    runner_id: Uuid,
    excluded_ids: &[Uuid],
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<(Uuid, Uuid)>> {
    let mut select = sweep_stale(cutoff, runner_id).columns([run::Column::Id, run::Column::PodId]);
    if !excluded_ids.is_empty() {
        select = select.filter(run::Column::Id.is_not_in(excluded_ids.iter().copied()));
    }
    select
        .order_by_desc(run::Column::CreatedAt)
        .into_tuple::<(Uuid, Uuid)>()
}

/// Stopped-cancel barrier write with the offline stamp
/// (`stamp_cancelled=True`): `.update()` call order (`status`,
/// `ended_at`, `error_details`). The code is merged
/// into the bag in SQL (`merge_error_details_expression`:
/// `error_details || '{"code": …}'`, PDASHOSS01-187) — a bulk update
/// has no locked row to merge in Rust. `$1` is `now` (`ended_at`),
/// `$2` the [`offline_stamp_patch`], `$3…` the stopped-cancel ids.
/// Call only with a non-empty id list — the source guards the write
/// with `if stopped_cancel_ids:`.
pub fn sweep_cancel_barrier_stamped_sql(
    now: DateTime<Utc>,
    error_code: &str,
    ids: &[Uuid],
) -> sea_orm::UpdateMany<run::Entity> {
    debug_assert!(!ids.is_empty(), "barrier write needs stopped-cancel ids");
    run::Entity::update_many()
        .col_expr(
            run::Column::Status,
            Expr::value(AgentRunStatus::Cancelled.value()),
        )
        .col_expr(run::Column::EndedAt, Expr::value(now))
        .col_expr(
            run::Column::ErrorDetails,
            Expr::col((run::Entity, run::Column::ErrorDetails)).binary(
                PgBinOper::Concatenate,
                Expr::val(offline_stamp_patch(error_code)).cast_as(Alias::new("jsonb")),
            ),
        )
        .filter(run::Column::Id.is_in(ids.iter().copied()))
}

/// The `$2` patch [`sweep_cancel_barrier_stamped_sql`] merges: the
/// stamp's `code` key alone.
pub fn offline_stamp_patch(error_code: &str) -> serde_json::Value {
    serde_json::json!({ pidash_db::runner_runs::agent_run::ERROR_DETAILS_CODE: error_code })
}

/// The `error` detail every sweep-failed run finalizes with.
pub fn offline_fail_detail(grace_secs: i64) -> String {
    format!("runner offline: no heartbeat for >{grace_secs}s while this run was assigned to it")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_release_list_sql_matches_django() {
        // No runner join, no age gate, no LIMIT: the pause owns the set.
        assert_eq!(
            paused_runner_pin_ids_select(),
            "SELECT \"agent_run\".\"id\" FROM \"agent_run\" WHERE \
             (\"agent_run\".\"executor_kind\" = $1 AND \"agent_run\".\"pinned_runner_id\" IS NOT NULL AND \
             \"agent_run\".\"status\" = $2 AND \"agent_run\".\"pinned_runner_id\" = $3) \
             ORDER BY \"agent_run\".\"created_at\" DESC",
        );
    }

    #[test]
    fn paused_release_lock_sql_shape_matches_django() {
        let sql = paused_runner_pin_lock_select();
        let head = format!(
            "SELECT {}, {} FROM \"agent_run\"",
            qualified_columns("agent_run", agent_run::COLUMNS),
            aliased_columns("T3", agent_run::COLUMNS),
        );
        assert!(sql.starts_with(&head), "projection drifted");
        // Same lock shape as the sweep, but the re-check is the
        // still-paused arm alone (no cutoff bind).
        assert!(sql.contains(
            "WHERE (\"agent_run\".\"id\" = $1 AND \"agent_run\".\"pinned_runner_id\" IS NOT NULL AND \
             \"agent_run\".\"status\" = $2 AND \"runner\".\"paused_at\" IS NOT NULL) \
             ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1 FOR UPDATE OF \"agent_run\""
        ));
        assert!(!sql.contains("\"T3\""));
    }

    #[test]
    fn clear_writes_match_django_saves() {
        assert_eq!(
            clear_parent_thread_sql(),
            "UPDATE \"agent_run\" SET \"thread_id\" = $1 WHERE \"agent_run\".\"id\" = $2",
        );
        assert_eq!(
            clear_pin_sql(),
            "UPDATE \"agent_run\" SET \"pinned_runner_id\" = NULL WHERE \"agent_run\".\"id\" = $1",
        );
    }

    fn pg<S: sea_orm::SelectorTrait>(selector: sea_orm::Selector<S>) -> String {
        selector
            .into_statement(sea_orm::DbBackend::Postgres)
            .to_string()
    }

    fn at() -> DateTime<Utc> {
        "2026-01-02T03:04:05Z".parse().expect("ts")
    }

    #[test]
    fn stuck_list_statement_shape() {
        let sql = pg(stuck_queued_run_ids_select(at()));
        assert!(sql.starts_with(
            "SELECT \"agent_run\".\"id\" FROM \"agent_run\" INNER JOIN \"runner\" ON \
             \"agent_run\".\"pinned_runner_id\" = \"runner\".\"id\" WHERE"
        ));
        for part in [
            "\"agent_run\".\"created_at\" < '2026-01-02 03:04:05.000000 +00:00'",
            "\"agent_run\".\"executor_kind\" = 'local_runner'",
            "\"agent_run\".\"pinned_runner_id\" IS NOT NULL",
            "\"agent_run\".\"status\" = 'queued'",
            "(\"runner\".\"last_heartbeat_at\" < '2026-01-02 03:04:05.000000 +00:00' OR \
             \"runner\".\"last_heartbeat_at\" IS NULL OR \
             \"runner\".\"paused_at\" IS NOT NULL OR \
             (\"runner\".\"dev_machine_id\" IN (SELECT \"id\" FROM \"dev_machine\" \
             WHERE \"dev_machine\".\"engine_paused_by_owner\" = TRUE OR \
             \"dev_machine\".\"engine_auto_pause_reason\" > '') AND \
             \"runner\".\"provisioning\" = 'desktop_bundled') OR \
             (\"runner\".\"pod_dispatch\" = 'pinned_only' AND \
             \"runner\".\"provisioning\" = 'desktop_bundled'))",
        ] {
            assert!(sql.contains(part), "{part} missing: {sql}");
        }
        assert!(sql.ends_with("ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 500"));
    }

    #[test]
    fn release_lock_statement_shape() {
        let sql = pg(release_pin_lock_select(Uuid::nil(), at()));
        assert!(
            sql.starts_with(
                "SELECT \"agent_run\".\"pod_id\", \"agent_run\".\"parent_run_id\", \
             \"T3\".\"thread_id\" AS \"parent_thread_id\" FROM \"agent_run\" \
             INNER JOIN \"runner\" ON \"agent_run\".\"pinned_runner_id\" = \"runner\".\"id\" \
             LEFT JOIN \"agent_run\" AS \"T3\" ON \"agent_run\".\"parent_run_id\" = \"T3\".\"id\""
            ),
            "{sql}"
        );
        assert!(sql.contains("\"runner\".\"paused_at\" IS NOT NULL"));
        assert!(
            sql.ends_with(
                "ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1 FOR UPDATE OF \"agent_run\""
            ),
            "{sql}"
        );
    }

    #[test]
    fn runner_statements_shape() {
        let sql = pg(silent_runner_ids_select(at()));
        assert!(
            sql.starts_with(
                "SELECT DISTINCT \"runner\".\"id\", \"runner\".\"last_heartbeat_at\", \
             \"runner\".\"created_at\" FROM \"runner\" INNER JOIN \"agent_run\" ON \
             \"runner\".\"id\" = \"agent_run\".\"runner_id\" WHERE"
            ),
            "{sql}"
        );
        assert!(sql.contains(
            "\"agent_run\".\"status\" IN ('assigned', 'waiting_for_worktree', 'running', \
             'cancel_requested', 'awaiting_approval', 'awaiting_reauth')"
        ));
        assert!(sql.ends_with(
            "ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 500"
        ));
        let lock = pg(runner_silent_lock_select(Uuid::nil(), at()));
        assert!(lock.starts_with("SELECT \"runner\".\"id\" FROM \"runner\" WHERE"));
        assert!(
            lock.ends_with(
                "ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC \
             LIMIT 1 FOR UPDATE"
            ),
            "{lock}"
        );
    }

    #[test]
    fn stale_scans_shape() {
        let ids = pg(sweep_stale_cancel_ids_sql(at(), Uuid::nil()));
        assert!(ids.starts_with("SELECT \"agent_run\".\"id\" FROM \"agent_run\" WHERE"));
        assert!(ids.contains("AND \"agent_run\".\"status\" = 'cancel_requested'"));
        assert!(ids.ends_with("ORDER BY \"agent_run\".\"created_at\" DESC"));
        let pods = pg(sweep_stale_cancel_pods_sql(at(), Uuid::nil()));
        assert!(pods.starts_with("SELECT \"agent_run\".\"pod_id\" FROM \"agent_run\" WHERE"));
        let pairs = pg(sweep_stale_pairs_sql(at(), Uuid::nil(), &[]));
        assert!(pairs.starts_with(
            "SELECT \"agent_run\".\"id\", \"agent_run\".\"pod_id\" FROM \"agent_run\" WHERE"
        ));
        assert!(!pairs.contains("NOT IN"));
        let excluded = pg(sweep_stale_pairs_sql(at(), Uuid::nil(), &[Uuid::nil()]));
        assert!(excluded
            .contains("\"agent_run\".\"id\" NOT IN ('00000000-0000-0000-0000-000000000000')"));
    }

    #[test]
    fn stamped_barrier_statement_shape() {
        let sql = sea_orm::QueryTrait::build(
            &sweep_cancel_barrier_stamped_sql(at(), "runner_offline", &[Uuid::nil()]),
            sea_orm::DbBackend::Postgres,
        )
        .to_string();
        assert!(sql.starts_with(
            "UPDATE \"agent_run\" SET \"status\" = 'cancelled', \
             \"ended_at\" = '2026-01-02 03:04:05.000000 +00:00', \
             \"error_details\" = \"agent_run\".\"error_details\" || CAST(E'{\\\"code\\\":\\\"runner_offline\\\"}' AS jsonb) \
             WHERE \"agent_run\".\"id\" IN ("
        ), "{sql}");
        assert_eq!(
            offline_stamp_patch("runner_offline"),
            serde_json::json!({"code": "runner_offline"})
        );
    }

    #[test]
    fn fail_detail_matches_django() {
        assert_eq!(
            offline_fail_detail(3600),
            "runner offline: no heartbeat for >3600s while this run was assigned to it",
        );
    }
}
