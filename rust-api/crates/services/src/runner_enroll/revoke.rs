#![forbid(unsafe_code)]

//! Runner revocation cascade (services-C, PIDASHCONV-588).
//!
//! Port of `Runner.revoke` (`runner/models.py:542-685`): idempotent early
//! return, reason allow-list warning + 32-char truncation, the atomic
//! cascade (runner row update → active `RunnerSession` revoke →
//! non-terminal `AgentRun` cancel via finalization → QUEUED unpin), and
//! the `on_commit` handoff + pod-drain + stream-cleanup scheduling.
//!
//! # Shape: store seam, not inline SQL
//!
//! The services crate has no `sqlx` dependency, so — following the
//! sibling [`RunCreationStore`](super::validation::RunCreationStore)
//! precedent — the reads/writes live behind [`RevokeStore`]: one async
//! method per Django ORM call, with the SQL text in the adjacent `*_SQL`
//! consts (plus [`active_runs_lock_sql`]), which the pool implementation
//! must execute verbatim. [`revoke_runner`] keeps the full orchestration
//! (including the conditional-step and registration order), so the
//! executing layer issues exactly the statements Python issues, in the
//! same order.
//!
//! # Transactions
//!
//! Python holds one `transaction.atomic()` for S1–S4 and registers the
//! post-commit hooks on it. The executing layer (D-13 handlers,
//! PIDASHCONV-592/593) opens one
//! [`Transaction`][pidash_db::tx::Transaction], runs the driver on its
//! store, commits, then fires the collected effects in registration
//! order. The nested `atomic` of `finalize_agent_run` is a savepoint;
//! the port runs it flat on the same transaction (no partial-rollback
//! path exists inside, so the difference is unobservable — the 586
//! precedent). `delete_runner` nests this whole driver as a savepoint
//! the same flat way ([`super::delete`]).
//!
//! # Post-commit effects
//!
//! `AfterCommit` actions are synchronous closures, so the async provider
//! calls cannot queue directly. Instead the driver records them through
//! the `*_after_commit` collectors — in Python's registration order,
//! handoffs (one per affected run, S3 row order) → drains (one per
//! affected pod) → stream cleanup (always) — and the executor fires them
//! after commit through the named providers:
//!
//! * handoff → D-12 `complete_project_move_handoff`
//!   (`services::orchestration::creation`), failures swallowed after
//!   logging [`handoff_failure_log`] (`models.py:653-672`);
//! * drain → D-14 `drain_pod_by_id`
//!   (`services::runner_sessions::drain` builders);
//! * cleanup → D-14 `schedule_stream_cleanup_for_runner`
//!   (`db::runner_sessions::outbox`).
//!
//! The per-run finalize also registers D-15's terminal-effects
//! publication; the pool implementation captures it into the same drain
//! (see [`RevokeStore::finalize_cancelled_run`]).
//!
//! # Warnings
//!
//! Python logs (`_logger.warning`) where there is no error to return.
//! Like the D-14 [`SendOutcome`](super::super::runner_sessions::pubsub::SendOutcome)
//! precedent, warnings come back as strings in
//! [`RevokeOutcome::warnings`]; the executor logs them.
//!
//! # Fixture source of truth
//!
//! * D13-F5 `queries/revoke_cascade.sql` (+ `.rows.json`): S1–S4 text,
//!   the single-`atomic` boundary, the hook order. The `#[cfg(test)]`
//!   suite pins every const/builder against the probe-verified text.
//! * D13-F6 `services/flows.golden.json` (`revoke_cascade_steps`): the
//!   no-op rule, reason handling, post-commit hook order.
//! * D13-F8 `external/wire_pins.json`: the provider call shapes (called,
//!   never inlined).
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * QUIRK-set-ordered-drains (`models.py:675-676`): Python registers one
//!   drain closure per pod by iterating `affected_pod_ids`, a `set`.
//!   The port registers the same pods in first-seen order (S3 rows, then
//!   S4a rows, deduplicated) — one drain per affected pod is preserved;
//!   relative order between independent pod drains was hash-dependent.
//! * QUIRK-default-ordering (`models.py:622-648`): S3/S4a carry
//!   `ORDER BY agent_run.created_at DESC` (the model default leaks into
//!   `values_list`). Ported verbatim.
//! * QUIRK-warnings-before-noop (`models.py:575-593`): the reason
//!   warnings fire even when the runner is already revoked (the
//!   `revoked_at` check runs after truncation). Ported verbatim.
//! * `reason[:32]` counts Unicode code points, not bytes (Porting guide
//!   semantic trap); the port slices chars and counts chars.
//!
//! Ported bugs: none found in this unit on read-through.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use pidash_db::runner_enroll::columns::revoke_reasons::{
    KNOWN_REVOKE_REASONS, REVOKE_REASON_MAX_LEN,
};

