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
//!   (default 300) and created before the cutoff go back to the pod
//!   queue (pin cleared, parent `thread_id` cleared), 500 per pass,
//!   then each affected pod drains.
//! * `fail_runs_on_offline_runners`: BUSY `local_runner` runs on a
//!   runner silent past `RUNNER_OFFLINE_RUN_FAIL_SECS` (default 3600)
//!   are terminated through `_finish_unclaimed_runs` with error code
//!   [`RUNNER_OFFLINE_ERROR_CODE`](super::session_service::RUNNER_OFFLINE_ERROR_CODE)
//!   and CANCELLED rows stamped.
//!
//! Either sweep is disabled when its setting is `<= 0`.
//!
//! This module is pure planners only (SQL text in Django shape, quoted
//! identifiers, `%s` params rendered as Postgres `$N`): the executing
//! drivers live in `pidash_jobs::runner_runs::offline_recovery`. SQL
//! text below is pinned against Django 4.2.30 output captured from
//! `origin/main` (see `.probe1063-sql.py` in the issue workspace).
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

use pidash_db::runner_runs::agent_run;
use pidash_types::runner_runs::AgentRunStatus;

use super::guards::{BUSY_STATUSES, RUNNER_COLUMNS};

/// Render a status `IN` list in slice order:
/// `('assigned', 'waiting_for_worktree', …)`.
fn status_in_list(statuses: &[AgentRunStatus]) -> String {
    let inner: Vec<&str> = statuses.iter().map(AgentRunStatus::value).collect();
    format!("('{}')", inner.join("', '"))
}

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
/// cutoff or never heartbeated. Binds: `$1` cutoff (`created_at`),
/// `$2` `local_runner`, `$3` `queued`, `$4` cutoff (heartbeat).
pub fn stuck_queued_run_ids_select() -> String {
    "SELECT \"agent_run\".\"id\" FROM \"agent_run\" INNER JOIN \"runner\" ON \
     (\"agent_run\".\"pinned_runner_id\" = \"runner\".\"id\") WHERE \
     (\"agent_run\".\"created_at\" < $1 AND \"agent_run\".\"executor_kind\" = $2 AND \
     \"agent_run\".\"pinned_runner_id\" IS NOT NULL AND \"agent_run\".\"status\" = $3 AND \
     (\"runner\".\"last_heartbeat_at\" < $4 OR \"runner\".\"last_heartbeat_at\" IS NULL)) \
     ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 500"
        .to_owned()
}

