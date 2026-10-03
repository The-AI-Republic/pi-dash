#![forbid(unsafe_code)]

//! Approval expiry + runner/session sweeps (D-15 L6a, stage 5).
//!
//! Port of the five task bodies in `apps/api/pi_dash/runner/tasks.py:42-202`
//! (PIDASHCONV-539):
//!
//! * [`EXPIRE_TASK`] — `expire_stale_approvals` (`:42-97`).
//! * [`MARK_OFFLINE_TASK`] — `mark_offline_runners` (`:100-111`).
//! * [`SWEEP_IDLE_TASK`] — `sweep_idle_sessions` (`:117-138`).
//! * [`SWEEP_STALE_TASK`] — `sweep_stale_runners` (`:141-153`).
//! * [`SWEEP_STREAMS_TASK`] — `sweep_old_streams` (`:155-202`).
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-07-tasks.golden.json`
//! (FX-RUN-07); the `#[cfg(test)]` suite replays its sections. SQL text is
//! pinned byte-for-byte against Django 4.2.30 output (fixture capture plus
//! a query-compiler probe over the same venv; `%s` → `$N` positionally).
//!
//! # Layering: drivers over seams
//!
//! Each task is a [`drive_*`][drive_expire_stale_approvals] function over
//! three seams. [`SweepsRunsStore`] is the Postgres boundary with a live
//! `sqlx` implementation ([`LiveSweepsRunsStore`]) in this file.
//! [`SweepsRunsOutbox`] is the D-14 boundary (`pubsub.send_to_runner`,
//! `outbox.*`): D-14 is unsplit, so only fakes exist here and the
//! D-14 port supplies the live client later (the D-07
//! `tasks_mail::RedisLock` precedent: the trait records the exact call
//! shapes, the task layer supplies the client). [`InlineTerminalEffects`]
//! is the one L6b seam: the expiry path's inline `_publish_effects` half
//! runs L6b's `apply_agent_run_terminal_effects` executor, which does not
//! exist yet — the domain gate wires it when both halves land. The `.delay`
//! half is a real [`queue::enqueue`][crate::queue::enqueue] today (the
//! loop-scan fan-out precedent: wire-identical whichever plane serves it).
//!
//! Reused, not forked: L4 finalization planners (the expiry cancel runs
//! [`plan_finalize_values`][pidash_services::runner_runs::finalization::plan_finalize_values]
//! plus the lock/update/terminal-event SQL), the L4
//! [`TERMINAL_EFFECTS_TASK`][pidash_services::runner_runs::finalization::TERMINAL_EFFECTS_TASK]
//! wire name, L2 column lists (`agent_run`, `agent_run_approval`), L1
//! status enums, and the D-13 `RUNNER_STATUS_*` values.
//!
//! Beat entries stay Django-owned: `schedule.rs` already transcribes
//! `runner-expire-stale-approvals` and `runner-mark-offline-runners`
//! (pinned read-only by the tests); the other three tasks have no beat
//! entry (fixture `beat_not_scheduled`).
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * `mark_offline_runners` (90s grace) and `sweep_stale_runners` (50s
//!   threshold) are the same `UPDATE` with different cutoffs; only the
//!   90s one is beat-scheduled, so the 50s flip shadows it wherever both
//!   run. Both are ported; they share one store method, which makes the
//!   overlap structural.
//! * `runner_ids_with_sessions` keeps Django's `DISTINCT (runner_id,
//!   created_at)` pair quirk: `created_at` rides along so the `ORDER BY`
//!   survives `DISTINCT`, which can repeat a runner id; the driver
//!   dedupes through a set exactly like the source.
//! * The approval lock is a joined `FOR UPDATE` with no `OF`, so it locks
//!   the `agent_run` row too; the finalize lock later in the same
//!   transaction re-selects the already-held row (its miss arm is kept
//!   for shape but unreachable on this path).
//! * The finalize `atomic()` block nests inside the approval `atomic()`
//!   block (a savepoint); the port runs one flat transaction — the inner
//!   block never rolls back independently (its errors propagate and roll
//!   back the outer block identically), so the committed outcome is the
//!   same.
//! * `_publish_effects` (`.delay` + inline, each isolated) fires on commit,
//!   *before* the cancel frame, which is unisolated: an offline runner
//!   fails the whole expiry task with `expired` unincremented and the
//!   remaining approvals unprocessed.
//! * A lost finalize race still stamps `EXPIRED`: the outer transaction
//!   commits the stamp while skipping effects and the cancel frame.
//! * A `NULL` heartbeat flips offline: the `exclude()` renders
//!   `NOT (x >= $ AND x IS NOT NULL)`, which is true for `NULL`.
//! * Fixture artifact, *not* ported: FX-RUN-07 records reap keep-sets as
//!   `consumer:<sid>` (colon), but the real `outbox.consumer_name`
//!   renders `consumer-<sid>` (dash) at `Ported from` and today — the
//!   probe mocked the helper. This port renders the real dash format.
//!   Flagged in the PR for the domain gate.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_db::runner_enroll::columns::enums::{RUNNER_STATUS_OFFLINE, RUNNER_STATUS_ONLINE};
use pidash_db::runner_runs::{agent_run, approval};
use pidash_services::runner_runs::finalization::{
    finalize_error_code, finalize_update_sql, lock_run_for_finalize_sql, plan_finalize_values,
    plan_terminal_event, terminal_event_exists_sql, terminal_event_insert_sql,
    terminal_event_max_seq_sql, TERMINAL_EFFECTS_TASK,
};
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::{AgentRunStatus, ApprovalStatus, TERMINAL_RUN_STATUSES};

use crate::celery::CeleryTaskMessage;
use crate::queue::{self, JobRow, NewJob};
use crate::worker::{Handler, HandlerError, Registry, Verdict};

// ---------------------------------------------------------------------------
// Task names + beat names
// ---------------------------------------------------------------------------

/// `runner.expire_stale_approvals` (`tasks.py:42`).
pub const EXPIRE_TASK: &str = "runner.expire_stale_approvals";
/// `runner.mark_offline_runners` (`tasks.py:100`).
pub const MARK_OFFLINE_TASK: &str = "runner.mark_offline_runners";
/// `runner.sweep_idle_sessions` (`tasks.py:117`).
pub const SWEEP_IDLE_TASK: &str = "runner.sweep_idle_sessions";
/// `runner.sweep_stale_runners` (`tasks.py:141`).
pub const SWEEP_STALE_TASK: &str = "runner.sweep_stale_runners";
/// `runner.sweep_old_streams` (`tasks.py:155`).
pub const SWEEP_STREAMS_TASK: &str = "runner.sweep_old_streams";

/// Beat entry for [`EXPIRE_TASK`] (`celery.py`, every minute).
pub const EXPIRE_BEAT_NAME: &str = "runner-expire-stale-approvals";
/// Beat entry for [`MARK_OFFLINE_TASK`] (`celery.py`, every minute).
pub const MARK_OFFLINE_BEAT_NAME: &str = "runner-mark-offline-runners";

// ---------------------------------------------------------------------------
// Settings (keys, defaults, readers)
// ---------------------------------------------------------------------------

/// `HEARTBEAT_OFFLINE_GRACE` (`tasks.py:39`): fixed 90s, not a setting.
pub const HEARTBEAT_OFFLINE_GRACE_SECS: i64 = 90;
/// `LONG_POLL_INTERVAL_SECS` default (`settings/common.py`, fixture `settings`).
pub const DEFAULT_LONG_POLL_INTERVAL_SECS: i64 = 25;
/// `RUNNER_OFFLINE_THRESHOLD_SECS` default (fixture `settings`).
pub const DEFAULT_RUNNER_OFFLINE_THRESHOLD_SECS: i64 = 50;
/// `RUNNER_STREAM_MIN_RETENTION_SECS` default (fixture `settings`).
pub const DEFAULT_RUNNER_STREAM_MIN_RETENTION_SECS: i64 = 3600;

/// Env knob for the long-poll interval (same name as Django).
pub const LONG_POLL_INTERVAL_ENV: &str = "LONG_POLL_INTERVAL_SECS";
/// Env knob for the stale-runner threshold (same name as Django).
pub const RUNNER_OFFLINE_THRESHOLD_ENV: &str = "RUNNER_OFFLINE_THRESHOLD_SECS";
/// Env knob for the stream retention floor (same name as Django).
pub const RUNNER_STREAM_MIN_RETENTION_ENV: &str = "RUNNER_STREAM_MIN_RETENTION_SECS";

/// `int(getattr(settings, "LONG_POLL_INTERVAL_SECS", 25))`
/// (`tasks.py:125`). Unparseable input falls back to the default: Python
/// parses the same knob once at Django boot (where an invalid value
/// refuses to boot), while here the value is read per tick and the task
/// must never crash on it (the loop-scan settings precedent).
pub fn poll_secs_raw(raw: Option<&str>) -> i64 {
    raw.and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_LONG_POLL_INTERVAL_SECS)
}

/// `int(getattr(settings, "RUNNER_OFFLINE_THRESHOLD_SECS", 50))`
/// (`tasks.py:144`); same per-tick fallback as [`poll_secs_raw`].
pub fn offline_threshold_secs_raw(raw: Option<&str>) -> i64 {
    raw.and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_RUNNER_OFFLINE_THRESHOLD_SECS)
}

/// `int(getattr(settings, "RUNNER_STREAM_MIN_RETENTION_SECS", 3600))`
/// (`tasks.py:165`); same per-tick fallback as [`poll_secs_raw`].
pub fn stream_retention_secs_raw(raw: Option<&str>) -> i64 {
    raw.and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_RUNNER_STREAM_MIN_RETENTION_SECS)
}

/// Long-poll interval in seconds (`tasks.py:125`).
pub fn poll_secs() -> i64 {
    poll_secs_raw(std::env::var(LONG_POLL_INTERVAL_ENV).ok().as_deref())
}

/// Stale-runner threshold in seconds (`tasks.py:144`).
pub fn offline_threshold_secs() -> i64 {
    offline_threshold_secs_raw(std::env::var(RUNNER_OFFLINE_THRESHOLD_ENV).ok().as_deref())
}

/// Stream retention floor in seconds (`tasks.py:165`).
pub fn stream_retention_secs() -> i64 {
    stream_retention_secs_raw(
        std::env::var(RUNNER_STREAM_MIN_RETENTION_ENV)
            .ok()
            .as_deref(),
    )
}

/// Largest cutoff delta handed to `TimeDelta::seconds` (±285 years):
/// inside chrono's range and Postgres' `timestamptz` range from any
/// plausible `now`, so absurd configured knobs clamp instead of
/// panicking (`TimeDelta::seconds` panics out of bounds).
const MAX_CUTOFF_DELTA_SECS: i64 = 9_000_000_000;