use crate::runner_sessions::guards;

// ---------------------------------------------------------------------------
// Reasons (`models.py:575-590`)
// ---------------------------------------------------------------------------

/// CPython `repr` for `str`, for the `%r` interpolations (`:577,583`).
///
/// Private mirror of the sibling `pod_naming::py_repr` (same boundary:
/// single quotes unless the value holds a lone `'`, short escapes for
/// backslash/`\n`/`\r`/`\t`, lowercase `\xXX` for other C0 controls and
/// DEL, printable non-ASCII through).
fn py_repr(value: &str) -> String {
    let use_double = value.contains('\'') && !value.contains('"');
    let (open, close) = if use_double { ('"', '"') } else { ('\'', '\'') };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(open);
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\'' if !use_double => out.push_str("\\'"),
            '"' if use_double => out.push_str("\\\""),
            c if c < ' ' || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(close);
    out
}

/// Validate `reason` and compute the stored form (`models.py:575-590`).
///
/// Unknown reasons warn and proceed (no raise); reasons over
/// [`REVOKE_REASON_MAX_LEN`] code points warn and truncate to that many
/// code points. Returns `(stored_reason, warnings)`; warnings render the
/// `_logger.warning` lines byte-for-byte (`%r` via the private `py_repr`).
pub fn check_revoke_reason(reason: &str) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    if !KNOWN_REVOKE_REASONS.contains(&reason) {
        warnings.push(format!(
            "Runner.revoke called with unknown reason {}; downstream canonical-set match may misbehave. Add the reason to KNOWN_REVOKE_REASONS once intentional.",
            py_repr(reason),
        ));
    }
    if reason.chars().count() > REVOKE_REASON_MAX_LEN {
        warnings.push(format!(
            "Runner.revoke reason {} exceeds {} chars; truncating. The truncated form will not match the daemon's canonical-set synthesizer.",
            py_repr(reason),
            REVOKE_REASON_MAX_LEN,
        ));
    }
    let stored: String = reason.chars().take(REVOKE_REASON_MAX_LEN).collect();
    (stored, warnings)
}

// ---------------------------------------------------------------------------
// SQL (`models.py:597-651`, probe-verified on Django 4.2.30)
// ---------------------------------------------------------------------------

/// S1 — mark the runner revoked (`:599-603`).
///
/// `QuerySet.update`, so `updated_at` is UNCHANGED (no such column here).
/// `$1` = now, `$2` = stored reason, `$3` = runner. `status` is the
/// `RUNNER_STATUS_REVOKED` literal (fixed enum, rendered literally like
/// the D-14 drain consts).
pub const MARK_RUNNER_REVOKED_SQL: &str = "UPDATE \"runner\" SET \"status\" = 'revoked', \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE \"runner\".\"id\" = $3";

/// S2 — revoke active sessions (`:618-620`).
///
/// `$1` = now, `$2` = stored reason, `$3` = runner. The `WHERE` order
/// (`revoked_at IS NULL` first) is Django's `Q`-sorted kwargs, not the
/// source filter order (`runner=self, revoked_at__isnull=True`).
pub const REVOKE_ACTIVE_SESSIONS_SQL: &str = "UPDATE \"runner_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE (\"runner_session\".\"revoked_at\" IS NULL AND \"runner_session\".\"runner_id\" = $3)";

