//! Per-binding fire: `fire_scheduler_binding` three-phase claim/dispatch/rollback (D-10).
//!
//! Ports the fire half of `pi_dash/bgtasks/scheduler.py`
//! (`_next_fire_for_binding` + `_is_last_run_in_flight` + `fire_scheduler_binding`,
//! lines 50-97 and 139-283) via [`crate::tasks_ticker::rrule::next_fire_from_rrule`]
//! (owned by PIDASHCONV-205 — called, never re-ported).
//!
//! The three phases mirror the Python docstring (`scheduler.py:5-24`, design §6.2):
//!
//! 1. **Claim** under `SELECT ... FOR UPDATE` (the F-09 `SKIP LOCKED`-equivalent
//!    SFU pattern from the Porting guide): re-check enabled flags, skip when the
//!    previous run is still non-terminal, advance `next_run_at` from the RRULE
//!    bundle, commit.
//! 2. **Dispatch** outside the transaction through the D-12 seam
//!    ([`SchedulerDispatch`], standing in for
//!    `pi_dash.orchestration.service.dispatch_scheduler_run`, which D-12 owns —
//!    this epic is blocked *by* the scanners + rrule helper only, and D-12
//!    (PIDASHCONV-49) is blocked by this epic, so the call goes through the
//!    seam and handles `(run, fail_reason)` exactly as below).
//! 3. **Record or rollback**: a produced run pins `last_run` (with the
//!    prompt-build-failure wording when the run is FAILED); `None` restores the
//!    pre-claim `next_run_at` so the budget is not burned silently.
//!
//! The handler is registered under
//! `pi_dash.bgtasks.scheduler.fire_scheduler_binding` with
//! `bind`/`max_retries=0` semantics: it never asks for a retry —
//! [`crate::worker::Verdict::Ack`] on every settled fire (skip or dispatch),
//! [`crate::worker::Verdict::Fail`] only on infrastructure failure.
//!
//! Fixture: FX-TICKER-05 (`fixtures/tasks_ticker/scheduler/fire.before_after.json`;
//! the FX-TICKER-02 `next_fire` vectors are reused as already-done input).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use super::rrule::next_fire_from_rrule;
use super::scan::{scheduler_enabled, FIRE_SCHEDULER_BINDING_TASK};
use crate::py_repr;
use crate::queue::JobRow;
use crate::worker::{HandlerError, Registry, Verdict};
use pidash_db::tasks_ticker::models::LAST_ERROR_MAX_LEN;

/// Run status that marks a produced run as a prompt-build failure
/// (`runner/models.py:207`, `AgentRunStatus.FAILED = "failed"`).
pub const FAILED_RUN_STATUS: &str = "failed";

/// Non-terminal run statuses: a binding whose previous run sits in any of these
/// is "still in flight" and the tick is skipped (`scheduler.py:50-62`,
/// `NON_TERMINAL_STATUSES`; `db/models/scheduler.py` deliberately carries no
/// duplicated status enum — status is read off `last_run.status`).
pub const NON_TERMINAL_STATUSES: [&str; 9] = [
    "queued",
    "assigned",
    "waiting_for_worktree",
    "running",
    "cancel_requested",
    "awaiting_approval",
    "awaiting_reauth",
    "paused_awaiting_input",
    "blocked",
];

/// True when `status` is one of [`NON_TERMINAL_STATUSES`].
pub fn is_non_terminal_status(status: &str) -> bool {
    NON_TERMINAL_STATUSES.contains(&status)
}

/// True when the previous run is still non-terminal
/// (`scheduler.py:86-96`, `_is_last_run_in_flight`).
///
/// `None` id means no previous run — never in flight (the early return).
/// A set id with no readable status is *not* answered here: Python would raise
/// `AttributeError` on `None.status`, so the caller surfaces it as
/// [`FireError::OrphanedRun`] (task failure, no retry) rather than guessing.
pub fn is_in_flight(last_run_id: Option<&Uuid>, last_run_status: Option<&str>) -> bool {
    if last_run_id.is_none() {
        return false;
    }
    match last_run_status {
        Some(status) => is_non_terminal_status(status),
        None => false,
    }
}

/// Cap a `last_error` value exactly like Python's `[:LAST_ERROR_MAX_LEN]`
/// (`db/models/scheduler.py:22`, `LAST_ERROR_MAX_LEN = 1000`).
///
/// Python slices by code points; a byte slice in Rust would panic on a UTF-8
/// boundary (Porting guide semantic trap), so this counts `char`s.
/// Single source for this module's three truncation sites (bad-RRULE row,
/// FAILED-run wording, rollback wording).
pub fn truncate_last_error(value: &str) -> String {
    if value.chars().count() <= LAST_ERROR_MAX_LEN {
        return value.to_owned();
    }
    value.chars().take(LAST_ERROR_MAX_LEN).collect()
}

/// CPython `repr` of a tz-aware UTC datetime, for the bad-RRULE message
/// (`scheduler.py:188-190`, `f"invalid rrule: dtstart={binding.dtstart!r} ..."`).
///
/// `repr(datetime(2026, 9, 27, 12, 0, tzinfo=timezone.utc))` spells
/// `datetime.datetime(2026, 9, 27, 12, 0, tzinfo=datetime.timezone.utc)`:
/// CPython omits the seconds field when second and microsecond are both zero,
/// and omits microseconds when zero; `tzinfo` is always present for aware
/// values. Binding `dtstart` values come from `timestamptz` (microsecond
/// resolution), so sub-microsecond remainders cannot occur.
pub fn py_datetime_repr(dt: &DateTime<Utc>) -> String {
    use chrono::{Datelike, Timelike};
    let base = format!(
        "datetime.datetime({}, {}, {}, {}, {}",
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute()
    );
    let second = dt.second();
    let nanos = dt.timestamp_subsec_nanos();
    if nanos != 0 {
        // Timestamptz resolution is microseconds; truncate, never round.
        format!(
            "{base}, {second}, {}, tzinfo=datetime.timezone.utc)",
            nanos / 1_000
        )
    } else if second != 0 {
        format!("{base}, {second}, tzinfo=datetime.timezone.utc)")
    } else {
        format!("{base}, tzinfo=datetime.timezone.utc)")
    }
}