fn clamp_delta_secs(secs: i64) -> i64 {
    secs.clamp(-MAX_CUTOFF_DELTA_SECS, MAX_CUTOFF_DELTA_SECS)
}

/// Offline cutoff (`tasks.py:103,144`): `now - grace`. Absurd input
/// clamps (see [`MAX_CUTOFF_DELTA_SECS`]); a `now` at chrono's own
/// edge falls back to `MIN_UTC` instead of panicking.
pub fn offline_cutoff(now: &DateTime<Utc>, grace_secs: i64) -> DateTime<Utc> {
    now.checked_sub_signed(Duration::seconds(clamp_delta_secs(grace_secs)))
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

/// Idle-session cutoff (`tasks.py:126`): `now - 2 * poll_secs`.
/// Saturating multiply plus the same clamp as [`offline_cutoff`].
pub fn idle_cutoff(now: &DateTime<Utc>, poll_secs: i64) -> DateTime<Utc> {
    now.checked_sub_signed(Duration::seconds(clamp_delta_secs(
        poll_secs.saturating_mul(2),
    )))
    .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

// ---------------------------------------------------------------------------
// SQL builders (Django 4.2.30 text, `%s` → `$N`)
// ---------------------------------------------------------------------------

/// Expired-pending approval ids (`tasks.py:47-49`).
/// Binds: `$1` now, `$2` `pending`.
pub fn pending_expired_approvals_select() -> String {
    "SELECT \"agent_run_approval\".\"id\" FROM \"agent_run_approval\" WHERE \
     (\"agent_run_approval\".\"expires_at\" < $1 AND \"agent_run_approval\".\"status\" = $2) \
     ORDER BY \"agent_run_approval\".\"requested_at\" DESC"
        .to_owned()
}

/// Approval lock + run fetch (`tasks.py:54-59`): `select_for_update()`
/// (no `OF`, so the joined `agent_run` row locks too) with
/// `select_related("agent_run")`, `filter(pk, status=pending).first()`.
/// Binds: `$1` approval id, `$2` `pending`.
pub fn lock_approval_for_expiry_select() -> String {
    let approval_cols = approval::COLUMNS
        .iter()
        .map(|c| format!("\"agent_run_approval\".\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let run_cols = agent_run::COLUMNS
        .iter()
        .map(|c| format!("\"agent_run\".\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {approval_cols}, {run_cols} FROM \"agent_run_approval\" INNER JOIN \"agent_run\" ON \
         (\"agent_run_approval\".\"agent_run_id\" = \"agent_run\".\"id\") WHERE \
         (\"agent_run_approval\".\"id\" = $1 AND \"agent_run_approval\".\"status\" = $2) \
         ORDER BY \"agent_run_approval\".\"requested_at\" DESC LIMIT 1 FOR UPDATE"
    )
}

/// `EXPIRED` stamp (`tasks.py:62-65`): `SET` in `update()` call order.
/// Binds: `$1` `expired`, `$2` now, `$3` approval id.
pub fn stamp_approval_expired_sql() -> String {
    "UPDATE \"agent_run_approval\" SET \"status\" = $1, \"decided_at\" = $2 WHERE \
     \"agent_run_approval\".\"id\" = $3"
        .to_owned()
}

/// Shared offline flip (`tasks.py:104-108`, `:145-149`): `UPDATE` runners
/// `ONLINE` and `NOT (last_heartbeat_at >= $ AND NOT NULL)` to `OFFLINE`.
/// Both tasks share this shape; only the threshold differs (the ported
/// overlap). Binds: `$1` `offline`, `$2` `online`, `$3` threshold.
pub fn mark_online_runners_offline_sql() -> String {
    "UPDATE \"runner\" SET \"status\" = $1 WHERE (\"runner\".\"status\" = $2 AND NOT \
     (\"runner\".\"last_heartbeat_at\" >= $3 AND \"runner\".\"last_heartbeat_at\" IS NOT NULL))"
        .to_owned()
}

/// Idle-session candidates (`tasks.py:127-129`): unrevoked rows with
/// `last_seen_at` past the cutoff, `(id, runner_id)` in `values_list`
/// order. Binds: `$1` threshold.
pub fn idle_sessions_select() -> String {
    "SELECT \"runner_session\".\"id\", \"runner_session\".\"runner_id\" FROM \"runner_session\" WHERE \
     (\"runner_session\".\"last_seen_at\" < $1 AND \"runner_session\".\"revoked_at\" IS NULL) \
     ORDER BY \"runner_session\".\"created_at\" DESC"
        .to_owned()
}

/// Idle-session revoke (`tasks.py:133`): `SET` in `update()` call order.
/// Binds: `$1` now, `$2` `idle_timeout`, `$3..` session ids.
/// The driver returns early on an empty set, so the live layer never
/// builds this with zero ids.
pub fn revoke_sessions_update(session_count: usize) -> String {
    let placeholders = (0..session_count)
        .map(|i| format!("${}", i + 3))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "UPDATE \"runner_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE \
         \"runner_session\".\"id\" IN ({placeholders})"
    )
}

/// Active `(runner_id, session_id)` pairs (`tasks.py:171`), in
/// `values_list` order. No binds.
pub fn active_session_pairs_select() -> String {
    "SELECT \"runner_session\".\"runner_id\", \"runner_session\".\"id\" FROM \"runner_session\" WHERE \
     \"runner_session\".\"revoked_at\" IS NULL ORDER BY \"runner_session\".\"created_at\" DESC"
        .to_owned()
}

/// Runner ids with sessions (`tasks.py:175`): `values_list("runner_id",
/// flat=True).distinct()`. Django adds `created_at` to the `DISTINCT`
/// list so the `ORDER BY` survives — the pair can repeat a runner id,
/// and the driver dedupes through a set exactly like the source. The
/// live layer reads column 0 only. No binds.
pub fn runner_ids_with_sessions_select() -> String {
    "SELECT DISTINCT \"runner_session\".\"runner_id\", \"runner_session\".\"created_at\" FROM \
     \"runner_session\" ORDER BY \"runner_session\".\"created_at\" DESC"
        .to_owned()
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Revoke reason the idle sweep stamps (`tasks.py:133`).
pub const REVOKED_REASON_IDLE: &str = "idle_timeout";
/// Cancel reason through the expiry path (`tasks.py:77-91`).
pub const CANCEL_REASON_APPROVAL_TIMEOUT: &str = "approval_timeout";
/// Expiry error detail (`tasks.py:78`).
pub const APPROVAL_TIMEOUT_ERROR: &str = "approval request expired";
/// Terminal-event kind (`finalize_agent_run:74`).
pub const TERMINAL_EVENT_KIND: &str = "terminal";

/// Consumer name for a session (`outbox.py:97-98`): `consumer-{sid}`.
/// D-14 owns the canonical port; this pure helper is pinned here so the
/// sweep drivers do not fork it (the L4 `RUNNER_COLUMNS` precedent).
pub fn consumer_name(session_id: &Uuid) -> String {
    format!("consumer-{session_id}")
}

/// Synthetic stream id for N seconds ago (`outbox.py:732-737`): ms part
/// is `now - secs * 1000`, floored at zero, seq part `0`. Saturating:
/// Python ints are unbounded, `i64` is not.
pub fn id_for_secs_ago_at(now_ms: i64, secs: i64) -> String {
    let ms = now_ms.saturating_sub(secs.saturating_mul(1000)).max(0);
    format!("{ms}-0")
}

/// [`id_for_secs_ago_at`] at the current wall-clock instant.
pub fn id_for_secs_ago(secs: i64) -> String {
    id_for_secs_ago_at(Utc::now().timestamp_millis(), secs)
}

/// Cancel control frame (`tasks.py:86-93`): `type` / `run_id` / `reason`.
/// The `mid` envelope is D-14's (`pubsub._ensure_envelope`), added by the
/// provider behind [`SweepsRunsOutbox::send_to_runner`].
pub fn cancel_control_message(run_id: &Uuid) -> Value {
    serde_json::json!({
        "type": "cancel",
        "run_id": run_id.to_string(),
        "reason": CANCEL_REASON_APPROVAL_TIMEOUT,
    })
}

/// `apply_agent_run_terminal_effects.delay(str(run_id))`
/// (`finalization.py:93`): `args=[str(id)]`, `kwargs={}` on the wire
/// (the loop-scan fan-out precedent).
pub fn terminal_effects_job(run_id: &Uuid) -> NewJob {
    NewJob::new(
        TERMINAL_EFFECTS_TASK,
        serde_json::json!([run_id.to_string()]),
        serde_json::json!({}),
    )
}

/// The Celery v2 message a fan-out row becomes on the wire: same task
/// name, `args=[str(id)]`, empty kwargs — byte-shape identical to the
/// Python `.delay(str(id))` call. Repeats
/// [`crate::worker::dispatch`]'s forward path arm for arm (that function
/// is the runtime source of truth; this one exists so tests can assert
/// the wire contract broker-free).
pub fn terminal_effects_message(run_id: &Uuid) -> CeleryTaskMessage {
    let (args, kwargs) = terminal_effects_job(run_id).into_message_parts();
    CeleryTaskMessage::new(TERMINAL_EFFECTS_TASK, args, kwargs)
}

// ---------------------------------------------------------------------------
// Errors + outcomes
// ---------------------------------------------------------------------------

/// Every failure the sweep drivers report. Plain text: a handler error
/// becomes the queue row's `last_error` through worker settlement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepsError(pub String);

impl std::fmt::Display for SweepsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SweepsError {}

impl From<sqlx::Error> for SweepsError {
    fn from(error: sqlx::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<SendOfflineError> for SweepsError {
    fn from(error: SendOfflineError) -> Self {
        Self(error.to_string())
    }
}

impl SweepsError {
    /// Logic-invariant failure (an L4 plan changed shape under us): loud,
    /// never silent.
    pub fn internal(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// `RunnerOfflineError` (`outbox.py:70-83`): a non-queueable control
/// message for a runner with no active session. Carries the runner id
/// and message type, like the source. This is the *only* failure the
/// [`SweepsRunsOutbox::send_to_runner`] seam reports: every other Redis
/// failure is swallowed inside `send_to_runner` (`pubsub.py:59-60`) by
/// the D-14 provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendOfflineError {
    pub runner_id: String,
    pub message_type: String,
}

impl std::fmt::Display for SendOfflineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "runner {} is offline; type '{}' cannot queue",
            self.runner_id, self.message_type
        )
    }
}

impl std::error::Error for SendOfflineError {}

/// What one approval's atomic expiry block did (`tasks.py:53-84`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpireOutcome {
    /// The lock found a still-pending row and stamped it `EXPIRED`.
    pub stamped: bool,
    /// The run finalized to `CANCELLED` (publish-effects target).
    pub finalized_run: Option<Uuid>,
    /// `(runner_id, run_id)` for the cancel frame (only when the run has
    /// a runner).
    pub cancel_send: Option<(Uuid, Uuid)>,
}

impl ExpireOutcome {
    /// The lock missed (decided or deleted concurrently): skip, uncounted
    /// (`tasks.py:60-61`, `continue`).
    pub fn skipped() -> Self {
        Self {
            stamped: false,
            finalized_run: None,
            cancel_send: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Seams
// ---------------------------------------------------------------------------

/// Store boundary for the five sweeps: one method per SQL statement the
/// drivers run, with [`expire_approval_atomic`][Self::expire_approval_atomic]
/// owning the whole approval transaction (lock → stamp → maybe-finalize →
/// maybe-terminal-event, committed together).
pub trait SweepsRunsStore: Send + Sync {
    /// Expired-pending approval ids, `ORDER BY requested_at DESC`
    /// (`tasks.py:47-49`).
    fn pending_expired_approval_ids(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<Uuid>, SweepsError>> + Send;
    /// One approval's atomic block (`tasks.py:53-84`): lock the
    /// still-pending row (joined run fetch), stamp `EXPIRED`, and when
    /// the run is awaiting-approval/running finalize it to `CANCELLED`
    /// (L4 planners, cloud-only terminal event), all in one transaction.
    fn expire_approval_atomic(
        &self,
        approval_id: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<ExpireOutcome, SweepsError>> + Send;
    /// `apply_agent_run_terminal_effects.delay(str(run_id))`
    /// (`finalization.py:93`): the first `_publish_effects` half, after
    /// the finalize transaction commits (transaction `on_commit`).
    fn emit_terminal_effects(
        &self,
        run_id: Uuid,
    ) -> impl Future<Output = Result<(), SweepsError>> + Send;
    /// Shared offline flip for both runner tasks (`tasks.py:104-108`,
    /// `:145-149`): `ONLINE` rows past `threshold` to `OFFLINE`.
    /// Returns the affected count.
    fn mark_online_runners_offline(
        &self,
        threshold: DateTime<Utc>,
    ) -> impl Future<Output = Result<u64, SweepsError>> + Send;
    /// Idle-session candidates (`tasks.py:127-129`): `(id, runner_id)` in
    /// `values_list` order.
    fn idle_sessions(
        &self,
        threshold: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<(Uuid, Uuid)>, SweepsError>> + Send;
    /// Revoke sessions with `idle_timeout` (`tasks.py:133`).
    fn revoke_sessions(
        &self,
        session_ids: &[Uuid],
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<u64, SweepsError>> + Send;
    /// Active `(runner_id, session_id)` pairs (`tasks.py:171`).
    fn active_session_pairs(
        &self,
    ) -> impl Future<Output = Result<Vec<(Uuid, Uuid)>, SweepsError>> + Send;
    /// Runner ids with sessions, first column of the `DISTINCT` pair
    /// (`tasks.py:175`); may repeat ids (the pair quirk) — the driver
    /// dedupes.
    fn runner_ids_with_sessions(
        &self,
    ) -> impl Future<Output = Result<Vec<Uuid>, SweepsError>> + Send;
}

/// D-14 outbox boundary for the sweeps (`pubsub.send_to_runner`,
/// `outbox.*`). Each method records the exact call shape the task makes;
/// the D-14 port supplies the live Redis client later. Error
/// propagation is per call site, exactly as the source isolates (or
/// does not isolate) each call.
pub trait SweepsRunsOutbox: Send + Sync {
    /// `send_to_runner(runner_id, message)` (`pubsub.py:45-60`): the
    /// expiry cancel frame. Raises only the offline error; the provider
    /// swallows every other failure, like the source.
    fn send_to_runner(
        &self,
        runner_id: Uuid,
        message: Value,
    ) -> impl Future<Output = Result<(), SendOfflineError>> + Send;
    /// `clear_session_marker(sid)` (`outbox.py:444-449`).
    fn clear_session_marker(
        &self,
        session_id: Uuid,
    ) -> impl Future<Output = Result<(), SweepsError>> + Send;
    /// `publish_session_eviction(runner, old, new)` (`outbox.py:544-556`).
    fn publish_session_eviction(
        &self,
        runner_id: Uuid,
        old_session_id: Uuid,
        new_session_id: &str,
    ) -> impl Future<Output = Result<(), SweepsError>> + Send;
    /// `safe_trim_runner_stream(rid, time_cutoff_id=...)`
    /// (`outbox.py:609-681`): removed-entry count, or `None` when the
    /// trim was skipped.
    fn safe_trim_runner_stream(
        &self,
        runner_id: Uuid,
        time_cutoff_id: &str,
    ) -> impl Future<Output = Result<Option<i64>, SweepsError>> + Send;
    /// `reap_idle_consumers(rid, keep_consumers=...)`
    /// (`outbox.py:382-425`): removed-consumer count.
    fn reap_idle_consumers(
        &self,
        runner_id: Uuid,
        keep_consumers: &HashSet<String>,
    ) -> impl Future<Output = Result<i64, SweepsError>> + Send;
    /// `due_runners_for_stream_cleanup()` (`outbox.py:581-591`): runner
    /// ids whose cleanup marker is due, as stored (strings).
    fn due_runners_for_stream_cleanup(
        &self,
    ) -> impl Future<Output = Result<Vec<String>, SweepsError>> + Send;
    /// `delete_runner_stream(rid)` (`outbox.py:601-606`).
    fn delete_runner_stream(
        &self,
        runner_id: &str,
    ) -> impl Future<Output = Result<(), SweepsError>> + Send;
    /// `remove_stream_cleanup_marker(rid)` (`outbox.py:594-598`).
    fn remove_stream_cleanup_marker(
        &self,
        runner_id: &str,
    ) -> impl Future<Output = Result<(), SweepsError>> + Send;
}

/// Inline terminal-effects boundary: the second `_publish_effects` half
/// (`finalization.py:99-102`), executed by L6b's
/// `apply_agent_run_terminal_effects` port (PIDASHCONV-540), which the
/// domain gate wires in when both halves land.
pub trait InlineTerminalEffects: Send + Sync {
    /// `apply_terminal_effects(run_id)`: hooks once, capacity
    /// at-least-once. Returns the source's boolean.
    fn apply_inline(&self, run_id: Uuid) -> impl Future<Output = Result<bool, SweepsError>> + Send;
}

// ---------------------------------------------------------------------------
// Drivers
// ---------------------------------------------------------------------------

/// `expire_stale_approvals` (`tasks.py:42-97`): stamp every expired
/// `PENDING` approval `EXPIRED`, finalize awaiting/running runs to
/// `CANCELLED`, publish effects (isolated, on commit), send the cancel
/// frame (unisolated — an offline runner fails the task), and return
/// the stamped count. A lock miss skips, uncounted.
pub async fn drive_expire_stale_approvals<S, O, E>(
    store: &S,
    outbox: &O,
    effects: &E,
    now: &DateTime<Utc>,
) -> Result<usize, SweepsError>
where
    S: SweepsRunsStore,
    O: SweepsRunsOutbox,
    E: InlineTerminalEffects,
{
    let pending_ids = store.pending_expired_approval_ids(*now).await?;
    let mut expired = 0usize;
    for approval_id in pending_ids {
        let outcome = store.expire_approval_atomic(approval_id, *now).await?;
        if !outcome.stamped {
            continue;
        }
        if let Some(run_id) = outcome.finalized_run {
            // `_publish_effects` (`finalization.py:89-103`): the Celery
            // emit first, then the inline run — each isolated, in order.
            if let Err(error) = store.emit_terminal_effects(run_id).await {
                tracing::error!(
                    %error,
                    run_id = %run_id,
                    "failed to publish terminal effects for run"
                );
            }
            if let Err(error) = effects.apply_inline(run_id).await {
                tracing::error!(
                    %error,
                    run_id = %run_id,
                    "failed to apply terminal effects for run"
                );
            }
        }
        if let Some((runner_id, run_id)) = outcome.cancel_send {
            outbox
                .send_to_runner(runner_id, cancel_control_message(&run_id))
                .await?;
        }
        expired += 1;
    }
    if expired > 0 {
        tracing::info!("expired {expired} stale approval(s)");
    }
    Ok(expired)
}

/// `mark_offline_runners` (`tasks.py:100-111`): one `UPDATE` flipping
/// `ONLINE` runners with no heartbeat in
/// [`HEARTBEAT_OFFLINE_GRACE_SECS`] to `OFFLINE`. Returns the affected
/// count.
pub async fn drive_mark_offline_runners<S>(
    store: &S,
    now: &DateTime<Utc>,
) -> Result<u64, SweepsError>
where
    S: SweepsRunsStore,
{
    let threshold = offline_cutoff(now, HEARTBEAT_OFFLINE_GRACE_SECS);
    let affected = store.mark_online_runners_offline(threshold).await?;
    if affected > 0 {
        tracing::info!("marked {affected} runner(s) offline via heartbeat timeout");
    }
    Ok(affected)
}

/// `sweep_idle_sessions` (`tasks.py:117-138`): revoke unrevoked sessions
/// idle past `2 * poll_secs` with `idle_timeout`, one outbox clear +
/// eviction per row (unisolated), and return the evicted count. Empty
/// means an immediate `0`: no `UPDATE`, no outbox calls.
pub async fn drive_sweep_idle_sessions<S, O>(
    store: &S,
    outbox: &O,
    now: &DateTime<Utc>,
    poll_secs: i64,
) -> Result<usize, SweepsError>
where
    S: SweepsRunsStore,
    O: SweepsRunsOutbox,
{
    let threshold = idle_cutoff(now, poll_secs);
    let sessions = store.idle_sessions(threshold).await?;
    if sessions.is_empty() {
        return Ok(0);
    }
    let sids: Vec<Uuid> = sessions.iter().map(|(sid, _)| *sid).collect();
    store.revoke_sessions(&sids, *now).await?;
    for (sid, runner_id) in &sessions {
        outbox.clear_session_marker(*sid).await?;
        outbox
            .publish_session_eviction(*runner_id, *sid, "")
            .await?;
    }
    tracing::info!("sweep_idle_sessions evicted {} session(s)", sessions.len());
    Ok(sessions.len())
}

/// `sweep_stale_runners` (`tasks.py:141-153`): the same flip as
/// [`drive_mark_offline_runners`] with the configured threshold
/// (default 50s). Returns the affected count.
pub async fn drive_sweep_stale_runners<S>(
    store: &S,
    now: &DateTime<Utc>,
    threshold_secs: i64,
) -> Result<u64, SweepsError>
where
    S: SweepsRunsStore,
{
    let threshold = offline_cutoff(now, threshold_secs);
    let affected = store.mark_online_runners_offline(threshold).await?;
    if affected > 0 {
        tracing::info!("sweep_stale_runners flipped {affected} offline");
    }
    Ok(affected)
}

/// `sweep_old_streams` (`tasks.py:155-202`): per runner with sessions,
/// trim (only when an active consumer exists) and reap idle consumers
/// (always), each isolated per runner; then delete the streams due for
/// cleanup (delete + unmark isolated per runner). Returns the trimmed
/// count only — reaped consumers are only logged.
pub async fn drive_sweep_old_streams<S, O>(
    store: &S,
    outbox: &O,
    now: &DateTime<Utc>,
    retention_secs: i64,
) -> Result<i64, SweepsError>
where
    S: SweepsRunsStore,
    O: SweepsRunsOutbox,
{
    let time_cutoff_id = id_for_secs_ago_at(now.timestamp_millis(), retention_secs);
    let mut active_consumers_by_runner: HashMap<Uuid, HashSet<String>> = HashMap::new();
    for (runner_id, session_id) in store.active_session_pairs().await? {
        active_consumers_by_runner
            .entry(runner_id)
            .or_default()
            .insert(consumer_name(&session_id));
    }
    // `set(...)` (`tasks.py:175`): dedupe the `DISTINCT`-pair rows;
    // iteration order is nondeterministic, like the source's set.
    let mut runner_ids = store.runner_ids_with_sessions().await?;
    {
        let mut seen = HashSet::new();
        runner_ids.retain(|rid| seen.insert(*rid));
    }
    let mut trimmed_count = 0i64;
    let mut reaped_consumers = 0i64;
    for rid in runner_ids {
        let keep_consumers = active_consumers_by_runner
            .get(&rid)
            .cloned()
            .unwrap_or_default();
        if !keep_consumers.is_empty() {
            match outbox.safe_trim_runner_stream(rid, &time_cutoff_id).await {
                Ok(Some(removed)) if removed != 0 => trimmed_count += removed,
                Ok(_) => {}
                Err(error) => {
                    tracing::error!(%error, runner_id = %rid, "safe_trim_runner_stream failed");
                }
            }
        }
        match outbox.reap_idle_consumers(rid, &keep_consumers).await {
            Ok(reaped) => reaped_consumers += reaped,
            Err(error) => {
                tracing::error!(%error, runner_id = %rid, "reap_idle_consumers failed");
            }
        }
    }
    for rid in outbox.due_runners_for_stream_cleanup().await? {
        let cleanup = async {
            outbox.delete_runner_stream(&rid).await?;
            outbox.remove_stream_cleanup_marker(&rid).await
        }
        .await;
        if let Err(error) = cleanup {
            tracing::error!(%error, runner_id = %rid, "delete_runner_stream failed");
        }
    }
    if reaped_consumers > 0 {
        tracing::info!("sweep_old_streams reaped {reaped_consumers} idle consumer(s)");
    }
    Ok(trimmed_count)
}

// ---------------------------------------------------------------------------
// Live store
// ---------------------------------------------------------------------------

/// Live [`SweepsRunsStore`]: the SQL builders above over a `sqlx` pool.
pub struct LiveSweepsRunsStore {
    pool: PgPool,
}

impl LiveSweepsRunsStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Expected `SET` column order for the expiry finalize: the five L4 base
/// clauses plus the three extras in `tasks.py:76-80` order. The live
/// bind below is positional; a mismatch fails loudly instead of binding
/// wrong. No `done_payload` clause, so the merge step is a no-op here.
const EXPIRY_FINALIZE_COLUMNS: [&str; 8] = [
    "status",
    "ended_at",
    "queue_position",
    "terminal_hooks_applied_at",
    "terminal_capacity_released_at",
    "error_code",
    "error",
    "cancel_reason",
];

impl SweepsRunsStore for LiveSweepsRunsStore {
    async fn pending_expired_approval_ids(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<Uuid>, SweepsError> {
        sqlx::query_scalar::<_, Uuid>(&pending_expired_approvals_select())
            .bind(now)
            .bind(ApprovalStatus::Pending.value())
            .fetch_all(&self.pool)
            .await
            .map_err(SweepsError::from)
    }

    async fn expire_approval_atomic(
        &self,
        approval_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<ExpireOutcome, SweepsError> {
        let mut tx = self.pool.begin().await?;
        let locked = sqlx::query(&lock_approval_for_expiry_select())
            .bind(approval_id)
            .bind(ApprovalStatus::Pending.value())
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = locked else {
            tx.rollback().await?;
            return Ok(ExpireOutcome::skipped());
        };
        // Positional decode: both tables carry `id` and `status`, so a
        // by-name lookup would read the approval's columns.
        let run_col = |name: &str| {
            agent_run::COLUMNS
                .iter()
                .position(|c| *c == name)
                .map(|i| approval::COLUMNS.len() + i)
                .ok_or_else(|| SweepsError::internal(format!("agent_run has no column {name}")))
        };
        let run_id: Uuid = row.try_get(run_col("id")?)?;
        let run_status: String = row.try_get(run_col("status")?)?;
        let run_runner_id: Option<Uuid> = row.try_get(run_col("runner_id")?)?;
        sqlx::query(&stamp_approval_expired_sql())
            .bind(ApprovalStatus::Expired.value())
            .bind(now)
            .bind(approval_id)
            .execute(&mut *tx)
            .await?;
        let cancellable = matches!(
            AgentRunStatus::from_value(&run_status),
            Some(AgentRunStatus::AwaitingApproval | AgentRunStatus::Running)
        );
        if !cancellable {
            tx.commit().await?;
            return Ok(ExpireOutcome {
                stamped: true,
                finalized_run: None,
                cancel_send: None,
            });
        }
        // `finalize_agent_run(run, CANCELLED, updates, expected_runner_id)`
        // (`finalization.py:48-86`) via the L4 planners. The nested
        // `atomic()` is a savepoint elided into this transaction (see the
        // module docs); the lock's miss arm is kept for shape although
        // the joined approval lock above already holds this row.
        let values = plan_finalize_values(
            AgentRunStatus::Cancelled,
            &[
                (
                    "error_code",
                    pidash_services::runner_runs::SetValue::Text(
                        CANCEL_REASON_APPROVAL_TIMEOUT.to_owned(),
                    ),
                ),
                (
                    "error",
                    pidash_services::runner_runs::SetValue::Text(APPROVAL_TIMEOUT_ERROR.to_owned()),
                ),
                (
                    "cancel_reason",
                    pidash_services::runner_runs::SetValue::Text(
                        CANCEL_REASON_APPROVAL_TIMEOUT.to_owned(),
                    ),
                ),
            ],
        )
        .map_err(|error| SweepsError::internal(error.to_string()))?;
        let columns: Vec<&str> = values.clauses.iter().map(|c| c.column).collect();
        if columns.as_slice() != EXPIRY_FINALIZE_COLUMNS {
            return Err(SweepsError::internal(format!(
                "expiry finalize SET order drifted: {columns:?}"
            )));
        }
        let with_runner = run_runner_id.is_some();
        let lock_sql = lock_run_for_finalize_sql(with_runner, false);
        let mut lock = sqlx::query(&lock_sql).bind(run_id);
        for status in TERMINAL_RUN_STATUSES {
            lock = lock.bind(status.value());
        }
        if let Some(runner_id) = run_runner_id {
            lock = lock.bind(runner_id);
        }
        let finalized_row = lock.fetch_optional(&mut *tx).await?;
        let Some(finalized_row) = finalized_row else {
            tx.commit().await?;
            return Ok(ExpireOutcome {
                stamped: true,
                finalized_run: None,
                cancel_send: None,
            });
        };
        sqlx::query(&finalize_update_sql(&values))
            .bind(AgentRunStatus::Cancelled.value())
            .bind(now)
            .bind(None::<i16>)
            .bind(None::<DateTime<Utc>>)
            .bind(None::<DateTime<Utc>>)
            .bind(CANCEL_REASON_APPROVAL_TIMEOUT)
            .bind(APPROVAL_TIMEOUT_ERROR)
            .bind(CANCEL_REASON_APPROVAL_TIMEOUT)
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        let executor_kind: String = finalized_row.try_get("executor_kind")?;
        if AgentExecutorKind::from_value(&executor_kind) == Some(AgentExecutorKind::CloudAgent) {
            let exists = sqlx::query_scalar::<_, i32>(terminal_event_exists_sql())
                .bind(run_id)
                .bind(TERMINAL_EVENT_KIND)
                .fetch_optional(&mut *tx)
                .await?;
            if exists.is_none() {
                let max_seq = sqlx::query_scalar::<_, i32>(terminal_event_max_seq_sql())
                    .bind(run_id)
                    .fetch_optional(&mut *tx)
                    .await?;
                let plan = plan_terminal_event(
                    max_seq,
                    AgentRunStatus::Cancelled,
                    &finalize_error_code(&values),
                );
                sqlx::query(&terminal_event_insert_sql())
                    .bind(run_id)
                    .bind(plan.seq)
                    .bind(TERMINAL_EVENT_KIND)
                    .bind(plan.payload)
                    .bind(now)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(ExpireOutcome {
            stamped: true,
            finalized_run: Some(run_id),
            cancel_send: run_runner_id.map(|runner_id| (runner_id, run_id)),
        })
    }

    async fn emit_terminal_effects(&self, run_id: Uuid) -> Result<(), SweepsError> {
        queue::enqueue(&self.pool, &terminal_effects_job(&run_id)).await?;
        Ok(())
    }

    async fn mark_online_runners_offline(
        &self,
        threshold: DateTime<Utc>,
    ) -> Result<u64, SweepsError> {
        let result = sqlx::query(&mark_online_runners_offline_sql())
            .bind(RUNNER_STATUS_OFFLINE)
            .bind(RUNNER_STATUS_ONLINE)
            .bind(threshold)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    async fn idle_sessions(
        &self,
        threshold: DateTime<Utc>,
    ) -> Result<Vec<(Uuid, Uuid)>, SweepsError> {
        sqlx::query_as::<_, (Uuid, Uuid)>(&idle_sessions_select())
            .bind(threshold)
            .fetch_all(&self.pool)
            .await
            .map_err(SweepsError::from)
    }

    async fn revoke_sessions(
        &self,
        session_ids: &[Uuid],
        now: DateTime<Utc>,
    ) -> Result<u64, SweepsError> {
        if session_ids.is_empty() {
            return Ok(0);
        }
        let revoke_sql = revoke_sessions_update(session_ids.len());
        let mut revoke = sqlx::query(&revoke_sql).bind(now).bind(REVOKED_REASON_IDLE);
        for sid in session_ids {
            revoke = revoke.bind(*sid);
        }
        let result = revoke.execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    async fn active_session_pairs(&self) -> Result<Vec<(Uuid, Uuid)>, SweepsError> {
        sqlx::query_as::<_, (Uuid, Uuid)>(&active_session_pairs_select())
            .fetch_all(&self.pool)
            .await
            .map_err(SweepsError::from)
    }

    async fn runner_ids_with_sessions(&self) -> Result<Vec<Uuid>, SweepsError> {
        let rows = sqlx::query_as::<_, (Uuid, DateTime<Utc>)>(&runner_ids_with_sessions_select())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|(rid, _)| rid).collect())
    }
}

// ---------------------------------------------------------------------------
// Handlers + registration
// ---------------------------------------------------------------------------

fn expire_handler<O, E>(pool: PgPool, outbox: Arc<O>, effects: Arc<E>) -> Handler
where
    O: SweepsRunsOutbox + 'static,
    E: InlineTerminalEffects + 'static,
{
    let store = Arc::new(LiveSweepsRunsStore::new(pool));
    Arc::new(move |_job: JobRow| {
        let store = store.clone();
        let outbox = outbox.clone();
        let effects = effects.clone();
        let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
            Box::pin(async move {
                let now = Utc::now();
                drive_expire_stale_approvals(
                    store.as_ref(),
                    outbox.as_ref(),
                    effects.as_ref(),
                    &now,
                )
                .await
                .map(|_| Verdict::Ack)
                .map_err(|error| error.to_string())
            });
        fut
    })
}

fn mark_offline_handler(pool: PgPool) -> Handler {
    let store = Arc::new(LiveSweepsRunsStore::new(pool));
    Arc::new(move |_job: JobRow| {
        let store = store.clone();
        let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
            Box::pin(async move {
                let now = Utc::now();
                drive_mark_offline_runners(store.as_ref(), &now)
                    .await
                    .map(|_| Verdict::Ack)
                    .map_err(|error| error.to_string())
            });
        fut
    })
}

fn sweep_idle_handler<O>(pool: PgPool, outbox: Arc<O>) -> Handler
where
    O: SweepsRunsOutbox + 'static,
{
    let store = Arc::new(LiveSweepsRunsStore::new(pool));
    Arc::new(move |_job: JobRow| {
        let store = store.clone();
        let outbox = outbox.clone();
        let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
            Box::pin(async move {
                let now = Utc::now();
                drive_sweep_idle_sessions(store.as_ref(), outbox.as_ref(), &now, poll_secs())
                    .await
                    .map(|_| Verdict::Ack)
                    .map_err(|error| error.to_string())
            });
        fut
    })
}

fn sweep_stale_handler(pool: PgPool) -> Handler {
    let store = Arc::new(LiveSweepsRunsStore::new(pool));
    Arc::new(move |_job: JobRow| {
        let store = store.clone();
        let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
            Box::pin(async move {
                let now = Utc::now();
                drive_sweep_stale_runners(store.as_ref(), &now, offline_threshold_secs())
                    .await
                    .map(|_| Verdict::Ack)
                    .map_err(|error| error.to_string())
            });
        fut
    })
}

fn sweep_streams_handler<O>(pool: PgPool, outbox: Arc<O>) -> Handler
where
    O: SweepsRunsOutbox + 'static,
{
    let store = Arc::new(LiveSweepsRunsStore::new(pool));
    Arc::new(move |_job: JobRow| {
        let store = store.clone();
        let outbox = outbox.clone();
        let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
            Box::pin(async move {
                let now = Utc::now();
                drive_sweep_old_streams(
                    store.as_ref(),
                    outbox.as_ref(),
                    &now,
                    stream_retention_secs(),
                )
                .await
                .map(|_| Verdict::Ack)
                .map_err(|error| error.to_string())
            });
        fut
    })
}

/// Register the five L6a task handlers.
///
/// The pool feeds the live store; `outbox` is the D-14 client and
/// `effects` is L6b's inline executor, both supplied by the binary when
/// their providers land (the assistant `register_assistant_tasks`
/// precedent). Like every other port, this only builds the handler
/// table — flipping the live worker to it is the domain gate's call
/// (PIDASHCONV-544, after the PIDASHCONV-23 proxy pass), so every name
/// still routes to `PythonOwned` (see [`crate::worker::route_for`]).
pub fn register_sweeps_runs_tasks<O, E>(
    registry: &mut Registry,
    pool: PgPool,
    outbox: Arc<O>,
    effects: Arc<E>,
) where
    O: SweepsRunsOutbox + 'static,
    E: InlineTerminalEffects + 'static,
{
    registry.register(
        EXPIRE_TASK,
        expire_handler(pool.clone(), outbox.clone(), effects.clone()),
    );
    registry.register(MARK_OFFLINE_TASK, mark_offline_handler(pool.clone()));
    registry.register(
        SWEEP_IDLE_TASK,
        sweep_idle_handler(pool.clone(), outbox.clone()),
    );
    registry.register(SWEEP_STALE_TASK, sweep_stale_handler(pool.clone()));
    registry.register(SWEEP_STREAMS_TASK, sweep_streams_handler(pool, outbox));
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;
    use std::sync::Mutex;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-07-tasks.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    // ------------------------------------------------------------------
    // Fakes
    // ------------------------------------------------------------------

    #[derive(Default)]
    struct FakeStore {
        pending: Vec<Uuid>,
        pending_err: Option<String>,
        outcomes: HashMap<Uuid, ExpireOutcome>,
        expire_calls: Mutex<Vec<Uuid>>,
        emitted: Mutex<Vec<Uuid>>,
        emit_err_runs: HashSet<Uuid>,
        offline_thresholds: Mutex<Vec<DateTime<Utc>>>,
        offline_affected: u64,
        idle: Vec<(Uuid, Uuid)>,
        revoked: Mutex<Vec<Vec<Uuid>>>,
        active_pairs: Vec<(Uuid, Uuid)>,
        rids_with_sessions: Vec<Uuid>,
    }

    impl SweepsRunsStore for FakeStore {
        async fn pending_expired_approval_ids(
            &self,
            _now: DateTime<Utc>,
        ) -> Result<Vec<Uuid>, SweepsError> {
            if let Some(err) = &self.pending_err {
                return Err(SweepsError(err.clone()));
            }
            Ok(self.pending.clone())
        }
        async fn expire_approval_atomic(
            &self,
            approval_id: Uuid,
            _now: DateTime<Utc>,
        ) -> Result<ExpireOutcome, SweepsError> {
            self.expire_calls.lock().unwrap().push(approval_id);
            Ok(self
                .outcomes
                .get(&approval_id)
                .copied()
                .unwrap_or_else(ExpireOutcome::skipped))
        }
        async fn emit_terminal_effects(&self, run_id: Uuid) -> Result<(), SweepsError> {
            if self.emit_err_runs.contains(&run_id) {
                return Err(SweepsError("emit boom".to_owned()));
            }
            self.emitted.lock().unwrap().push(run_id);
            Ok(())
        }
        async fn mark_online_runners_offline(
            &self,
            threshold: DateTime<Utc>,
        ) -> Result<u64, SweepsError> {
            self.offline_thresholds.lock().unwrap().push(threshold);
            Ok(self.offline_affected)
        }
        async fn idle_sessions(
            &self,
            _threshold: DateTime<Utc>,
        ) -> Result<Vec<(Uuid, Uuid)>, SweepsError> {
            Ok(self.idle.clone())
        }
        async fn revoke_sessions(
            &self,
            session_ids: &[Uuid],
            _now: DateTime<Utc>,
        ) -> Result<u64, SweepsError> {
            self.revoked.lock().unwrap().push(session_ids.to_vec());
            Ok(session_ids.len() as u64)
        }
        async fn active_session_pairs(&self) -> Result<Vec<(Uuid, Uuid)>, SweepsError> {
            Ok(self.active_pairs.clone())
        }
        async fn runner_ids_with_sessions(&self) -> Result<Vec<Uuid>, SweepsError> {
            Ok(self.rids_with_sessions.clone())
        }
    }

    #[derive(Default)]
    struct FakeOutbox {
        sent: Mutex<Vec<(Uuid, Value)>>,
        offline_runners: HashSet<Uuid>,
        cleared: Mutex<Vec<Uuid>>,
        clear_errors: HashSet<Uuid>,
        evicted: Mutex<Vec<(Uuid, Uuid, String)>>,
        evict_errors: HashSet<Uuid>,
        trims: Mutex<Vec<(Uuid, String)>>,
        trim_results: HashMap<Uuid, Option<i64>>,
        trim_errors: HashSet<Uuid>,
        reaps: Mutex<Vec<(Uuid, Vec<String>)>>,
        reap_results: HashMap<Uuid, i64>,
        reap_errors: HashSet<Uuid>,
        due: Vec<String>,
        due_err: Option<String>,
        deleted: Mutex<Vec<String>>,
        delete_errors: HashSet<String>,
        unmarked: Mutex<Vec<String>>,
        unmark_errors: HashSet<String>,
    }

    impl SweepsRunsOutbox for FakeOutbox {
        async fn send_to_runner(
            &self,
            runner_id: Uuid,
            message: Value,
        ) -> Result<(), SendOfflineError> {
            if self.offline_runners.contains(&runner_id) {
                let message_type = message
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("cancel")
                    .to_owned();
                return Err(SendOfflineError {
                    runner_id: runner_id.to_string(),
                    message_type,
                });
            }
            self.sent.lock().unwrap().push((runner_id, message));
            Ok(())
        }
        async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), SweepsError> {
            if self.clear_errors.contains(&session_id) {
                return Err(SweepsError("clear boom".to_owned()));
            }
            self.cleared.lock().unwrap().push(session_id);
            Ok(())
        }
        async fn publish_session_eviction(
            &self,
            runner_id: Uuid,
            old_session_id: Uuid,
            new_session_id: &str,
        ) -> Result<(), SweepsError> {
            if self.evict_errors.contains(&old_session_id) {
                return Err(SweepsError("evict boom".to_owned()));
            }
            self.evicted.lock().unwrap().push((
                runner_id,
                old_session_id,
                new_session_id.to_owned(),
            ));
            Ok(())
        }
        async fn safe_trim_runner_stream(
            &self,
            runner_id: Uuid,
            time_cutoff_id: &str,
        ) -> Result<Option<i64>, SweepsError> {
            if self.trim_errors.contains(&runner_id) {
                return Err(SweepsError("trim boom".to_owned()));
            }
            self.trims
                .lock()
                .unwrap()
                .push((runner_id, time_cutoff_id.to_owned()));
            Ok(self.trim_results.get(&runner_id).copied().flatten())
        }
        async fn reap_idle_consumers(
            &self,
            runner_id: Uuid,
            keep_consumers: &HashSet<String>,
        ) -> Result<i64, SweepsError> {
            if self.reap_errors.contains(&runner_id) {
                return Err(SweepsError("reap boom".to_owned()));
            }
            let mut keep: Vec<String> = keep_consumers.iter().cloned().collect();
            keep.sort();
            self.reaps.lock().unwrap().push((runner_id, keep));
            Ok(self.reap_results.get(&runner_id).copied().unwrap_or(0))
        }
        async fn due_runners_for_stream_cleanup(&self) -> Result<Vec<String>, SweepsError> {
            if let Some(err) = &self.due_err {
                return Err(SweepsError(err.clone()));
            }
            Ok(self.due.clone())
        }
        async fn delete_runner_stream(&self, runner_id: &str) -> Result<(), SweepsError> {
            if self.delete_errors.contains(runner_id) {
                return Err(SweepsError("delete boom".to_owned()));
            }
            self.deleted.lock().unwrap().push(runner_id.to_owned());
            Ok(())
        }
        async fn remove_stream_cleanup_marker(&self, runner_id: &str) -> Result<(), SweepsError> {
            if self.unmark_errors.contains(runner_id) {
                return Err(SweepsError("unmark boom".to_owned()));
            }
            self.unmarked.lock().unwrap().push(runner_id.to_owned());
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeEffects {
        applied: Mutex<Vec<Uuid>>,
        apply_errors: HashSet<Uuid>,
    }

    impl InlineTerminalEffects for FakeEffects {
        async fn apply_inline(&self, run_id: Uuid) -> Result<bool, SweepsError> {
            if self.apply_errors.contains(&run_id) {
                return Err(SweepsError("apply boom".to_owned()));
            }
            self.applied.lock().unwrap().push(run_id);
            Ok(true)
        }
    }

    // ------------------------------------------------------------------
    // Fixture + names + SQL pins
    // ------------------------------------------------------------------

    #[test]
    fn fixture_sections_for_this_issue() {
        let fx = fixture();
        // The five L6a tasks and their recorded returns.
        assert_eq!(fx["expire_stale_approvals"]["returned"], json!(4));
        assert_eq!(fx["mark_offline_runners"]["returned"], json!(2));
        assert_eq!(fx["sweep_idle_sessions"]["returned"], json!(1));
        assert_eq!(fx["sweep_stale_runners"]["returned"], json!(4));
        assert_eq!(fx["sweep_old_streams"]["returned"], json!(7));
        // Settings this port reads.
        assert_eq!(fx["settings"]["LONG_POLL_INTERVAL_SECS"], json!(25));
        assert_eq!(fx["settings"]["RUNNER_OFFLINE_THRESHOLD_SECS"], json!(50));
        assert_eq!(
            fx["settings"]["RUNNER_STREAM_MIN_RETENTION_SECS"],
            json!(3600)
        );
        assert_eq!(fx["HEARTBEAT_OFFLINE_GRACE_SECS"], json!(90.0));
    }

    #[test]
    fn task_names_match_celery() {
        assert_eq!(EXPIRE_TASK, "runner.expire_stale_approvals");
        assert_eq!(MARK_OFFLINE_TASK, "runner.mark_offline_runners");
        assert_eq!(SWEEP_IDLE_TASK, "runner.sweep_idle_sessions");
        assert_eq!(SWEEP_STALE_TASK, "runner.sweep_stale_runners");
        assert_eq!(SWEEP_STREAMS_TASK, "runner.sweep_old_streams");
        assert_eq!(EXPIRE_BEAT_NAME, "runner-expire-stale-approvals");
        assert_eq!(MARK_OFFLINE_BEAT_NAME, "runner-mark-offline-runners");
    }

    #[test]
    fn beat_entries_pinned_read_only() {
        // F-09 already transcribes the two scheduled tasks; the other
        // three have no beat entry (fixture `beat_not_scheduled`). This
        // test only reads `schedule.rs` — it owns nothing there.
        let entries = crate::schedule::beat_schedule();
        let every_minute = crate::schedule::Cadence::Crontab(
            crate::schedule::Crontab::parse("*/1", "*", "*", "*", "*").expect("valid"),
        );
        for (beat, task) in [
            (EXPIRE_BEAT_NAME, EXPIRE_TASK),
            (MARK_OFFLINE_BEAT_NAME, MARK_OFFLINE_TASK),
        ] {
            let entry = entries
                .iter()
                .find(|e| e.name == beat)
                .unwrap_or_else(|| panic!("{beat} transcribed"));
            assert_eq!(entry.task, task);
            assert_eq!(entry.cadence, every_minute);
        }
        for unscheduled in [SWEEP_IDLE_TASK, SWEEP_STALE_TASK, SWEEP_STREAMS_TASK] {
            assert!(
                entries.iter().all(|e| e.task != unscheduled),
                "{unscheduled} has no beat entry"
            );
        }
    }

    #[test]
    fn pending_expired_sql_byte_exact() {
        assert_eq!(
            pending_expired_approvals_select(),
            "SELECT \"agent_run_approval\".\"id\" FROM \"agent_run_approval\" WHERE \
             (\"agent_run_approval\".\"expires_at\" < $1 AND \"agent_run_approval\".\"status\" = $2) \
             ORDER BY \"agent_run_approval\".\"requested_at\" DESC"
        );
        // Cross-check against the fixture capture (modulo the timestamp
        // literal and the pending bind).
        let fx = fixture()["expire_stale_approvals"]["first_sql"][0]
            .as_str()
            .expect("first_sql")
            .to_owned();
        let head = fx.split(" < '").next().expect("head");
        let tail = fx.split("::timestamptz AND ").nth(1).expect("tail");
        let mine = pending_expired_approvals_select();
        assert!(mine.starts_with(head), "{mine}");
        assert!(mine.ends_with(&tail.replace("'pending'", "$2")), "{mine}");
        assert!(mine.contains(" < $1 AND "), "{mine}");
    }

    #[test]
    fn stamp_sql_byte_exact() {
        assert_eq!(
            stamp_approval_expired_sql(),
            "UPDATE \"agent_run_approval\" SET \"status\" = $1, \"decided_at\" = $2 WHERE \
             \"agent_run_approval\".\"id\" = $3"
        );
    }

    #[test]
    fn offline_flip_sql_byte_exact() {
        assert_eq!(
            mark_online_runners_offline_sql(),
            "UPDATE \"runner\" SET \"status\" = $1 WHERE (\"runner\".\"status\" = $2 AND NOT \
             (\"runner\".\"last_heartbeat_at\" >= $3 AND \"runner\".\"last_heartbeat_at\" IS NOT NULL))"
        );
        // The `IS NOT NULL` inside the `NOT` is what flips `NULL`
        // heartbeats offline (fixture `mark_offline_rule`).
        assert!(mark_online_runners_offline_sql().contains("IS NOT NULL"));
        // Cross-check against the fixture capture (modulo literals).
        let fx = fixture()["mark_offline_runners"]["sql"][0]
            .as_str()
            .expect("sql")
            .to_owned();
        assert!(fx.starts_with("UPDATE \"runner\" SET \"status\" = 'offline' WHERE "));
        assert!(fx.contains("NOT (\"runner\".\"last_heartbeat_at\" >= "));
        assert!(fx.ends_with("AND \"runner\".\"last_heartbeat_at\" IS NOT NULL))"));
    }

    #[test]
    fn idle_sql_byte_exact() {
        assert_eq!(
            idle_sessions_select(),
            "SELECT \"runner_session\".\"id\", \"runner_session\".\"runner_id\" FROM \"runner_session\" WHERE \
             (\"runner_session\".\"last_seen_at\" < $1 AND \"runner_session\".\"revoked_at\" IS NULL) \
             ORDER BY \"runner_session\".\"created_at\" DESC"
        );
        assert_eq!(
            revoke_sessions_update(1),
            "UPDATE \"runner_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE \
             \"runner_session\".\"id\" IN ($3)"
        );
        assert_eq!(
            revoke_sessions_update(3),
            "UPDATE \"runner_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE \
             \"runner_session\".\"id\" IN ($3, $4, $5)"
        );
    }

    #[test]
    fn old_streams_sql_byte_exact() {
        assert_eq!(
            active_session_pairs_select(),
            "SELECT \"runner_session\".\"runner_id\", \"runner_session\".\"id\" FROM \"runner_session\" WHERE \
             \"runner_session\".\"revoked_at\" IS NULL ORDER BY \"runner_session\".\"created_at\" DESC"
        );
        // The `DISTINCT` pair quirk: `created_at` rides along for the
        // `ORDER BY`; the driver reads column 0 and dedupes.
        assert_eq!(
            runner_ids_with_sessions_select(),
            "SELECT DISTINCT \"runner_session\".\"runner_id\", \"runner_session\".\"created_at\" FROM \
             \"runner_session\" ORDER BY \"runner_session\".\"created_at\" DESC"
        );
    }

    #[test]
    fn approval_lock_sql_byte_exact() {
        // All 52 columns spelled out: the 11 approval columns in L2
        // order, then the 41 agent_run columns in L2 order.
        let expected = "SELECT \"agent_run_approval\".\"id\", \"agent_run_approval\".\"agent_run_id\", \
             \"agent_run_approval\".\"kind\", \"agent_run_approval\".\"payload\", \
             \"agent_run_approval\".\"reason\", \"agent_run_approval\".\"status\", \
             \"agent_run_approval\".\"decision_source\", \"agent_run_approval\".\"decided_by_id\", \
             \"agent_run_approval\".\"requested_at\", \"agent_run_approval\".\"expires_at\", \
             \"agent_run_approval\".\"decided_at\", \"agent_run\".\"id\", \"agent_run\".\"workspace_id\", \
             \"agent_run\".\"owner_id\", \"agent_run\".\"created_by_id\", \"agent_run\".\"pod_id\", \
             \"agent_run\".\"runner_id\", \"agent_run\".\"pinned_runner_id\", \
             \"agent_run\".\"work_item_id\", \"agent_run\".\"scheduler_binding_id\", \
             \"agent_run\".\"parent_run_id\", \"agent_run\".\"status\", \"agent_run\".\"executor_kind\", \
             \"agent_run\".\"dispatch_attempts\", \"agent_run\".\"cancel_requested_at\", \
             \"agent_run\".\"cancel_reason\", \"agent_run\".\"error_code\", \"agent_run\".\"tool_plan\", \
             \"agent_run\".\"terminal_hooks_applied_at\", \
             \"agent_run\".\"terminal_capacity_released_at\", \"agent_run\".\"prompt\", \
             \"agent_run\".\"trigger\", \"agent_run\".\"prompt_manifest\", \"agent_run\".\"phase_kind\", \
             \"agent_run\".\"run_config\", \"agent_run\".\"required_capabilities\", \
             \"agent_run\".\"thread_id\", \"agent_run\".\"agent_metadata\", \
             \"agent_run\".\"lease_expires_at\", \"agent_run\".\"done_payload\", \"agent_run\".\"error\", \
             \"agent_run\".\"refusal_category\", \"agent_run\".\"llm_model\", \"agent_run\".\"usage\", \
             \"agent_run\".\"input_tokens\", \"agent_run\".\"output_tokens\", \
             \"agent_run\".\"total_tokens\", \"agent_run\".\"created_at\", \
             \"agent_run\".\"assigned_at\", \"agent_run\".\"queue_position\", \
             \"agent_run\".\"started_at\", \"agent_run\".\"ended_at\" FROM \"agent_run_approval\" INNER JOIN \
             \"agent_run\" ON (\"agent_run_approval\".\"agent_run_id\" = \"agent_run\".\"id\") WHERE \
             (\"agent_run_approval\".\"id\" = $1 AND \"agent_run_approval\".\"status\" = $2) ORDER BY \
             \"agent_run_approval\".\"requested_at\" DESC LIMIT 1 FOR UPDATE";
        assert_eq!(lock_approval_for_expiry_select(), expected);
    }

    // ------------------------------------------------------------------
    // Pure helpers
    // ------------------------------------------------------------------

    #[test]
    fn settings_readers_match_django() {
        assert_eq!(poll_secs_raw(None), 25);
        assert_eq!(poll_secs_raw(Some("30")), 30);
        assert_eq!(poll_secs_raw(Some(" 40 ")), 40);
        assert_eq!(poll_secs_raw(Some("nope")), 25);
        assert_eq!(poll_secs_raw(Some("")), 25);
        assert_eq!(offline_threshold_secs_raw(None), 50);
        assert_eq!(offline_threshold_secs_raw(Some("51")), 51);
        assert_eq!(offline_threshold_secs_raw(Some("bad")), 50);
        assert_eq!(stream_retention_secs_raw(None), 3600);
        assert_eq!(stream_retention_secs_raw(Some("60")), 60);
        assert_eq!(stream_retention_secs_raw(Some("bad")), 3600);
        assert_eq!(HEARTBEAT_OFFLINE_GRACE_SECS, 90);
    }

    #[test]
    fn cutoff_math_matches_tasks() {
        let now = Utc.with_ymd_and_hms(2026, 10, 2, 22, 53, 29).unwrap();
        assert_eq!(
            offline_cutoff(&now, 90),
            Utc.with_ymd_and_hms(2026, 10, 2, 22, 51, 59).unwrap()
        );
        assert_eq!(
            offline_cutoff(&now, 50),
            Utc.with_ymd_and_hms(2026, 10, 2, 22, 52, 39).unwrap()
        );
        // Idle eviction is 2x the poll interval (fixture `sweep_idle_rule`).
        assert_eq!(
            idle_cutoff(&now, 25),
            Utc.with_ymd_and_hms(2026, 10, 2, 22, 52, 39).unwrap()
        );
        // Absurd input clamps instead of panicking (Python ints are
        // unbounded; `TimeDelta::seconds` panics out of bounds).
        assert_eq!(
            idle_cutoff(&now, i64::MAX),
            now.checked_sub_signed(Duration::seconds(MAX_CUTOFF_DELTA_SECS))
                .unwrap()
        );
        assert!(idle_cutoff(&now, i64::MAX) < now);
        assert_eq!(
            offline_cutoff(&now, i64::MIN),
            now.checked_add_signed(Duration::seconds(MAX_CUTOFF_DELTA_SECS))
                .unwrap()
        );
        assert!(offline_cutoff(&now, i64::MIN) > now);
    }

    #[test]
    fn consumer_name_renders_dash_format() {
        // The real `outbox.consumer_name` (`outbox.py:97-98`): dash, not
        // the colon the fixture probe's mock recorded (see module docs).
        let sid = Uuid::parse_str("41985f63-290b-4bb3-bd86-eff4c278072c").unwrap();
        assert_eq!(
            consumer_name(&sid),
            "consumer-41985f63-290b-4bb3-bd86-eff4c278072c"
        );
    }

    #[test]
    fn stream_cutoff_id_vectors() {
        assert_eq!(
            id_for_secs_ago_at(1_700_000_000_000, 3600),
            "1699996400000-0"
        );
        assert_eq!(id_for_secs_ago_at(1_700_000_000_000, 0), "1700000000000-0");
        // Floored at zero (`outbox.py:736`, `max(0, ...)`).
        assert_eq!(id_for_secs_ago_at(1_000, 3600), "0-0");
        assert_eq!(id_for_secs_ago_at(1_700_000_000_000, i64::MAX), "0-0");
    }

    #[test]
    fn cancel_frame_matches_fixture_send() {
        let fx = fixture();
        let sent = fx["expire_stale_approvals"]["sent"][0].clone();
        let run_id: Uuid = sent["payload"]["run_id"]
            .as_str()
            .expect("run_id")
            .parse()
            .expect("uuid");
        assert_eq!(cancel_control_message(&run_id), sent["payload"]);
        let runner_id: Uuid = sent["runner_id"]
            .as_str()
            .expect("runner_id")
            .parse()
            .expect("uuid");
        assert_eq!(
            runner_id.to_string(),
            "6473049e-8c9c-45bb-af5c-e11165114e26"
        );
    }

    #[test]
    fn terminal_effects_wire_matches_delay() {
        let run_id = Uuid::parse_str("a9520630-40b7-4a64-9234-a396afd9447d").unwrap();
        let job = terminal_effects_job(&run_id);
        assert_eq!(job.task, "runner.apply_agent_run_terminal_effects");
        assert_eq!(job.task, TERMINAL_EFFECTS_TASK);
        assert_eq!(job.args, json!(["a9520630-40b7-4a64-9234-a396afd9447d"]));
        assert_eq!(job.kwargs, json!({}));
        let message = terminal_effects_message(&run_id);
        assert_eq!(message.task, "runner.apply_agent_run_terminal_effects");
        assert_eq!(
            message.args,
            vec![json!("a9520630-40b7-4a64-9234-a396afd9447d")]
        );
        assert!(message.kwargs.is_empty());
    }

    #[test]
    fn offline_error_mirrors_runner_offline_error() {
        let err = SendOfflineError {
            runner_id: "6473049e-8c9c-45bb-af5c-e11165114e26".to_owned(),
            message_type: "cancel".to_owned(),
        };
        assert_eq!(
            err.to_string(),
            "runner 6473049e-8c9c-45bb-af5c-e11165114e26 is offline; type 'cancel' cannot queue"
        );
        let sweeps = SweepsError::from(err);
        assert!(sweeps.to_string().contains("cannot queue"));
    }

    // ------------------------------------------------------------------
    // Drivers
    // ------------------------------------------------------------------

    fn uuid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[tokio::test]
    async fn expire_stamps_finalizes_sends_and_counts() {
        // Mirrors the fixture rows: an awaiting run (cancelled + send),
        // a lock miss (skip), a completed run (stamped, no finalize).
        let (a1, a2, a3) = (uuid(1), uuid(2), uuid(3));
        let (r1, rr1) = (uuid(11), uuid(21));
        let store = FakeStore {
            pending: vec![a1, a2, a3],
            outcomes: [
                (
                    a1,
                    ExpireOutcome {
                        stamped: true,
                        finalized_run: Some(r1),
                        cancel_send: Some((rr1, r1)),
                    },
                ),
                (a2, ExpireOutcome::skipped()),
                (
                    a3,
                    ExpireOutcome {
                        stamped: true,
                        finalized_run: None,
                        cancel_send: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let outbox = FakeOutbox::default();
        let effects = FakeEffects::default();
        let now = Utc::now();
        let expired = drive_expire_stale_approvals(&store, &outbox, &effects, &now)
            .await
            .unwrap();
        assert_eq!(expired, 2);
        assert_eq!(store.expire_calls.lock().unwrap().as_slice(), &[a1, a2, a3]);
        assert_eq!(store.emitted.lock().unwrap().as_slice(), &[r1]);
        assert_eq!(effects.applied.lock().unwrap().as_slice(), &[r1]);
        assert_eq!(
            outbox.sent.lock().unwrap().as_slice(),
            &[(rr1, cancel_control_message(&r1))]
        );
    }

    #[tokio::test]
    async fn expire_runnerless_finalizes_without_send() {
        // Fixture `expired_runnerless`: cancelled, no cancel frame.
        let (a1, r1) = (uuid(1), uuid(11));
        let store = FakeStore {
            pending: vec![a1],
            outcomes: [(
                a1,
                ExpireOutcome {
                    stamped: true,
                    finalized_run: Some(r1),
                    cancel_send: None,
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let outbox = FakeOutbox::default();
        let effects = FakeEffects::default();
        let now = Utc::now();
        let expired = drive_expire_stale_approvals(&store, &outbox, &effects, &now)
            .await
            .unwrap();
        assert_eq!(expired, 1);
        assert_eq!(store.emitted.lock().unwrap().as_slice(), &[r1]);
        assert_eq!(effects.applied.lock().unwrap().as_slice(), &[r1]);
        assert!(outbox.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn expire_offline_send_fails_task_after_publish() {
        // Unisolated send (`tasks.py:85-93`): the offline raise fails the
        // task with `expired` unincremented — but `_publish_effects`
        // already ran (on commit, before the send).
        let (a1, r1, rr1) = (uuid(1), uuid(11), uuid(21));
        let store = FakeStore {
            pending: vec![a1],
            outcomes: [(
                a1,
                ExpireOutcome {
                    stamped: true,
                    finalized_run: Some(r1),
                    cancel_send: Some((rr1, r1)),
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let outbox = FakeOutbox {
            offline_runners: [rr1].into_iter().collect(),
            ..Default::default()
        };
        let effects = FakeEffects::default();
        let now = Utc::now();
        let err = drive_expire_stale_approvals(&store, &outbox, &effects, &now)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cannot queue"), "{err}");
        assert_eq!(store.emitted.lock().unwrap().as_slice(), &[r1]);
        assert_eq!(effects.applied.lock().unwrap().as_slice(), &[r1]);
        assert!(outbox.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn expire_publish_halves_are_isolated() {
        // Both `_publish_effects` halves fail: the sweep still sends and
        // counts (`finalization.py:89-103`, `try/except` each).
        let (a1, r1, rr1) = (uuid(1), uuid(11), uuid(21));
        let store = FakeStore {
            pending: vec![a1],
            outcomes: [(
                a1,
                ExpireOutcome {
                    stamped: true,
                    finalized_run: Some(r1),
                    cancel_send: Some((rr1, r1)),
                },
            )]
            .into_iter()
            .collect(),
            emit_err_runs: [r1].into_iter().collect(),
            ..Default::default()
        };
        let outbox = FakeOutbox::default();
        let effects = FakeEffects {
            apply_errors: [r1].into_iter().collect(),
            ..Default::default()
        };
        let now = Utc::now();
        let expired = drive_expire_stale_approvals(&store, &outbox, &effects, &now)
            .await
            .unwrap();
        assert_eq!(expired, 1);
        assert_eq!(outbox.sent.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn expire_pending_error_propagates() {
        let store = FakeStore {
            pending_err: Some("db down".to_owned()),
            ..Default::default()
        };
        let outbox = FakeOutbox::default();
        let effects = FakeEffects::default();
        let now = Utc::now();
        let err = drive_expire_stale_approvals(&store, &outbox, &effects, &now)
            .await
            .unwrap_err();
        assert_eq!(err, SweepsError("db down".to_owned()));
    }

    #[tokio::test]
    async fn mark_offline_uses_90s_grace() {
        let store = FakeStore {
            offline_affected: 2,
            ..Default::default()
        };
        let now = Utc.with_ymd_and_hms(2026, 10, 2, 22, 53, 29).unwrap();
        let affected = drive_mark_offline_runners(&store, &now).await.unwrap();
        assert_eq!(affected, 2);
        assert_eq!(
            store.offline_thresholds.lock().unwrap().as_slice(),
            &[Utc.with_ymd_and_hms(2026, 10, 2, 22, 51, 59).unwrap()]
        );
    }

    #[tokio::test]
    async fn sweep_stale_uses_configured_threshold() {
        let store = FakeStore {
            offline_affected: 4,
            ..Default::default()
        };
        let now = Utc.with_ymd_and_hms(2026, 10, 2, 22, 53, 29).unwrap();
        let affected = drive_sweep_stale_runners(&store, &now, 50).await.unwrap();
        assert_eq!(affected, 4);
        assert_eq!(
            store.offline_thresholds.lock().unwrap().as_slice(),
            &[Utc.with_ymd_and_hms(2026, 10, 2, 22, 52, 39).unwrap()]
        );
        // The threshold is a passthrough, not a constant: a 51s knob
        // moves the cutoff by exactly one second (fixture `edge51s`).
        let store = FakeStore::default();
        drive_sweep_stale_runners(&store, &now, 51).await.unwrap();
        assert_eq!(
            store.offline_thresholds.lock().unwrap().as_slice(),
            &[Utc.with_ymd_and_hms(2026, 10, 2, 22, 52, 38).unwrap()]
        );
    }

    #[tokio::test]
    async fn idle_empty_returns_zero_without_writes() {
        let store = FakeStore::default();
        let outbox = FakeOutbox::default();
        let now = Utc::now();
        let evicted = drive_sweep_idle_sessions(&store, &outbox, &now, 25)
            .await
            .unwrap();
        assert_eq!(evicted, 0);
        assert!(store.revoked.lock().unwrap().is_empty());
        assert!(outbox.cleared.lock().unwrap().is_empty());
        assert!(outbox.evicted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn idle_evicts_with_fixture_call_shapes() {
        // Ids from the fixture `outbox_calls`: the recorded clear + evict
        // shapes replay exactly.
        let sid = Uuid::parse_str("365b55bc-d19a-40b5-bf1e-065e5995a293").unwrap();
        let rid = Uuid::parse_str("1ecbbe20-ed42-4bf0-89e8-f63a802f584e").unwrap();
        let store = FakeStore {
            idle: vec![(sid, rid)],
            ..Default::default()
        };
        let outbox = FakeOutbox::default();
        let now = Utc::now();
        let evicted = drive_sweep_idle_sessions(&store, &outbox, &now, 25)
            .await
            .unwrap();
        assert_eq!(evicted, 1);
        assert_eq!(store.revoked.lock().unwrap().as_slice(), &[vec![sid]]);
        assert_eq!(outbox.cleared.lock().unwrap().as_slice(), &[sid]);
        assert_eq!(
            outbox.evicted.lock().unwrap().as_slice(),
            &[(rid, sid, String::new())]
        );
        let fx = fixture();
        let calls = fx["sweep_idle_sessions"]["outbox_calls"].clone();
        assert_eq!(calls[0], json!(["clear", sid.to_string()]));
        assert_eq!(
            calls[1],
            json!([
                "evict",
                rid.to_string(),
                {"old_session_id": sid.to_string(), "new_session_id": ""}
            ])
        );
    }

    #[tokio::test]
    async fn idle_outbox_failure_propagates() {
        // No `try/except` around the per-row outbox calls
        // (`tasks.py:134-136`): a clear failure fails the task.
        let (sid, rid) = (uuid(1), uuid(2));
        let store = FakeStore {
            idle: vec![(sid, rid)],
            ..Default::default()
        };
        let outbox = FakeOutbox {
            clear_errors: [sid].into_iter().collect(),
            ..Default::default()
        };
        let now = Utc::now();
        let err = drive_sweep_idle_sessions(&store, &outbox, &now, 25)
            .await
            .unwrap_err();
        assert_eq!(err, SweepsError("clear boom".to_owned()));
        // The revoke already committed before the outbox loop.
        assert_eq!(store.revoked.lock().unwrap().as_slice(), &[vec![sid]]);
    }

    #[tokio::test]
    async fn old_streams_trims_reaps_and_deletes() {
        let (ra, rb) = (uuid(101), uuid(102));
        let sa = uuid(201);
        let store = FakeStore {
            active_pairs: vec![(ra, sa)],
            // The `DISTINCT`-pair quirk surfaces `ra` twice; the driver
            // dedupes through a set like the source.
            rids_with_sessions: vec![ra, rb, ra],
            ..Default::default()
        };
        let outbox = FakeOutbox {
            trim_results: [(ra, Some(7))].into_iter().collect(),
            reap_results: [(ra, 2), (rb, 0)].into_iter().collect(),
            due: vec![rb.to_string()],
            ..Default::default()
        };
        let now = Utc.with_ymd_and_hms(2026, 10, 2, 22, 53, 29).unwrap();
        let trimmed = drive_sweep_old_streams(&store, &outbox, &now, 3600)
            .await
            .unwrap();
        // Returns the trimmed count only (fixture `returned == 7`).
        assert_eq!(trimmed, 7);
        // Trim only where an active consumer exists, with the retention
        // cutoff; `ra` trimmed once despite the duplicated row.
        let expected_cutoff = id_for_secs_ago_at(now.timestamp_millis(), 3600);
        assert_eq!(
            outbox.trims.lock().unwrap().as_slice(),
            &[(ra, expected_cutoff)]
        );
        // Reap always runs, with the active-consumer keep set (possibly
        // empty, like the fixture's `[]` for the revoked runner).
        let mut reaps = outbox.reaps.lock().unwrap().clone();
        reaps.sort_by_key(|(rid, _)| *rid);
        assert_eq!(
            reaps,
            vec![(ra, vec![format!("consumer-{sa}")]), (rb, vec![]),]
        );
        assert_eq!(outbox.deleted.lock().unwrap().as_slice(), &[rb.to_string()]);
        assert_eq!(
            outbox.unmarked.lock().unwrap().as_slice(),
            &[rb.to_string()]
        );
    }

    #[tokio::test]
    async fn old_streams_trim_none_and_zero_skip() {
        let (ra, rb) = (uuid(101), uuid(102));
        let store = FakeStore {
            active_pairs: vec![(ra, uuid(201)), (rb, uuid(202))],
            rids_with_sessions: vec![ra, rb],
            ..Default::default()
        };
        // `None` (skipped/unavailable) and `0` both skip the sum
        // (`outbox.py:619-620`, `if removed:`).
        let outbox = FakeOutbox {
            trim_results: [(ra, None), (rb, Some(0))].into_iter().collect(),
            ..Default::default()
        };
        let now = Utc::now();
        let trimmed = drive_sweep_old_streams(&store, &outbox, &now, 3600)
            .await
            .unwrap();
        assert_eq!(trimmed, 0);
        assert_eq!(outbox.trims.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn old_streams_isolates_per_runner_errors() {
        let (ra, rb) = (uuid(101), uuid(102));
        let (rd, re) = (uuid(104).to_string(), uuid(105).to_string());
        let store = FakeStore {
            active_pairs: vec![(ra, uuid(201)), (rb, uuid(202))],
            rids_with_sessions: vec![ra, rb],
            ..Default::default()
        };
        let outbox = FakeOutbox {
            trim_results: [(rb, Some(3))].into_iter().collect(),
            trim_errors: [ra].into_iter().collect(),
            reap_errors: [rb].into_iter().collect(),
            due: vec![rd.clone(), re.clone()],
            delete_errors: [rd.clone()].into_iter().collect(),
            ..Default::default()
        };
        let now = Utc::now();
        let trimmed = drive_sweep_old_streams(&store, &outbox, &now, 3600)
            .await
            .unwrap();
        // The failed trim contributes nothing; the healthy one still sums.
        assert_eq!(trimmed, 3);
        // Every runner was still attempted.
        assert_eq!(
            outbox.trims.lock().unwrap().as_slice(),
            &[(rb, id_for_secs_ago_at(now.timestamp_millis(), 3600))]
        );
        assert_eq!(outbox.reaps.lock().unwrap().len(), 1);
        // Delete + unmark share one `try`: the failed delete skips its
        // unmark, while the healthy runner completes both.
        assert_eq!(
            outbox.deleted.lock().unwrap().as_slice(),
            std::slice::from_ref(&re)
        );
        assert_eq!(outbox.unmarked.lock().unwrap().as_slice(), &[re]);
        let _ = rd;
    }

    #[tokio::test]
    async fn old_streams_due_error_propagates() {
        // No `try/except` around the due-runners fetch itself
        // (`tasks.py:194`): a failure fails the task.
        let store = FakeStore::default();
        let outbox = FakeOutbox {
            due_err: Some("zset boom".to_owned()),
            ..Default::default()
        };
        let now = Utc::now();
        let err = drive_sweep_old_streams(&store, &outbox, &now, 3600)
            .await
            .unwrap_err();
        assert_eq!(err, SweepsError("zset boom".to_owned()));
    }

    #[tokio::test]
    async fn register_owns_all_five_tasks() {
        // `connect_lazy` never touches the network, but sqlx still needs
        // a Tokio context to build the pool: registration wiring stays
        // testable with no database.
        let pool =
            sqlx::PgPool::connect_lazy("postgres://localhost:1/unused").expect("lazy pool builds");
        let mut registry = Registry::new();
        for task in crate::runner_runs::TASK_NAMES {
            assert!(!registry.owns(task));
        }
        register_sweeps_runs_tasks(
            &mut registry,
            pool,
            Arc::new(FakeOutbox::default()),
            Arc::new(FakeEffects::default()),
        );
        for task in crate::runner_runs::TASK_NAMES {
            assert!(registry.owns(task), "{task} registered");
        }
        assert_eq!(crate::runner_runs::TASK_NAMES.len(), 5);
        assert!(crate::runner_runs::is_runner_runs_task(EXPIRE_TASK));
        assert!(!crate::runner_runs::is_runner_runs_task(
            "runner.reconcile_stalled_runs"
        ));
    }
}
