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
//!    release it ([`RELEASE_RUNNER_IF_IDLE_SQL`]) and keep the pin, so
//!    the runner serves its own pinned run on the next drain. A dead
//!    wedged runner (stale heartbeat) is left to the BUSY-but-dead
//!    sweep, which flips it OFFLINE instead of resurrecting it.
//! 2. Pin held past `RUNNER_PIN_AUTO_RELEASE_SECS` (from `created_at`)
//!    while the pinned runner is neither assignable (ONLINE + fresh
//!    heartbeat) nor legitimately working (BUSY + fresh heartbeat + a
//!    busy-status run): clear the pin and the parent's `thread_id`
//!    (the manual release-pin endpoint's resume-hint drop).
//!    `local_runner` runs only.
//!
//! Pure planners only (SQL text in Django shape, quoted identifiers,
//! scalar `%s` params rendered as Postgres `$N`, enum values inlined
//! like [`RELEASE_RUNNER_IF_IDLE_SQL`]); the executing driver lives in
//! `pidash_jobs::runner_runs::unsatisfiable_pins`. SQL text is pinned
//! against Django 4.2.30 output captured from PR #541's head.
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
//!
//! # Deliberate difference
//!
//! Django's "assignable" carve-out also requires `paused_at IS NULL`
//! (runner pause, main #1944). rust-dev's schema predates that column
//! (runner migrations stop at 0029), so the Rust predicate is ONLINE +
//! fresh heartbeat, the same as every other Rust eligibility check.
//! The pause port adds the clause here with the rest of the feature.
//!
//! [`RELEASE_RUNNER_IF_IDLE_SQL`]: super::drain::RELEASE_RUNNER_IF_IDLE_SQL

/// `RUNNER_PIN_AUTO_RELEASE_SECS` default (`settings/common.py`).
pub const DEFAULT_PIN_AUTO_RELEASE_SECS: i64 = 600;

/// Wedged-busy pinned runners (remedy 1): `(pinned_runner_id, pod_id,
/// created_at)` for QUEUED machine-executor runs whose pinned runner is
/// `busy` and heartbeat-fresh. Binds: `$1` the alive threshold
/// (`now - HEARTBEAT_GRACE`).
pub fn wedged_pinned_runners_select() -> &'static str {
    "SELECT DISTINCT \"agent_run\".\"pinned_runner_id\", \"agent_run\".\"pod_id\", \
     \"agent_run\".\"created_at\" FROM \"agent_run\" INNER JOIN \"runner\" ON \
     (\"agent_run\".\"pinned_runner_id\" = \"runner\".\"id\") WHERE \
     (\"agent_run\".\"executor_kind\" IN ('local_runner', 'managed_runner') AND \
     \"runner\".\"last_heartbeat_at\" >= $1 AND \"runner\".\"status\" = 'busy' AND \
     \"agent_run\".\"status\" = 'queued') ORDER BY \"agent_run\".\"created_at\" DESC"
}

/// Stale pins past the bound (remedy 2), under `FOR UPDATE SKIP
/// LOCKED`: `(id, pinned_runner_id, pod_id)`. Binds: `$1` the bound
/// cutoff (`now - RUNNER_PIN_AUTO_RELEASE_SECS`, against
/// `created_at`), `$2` the alive threshold (both carve-outs).
pub fn stale_pins_lock_select() -> &'static str {
    "SELECT \"agent_run\".\"id\", \"agent_run\".\"pinned_runner_id\", \"agent_run\".\"pod_id\" \
     FROM \"agent_run\" INNER JOIN \"runner\" ON \
     (\"agent_run\".\"pinned_runner_id\" = \"runner\".\"id\") WHERE \
     (\"agent_run\".\"created_at\" < $1 AND \"agent_run\".\"executor_kind\" = 'local_runner' AND \
     \"agent_run\".\"pinned_runner_id\" IS NOT NULL AND \"agent_run\".\"status\" = 'queued' AND \
     NOT (\"runner\".\"last_heartbeat_at\" >= $2 AND \"runner\".\"last_heartbeat_at\" IS NOT NULL \
     AND \"runner\".\"status\" = 'online') AND \
     NOT (EXISTS(SELECT 1 AS \"a\" FROM \"agent_run\" U2 WHERE (U2.\"status\" IN ('assigned', \
     'waiting_for_worktree', 'running', 'cancel_requested', 'awaiting_approval', \
     'awaiting_reauth') AND U2.\"runner_id\" = (\"agent_run\".\"pinned_runner_id\")) LIMIT 1) \
     AND \"runner\".\"last_heartbeat_at\" >= $2 AND \"runner\".\"last_heartbeat_at\" IS NOT NULL \
     AND \"runner\".\"status\" = 'busy')) \
     ORDER BY \"agent_run\".\"created_at\" DESC FOR UPDATE SKIP LOCKED"
}