/// The bad-RRULE disable row's `last_error`
/// (`scheduler.py:188-190`): `invalid rrule: dtstart=<repr> rrule=<repr>`,
/// truncated to [`LAST_ERROR_MAX_LEN`]. The rrule half reuses the crate's
/// CPython string repr ([`crate::celery::py_repr`]).
pub fn bad_rrule_error(dtstart: &DateTime<Utc>, rrule: &str) -> String {
    truncate_last_error(&format!(
        "invalid rrule: dtstart={} rrule={}",
        py_datetime_repr(dtstart),
        py_repr(&Value::String(rrule.to_owned()))
    ))
}

/// The FAILED-run `last_error` wording (`scheduler.py:244-248`):
/// `prompt build failed: <run.error>`, truncated.
pub fn prompt_build_error(run_error: &str) -> String {
    truncate_last_error(&format!("prompt build failed: {run_error}"))
}

/// The rollback `last_error` wording (`scheduler.py:272-274`):
/// `dispatch failed: <reason>` when a reason was given, bare
/// `dispatch failed` otherwise (Python's `if fail_reason` treats `None` and
/// `""` the same), truncated.
pub fn rollback_error(fail_reason: Option<&str>) -> String {
    let message = match fail_reason {
        Some(reason) if !reason.is_empty() => format!("dispatch failed: {reason}"),
        _ => "dispatch failed".to_owned(),
    };
    truncate_last_error(&message)
}

/// The `last_error` value stored by the advance branch (`scheduler.py:203-204`):
/// a stale short-circuit error is cleared, an already-empty value stays empty
/// (always included in `update_fields`, so the stored value is `""` either way
/// once this branch runs — a real terminate-hook update rewrites it later).
pub fn advanced_last_error(current: &str) -> String {
    if current.is_empty() {
        current.to_owned()
    } else {
        String::new()
    }
}

/// Parse one ISO 8601 string the way `parse_iso_utc` does
/// (`utils/iso_datetime.py:22-34`): `Z` and `+00:00` suffixes, arbitrary
/// numeric offsets converted to UTC, naive values assumed UTC, `None` (here:
/// unparseable) on empty input or parse failure.
fn parse_iso_utc(value: &str) -> Option<DateTime<Utc>> {
    if value.is_empty() {
        return None;
    }
    let normalized = match value.strip_suffix('Z') {
        Some(stem) => format!("{stem}+00:00"),
        None => value.to_owned(),
    };
    if let Ok(aware) = DateTime::parse_from_rfc3339(&normalized) {
        return Some(aware.with_timezone(&Utc));
    }
    // `datetime.fromisoformat` also accepts a space separator with an offset.
    if normalized.contains(' ') {
        let mangled = normalized.replacen(' ', "T", 1);
        if let Ok(aware) = DateTime::parse_from_rfc3339(&mangled) {
            return Some(aware.with_timezone(&Utc));
        }
    }
    // Naive → assume UTC. `%.f` matches with or without a fraction.
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(&normalized, format) {
            return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(&normalized, "%Y-%m-%d") {
        let naive = date.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    None
}

/// Coerce a JSON `rdates`/`exdates` column the way `coerce_iso_datetimes` does
/// (`utils/iso_datetime.py:36-57`): a list of ISO strings (or datetimes —
/// JSON storage only ever holds strings) into tz-aware UTC datetimes.
/// Unparseable items and non-strings are skipped, never fatal.
pub fn coerce_iso_datetimes(raw: &Value) -> Vec<DateTime<Utc>> {
    match raw.as_array() {
        Some(items) => items
            .iter()
            .filter_map(|item| item.as_str())
            .filter_map(parse_iso_utc)
            .collect(),
        None => Vec::new(),
    }
}

/// Every failure the fire path reports. A handler maps these to
/// [`crate::worker::Verdict::Fail`] — with `max_retries=0` there is no retry
/// budget, so no failure ever becomes a `Retry`.
#[derive(Debug)]
pub enum FireError {
    /// Database failure in a claim/record/rollback phase.
    Db(sqlx::Error),
    /// The D-12 seam itself failed (transport/shape), outside the
    /// `(run, fail_reason)` contract.
    Dispatch(String),
    /// `last_run_id` is set but the `agent_run` row is gone: Python's
    /// `binding.last_run.status` would raise `AttributeError` on `None`.
    OrphanedRun(Uuid),
}

impl std::fmt::Display for FireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FireError::Db(error) => write!(f, "fire_scheduler_binding: database error: {error}"),
            FireError::Dispatch(error) => {
                write!(f, "fire_scheduler_binding: dispatch error: {error}")
            }
            FireError::OrphanedRun(id) => write!(
                f,
                "fire_scheduler_binding: last_run {id} has no agent_run row"
            ),
        }
    }
}

impl std::error::Error for FireError {}

impl From<sqlx::Error> for FireError {
    fn from(error: sqlx::Error) -> Self {
        FireError::Db(error)
    }
}

// ---------------------------------------------------------------------------
// Phase SQL (mirrors the Django ORM statements arm for arm)
// ---------------------------------------------------------------------------