/// S3 — lock + list non-terminal runs (`:622-626`).
///
/// `$1` = runner. The `IN` list renders
/// [`NON_TERMINAL_STATUSES`](guards::NON_TERMINAL_STATUSES) in
/// source-tuple order (called, not copied — the D-14
/// `next_for_runner_sql` precedent); the `ORDER BY` is the `AgentRun`
/// default ordering. Rows come back `(run_id, pod_id)` in listed order.
pub fn active_runs_lock_sql() -> String {
    let statuses = guards::NON_TERMINAL_STATUSES
        .iter()
        .map(|status| format!("'{}'", status.value()))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT \"agent_run\".\"id\", \"agent_run\".\"pod_id\" FROM \"agent_run\" WHERE (\"agent_run\".\"runner_id\" = $1 AND \"agent_run\".\"status\" IN ({statuses})) ORDER BY \"agent_run\".\"created_at\" DESC FOR UPDATE"
    )
}

/// S4a — queued-run pod ids for the unpin (`:644-648`).
///
/// `$1` = runner. `'queued'` is the `AgentRunStatus::Queued` literal.
pub const PINNED_QUEUED_PODS_SQL: &str = "SELECT \"agent_run\".\"pod_id\" FROM \"agent_run\" WHERE (\"agent_run\".\"pinned_runner_id\" = $1 AND \"agent_run\".\"status\" = 'queued') ORDER BY \"agent_run\".\"created_at\" DESC";

/// S4b — unpin QUEUED follow-ups (`:650`), only when S4a is non-empty.
///
/// `$1` = runner.
pub const UNPIN_QUEUED_RUNS_SQL: &str = "UPDATE \"agent_run\" SET \"pinned_runner_id\" = NULL WHERE (\"agent_run\".\"pinned_runner_id\" = $1 AND \"agent_run\".\"status\" = 'queued')";

/// Per-run finalize target (`:631-640`): `AgentRunStatus.CANCELLED`.
pub const FINALIZE_TO_STATUS: &str = "cancelled";

/// Per-run finalize `updates["error"]` (`:635`).
pub const FINALIZE_ERROR: &str = "runner revoked";

/// Per-run finalize `updates["error_code"]` (`:636`).
pub const FINALIZE_ERROR_CODE: &str = "runner_revoked";

// ---------------------------------------------------------------------------
// Seam + driver
// ---------------------------------------------------------------------------

/// Storage failure for the [`RevokeStore`] calls.
///
/// Python lets any DB/Redis exception propagate out of `revoke` (rolling
/// back the atomic); the pool implementation stringifies it here and the
/// executor maps it to a 500. Provider calls that swallow in Python
/// (none on this path — finalize propagates) would return warnings.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RevokeError {
    /// Any store failure; the message is the executor's 500 detail.
    #[error("revoke store error: {0}")]
    Store(String),
}

/// Storage seam for the revoke cascade.
///
/// Methods mirror the Django ORM calls in `Runner.revoke`, one per
/// statement shape, plus the D-15 finalize step and the three
/// post-commit collectors. The SQL text for each lives in the adjacent
/// `*_SQL` consts, which the pool implementation must execute verbatim.
///
/// Native `async fn` in trait (stable since 1.75): no `async-trait`
/// dependency enters the lockfile for this seam (the `GitStore`
/// precedent).
#[allow(async_fn_in_trait)]
pub trait RevokeStore {
    /// S1: mark the runner revoked ([`MARK_RUNNER_REVOKED_SQL`]).
    async fn mark_runner_revoked(
        &mut self,
        runner_id: Uuid,
        now: DateTime<Utc>,
        stored_reason: &str,
    ) -> Result<(), RevokeError>;

    /// S2: revoke active sessions ([`REVOKE_ACTIVE_SESSIONS_SQL`]).
    async fn revoke_active_sessions(
        &mut self,
        runner_id: Uuid,
        now: DateTime<Utc>,
        stored_reason: &str,
    ) -> Result<(), RevokeError>;

    /// S3: locked non-terminal `(run_id, pod_id)` rows
    /// ([`active_runs_lock_sql`]), in listed order.
    async fn lock_active_runs(
        &mut self,
        runner_id: Uuid,
    ) -> Result<Vec<(Uuid, Option<Uuid>)>, RevokeError>;