/// Per-run re-check lock: the candidate back under `FOR UPDATE OF
/// "agent_run"` with the silence predicate re-applied (the runner may
/// have come back), plus the parent row via `select_related`.
/// Binds: `$1` run id, `$2` `queued`, `$3` cutoff.
pub fn release_pin_lock_select() -> String {
    format!(
        "SELECT {}, {} FROM \"agent_run\" INNER JOIN \"runner\" ON \
         (\"agent_run\".\"pinned_runner_id\" = \"runner\".\"id\") \
         LEFT OUTER JOIN \"agent_run\" T3 ON (\"agent_run\".\"parent_run_id\" = T3.\"id\") \
         WHERE (\"agent_run\".\"id\" = $1 AND \"agent_run\".\"pinned_runner_id\" IS NOT NULL AND \
         \"agent_run\".\"status\" = $2 AND (\"runner\".\"last_heartbeat_at\" < $3 OR \
         \"runner\".\"last_heartbeat_at\" IS NULL)) \
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
pub fn silent_runner_ids_select() -> String {
    format!(
        "SELECT DISTINCT \"runner\".\"id\", \"runner\".\"last_heartbeat_at\", \
         \"runner\".\"created_at\" FROM \"runner\" INNER JOIN \"agent_run\" ON \
         (\"runner\".\"id\" = \"agent_run\".\"runner_id\") WHERE \
         (\"agent_run\".\"assigned_at\" < $1 AND \"agent_run\".\"executor_kind\" = $2 AND \
         \"agent_run\".\"status\" IN {} AND (\"runner\".\"last_heartbeat_at\" < $3 OR \
         \"runner\".\"last_heartbeat_at\" IS NULL)) \
         ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 500",
        status_in_list(&BUSY_STATUSES),
    )
}

/// Per-runner re-check lock: the runner row back under plain `FOR
/// UPDATE` with the silence predicate re-applied (a runner that polled
/// since the list was read keeps its runs — the poll takes the same
/// row lock before writing the heartbeat). Binds: `$1` runner id, `$2`
/// cutoff.
pub fn runner_silent_lock_select() -> String {
    format!(
        "SELECT {} FROM \"runner\" WHERE (\"runner\".\"id\" = $1 AND \
         (\"runner\".\"last_heartbeat_at\" < $2 OR \"runner\".\"last_heartbeat_at\" IS NULL)) \
         ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC \
         LIMIT 1 FOR UPDATE",
        qualified_columns("runner", &RUNNER_COLUMNS),
    )
}

/// Shared sweep-stale `WHERE` core: `assigned_at < $1 AND
/// executor_kind = $2 AND runner_id = $3 AND status IN (…BUSY…)` —
/// the `_finish_unclaimed_runs` `stale` queryset. Django sorts the
/// `filter()` kwargs (`assigned_at`, `executor_kind`, `runner_id`,
/// `status`).
fn sweep_stale_where() -> String {
    format!(
        "(\"agent_run\".\"assigned_at\" < $1 AND \"agent_run\".\"executor_kind\" = $2 AND \
         \"agent_run\".\"runner_id\" = $3 AND \"agent_run\".\"status\" IN {}",
        status_in_list(&BUSY_STATUSES),
    )
}

/// Stopped-cancel barrier id scan: the stale core plus the redundant
/// `AND status = 'cancel_requested'` (kept verbatim).
pub fn sweep_stale_cancel_ids_sql() -> String {
    format!(
        "SELECT \"agent_run\".\"id\" FROM \"agent_run\" WHERE {} AND \
         \"agent_run\".\"status\" = 'cancel_requested') \
         ORDER BY \"agent_run\".\"created_at\" DESC",
        sweep_stale_where(),
    )
}

/// Stopped-cancel barrier pod-id scan: same `WHERE`, `pod_id` projection.
pub fn sweep_stale_cancel_pods_sql() -> String {
    format!(
        "SELECT \"agent_run\".\"pod_id\" FROM \"agent_run\" WHERE {} AND \
         \"agent_run\".\"status\" = 'cancel_requested') \
         ORDER BY \"agent_run\".\"created_at\" DESC",
        sweep_stale_where(),
    )
}

/// Sweep reap candidate scan: `SELECT id, pod_id` over the stale core,
/// plus `AND NOT (id IN (…))` once the barrier has moved stopped
/// cancels out. `excluded_ids` is the barrier id count (0 renders no
/// term); the `IN` params bind `$4…`.
pub fn sweep_stale_pairs_sql(excluded_ids: usize) -> String {
    let mut where_ = sweep_stale_where();
    if excluded_ids > 0 {
        let params: Vec<String> = (0..excluded_ids).map(|i| format!("${}", 4 + i)).collect();
        where_.push_str(&format!(
            " AND NOT (\"agent_run\".\"id\" IN ({}))",
            params.join(", ")
        ));
    }
    format!(
        "SELECT \"agent_run\".\"id\", \"agent_run\".\"pod_id\" FROM \"agent_run\" WHERE {where_}) \
         ORDER BY \"agent_run\".\"created_at\" DESC"
    )
}

/// Stopped-cancel barrier write with the offline stamp
/// (`stamp_cancelled=True`): `.update()` call order (`status`,
/// `ended_at`, `error_code`). `$1` is `now`
/// (`ended_at`), `$2` the error code, `$3…` the stopped-cancel ids.
/// Call only with a non-empty id list — the source guards the write
/// with `if stopped_cancel_ids:`.
pub fn sweep_cancel_barrier_stamped_sql(id_count: usize) -> String {
    debug_assert_ne!(id_count, 0, "barrier write needs stopped-cancel ids");
    let params: Vec<String> = (0..id_count).map(|i| format!("${}", i + 3)).collect();
    format!(
        "UPDATE \"agent_run\" SET \"status\" = 'cancelled', \"ended_at\" = $1, \
         \"error_code\" = $2 \
         WHERE \"agent_run\".\"id\" IN ({})",
        params.join(", ")
    )
}

/// The `error` detail every sweep-failed run finalizes with.
pub fn offline_fail_detail(grace_secs: i64) -> String {
    format!("runner offline: no heartbeat for >{grace_secs}s while this run was assigned to it")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stuck_list_sql_matches_django() {
        assert_eq!(
            stuck_queued_run_ids_select(),
            "SELECT \"agent_run\".\"id\" FROM \"agent_run\" INNER JOIN \"runner\" ON \
             (\"agent_run\".\"pinned_runner_id\" = \"runner\".\"id\") WHERE \
             (\"agent_run\".\"created_at\" < $1 AND \"agent_run\".\"executor_kind\" = $2 AND \
             \"agent_run\".\"pinned_runner_id\" IS NOT NULL AND \"agent_run\".\"status\" = $3 AND \
             (\"runner\".\"last_heartbeat_at\" < $4 OR \"runner\".\"last_heartbeat_at\" IS NULL)) \
             ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 500",
        );
    }

    #[test]
    fn release_lock_sql_shape_matches_django() {
        let sql = release_pin_lock_select();
        // Projection: full run row plus the full parent row as T3.
        let head = format!(
            "SELECT {}, {} FROM \"agent_run\"",
            qualified_columns("agent_run", agent_run::COLUMNS),
            aliased_columns("T3", agent_run::COLUMNS),
        );
        assert!(sql.starts_with(&head), "projection drifted");
        assert!(sql.contains(
            "INNER JOIN \"runner\" ON (\"agent_run\".\"pinned_runner_id\" = \"runner\".\"id\") \
             LEFT OUTER JOIN \"agent_run\" T3 ON (\"agent_run\".\"parent_run_id\" = T3.\"id\")"
        ));
        assert!(sql.contains(
            "WHERE (\"agent_run\".\"id\" = $1 AND \"agent_run\".\"pinned_runner_id\" IS NOT NULL AND \
             \"agent_run\".\"status\" = $2 AND (\"runner\".\"last_heartbeat_at\" < $3 OR \
             \"runner\".\"last_heartbeat_at\" IS NULL)) \
             ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1 FOR UPDATE OF \"agent_run\""
        ));
        // The parent alias is bare T3 (Django's join counter), not T1 —
        // and never double-quoted (that would be another alias).
        assert!(sql.contains("T3.\"thread_id\""));
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

    #[test]
    fn runner_list_sql_matches_django() {
        assert_eq!(
            silent_runner_ids_select(),
            "SELECT DISTINCT \"runner\".\"id\", \"runner\".\"last_heartbeat_at\", \
             \"runner\".\"created_at\" FROM \"runner\" INNER JOIN \"agent_run\" ON \
             (\"runner\".\"id\" = \"agent_run\".\"runner_id\") WHERE \
             (\"agent_run\".\"assigned_at\" < $1 AND \"agent_run\".\"executor_kind\" = $2 AND \
             \"agent_run\".\"status\" IN ('assigned', 'waiting_for_worktree', 'running', \
             'cancel_requested', 'awaiting_approval', 'awaiting_reauth') AND \
             (\"runner\".\"last_heartbeat_at\" < $3 OR \"runner\".\"last_heartbeat_at\" IS NULL)) \
             ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 500",
        );
    }

    #[test]
    fn runner_lock_sql_shape_matches_django() {
        let sql = runner_silent_lock_select();
        let head = format!(
            "SELECT {} FROM \"runner\"",
            qualified_columns("runner", &RUNNER_COLUMNS),
        );
        assert!(sql.starts_with(&head), "projection drifted");
        assert!(sql.ends_with(
            "WHERE (\"runner\".\"id\" = $1 AND (\"runner\".\"last_heartbeat_at\" < $2 OR \
             \"runner\".\"last_heartbeat_at\" IS NULL)) \
             ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC \
             LIMIT 1 FOR UPDATE"
        ));
    }

    #[test]
    fn stale_scans_match_django() {
        let core =
            "(\"agent_run\".\"assigned_at\" < $1 AND \"agent_run\".\"executor_kind\" = $2 AND \
         \"agent_run\".\"runner_id\" = $3 AND \"agent_run\".\"status\" IN ('assigned', \
         'waiting_for_worktree', 'running', 'cancel_requested', 'awaiting_approval', \
         'awaiting_reauth')";
        assert_eq!(
            sweep_stale_cancel_ids_sql(),
            format!(
                "SELECT \"agent_run\".\"id\" FROM \"agent_run\" WHERE {core} AND \
                 \"agent_run\".\"status\" = 'cancel_requested') \
                 ORDER BY \"agent_run\".\"created_at\" DESC"
            ),
        );
        assert_eq!(
            sweep_stale_cancel_pods_sql(),
            format!(
                "SELECT \"agent_run\".\"pod_id\" FROM \"agent_run\" WHERE {core} AND \
                 \"agent_run\".\"status\" = 'cancel_requested') \
                 ORDER BY \"agent_run\".\"created_at\" DESC"
            ),
        );
        assert_eq!(
            sweep_stale_pairs_sql(0),
            format!(
                "SELECT \"agent_run\".\"id\", \"agent_run\".\"pod_id\" FROM \"agent_run\" \
                 WHERE {core}) ORDER BY \"agent_run\".\"created_at\" DESC"
            ),
        );
        assert!(sweep_stale_pairs_sql(2).contains(
            "AND NOT (\"agent_run\".\"id\" IN ($4, $5))) ORDER BY \"agent_run\".\"created_at\" DESC"
        ));
    }

    #[test]
    fn stamped_barrier_binds_code_before_ids() {
        assert_eq!(
            sweep_cancel_barrier_stamped_sql(2),
            "UPDATE \"agent_run\" SET \"status\" = 'cancelled', \"ended_at\" = $1, \
             \"error_code\" = $2 \
             WHERE \"agent_run\".\"id\" IN ($3, $4)",
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