/// Phase 1 claim select (`scheduler.py:157-162`):
/// `select_for_update(of=("self",))` + `select_related("scheduler", "last_run")`
/// + `filter(pk, deleted_at__isnull=True)`.
///
/// `FOR UPDATE OF scheduler_bindings` locks only the binding row (the F-09 SFU
/// pattern); the scheduler join is inner (non-nullable FK, as Django emits)
/// and the run join is a left join (nullable `last_run` FK).
pub fn claim_select_sql() -> &'static str {
    "SELECT b.id AS id, b.enabled AS enabled, b.dtstart AS dtstart, \
     b.tzid AS tzid, b.rrule AS rrule, b.rdates AS rdates, b.exdates AS exdates, \
     b.next_run_at AS next_run_at, b.last_run_id AS last_run_id, \
     b.last_error AS last_error, s.is_enabled AS scheduler_is_enabled, \
     r.status AS last_run_status \
     FROM scheduler_bindings AS b \
     INNER JOIN schedulers AS s ON b.scheduler_id = s.id \
     LEFT JOIN agent_run AS r ON b.last_run_id = r.id \
     WHERE b.id = $1 AND b.deleted_at IS NULL \
     FOR UPDATE OF b"
}

/// Bad-RRULE disable write (`scheduler.py:193-195`,
/// `update_fields=["last_error", "enabled", "next_run_at", "updated_at"]`).
pub fn bad_rrule_update_sql() -> &'static str {
    "UPDATE scheduler_bindings SET last_error = $1, enabled = FALSE, \
     next_run_at = NULL, updated_at = $2 WHERE id = $3"
}

/// Advance write (`scheduler.py:205`,
/// `update_fields=["next_run_at", "last_error", "updated_at"]`).
pub fn advance_update_sql() -> &'static str {
    "UPDATE scheduler_bindings SET next_run_at = $1, last_error = $2, \
     updated_at = $3 WHERE id = $4"
}

/// Phase 2 re-fetch (`scheduler.py:215-221`): `select_related("scheduler",
/// "project", "workspace", "actor")`, `filter(pk)`, no SFU, no `deleted_at`
/// filter. The dispatcher only needs the FK values it copies onto the
/// `AgentRun`, so the seam struct carries the ids with no joins: under intact
/// foreign keys this admits exactly the rows the joined Django query returns.
pub fn dispatch_select_sql() -> &'static str {
    "SELECT b.id AS id, b.scheduler_id AS scheduler_id, \
     b.project_id AS project_id, b.workspace_id AS workspace_id, \
     b.actor_id AS actor_id, b.pod_id AS pod_id \
     FROM scheduler_bindings AS b WHERE b.id = $1"
}

/// Phase 3 re-acquire (`scheduler.py:237-241` and `264-269`):
/// `select_for_update(of=("self",))` + `filter(pk)` — deliberately no
/// `deleted_at` filter, exactly as Python omits it on both re-acquires.
pub fn reacquire_select_sql() -> &'static str {
    "SELECT b.id AS id FROM scheduler_bindings AS b \
     WHERE b.id = $1 FOR UPDATE OF b"
}

/// Phase 3a pointer write (`scheduler.py:251`,
/// `update_fields=["last_run", "last_error", "updated_at"]` via `save()`, so
/// `auto_now` fires on `updated_at` — mirrored with an explicit `now()`).
pub fn pointer_update_sql() -> &'static str {
    "UPDATE scheduler_bindings SET last_run_id = $1, last_error = $2, \
     updated_at = $3 WHERE id = $4"
}

/// Phase 3b rollback write (`scheduler.py:275-277`,
/// `update_fields=["next_run_at", "last_error", "updated_at"]`; `next_run_at`
/// may be restored to NULL when the binding had never fired).
pub fn rollback_update_sql() -> &'static str {
    "UPDATE scheduler_bindings SET next_run_at = $1, last_error = $2, \
     updated_at = $3 WHERE id = $4"
}

/// One phase-1 claim row: the binding's RRULE bundle and runtime state plus
/// the joined `scheduler.is_enabled` and `last_run.status`.
#[derive(Debug, Clone, FromRow)]
pub struct ClaimRow {
    pub id: Uuid,
    pub enabled: bool,
    pub dtstart: DateTime<Utc>,
    pub tzid: String,
    pub rrule: String,
    pub rdates: Value,
    pub exdates: Value,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_run_id: Option<Uuid>,
    pub last_error: String,
    pub scheduler_is_enabled: bool,
    pub last_run_status: Option<String>,
}

/// The re-fetched binding for dispatch: exactly the FK values the dispatcher
/// copies onto the `AgentRun` (`scheduler.py:213-214`).
#[derive(Debug, Clone, FromRow)]
pub struct DispatchRow {
    pub id: Uuid,
    pub scheduler_id: Uuid,
    pub project_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub actor_id: Option<Uuid>,
    pub pod_id: Option<Uuid>,
}

// ---------------------------------------------------------------------------
// D-12 dispatch seam (`dispatch_scheduler_run`)
// ---------------------------------------------------------------------------

/// The re-fetched binding as the dispatcher sees it: the FK values
/// `dispatch_scheduler_run` copies onto the `AgentRun`
/// (`orchestration/service.py:846-848`), including the late-bound `pod_id`
/// override the dispatcher resolves per-fire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingForDispatch {
    pub binding_id: Uuid,
    pub scheduler_id: Uuid,
    pub project_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub actor_id: Option<Uuid>,
    pub pod_id: Option<Uuid>,
}

impl From<DispatchRow> for BindingForDispatch {
    fn from(row: DispatchRow) -> Self {
        BindingForDispatch {
            binding_id: row.id,
            scheduler_id: row.scheduler_id,
            project_id: row.project_id,
            workspace_id: row.workspace_id,
            actor_id: row.actor_id,
            pod_id: row.pod_id,
        }
    }
}

/// A run the dispatcher produced: the id, terminal-or-not status, and the
/// `error` text surfaced on `last_error` when the run FAILED.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchRun {
    pub id: Uuid,
    pub status: String,
    pub error: String,
}