    /// Per-run cancel (`:630-640`): `finalize_agent_run(run_id,
    /// CANCELLED, updates={error, error_code, cancel_reason:
    /// stored_reason}, expected_runner_id=runner_id)`.
    ///
    /// D-15 owns the row effects; the pool implementation delegates to
    /// the `services::runner_runs::finalization` builders on the
    /// caller's transaction (the nested atomic runs flat — no
    /// partial-rollback path, the 586 precedent):
    /// `lock_run_for_finalize_sql(with_runner=true)` first-writer-wins
    /// (a miss is `Ok(())` — Python ignores the `False`),
    /// `finalize_update_sql` with
    /// [`FINALIZE_ERROR`]/[`FINALIZE_ERROR_CODE`]/`cancel_reason`, the
    /// cloud-only terminal event, and — on success only — the
    /// terminal-effects publication captured into the executor's
    /// post-commit drain (`plan_publish_effects`; it registers before
    /// this driver's own collectors, as in Python). No `done_payload`
    /// merge: these updates carry none.
    async fn finalize_cancelled_run(
        &mut self,
        run_id: Uuid,
        runner_id: Uuid,
        stored_reason: &str,
    ) -> Result<(), RevokeError>;

    /// S4a: queued-run pod ids ([`PINNED_QUEUED_PODS_SQL`]), in row
    /// order (NULLs included; the driver filters).
    async fn pinned_queued_pod_ids(
        &mut self,
        runner_id: Uuid,
    ) -> Result<Vec<Option<Uuid>>, RevokeError>;

    /// S4b: unpin QUEUED follow-ups ([`UNPIN_QUEUED_RUNS_SQL`]). The
    /// driver calls this only when S4a is non-empty.
    async fn unpin_queued_runs(&mut self, runner_id: Uuid) -> Result<(), RevokeError>;

    /// Collect one project-move handoff for the post-commit drain
    /// (`:653-674`, one closure over all runs — the executor fires one
    /// call per collected id, in collection order). The executor runs
    /// D-12 `complete_project_move_handoff(run_id)` after commit,
    /// swallowing each failure after logging
    /// [`handoff_failure_log`].
    fn complete_handoff_after_commit(&mut self, run_id: Uuid);

    /// Collect one pod drain for the post-commit drain (`:675-676`).
    /// The executor runs the D-14 drain after commit; the return is
    /// ignored.
    fn drain_pod_after_commit(&mut self, pod_id: Uuid);

    /// Collect the stream-cleanup scheduling for the post-commit drain
    /// (`:681-685`, always). The executor calls D-14
    /// `schedule_stream_cleanup_for_runner` after commit.
    fn schedule_stream_cleanup_after_commit(&mut self, runner_id: Uuid);
}

/// The handoff-failure log line (`models.py:666-671`).
///
/// `%s` of a UUID is its canonical form, matching `Uuid`'s `Display`.
pub fn handoff_failure_log(runner_id: &Uuid, run_id: &Uuid) -> String {
    format!(
        "failed to complete project-move handoff after revoking runner {runner_id} (run {run_id})"
    )
}

/// Outcome of [`revoke_runner`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevokeOutcome {
    /// The truncated reason written to S1/S2 and the finalize updates.
    pub stored_reason: String,
    /// Reason warnings, in source order (unknown, then too-long).
    pub warnings: Vec<String>,
    /// The idempotent early return was taken (`revoked_at` was set):
    /// no store call ran.
    pub already_revoked: bool,
    /// The S1 `revoked_at`: the passed `now` on a fresh revoke, the
    /// pre-existing value on the early return. The executor refreshes
    /// its in-memory view from this (Python mutates `self`, `:604-606`).
    pub revoked_at: Option<DateTime<Utc>>,
    /// Affected run ids, in S3 row order.
    pub cancelled_run_ids: Vec<Uuid>,
    /// Affected pod ids, in drain-registration order.
    pub drained_pod_ids: Vec<Uuid>,
}