/// `", "`-joined `$start…$(start+count-1)`.
fn params(start: usize, count: usize) -> String {
    (0..count)
        .map(|i| format!("${}", start + i))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Bulk pin clear: `AgentRun.objects.filter(id__in=stale_ids)
/// .update(pinned_runner=None)`. Binds: `$1…` the stale run ids. Call
/// only with a non-empty list (the source guards with `if stale:`).
pub fn clear_pins_sql(id_count: usize) -> String {
    debug_assert_ne!(id_count, 0, "pin clear needs stale run ids");
    format!(
        "UPDATE \"agent_run\" SET \"pinned_runner_id\" = NULL WHERE \"agent_run\".\"id\" IN ({})",
        params(1, id_count)
    )
}

/// Parent resume-hint drop: `AgentRun.objects.filter(id__in=
/// AgentRun.objects.filter(id__in=stale_ids).values("parent_run_id"))
/// .exclude(thread_id="").update(thread_id="")`. Binds: `$1…` the
/// stale run ids. Same non-empty precondition as [`clear_pins_sql`].
pub fn clear_parent_threads_sql(id_count: usize) -> String {
    debug_assert_ne!(id_count, 0, "parent clear needs stale run ids");
    format!(
        "UPDATE \"agent_run\" SET \"thread_id\" = '' WHERE (\"agent_run\".\"id\" IN \
         (SELECT U0.\"parent_run_id\" FROM \"agent_run\" U0 WHERE U0.\"id\" IN ({})) AND \
         NOT (\"agent_run\".\"thread_id\" = ''))",
        params(1, id_count)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Django's captured SQL with literals swapped for the Rust binds,
    /// minus the `paused_at` clause (see "Deliberate difference").
    #[test]
    fn wedged_select_matches_django() {
        let django = "SELECT DISTINCT \"agent_run\".\"pinned_runner_id\", \"agent_run\".\"pod_id\", \"agent_run\".\"created_at\" FROM \"agent_run\" INNER JOIN \"runner\" ON (\"agent_run\".\"pinned_runner_id\" = \"runner\".\"id\") WHERE (\"agent_run\".\"executor_kind\" IN ('local_runner', 'managed_runner') AND \"runner\".\"last_heartbeat_at\" >= $1 AND \"runner\".\"status\" = 'busy' AND \"agent_run\".\"status\" = 'queued') ORDER BY \"agent_run\".\"created_at\" DESC";
        assert_eq!(wedged_pinned_runners_select(), django);
    }

    #[test]
    fn stale_lock_matches_django_minus_paused() {
        let django = "SELECT \"agent_run\".\"id\", \"agent_run\".\"pinned_runner_id\", \"agent_run\".\"pod_id\" FROM \"agent_run\" INNER JOIN \"runner\" ON (\"agent_run\".\"pinned_runner_id\" = \"runner\".\"id\") WHERE (\"agent_run\".\"created_at\" < $1 AND \"agent_run\".\"executor_kind\" = 'local_runner' AND \"agent_run\".\"pinned_runner_id\" IS NOT NULL AND \"agent_run\".\"status\" = 'queued' AND NOT (\"runner\".\"last_heartbeat_at\" >= $2 AND \"runner\".\"last_heartbeat_at\" IS NOT NULL AND \"runner\".\"paused_at\" IS NULL AND \"runner\".\"status\" = 'online') AND NOT (EXISTS(SELECT 1 AS \"a\" FROM \"agent_run\" U2 WHERE (U2.\"status\" IN ('assigned', 'waiting_for_worktree', 'running', 'cancel_requested', 'awaiting_approval', 'awaiting_reauth') AND U2.\"runner_id\" = (\"agent_run\".\"pinned_runner_id\")) LIMIT 1) AND \"runner\".\"last_heartbeat_at\" >= $2 AND \"runner\".\"last_heartbeat_at\" IS NOT NULL AND \"runner\".\"status\" = 'busy')) ORDER BY \"agent_run\".\"created_at\" DESC FOR UPDATE SKIP LOCKED";
        let without_paused = django.replace(" AND \"runner\".\"paused_at\" IS NULL", "");
        assert_eq!(stale_pins_lock_select(), without_paused);
        assert!(!stale_pins_lock_select().contains("paused_at"));
    }

    #[test]
    fn bulk_writes_match_django() {
        assert_eq!(
            clear_pins_sql(1),
            "UPDATE \"agent_run\" SET \"pinned_runner_id\" = NULL WHERE \"agent_run\".\"id\" IN ($1)"
        );
        assert_eq!(
            clear_parent_threads_sql(1),
            "UPDATE \"agent_run\" SET \"thread_id\" = '' WHERE (\"agent_run\".\"id\" IN (SELECT U0.\"parent_run_id\" FROM \"agent_run\" U0 WHERE U0.\"id\" IN ($1)) AND NOT (\"agent_run\".\"thread_id\" = ''))"
        );
        assert!(clear_pins_sql(3).ends_with("IN ($1, $2, $3)"));
        assert!(clear_parent_threads_sql(2).contains("U0.\"id\" IN ($1, $2))"));
    }

    #[test]
    fn default_bound_matches_django() {
        assert_eq!(DEFAULT_PIN_AUTO_RELEASE_SECS, 600);
    }
}