/// The `(run, fail_reason)` tuple `dispatch_scheduler_run` returns
/// (`orchestration/service.py:848`): a prompt-build failure still produces a
/// run (marked FAILED) with `fail_reason` unset; only the short-circuit cases
/// (no pod / no creator) return `(None, reason)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerDispatchResult {
    pub run: Option<DispatchRun>,
    pub fail_reason: Option<String>,
}

/// The D-12 seam: `dispatch_scheduler_run(binding)` as an injectable call.
/// D-12 (PIDASHCONV-49, blocked by this epic) replaces the test/forwarding
/// implementation with the real dispatcher; this module only handles the
/// `(run, fail_reason)` contract exactly as `scheduler.py:226-283` does.
/// A transport-level failure outside that contract is `Err` (task failure).
pub type SchedulerDispatch = Arc<
    dyn Fn(
            BindingForDispatch,
        ) -> Pin<Box<dyn Future<Output = Result<SchedulerDispatchResult, String>> + Send>>
        + Send
        + Sync,
>;

// ---------------------------------------------------------------------------
// Phases
// ---------------------------------------------------------------------------

/// What phase 1 decided inside its commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase1 {
    /// Missing, disabled, scheduler-disabled, future `next_run_at`,
    /// in-flight, or bad-RRULE row: nothing to dispatch. (The bad-RRULE
    /// branch commits its disable row first.)
    Skipped,
    /// Claimed and advanced: the pre-claim cursor travels to phase 3b so a
    /// dispatch failure can restore it.
    Claimed {
        binding: Uuid,
        prev_next_run_at: Option<DateTime<Utc>>,
    },
}