/// `Runner.revoke` (`models.py:542-685`).
///
/// `revoked_at` is the instance's pre-read `self.revoked_at` (Python is a
/// method on the fetched row — no extra read); `now` is the single
/// `timezone.now()` S1/S2 share (`:598`). The executor opens the
/// transaction, runs this, commits, then fires the collected effects in
/// registration order (handoffs → drains → cleanup).
pub async fn revoke_runner<S: RevokeStore>(
    store: &mut S,
    runner_id: Uuid,
    revoked_at: Option<DateTime<Utc>>,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<RevokeOutcome, RevokeError> {
    let (stored_reason, warnings) = check_revoke_reason(reason);
    if revoked_at.is_some() {
        return Ok(RevokeOutcome {
            stored_reason,
            warnings,
            already_revoked: true,
            revoked_at,
            cancelled_run_ids: Vec::new(),
            drained_pod_ids: Vec::new(),
        });
    }
    store
        .mark_runner_revoked(runner_id, now, &stored_reason)
        .await?;
    store
        .revoke_active_sessions(runner_id, now, &stored_reason)
        .await?;
    let active_runs = store.lock_active_runs(runner_id).await?;
    let mut cancelled_run_ids = Vec::with_capacity(active_runs.len());
    for (run_id, _) in &active_runs {
        store
            .finalize_cancelled_run(*run_id, runner_id, &stored_reason)
            .await?;
        cancelled_run_ids.push(*run_id);
    }
    let pinned_pod_ids = store.pinned_queued_pod_ids(runner_id).await?;
    if !pinned_pod_ids.is_empty() {
        store.unpin_queued_runs(runner_id).await?;
    }
    // `affected_pod_ids`: S3 pods then S4a pods, non-null, first-seen
    // order (the set-order quirk — see the module docs).
    let mut drained_pod_ids: Vec<Uuid> = Vec::new();
    for (_, pod_id) in &active_runs {
        if let Some(pod_id) = pod_id {
            if !drained_pod_ids.contains(pod_id) {
                drained_pod_ids.push(*pod_id);
            }
        }
    }
    for pod_id in pinned_pod_ids.iter().flatten() {
        if !drained_pod_ids.contains(pod_id) {
            drained_pod_ids.push(*pod_id);
        }
    }
    for run_id in &cancelled_run_ids {
        store.complete_handoff_after_commit(*run_id);
    }
    for pod_id in &drained_pod_ids {
        store.drain_pod_after_commit(*pod_id);
    }
    store.schedule_stream_cleanup_after_commit(runner_id);
    Ok(RevokeOutcome {
        stored_reason,
        warnings,
        already_revoked: false,
        revoked_at: Some(now),
        cancelled_run_ids,
        drained_pod_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// The F6 golden file, loaded verbatim (the sibling `purge.rs`
    /// `include_str!` precedent).
    fn flows_fixture() -> serde_json::Value {
        let text = include_str!("../../../../fixtures/runner_enroll/services/flows.golden.json");
        serde_json::from_str(text).expect("flows.golden.json parses")
    }

    fn f5_sql() -> &'static str {
        include_str!("../../../../fixtures/runner_enroll/queries/revoke_cascade.sql")
    }

    fn frozen_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 4, 7, 0, 0).unwrap()
    }

    // -- reason handling ----------------------------------------------------

    #[test]
    fn known_short_reasons_pass_through_silently() {
        for reason in KNOWN_REVOKE_REASONS {
            let (stored, warnings) = check_revoke_reason(reason);
            assert_eq!(stored, *reason, "reason {reason}");
            assert!(warnings.is_empty(), "reason {reason}");
        }
        assert_eq!(KNOWN_REVOKE_REASONS.len(), 7);
    }

    #[test]
    fn unknown_reason_warns_and_proceeds() {
        let (stored, warnings) = check_revoke_reason("evicted_by_admin");
        assert_eq!(stored, "evicted_by_admin");
        assert_eq!(
            warnings,
            vec![
                "Runner.revoke called with unknown reason 'evicted_by_admin'; downstream canonical-set match may misbehave. Add the reason to KNOWN_REVOKE_REASONS once intentional."
                    .to_string(),
            ]
        );
    }

    #[test]
    fn long_reason_warns_and_truncates_to_32_chars() {
        let reason = "x".repeat(33);
        let (stored, warnings) = check_revoke_reason(&reason);
        assert_eq!(stored, "x".repeat(32));
        assert_eq!(warnings.len(), 2, "unknown + too-long");
        assert_eq!(
            warnings[1],
            format!(
                "Runner.revoke reason '{}' exceeds 32 chars; truncating. The truncated form will not match the daemon's canonical-set synthesizer.",
                "x".repeat(33),
            )
        );
        // Boundary: exactly 32 chars is fine.
        let edge = "y".repeat(32);
        let (stored, warnings) = check_revoke_reason(&edge);
        assert_eq!(stored, edge);
        assert_eq!(warnings.len(), 1, "unknown only");
    }

    #[test]
    fn truncation_counts_code_points_not_bytes() {
        // 33 code points, 34 bytes: `chars().count()` trips the warning
        // and the stored form keeps 32 code points (33 bytes). A byte
        // slice would panic or miscount (Porting guide semantic trap).
        let reason = format!("{}é", "z".repeat(32));
        assert_eq!(reason.chars().count(), 33);
        let (stored, warnings) = check_revoke_reason(&reason);
        assert_eq!(stored.chars().count(), 32);
        assert_eq!(stored, "z".repeat(32));
        assert!(warnings.iter().any(|w| w.contains("exceeds 32 chars")));
        // A 32-code-point multibyte reason passes without truncation.
        let ok = format!("{}é", "z".repeat(31));
        let (stored, warnings) = check_revoke_reason(&ok);
        assert_eq!(stored, ok);
        assert_eq!(warnings.len(), 1, "unknown only");
    }

    #[test]
    fn unknown_warning_quotes_like_python_repr() {
        let (_, warnings) = check_revoke_reason("a'b");
        assert!(warnings[0].contains("\"a'b\""), "{}", warnings[0]);
        let (_, warnings) = check_revoke_reason("a\nb");
        assert!(warnings[0].contains("'a\\nb'"), "{}", warnings[0]);
    }

    #[test]
    fn reason_handling_matches_f6_prose() {
        let loaded_fixture = flows_fixture();
        let steps = &loaded_fixture["revoke_cascade_steps"];
        assert_eq!(steps["source"], "runner/models.py:542-685");
        let prose = steps["reason_handling"].as_str().expect("prose");
        assert!(prose.contains("logger.warning + proceed"), "{prose}");
        assert!(prose.contains("reason[:32]"), "{prose}");
    }

    // -- SQL text -----------------------------------------------------------

    #[test]
    fn s1_marks_revoked_without_touching_updated_at() {
        assert_eq!(
            MARK_RUNNER_REVOKED_SQL,
            "UPDATE \"runner\" SET \"status\" = 'revoked', \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE \"runner\".\"id\" = $3"
        );
        assert!(
            !MARK_RUNNER_REVOKED_SQL.contains("updated_at"),
            "QuerySet.update leaves updated_at alone (F5 S1 note)"
        );
        let sql = f5_sql();
        assert!(sql.contains("UPDATE \"runner\""), "F5 names the S1 table");
        assert!(sql.contains("updated_at UNCHANGED"), "F5 pins the note");
    }

    #[test]
    fn s2_where_order_is_q_sorted() {
        assert_eq!(
            REVOKE_ACTIVE_SESSIONS_SQL,
            "UPDATE \"runner_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE (\"runner_session\".\"revoked_at\" IS NULL AND \"runner_session\".\"runner_id\" = $3)"
        );
    }

    #[test]
    fn s3_renders_non_terminal_statuses_from_guards() {
        let values: Vec<&str> = guards::NON_TERMINAL_STATUSES
            .iter()
            .map(|status| status.value())
            .collect();
        assert_eq!(
            values,
            vec![
                "queued",
                "assigned",
                "waiting_for_worktree",
                "running",
                "cancel_requested",
                "awaiting_approval",
                "awaiting_reauth",
                "paused_awaiting_input",
            ]
        );
        assert_eq!(
            active_runs_lock_sql(),
            "SELECT \"agent_run\".\"id\", \"agent_run\".\"pod_id\" FROM \"agent_run\" WHERE (\"agent_run\".\"runner_id\" = $1 AND \"agent_run\".\"status\" IN ('queued', 'assigned', 'waiting_for_worktree', 'running', 'cancel_requested', 'awaiting_approval', 'awaiting_reauth', 'paused_awaiting_input')) ORDER BY \"agent_run\".\"created_at\" DESC FOR UPDATE"
        );
    }

    #[test]
    fn s4_select_and_conditional_update() {
        assert_eq!(
            PINNED_QUEUED_PODS_SQL,
            "SELECT \"agent_run\".\"pod_id\" FROM \"agent_run\" WHERE (\"agent_run\".\"pinned_runner_id\" = $1 AND \"agent_run\".\"status\" = 'queued') ORDER BY \"agent_run\".\"created_at\" DESC"
        );
        assert_eq!(
            UNPIN_QUEUED_RUNS_SQL,
            "UPDATE \"agent_run\" SET \"pinned_runner_id\" = NULL WHERE (\"agent_run\".\"pinned_runner_id\" = $1 AND \"agent_run\".\"status\" = 'queued')"
        );
    }

    #[test]
    fn finalize_literals_match_source() {
        use pidash_types::runner_runs::AgentRunStatus;
        assert_eq!(FINALIZE_TO_STATUS, AgentRunStatus::Cancelled.value());
        assert_eq!(FINALIZE_ERROR, "runner revoked");
        assert_eq!(FINALIZE_ERROR_CODE, "runner_revoked");
    }

    #[test]
    fn embedded_status_literals_match_their_owners() {
        use pidash_db::runner_enroll::columns::enums::RUNNER_STATUS_REVOKED;
        use pidash_types::runner_runs::AgentRunStatus;
        assert!(MARK_RUNNER_REVOKED_SQL.contains(&format!("'{RUNNER_STATUS_REVOKED}'")));
        assert_eq!(RUNNER_STATUS_REVOKED, "revoked");
        assert!(PINNED_QUEUED_PODS_SQL.contains(&format!("'{}'", AgentRunStatus::Queued.value())));
        assert!(UNPIN_QUEUED_RUNS_SQL.contains(&format!("'{}'", AgentRunStatus::Queued.value())));
    }

    #[test]
    fn handoff_failure_log_matches_source() {
        let runner = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let run = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        assert_eq!(
            handoff_failure_log(&runner, &run),
            "failed to complete project-move handoff after revoking runner 11111111-1111-1111-1111-111111111111 (run 22222222-2222-2222-2222-222222222222)"
        );
    }

    // -- driver order (fake store) ------------------------------------------

    #[derive(Debug, PartialEq)]
    enum Call {
        Mark(Uuid),
        Sessions(Uuid),
        LockRuns(Uuid),
        Finalize(Uuid, Uuid, String),
        PinnedSelect(Uuid),
        Unpin(Uuid),
        Handoff(Uuid),
        Drain(Uuid),
        Cleanup(Uuid),
    }

    struct Fake {
        calls: Vec<Call>,
        active_runs: Vec<(Uuid, Option<Uuid>)>,
        pinned_pods: Vec<Option<Uuid>>,
    }

    impl Fake {
        fn new(active_runs: Vec<(Uuid, Option<Uuid>)>, pinned_pods: Vec<Option<Uuid>>) -> Self {
            Self {
                calls: Vec::new(),
                active_runs,
                pinned_pods,
            }
        }
    }

    impl RevokeStore for Fake {
        async fn mark_runner_revoked(
            &mut self,
            runner_id: Uuid,
            _now: DateTime<Utc>,
            _stored_reason: &str,
        ) -> Result<(), RevokeError> {
            self.calls.push(Call::Mark(runner_id));
            Ok(())
        }

        async fn revoke_active_sessions(
            &mut self,
            runner_id: Uuid,
            _now: DateTime<Utc>,
            _stored_reason: &str,
        ) -> Result<(), RevokeError> {
            self.calls.push(Call::Sessions(runner_id));
            Ok(())
        }

        async fn lock_active_runs(
            &mut self,
            runner_id: Uuid,
        ) -> Result<Vec<(Uuid, Option<Uuid>)>, RevokeError> {
            self.calls.push(Call::LockRuns(runner_id));
            Ok(self.active_runs.clone())
        }

        async fn finalize_cancelled_run(
            &mut self,
            run_id: Uuid,
            runner_id: Uuid,
            stored_reason: &str,
        ) -> Result<(), RevokeError> {
            self.calls
                .push(Call::Finalize(run_id, runner_id, stored_reason.to_string()));
            Ok(())
        }

        async fn pinned_queued_pod_ids(
            &mut self,
            runner_id: Uuid,
        ) -> Result<Vec<Option<Uuid>>, RevokeError> {
            self.calls.push(Call::PinnedSelect(runner_id));
            Ok(self.pinned_pods.clone())
        }

        async fn unpin_queued_runs(&mut self, runner_id: Uuid) -> Result<(), RevokeError> {
            self.calls.push(Call::Unpin(runner_id));
            Ok(())
        }

        fn complete_handoff_after_commit(&mut self, run_id: Uuid) {
            self.calls.push(Call::Handoff(run_id));
        }

        fn drain_pod_after_commit(&mut self, pod_id: Uuid) {
            self.calls.push(Call::Drain(pod_id));
        }

        fn schedule_stream_cleanup_after_commit(&mut self, runner_id: Uuid) {
            self.calls.push(Call::Cleanup(runner_id));
        }
    }

    fn uuid(n: u8) -> Uuid {
        Uuid::parse_str(&format!("00000000-0000-0000-0000-0000000000{n:02x}")).unwrap()
    }

    #[tokio::test]
    async fn already_revoked_returns_before_any_store_call() {
        let mut store = Fake::new(vec![(uuid(1), Some(uuid(9)))], vec![Some(uuid(9))]);
        let outcome = revoke_runner(
            &mut store,
            uuid(7),
            Some(frozen_now()),
            "bogus_reason",
            frozen_now(),
        )
        .await
        .expect("revoke");
        assert!(outcome.already_revoked);
        assert!(store.calls.is_empty(), "no SQL on the no-op path");
        // Warnings still fire (QUIRK-warnings-before-noop).
        assert_eq!(outcome.warnings.len(), 1);
        assert_eq!(outcome.revoked_at, Some(frozen_now()));
        assert!(outcome.cancelled_run_ids.is_empty());
        assert!(outcome.drained_pod_ids.is_empty());

        let loaded_fixture = flows_fixture();
        let steps = &loaded_fixture["revoke_cascade_steps"];
        assert!(steps["no_op"]
            .as_str()
            .expect("prose")
            .contains("return BEFORE any SQL"));
    }

    #[tokio::test]
    async fn full_cascade_runs_s1_through_s4_then_hooks_in_order() {
        let run1 = uuid(1);
        let run2 = uuid(2);
        let pod_a = uuid(10);
        let pod_b = uuid(11);
        let mut store = Fake::new(
            vec![(run1, Some(pod_a)), (run2, None)],
            vec![Some(pod_b), Some(pod_a)],
        );
        let outcome = revoke_runner(&mut store, uuid(7), None, "manual_revoke", frozen_now())
            .await
            .expect("revoke");
        assert_eq!(
            store.calls,
            vec![
                Call::Mark(uuid(7)),
                Call::Sessions(uuid(7)),
                Call::LockRuns(uuid(7)),
                Call::Finalize(run1, uuid(7), "manual_revoke".to_string()),
                Call::Finalize(run2, uuid(7), "manual_revoke".to_string()),
                Call::PinnedSelect(uuid(7)),
                Call::Unpin(uuid(7)),
                Call::Handoff(run1),
                Call::Handoff(run2),
                Call::Drain(pod_a),
                Call::Drain(pod_b),
                Call::Cleanup(uuid(7)),
            ]
        );
        assert!(!outcome.already_revoked);
        assert_eq!(outcome.stored_reason, "manual_revoke");
        assert!(outcome.warnings.is_empty());
        assert_eq!(outcome.revoked_at, Some(frozen_now()));
        assert_eq!(outcome.cancelled_run_ids, vec![run1, run2]);
        // First-seen pod order: S3's pod_a before S4a's pod_b (deduped).
        assert_eq!(outcome.drained_pod_ids, vec![pod_a, pod_b]);

        let loaded_fixture = flows_fixture();
        let steps = &loaded_fixture["revoke_cascade_steps"]["post_commit_hooks"]
            .as_str()
            .expect("prose");
        assert!(steps.starts_with("handoffs"), "{steps}");
    }

    #[tokio::test]
    async fn empty_cascade_skips_finalize_and_unpin_but_always_schedules_cleanup() {
        let mut store = Fake::new(Vec::new(), Vec::new());
        let outcome = revoke_runner(&mut store, uuid(7), None, "user_revoke", frozen_now())
            .await
            .expect("revoke");
        assert_eq!(
            store.calls,
            vec![
                Call::Mark(uuid(7)),
                Call::Sessions(uuid(7)),
                Call::LockRuns(uuid(7)),
                Call::PinnedSelect(uuid(7)),
                Call::Cleanup(uuid(7)),
            ]
        );
        assert_eq!(outcome.stored_reason, "user_revoke");
        assert!(outcome.cancelled_run_ids.is_empty());
        assert!(outcome.drained_pod_ids.is_empty());
    }
}