/// Phase 1: claim under SFU and advance `next_run_at` (`scheduler.py:152-205`).
///
/// Commits before returning whenever it wrote (bad-RRULE disable, advance);
/// pure skips return with nothing pending so the dropped transaction rolls
/// back an empty write set — observably identical to Python's early `False`
/// returns inside `transaction.atomic()`.
pub async fn phase1_claim(
    pool: &PgPool,
    binding_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<Phase1, FireError> {
    let mut tx = pool.begin().await?;
    let claim: Option<ClaimRow> = sqlx::query_as(claim_select_sql())
        .bind(binding_id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(row) = claim else {
        return Ok(Phase1::Skipped);
    };
    if !row.enabled {
        return Ok(Phase1::Skipped);
    }
    if !row.scheduler_is_enabled {
        return Ok(Phase1::Skipped);
    }
    // A future `next_run_at` means this firing is racing the scanner: skip
    // without advancing (`scheduler.py:169-173`).
    if row.next_run_at.is_some_and(|next| next > *now) {
        return Ok(Phase1::Skipped);
    }
    if row.last_run_id.is_some() {
        match row.last_run_status.as_deref() {
            None => return Err(FireError::OrphanedRun(row.id)),
            Some(status) if is_non_terminal_status(status) => {
                tracing::info!(
                    binding = %row.id,
                    status = status,
                    "scheduler.fire: skip reason=last-run-in-flight"
                );
                return Ok(Phase1::Skipped);
            }
            Some(_) => {}
        }
    }

    // `binding.rrule or ""`, `binding.tzid or "UTC"` (`scheduler.py:76-83`):
    // both columns are NOT NULL with those defaults, so only a stored-empty
    // tzid falls back.
    let tzid = if row.tzid.is_empty() {
        "UTC"
    } else {
        row.tzid.as_str()
    };
    let next = next_fire_from_rrule(
        row.dtstart,
        row.rrule.as_str(),
        tzid,
        &coerce_iso_datetimes(&row.rdates),
        &coerce_iso_datetimes(&row.exdates),
        *now,
    );
    match next {
        None => {
            // Bad RRULE bundle: record the error and disable the binding so
            // the scanner stops re-attempting every minute; recovery is the
            // API edit endpoint, which validates the RRULE on PATCH
            // (`scheduler.py:183-196`).
            let message = bad_rrule_error(&row.dtstart, &row.rrule);
            sqlx::query(bad_rrule_update_sql())
                .bind(&message)
                .bind(Utc::now())
                .bind(row.id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(Phase1::Skipped)
        }
        Some(advanced) => {
            let cleared = advanced_last_error(&row.last_error);
            sqlx::query(advance_update_sql())
                .bind(advanced)
                .bind(&cleared)
                .bind(Utc::now())
                .bind(row.id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(Phase1::Claimed {
                binding: row.id,
                prev_next_run_at: row.next_run_at,
            })
        }
    }
}

/// Phase 3a: a run was produced — pin the run pointer (`scheduler.py:229-261`).
///
/// `last_error` carries the prompt-build-failure wording when the run FAILED
/// (otherwise a permanently-broken prompt fails invisibly every tick) and is
/// cleared for healthy runs. Returns `True` regardless of the run's terminal
/// status — the tick is recorded and `next_run_at` stays advanced. A binding
/// deleted between phases keeps its advance (nothing to point at), still
/// returning `True`.
pub async fn phase3a_record_run(
    pool: &PgPool,
    binding_id: &Uuid,
    run: &DispatchRun,
) -> Result<(), FireError> {
    let failed = run.status == FAILED_RUN_STATUS;
    let mut tx = pool.begin().await?;
    let target: Option<(Uuid,)> = sqlx::query_as(reacquire_select_sql())
        .bind(binding_id)
        .fetch_optional(&mut *tx)
        .await?;
    if target.is_some() {
        let message = if failed {
            prompt_build_error(&run.error)
        } else {
            String::new()
        };
        sqlx::query(pointer_update_sql())
            .bind(run.id)
            .bind(&message)
            .bind(Utc::now())
            .bind(binding_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    tracing::info!(
        run = %run.id,
        binding = %binding_id,
        failed = failed,
        "scheduler.fire: dispatched"
    );
    Ok(())
}

/// Phase 3b: dispatch returned `None` — roll back the advance
/// (`scheduler.py:263-283`): restore the pre-claim `next_run_at` (possibly
/// NULL) and record `dispatch failed[: reason]`.
pub async fn phase3b_rollback(
    pool: &PgPool,
    binding_id: &Uuid,
    prev_next_run_at: Option<DateTime<Utc>>,
    fail_reason: Option<&str>,
) -> Result<(), FireError> {
    let mut tx = pool.begin().await?;
    let target: Option<(Uuid,)> = sqlx::query_as(reacquire_select_sql())
        .bind(binding_id)
        .fetch_optional(&mut *tx)
        .await?;
    if target.is_some() {
        let message = rollback_error(fail_reason);
        sqlx::query(rollback_update_sql())
            .bind(prev_next_run_at)
            .bind(&message)
            .bind(Utc::now())
            .bind(binding_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    tracing::info!(
        binding = %binding_id,
        reason = fail_reason.unwrap_or("(unspecified)"),
        "scheduler.fire: dispatch returned None; rolled back next_run_at"
    );
    Ok(())
}

/// Fire one binding through the three-phase claim/dispatch/rollback
/// (`scheduler.py:144-283`). Returns `True` when a run was dispatched
/// (regardless of its terminal status), `False` on every skip.
pub async fn fire_scheduler_binding(
    pool: &PgPool,
    binding_id: &Uuid,
    now: &DateTime<Utc>,
    dispatch: &SchedulerDispatch,
) -> Result<bool, FireError> {
    // Instance-level kill switch (`scheduler.py:149-150`, design §10).
    if !scheduler_enabled() {
        return Ok(false);
    }

    let Phase1::Claimed {
        binding,
        prev_next_run_at,
    } = phase1_claim(pool, binding_id, now).await?
    else {
        return Ok(false);
    };

    // Phase 2 runs outside the claim transaction: holding the row lock across
    // the dispatcher (which registers `transaction.on_commit(drain_pod_by_id)`)
    // would break pod drain (`scheduler.py:207-211`).
    let target: Option<DispatchRow> = sqlx::query_as(dispatch_select_sql())
        .bind(binding)
        .fetch_optional(pool)
        .await?;
    let Some(target) = target else {
        // Deleted between phases — nothing to roll back to.
        return Ok(false);
    };

    let outcome = dispatch(BindingForDispatch::from(target))
        .await
        .map_err(FireError::Dispatch)?;

    match outcome.run {
        Some(run) => {
            phase3a_record_run(pool, &binding, &run).await?;
            Ok(true)
        }
        None => {
            phase3b_rollback(
                pool,
                &binding,
                prev_next_run_at,
                outcome.fail_reason.as_deref(),
            )
            .await?;
            Ok(false)
        }
    }
}

/// Parse the `fire_scheduler_binding.delay(str(binding_id))` wire args back
/// into a binding id. The fan-out is `args=[str(id)]`, `kwargs={}`
/// (`scheduler.py:130-131`); anything else fails the task (with
/// `max_retries=0` there is no retry, mirroring Django raising
/// `ValidationError` on a bad `pk` lookup instead of skipping).
pub fn binding_id_from_job(job: &JobRow) -> Result<Uuid, String> {
    let first = job
        .args
        .as_array()
        .and_then(|args| args.first())
        .and_then(|value| value.as_str());
    match first {
        Some(raw) => Uuid::parse_str(raw)
            .map_err(|_| format!("fire_scheduler_binding: invalid binding id arg {raw:?}")),
        None => Err("fire_scheduler_binding: expected args=[binding_id]".to_owned()),
    }
}

/// Register the local `fire_scheduler_binding` handler: ownership flips from
/// the Python plane to Rust the moment this runs (see
/// [`crate::worker::route_for`]). The pool is captured because
/// [`crate::worker::Handler`] receives only the claimed row; each fire uses
/// the firing instant as `now`.
pub fn register_fire_binding(registry: &mut Registry, pool: PgPool, dispatch: SchedulerDispatch) {
    registry.register(
        FIRE_SCHEDULER_BINDING_TASK,
        Arc::new(move |job: JobRow| {
            let pool = pool.clone();
            let dispatch = dispatch.clone();
            let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                Box::pin(async move {
                    let binding_id = binding_id_from_job(&job)?;
                    let now = Utc::now();
                    fire_scheduler_binding(&pool, &binding_id, &now, &dispatch)
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
    use super::*;
    use crate::tasks_ticker::scan::{fire_message, SCHEDULER_ENABLED_ENV, TASK_NAMES};
    use crate::worker::{route_for, Route};
    use chrono::TimeZone;
    use serde_json::json;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/tasks_ticker")
    }

    fn fixture(name: &str) -> serde_json::Value {
        let text = std::fs::read_to_string(fixtures_dir().join(name))
            .unwrap_or_else(|e| panic!("read fixture {name}: {e}"));
        serde_json::from_str(&text).expect("fixture is valid JSON")
    }

    /// FX-TICKER-05 fire fixture (`scheduler/fire.before_after.json`).
    fn fire_fixture() -> serde_json::Value {
        fixture("scheduler/fire.before_after.json")
    }

    fn case(name: &str) -> serde_json::Value {
        fire_fixture()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("fixture case {name}"))
            .clone()
    }

    // The 9 non-terminal statuses (`scheduler.py:50-62`) are exactly the 9
    // the fixture records, as sets (Python's is a frozenset: order-free).
    #[test]
    fn non_terminal_set_matches_fixture() {
        let mut mine = NON_TERMINAL_STATUSES.to_vec();
        mine.sort_unstable();
        let fixture_case = case("last-run-in-flight-skip");
        let mut golden: Vec<&str> = fixture_case["non_terminal_statuses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        golden.sort_unstable();
        assert_eq!(mine, golden);
        assert_eq!(mine.len(), 9);
    }

    // Every non-terminal status with a previous run is in flight; no previous
    // run is never in flight; every terminal status lets the tick through
    // (`scheduler.py:86-96`).
    #[test]
    fn in_flight_matrix() {
        let run = Some(Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap());
        let id = run.as_ref();
        for status in NON_TERMINAL_STATUSES {
            assert!(is_in_flight(id, Some(status)), "{status} must be in flight");
        }
        // None last_run_id → not in flight, regardless of status argument.
        assert!(!is_in_flight(None, None));
        assert!(!is_in_flight(None, Some("running")));
        // Terminal statuses proceed.
        for status in ["completed", "failed", "cancelled", "refused", "bogus"] {
            assert!(
                !is_in_flight(id, Some(status)),
                "{status} must not be in flight"
            );
        }
    }

    // Bad-RRULE disable row, byte-exact against the golden
    // (`scheduler.py:188-195`): wording, truncation source, disable + NULL
    // cursor. The golden's `before` pins the preconditions this branch needs
    // (enabled, due `next_run_at`, no previous run).
    #[test]
    fn bad_rrule_row_matches_golden() {
        let golden = case("bad-rrule-disable");
        assert_eq!(golden["fire_result"], false);
        assert!(golden["before"]["enabled"].as_bool().unwrap());
        assert!(golden["before"]["last_run_id"].is_null());
        let dtstart = Utc.with_ymd_and_hms(2026, 9, 27, 12, 0, 0).unwrap();
        let message = bad_rrule_error(&dtstart, "this is not a real rrule");
        assert_eq!(message, golden["after"]["last_error"].as_str().unwrap());
        assert_eq!(message.chars().count(), 123);
        assert_eq!(golden["after"]["last_error_len"], 123);
        assert_eq!(golden["after"]["enabled"], false);
        assert!(golden["after"]["next_run_at"].is_null());
        assert!(golden["after"]["last_run_id"].is_null());
    }

    // Truncation at 1000 *chars*: the long-rrule golden is exactly 1000, and
    // a multibyte input truncates on a char boundary without panicking
    // (Porting guide trap: byte slicing `[:1000]` would panic).
    #[test]
    fn last_error_truncates_at_1000_chars() {
        let golden = case("bad-rrule-truncation-1000");
        let long_rrule = format!("X-{}", "z".repeat(2000));
        let dtstart = Utc.with_ymd_and_hms(2026, 9, 27, 12, 0, 0).unwrap();
        let message = bad_rrule_error(&dtstart, &long_rrule);
        assert_eq!(message.chars().count(), 1000);
        assert_eq!(message, golden["after"]["last_error"].as_str().unwrap());
        assert_eq!(golden["after"]["last_error_len"], 1000);
        assert!(
            message.starts_with("invalid rrule: dtstart=datetime.datetime(2026, 9, 27, 12, 0, ")
        );

        let wide = truncate_last_error(&"é".repeat(2000));
        assert_eq!(wide.chars().count(), 1000);
        assert!(!wide.is_ascii());
        // Short values pass through untouched.
        assert_eq!(truncate_last_error("short"), "short");
        assert_eq!(truncate_last_error(""), "");
    }

    // FAILED-run wording, byte-exact (`scheduler.py:244-248`): the golden's
    // `run.error` is itself a composed message, passed through verbatim.
    #[test]
    fn failed_run_wording_matches_golden() {
        let golden = case("failed-run-pointer");
        assert_eq!(golden["fire_result"], true);
        let message = prompt_build_error("prompt build failed: 'x' is undefined");
        assert_eq!(message, golden["after"]["last_error"].as_str().unwrap());
        assert_eq!(message.chars().count(), 58);
        // The tick is recorded and the cursor stays advanced even for FAILED.
        assert_eq!(golden["after"]["next_run_at"], "2026-09-28T12:01:00+00:00");
        assert_eq!(golden["after"]["last_run_status"], "failed");
        assert!(!golden["after"]["last_run_id"].is_null());
    }

    // Healthy runs clear a stale error both at claim and at pointer time
    // (`scheduler.py:203-205,244-248`); the golden's 29-char stale error
    // lands at `""`.
    #[test]
    fn healthy_run_clears_stale_error() {
        let golden = case("healthy-run-clears-error");
        assert_eq!(
            golden["before"]["last_error"],
            "stale error from a prior tick"
        );
        assert_eq!(golden["before"]["last_error_len"], 29);
        assert_eq!(
            advanced_last_error(golden["before"]["last_error"].as_str().unwrap()),
            ""
        );
        assert_eq!(advanced_last_error(""), "");
        assert_eq!(golden["after"]["last_error"], "");
        assert_eq!(golden["after"]["last_run_status"], "queued");
        assert_eq!(golden["after"]["next_run_at"], "2026-09-28T12:01:00+00:00");
        assert_eq!(golden["fire_result"], true);
    }

    // Rollback wording (`scheduler.py:272-274`): with-reason golden byte-exact
    // (50 chars), `None`/empty reason falls back to the bare wording, and the
    // pre-claim cursor (here NULL — never fired) is the value restored.
    #[test]
    fn rollback_wording_matches_golden() {
        let golden = case("dispatch-none-rollback");
        assert_eq!(golden["fire_result"], false);
        let message = rollback_error(Some("no default pod for workspace test"));
        assert_eq!(message, golden["after"]["last_error"].as_str().unwrap());
        assert_eq!(message.chars().count(), 50);
        assert_eq!(rollback_error(None), "dispatch failed");
        assert_eq!(rollback_error(Some("")), "dispatch failed");
        assert!(golden["before"]["next_run_at"].is_null());
        assert_eq!(
            golden["after"]["next_run_at"],
            golden["before"]["next_run_at"]
        );
        assert!(golden["after"]["last_run_id"].is_null());
    }

    // Happy-path golden pins the advance (`scheduler.py:198-205` + pointer
    // `229-261`): NULL cursor → `12:01:00` (the RRULE expansion output),
    // run pointer set, no error, `True`.
    #[test]
    fn happy_advance_matches_golden() {
        let golden = case("happy-claim-advance");
        assert_eq!(golden["fire_result"], true);
        assert!(golden["before"]["next_run_at"].is_null());
        assert_eq!(golden["after"]["next_run_at"], "2026-09-28T12:01:00+00:00");
        assert_eq!(golden["after"]["last_run_status"], "queued");
        assert_eq!(golden["after"]["last_error"], "");
        assert_eq!(golden["after"]["enabled"], true);
    }

    // Every in-flight skip leaves `next_run_at` untouched and returns `False`.
    #[test]
    fn in_flight_skips_leave_cursor_untouched() {
        let golden = case("last-run-in-flight-skip");
        let skips = golden["skips"].as_array().unwrap();
        assert_eq!(skips.len(), 9);
        for skip in skips {
            assert_eq!(skip["fire_result"], false);
            assert_eq!(skip["next_run_at_unchanged"], true);
        }
    }

    // `py_datetime_repr` matches CPython `repr(datetime)`: no padding, seconds
    // always, micros only when nonzero, `tzinfo=datetime.timezone.utc`.
    #[test]
    fn py_datetime_repr_matches_cpython() {
        let plain = Utc.with_ymd_and_hms(2026, 9, 27, 12, 0, 0).unwrap();
        assert_eq!(
            py_datetime_repr(&plain),
            "datetime.datetime(2026, 9, 27, 12, 0, tzinfo=datetime.timezone.utc)"
        );
        let micro = plain + chrono::Duration::microseconds(250);
        assert_eq!(
            py_datetime_repr(&micro),
            "datetime.datetime(2026, 9, 27, 12, 0, 0, 250, tzinfo=datetime.timezone.utc)"
        );
        let secs = plain + chrono::Duration::seconds(45);
        assert_eq!(
            py_datetime_repr(&secs),
            "datetime.datetime(2026, 9, 27, 12, 0, 45, tzinfo=datetime.timezone.utc)"
        );
    }

    // `coerce_iso_datetimes` mirrors `utils/iso_datetime.py:36-57`: `Z` and
    // offset suffixes, naive assumed UTC, unparseable and non-string items
    // skipped.
    #[test]
    fn coerce_iso_datetimes_mirrors_python() {
        let raw = json!([
            "2026-09-28T12:00:00Z",
            "2026-09-28T14:00:00+02:00",
            "2026-09-28T12:00:00",
            "not a date",
            "",
            42,
            {"nested": true},
            null,
        ]);
        let out = coerce_iso_datetimes(&raw);
        let expect = Utc.with_ymd_and_hms(2026, 9, 28, 12, 0, 0).unwrap();
        assert_eq!(out, vec![expect, expect, expect]);
        assert!(coerce_iso_datetimes(&json!([])).is_empty());
        assert!(coerce_iso_datetimes(&json!(null)).is_empty());
        assert!(coerce_iso_datetimes(&json!({})).is_empty());
    }

    fn parse(sql: &str) -> sqlparser::ast::Statement {
        let mut stmts =
            sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::PostgreSqlDialect {}, sql)
                .unwrap_or_else(|e| panic!("SQL parses: {e}\n{sql}"));
        assert_eq!(stmts.len(), 1);
        stmts.pop().unwrap()
    }

    // Phase 1 claim SQL (`scheduler.py:157-162`): SFU `OF` the binding table
    // only, `deleted_at` guard, inner scheduler join, left run join, pk param.
    #[test]
    fn claim_sql_has_sfu_and_guards() {
        let sql = claim_select_sql();
        assert!(matches!(parse(sql), sqlparser::ast::Statement::Query(_)));
        let lower = sql.to_ascii_lowercase();
        for fragment in [
            "from scheduler_bindings as b",
            "inner join schedulers as s on",
            "left join agent_run as r on",
            "b.deleted_at is null",
            "b.id = $1",
            "for update of b",
        ] {
            assert!(lower.contains(fragment), "missing {fragment} in:\n{sql}");
        }
        // No SKIP LOCKED here (claim-by-id, not a queue scan), no project
        // or workspace joins in phase 1 (Python selects scheduler + last_run
        // only).
        for absent in ["skip locked", "projects", "workspaces", "users"] {
            assert!(!lower.contains(absent), "unexpected {absent} in:\n{sql}");
        }
    }

    // Update statements pin their `update_fields` column sets
    // (`scheduler.py:193-195,205,251,275-277`).
    #[test]
    fn update_sqls_pin_column_sets() {
        let bad = bad_rrule_update_sql().to_ascii_lowercase();
        for fragment in ["last_error = $1", "enabled = false", "next_run_at = null"] {
            assert!(bad.contains(fragment), "missing {fragment} in:\n{bad}");
        }
        let advance = advance_update_sql().to_ascii_lowercase();
        for fragment in ["next_run_at = $1", "last_error = $2", "updated_at = $3"] {
            assert!(
                advance.contains(fragment),
                "missing {fragment} in:\n{advance}"
            );
        }
        assert!(!advance.contains("enabled"));
        let pointer = pointer_update_sql().to_ascii_lowercase();
        for fragment in ["last_run_id = $1", "last_error = $2", "updated_at = $3"] {
            assert!(
                pointer.contains(fragment),
                "missing {fragment} in:\n{pointer}"
            );
        }
        assert!(!pointer.contains("next_run_at"));
        assert!(!pointer.contains("enabled"));
        let rollback = rollback_update_sql().to_ascii_lowercase();
        for fragment in ["next_run_at = $1", "last_error = $2", "updated_at = $3"] {
            assert!(
                rollback.contains(fragment),
                "missing {fragment} in:\n{rollback}"
            );
        }
        // Both phase-3 re-acquires lock the binding row with no deleted_at
        // filter, exactly as Python omits it.
        let reacquire = reacquire_select_sql().to_ascii_lowercase();
        assert!(reacquire.contains("for update of b"));
        assert!(!reacquire.contains("deleted_at"));
        // Phase 2 re-fetch: no lock, no deleted filter.
        let dispatch = dispatch_select_sql().to_ascii_lowercase();
        assert!(!dispatch.contains("for update"));
        assert!(!dispatch.contains("deleted_at"));
        for fragment in [
            "scheduler_id",
            "project_id",
            "workspace_id",
            "actor_id",
            "pod_id",
        ] {
            assert!(
                dispatch.contains(fragment),
                "missing {fragment} in:\n{dispatch}"
            );
        }
    }

    // The registered task name is the exact Python task name, carried by the
    // shared scan const (single source with the fan-out).
    #[test]
    fn task_name_is_exact_python_name() {
        assert_eq!(
            FIRE_SCHEDULER_BINDING_TASK,
            "pi_dash.bgtasks.scheduler.fire_scheduler_binding"
        );
        assert!(TASK_NAMES.contains(&FIRE_SCHEDULER_BINDING_TASK));
    }

    // Fire payloads are Celery v2 wire-identical to `.delay(str(id))`:
    // `args=[str(id)]`, empty kwargs — the same body the worker forward path
    // publishes when the Python plane still owns the task.
    #[test]
    fn fire_payload_is_celery_wire_identical() {
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let message = fire_message(FIRE_SCHEDULER_BINDING_TASK, &id);
        assert_eq!(message.task, FIRE_SCHEDULER_BINDING_TASK);
        assert_eq!(message.args, vec![json!(id.to_string())]);
        assert!(message.kwargs.is_empty());
        assert_eq!(message.retries, 0);
        assert_eq!(
            message.body(),
            json!([
                [id.to_string()],
                {},
                {"callbacks": null, "errbacks": null, "chain": null, "chord": null}
            ])
        );
        let headers = message.headers();
        assert_eq!(headers["task"], FIRE_SCHEDULER_BINDING_TASK);
    }

    // Wire args parse back to the binding id; anything else fails the task
    // (never a silent skip — mirrors Django's `ValidationError` on bad `pk`).
    #[test]
    fn binding_id_from_job_parses_wire_args() {
        use crate::queue::JobRow;
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let mut job = JobRow {
            id: 1,
            celery_id: "celery-id".to_owned(),
            task: FIRE_SCHEDULER_BINDING_TASK.to_owned(),
            args: json!([id.to_string()]),
            kwargs: json!({}),
            queue: "default".to_owned(),
            status: "queued".to_owned(),
            attempts: 0,
            max_retries: 0,
            visible_at: Utc::now(),
            claimed_at: None,
            claimed_by: None,
            created_at: Utc::now(),
            last_error: None,
        };
        assert_eq!(binding_id_from_job(&job).unwrap(), id);
        job.args = json!(["not-a-uuid"]);
        assert!(binding_id_from_job(&job).is_err());
        job.args = json!([]);
        assert!(binding_id_from_job(&job).is_err());
        job.args = json!([42]);
        assert!(binding_id_from_job(&job).is_err());
    }

    // Registry: registering the fire handler flips ownership Local; the scan
    // names are untouched (coexistence handoff — `route_for` contract).
    #[tokio::test]
    async fn registry_owns_fire_after_register() {
        let pool = sqlx::pool::PoolOptions::<sqlx::Postgres>::new()
            .connect_lazy("postgres://127.0.0.1:1/none")
            .expect("lazy pool needs no server");
        let dispatch: SchedulerDispatch = Arc::new(|_| {
            Box::pin(async move {
                Ok(SchedulerDispatchResult {
                    run: None,
                    fail_reason: Some("no dispatcher in unit test".to_owned()),
                })
            })
        });
        let mut registry = Registry::new();
        assert_eq!(
            route_for(&registry, FIRE_SCHEDULER_BINDING_TASK),
            Route::PythonOwned
        );
        register_fire_binding(&mut registry, pool, dispatch);
        assert!(registry.owns(FIRE_SCHEDULER_BINDING_TASK));
        assert_eq!(
            route_for(&registry, FIRE_SCHEDULER_BINDING_TASK),
            Route::Local
        );
    }

    // The kill switch short-circuits before any database touch: with the
    // switch off, the fire returns `False` on a pool that could never connect
    // (`connect_lazy` opens no connection until first use).
    #[tokio::test]
    async fn disabled_switch_fires_false_without_db() {
        let saved = std::env::var(SCHEDULER_ENABLED_ENV).ok();
        std::env::set_var(SCHEDULER_ENABLED_ENV, "false");
        let pool = sqlx::pool::PoolOptions::<sqlx::Postgres>::new()
            .connect_lazy("postgres://127.0.0.1:1/none")
            .expect("lazy pool needs no server");
        let dispatch: SchedulerDispatch = Arc::new(|_| {
            Box::pin(async move {
                panic!("dispatch must not run while disabled");
            })
        });
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let now = Utc::now();
        let fired = fire_scheduler_binding(&pool, &id, &now, &dispatch)
            .await
            .expect("kill switch");
        assert!(!fired);
        match saved {
            Some(v) => std::env::set_var(SCHEDULER_ENABLED_ENV, v),
            None => std::env::remove_var(SCHEDULER_ENABLED_ENV),
        }
    }
}
