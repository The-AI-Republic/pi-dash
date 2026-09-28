//! Per-ticker fire task: `fire_tick` (D-10).
//!
//! Ports the fire half of `pi_dash/bgtasks/agent_ticker.py`
//! (`fire_tick`, lines 77-306). The scan half lives in [`super::scan`];
//! the fire payload it fans out (`args=[str(ticker_id)]`, `kwargs={}`) is
//! what [`register_fire_tick`] consumes here, so no fan-out is dropped or
//! double-run across the handoff.
//!
//! # Claim shape (`agent_ticker.py:97-242`)
//!
//! One transaction locks the ticker row (`SELECT ... FOR UPDATE`, locking
//! only our own row — the Python `select_for_update(of=("self",))`) and
//! reads the related issue (`state_id`, `project_id`) and project tick
//! columns the way `select_related("issue", "issue__state",
//! "issue__project")` does (related rows are read, never locked). After
//! the lock the row is re-checked (missing → `False`, disabled → `False`,
//! `next_run_at` `None`-or-future → `False`), then the cap gate runs
//! (`effective_max_ticks` mirrored as `pool + granted + waited`, `-1`
//! infinite — [`pidash_db::tasks_ticker::models::INFINITE_MAX_TICKS`]; a
//! non-free claim at cap disarms with `pool_spent` iff a pending entry is
//! owed, else `cap_hit`, and returns `False`).
//!
//! Pre-claim skips (non-ticking state, active run, no prior run) leave
//! `next_run_at` unchanged and spend no budget. The clock advance captures
//! every `prev_*` value, charges `used + 1` unless the claim is free,
//! stamps `last_tick_at`, clears the pending entry, and schedules
//! `next_run_at = now + interval + jitter`, disarming on cap-hit,
//! spent-pool-free, user-disabled, or project-ticking-off exactly as the
//! Python branches do.
//!
//! # Seam (D-12 owns the callees)
//!
//! `dispatch_continuation_run`, `is_ticking_state`, `_active_run_for` and
//! `_latest_prior_run` are orchestration territory (PIDASHCONV-49, which is
//! blocked *by* this epic, so no edge) and are not re-ported here: they
//! resolve through [`FireTickSeam`]. The stage→interval-column mapping
//! (`cadence_fields_for`) is orchestration too, so the seam also resolves
//! the tick interval; the models layer's
//! [`pidash_db::tasks_ticker::models::resolve_project_interval`] documents
//! the getattr-default contract the seam implements. Dispatch runs
//! *outside* the claim transaction (the Python `on_commit` drain
//! visibility rule); a `None` dispatch re-acquires the row lock and
//! restores every `prev_*` field *except* `last_tick_at`, which is
//! intentionally kept as the "we attempted a tick" observability stamp
//! (`agent_ticker.py:257-258`).
//!
//! # Replayable core
//!
//! [`decide`] is the whole state machine as a pure function over a
//! [`TickerSnapshot`] plus [`SeamAnswers`], so
//! `fixtures/tasks_ticker/ticker/fire.before_after.json` (FX-TICKER-04)
//! replays without a database. [`fire_tick`] is the same logic against
//! live rows.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * Log lines keep the Python `logging` text verbatim, including
//!   `True`/`False` capitalisation ([`py_bool`]).
//! * Related issue/project rows are read without soft-delete guards,
//!   matching `select_related` (the scanner pins the same absence for the
//!   due-set query in FX-TICKER-03).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rand08::SeedableRng;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::tasks_ticker::models::{
    jitter_seconds, pool_size_or_default, TickerDisarmReason, INFINITE_MAX_TICKS,
};

use crate::queue::JobRow;
use crate::worker::{HandlerError, Registry, Verdict};
use crate::Error;

pub use super::scan::FIRE_TICK_TASK;

/// Trigger for a machine-started tick (`scheduling.py:863`,
/// `AgentRunTrigger.TICK`).
pub const TRIGGER_TICK: &str = "tick";
/// Trigger for a human-started Run AI entry (`scheduling.py:865`,
/// `AgentRunTrigger.RUN_AI`); also the fallback when a free claim carries
/// an empty stored trigger (`agent_ticker.py:190`).
pub const TRIGGER_RUN_AI: &str = "run_ai";

/// Project tick columns read with the claim (`agent_ticker.py:100`,
/// `select_related("issue__project")`). Every field is `Option` because
/// the Python reads go through `getattr` defaults when the project row
/// lacks the field.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProjectTickCols {
    /// `agent_default_max_ticks` (`None` → [`pool_size_or_default`]).
    pub pool: Option<i32>,
    /// `agent_ticking_enabled` (`None` → `true`).
    pub ticking_enabled: Option<bool>,
    /// `agent_default_interval_seconds` (In Progress rhythm).
    pub interval_default: Option<i64>,
    /// `agent_review_default_interval_seconds` (In Review rhythm).
    pub interval_review: Option<i64>,
    /// `agent_test_default_interval_seconds` (In Test rhythm).
    pub interval_test: Option<i64>,
}

/// The ticker row as claimed under lock, plus the issue/project ids the
/// seam needs (`agent_ticker.py:98-103`).
#[derive(Debug, Clone, PartialEq)]
pub struct TickerSnapshot {
    /// `used` — machine-started runs consumed (only `fire_tick` writes it).
    pub used: i32,
    /// `granted` — Re-tick grants.
    pub granted: i32,
    /// `waited` — ticks bought back by `pidash issue wait`.
    pub waited: i32,
    /// `user_disabled` — a human disabled the clock.
    pub user_disabled: bool,
    /// `next_run_at` — `None` means never scheduled (not due).
    pub next_run_at: Option<DateTime<Utc>>,
    /// `last_tick_at` — observability stamp, never rolled back.
    pub last_tick_at: Option<DateTime<Utc>>,
    /// `enabled` — the persisted "clock live" answer.
    pub enabled: bool,
    /// `disarm_reason` — raw stored string (`""` when armed).
    pub disarm_reason: String,
    /// `pending_entry` — an entry run is owed (design §4.5).
    pub pending_entry: bool,
    /// `pending_entry_free` — the owed entry is human-started (unspent).
    pub pending_entry_free: bool,
    /// `pending_entry_actor_id` — who asked (human lever).
    pub pending_entry_actor_id: Option<Uuid>,
    /// `pending_entry_trigger` — `""` for an agent-queued entry.
    pub pending_entry_trigger: String,
}

/// Answers the D-12 seam gives for one fire. In [`decide`] these are plain
/// inputs (fixture-replayable); [`fire_tick`] gathers them through
/// [`FireTickSeam`] in Python order (cap gate first, so skipped rows cost
/// no seam reads).
#[derive(Debug, Clone, PartialEq)]
pub struct SeamAnswers {
    /// `is_ticking_state(issue.state)` (`agent_ticker.py:150`).
    pub ticking_state: bool,
    /// `_active_run_for(issue) is not None` (`:160`).
    pub active_run: bool,
    /// `_latest_prior_run(issue) is None` → `!prior_run` (`:166`).
    pub prior_run: bool,
    /// Stage-resolved tick interval in seconds (`:204`).
    pub interval_seconds: i64,
    /// `jitter_seconds(interval)` draw (`:205`).
    pub jitter_seconds: f64,
}

/// The D-12 seam [`fire_tick`] calls through. Sync methods are pure
/// registry/mapping lookups; async methods touch the database through the
/// implementor's own pool handle (never the claim transaction).
pub trait FireTickSeam: Send + Sync {
    /// `orchestration.agent_phases.is_ticking_state` (`agent_ticker.py:150`).
    fn is_ticking_state(&self, state_id: Option<Uuid>) -> bool;

    /// `ticker.effective_interval_seconds()` (`:204`): the stage→column
    /// resolution plus the project-column read with the stage default.
    fn tick_interval_seconds(&self, state_id: Option<Uuid>, project: &ProjectTickCols) -> i64;

    /// `_active_run_for(issue) is not None` (`:160`).
    fn has_active_run(
        &self,
        issue_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<bool, sqlx::Error>> + Send + '_>>;

    /// `_latest_prior_run(issue) is not None` (`:166`).
    fn has_prior_run(
        &self,
        issue_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<bool, sqlx::Error>> + Send + '_>>;

    /// `dispatch_continuation_run(issue, triggered_by, actor)` (`:253`):
    /// the created run's id, or `None` when dispatch declines (active run
    /// raced in, no pod, no creator, eligibility bounce).
    fn dispatch_continuation_run(
        &self,
        issue_id: Uuid,
        triggered_by: &str,
        actor: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Uuid>, sqlx::Error>> + Send + '_>>;
}

/// Render a bool the way Python `%s` does (`True`/`False`): the dispatch
/// log line keeps the exact field text (`agent_ticker.py:297-305`).
pub fn py_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

/// Mirror of `ticker.effective_max_ticks()` (`issue_agent_ticker.py:205-214`):
/// `-1` (infinite) short-circuits, otherwise `pool + granted + waited`.
/// `scan_due_tickers` reproduces this sum in SQL; change one and the other
/// must change.
pub fn effective_max_ticks(pool: i32, granted: i32, waited: i32) -> i32 {
    if pool == INFINITE_MAX_TICKS {
        return INFINITE_MAX_TICKS;
    }
    pool + granted + waited
}

/// Outcome of the post-lock re-checks plus the cap gate
/// (`agent_ticker.py:104-145`), before any seam read.
#[derive(Debug, Clone, PartialEq)]
pub enum PreDecision {
    /// Return `False`; the row is untouched.
    Skip,
    /// The pool is spent: write the disarm row and return `False`.
    CapDisarm {
        /// `pool_spent` iff a pending entry is owed, else `cap_hit` (`:127-129`).
        disarm_reason: TickerDisarmReason,
    },
    /// Past the gate: gather seam answers, then [`build_claim`].
    Proceed {
        /// `pending_entry AND pending_entry_free` (`:119`).
        free_claim: bool,
        /// Mirrored `effective_max_ticks()`.
        cap: i32,
    },
}

/// Post-lock re-checks and cap gate (`agent_ticker.py:104-145`).
pub fn precheck(ticker: &TickerSnapshot, pool: i32, now: &DateTime<Utc>) -> PreDecision {
    if !ticker.enabled {
        return PreDecision::Skip;
    }
    match ticker.next_run_at {
        Some(next) if next <= *now => {}
        _ => return PreDecision::Skip,
    }
    // A pending entry fires even on a spent pool when it is free
    // (human-started); the cap check below only ever stops timer ticks.
    let free_claim = ticker.pending_entry && ticker.pending_entry_free;
    let cap = effective_max_ticks(pool, ticker.granted, ticker.waited);
    if !free_claim && cap != INFINITE_MAX_TICKS && ticker.used >= cap {
        // A queued (counting) entry that finds the pool spent — the
        // project pool was lowered after it was queued — consumed
        // nothing, so it parks as `pool_spent`, not `cap_hit`.
        let disarm_reason = if ticker.pending_entry {
            TickerDisarmReason::PoolSpent
        } else {
            TickerDisarmReason::CapHit
        };
        return PreDecision::CapDisarm { disarm_reason };
    }
    PreDecision::Proceed { free_claim, cap }
}

/// A claimed tick: the row writes plus the dispatch arguments
/// (`agent_ticker.py:179-242`). `prev_*` is the full rollback image;
/// `last_tick_at` is deliberately *not* part of it (`:257-258`).
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimTicket {
    /// `used + 1`, unless the claim is free (`:194-195`).
    pub used: i32,
    /// `now + interval + jitter` (`:204-205`).
    pub next_run_at: DateTime<Utc>,
    /// `false` when a disarm branch fired, else the (enabled) current value.
    pub enabled: bool,
    /// Raw stored string: new reason on a disarm branch, current otherwise.
    pub disarm_reason: String,
    /// Pending actor on a free claim, else `None` (`:189`).
    pub claim_actor: Option<Uuid>,
    /// Pending trigger (or `run_ai` when empty) on a free claim, else
    /// `tick` (`:190`).
    pub claim_trigger: String,
    /// The `:119` free-claim bit, for the dispatch log line.
    pub free_claim: bool,
    /// `used >= cap` after charging (`:207-209`), for the log line.
    pub cap_hit_now: bool,
    /// Rollback image (`:179-186`).
    pub prev_used: i32,
    pub prev_next_run_at: Option<DateTime<Utc>>,
    pub prev_enabled: bool,
    pub prev_disarm_reason: String,
    pub prev_pending_entry: bool,
    pub prev_pending_entry_free: bool,
    pub prev_pending_entry_actor_id: Option<Uuid>,
    pub prev_pending_entry_trigger: String,
}

/// Pre-claim skips plus the clock advance (`agent_ticker.py:150-242`).
/// Returns `None` when a skip fires (`False`, row untouched).
pub fn build_claim(
    ticker: &TickerSnapshot,
    project_ticking_enabled: bool,
    now: &DateTime<Utc>,
    answers: &SeamAnswers,
    free_claim: bool,
    cap: i32,
) -> Option<ClaimTicket> {
    if !answers.ticking_state {
        return None;
    }
    // All three skips leave `next_run_at` unchanged so the scanner
    // re-checks next minute, and spend no budget.
    if answers.active_run {
        return None;
    }
    if !answers.prior_run {
        return None;
    }
    let claim_actor = if free_claim {
        ticker.pending_entry_actor_id
    } else {
        None
    };
    let claim_trigger = if free_claim {
        if ticker.pending_entry_trigger.is_empty() {
            TRIGGER_RUN_AI.to_owned()
        } else {
            ticker.pending_entry_trigger.clone()
        }
    } else {
        TRIGGER_TICK.to_owned()
    };
    // Only machine-started runs spend the pool; a human's free entry does not.
    let used = if free_claim {
        ticker.used
    } else {
        ticker.used + 1
    };
    let next_run_at = *now
        + chrono::Duration::milliseconds(
            ((answers.interval_seconds as f64 + answers.jitter_seconds) * 1000.0).round() as i64,
        );
    let cap_hit_now = cap != INFINITE_MAX_TICKS && used >= cap;
    let (enabled, disarm_reason) = if cap_hit_now && free_claim {
        // A free run on an already-spent pool stays stopped as
        // `pool_spent`, so the deferred auto-pause never fires on it.
        (false, TickerDisarmReason::PoolSpent.as_str().to_owned())
    } else if cap_hit_now {
        // Disarm immediately; the In Progress → Paused transition is
        // deferred to the run-terminate hook (§4.4.1).
        (false, TickerDisarmReason::CapHit.as_str().to_owned())
    } else if ticker.user_disabled || !project_ticking_enabled {
        // A queued human entry fires even on a switched-off clock, but no
        // timer tick may follow it.
        let reason = if ticker.user_disabled {
            TickerDisarmReason::UserDisabled
        } else {
            TickerDisarmReason::None
        };
        (false, reason.as_str().to_owned())
    } else {
        (ticker.enabled, ticker.disarm_reason.clone())
    };
    Some(ClaimTicket {
        used,
        next_run_at,
        enabled,
        disarm_reason,
        claim_actor,
        claim_trigger,
        free_claim,
        cap_hit_now,
        prev_used: ticker.used,
        prev_next_run_at: ticker.next_run_at,
        prev_enabled: ticker.enabled,
        prev_disarm_reason: ticker.disarm_reason.clone(),
        prev_pending_entry: ticker.pending_entry,
        prev_pending_entry_free: ticker.pending_entry_free,
        prev_pending_entry_actor_id: ticker.pending_entry_actor_id,
        prev_pending_entry_trigger: ticker.pending_entry_trigger.clone(),
    })
}

/// Full pure decision: [`precheck`] then the seam-gated [`build_claim`]
/// (`agent_ticker.py:104-242`). The fixture replay entry point.
#[derive(Debug, Clone, PartialEq)]
pub enum FireDecision {
    /// Return `False`; the row is untouched.
    Skip,
    /// Write the cap-disarm row, then return `False`.
    CapDisarm { disarm_reason: TickerDisarmReason },
    /// Row writes to apply, then dispatch outside the transaction.
    Claimed(ClaimTicket),
}

pub fn decide(
    ticker: &TickerSnapshot,
    pool: i32,
    project_ticking_enabled: bool,
    now: &DateTime<Utc>,
    answers: &SeamAnswers,
) -> FireDecision {
    match precheck(ticker, pool, now) {
        PreDecision::Skip => FireDecision::Skip,
        PreDecision::CapDisarm { disarm_reason } => FireDecision::CapDisarm { disarm_reason },
        PreDecision::Proceed { free_claim, cap } => match build_claim(
            ticker,
            project_ticking_enabled,
            now,
            answers,
            free_claim,
            cap,
        ) {
            None => FireDecision::Skip,
            Some(ticket) => FireDecision::Claimed(ticket),
        },
    }
}

/// Rollback image for a `None` dispatch (`agent_ticker.py:259-291`): every
/// `prev_*` field restored, `last_tick_at` intentionally kept at the claim
/// time passed in (observability, `:257-258`). Fields the claim never
/// writes (`granted`, `waited`, `user_disabled`) carry over from `before`.
pub fn rollback_snapshot(
    before: &TickerSnapshot,
    ticket: &ClaimTicket,
    kept_last_tick_at: DateTime<Utc>,
) -> TickerSnapshot {
    TickerSnapshot {
        used: ticket.prev_used,
        granted: before.granted,
        waited: before.waited,
        user_disabled: before.user_disabled,
        next_run_at: ticket.prev_next_run_at,
        last_tick_at: Some(kept_last_tick_at),
        enabled: ticket.prev_enabled,
        disarm_reason: ticket.prev_disarm_reason.clone(),
        pending_entry: ticket.prev_pending_entry,
        pending_entry_free: ticket.prev_pending_entry_free,
        pending_entry_actor_id: ticket.prev_pending_entry_actor_id,
        pending_entry_trigger: ticket.prev_pending_entry_trigger.clone(),
    }
}

/// Lock and read the ticker plus its related rows
/// (`agent_ticker.py:98-103`: `select_for_update(of=("self",))` with
/// `select_related("issue", "issue__state", "issue__project")`). Only the
/// ticker row is locked; the issue/project rows are read unlocked, exactly
/// like the Python. Returns `None` when the row is missing (`:104-105`).
async fn read_claim(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ticker_id: Uuid,
) -> Result<Option<ClaimRead>, sqlx::Error> {
    use sqlx::Row;
    let row = sqlx::query(
        r#"SELECT issue_id, "used", granted, waited, user_disabled,
                  next_run_at, last_tick_at, enabled, disarm_reason,
                  pending_entry, pending_entry_free,
                  pending_entry_actor_id, pending_entry_trigger
           FROM issue_agent_ticker
           WHERE id = $1 AND deleted_at IS NULL
           FOR UPDATE"#,
    )
    .bind(ticker_id)
    .fetch_optional(&mut **tx)
    .await?;
    let row = match row {
        Some(row) => row,
        None => return Ok(None),
    };
    let snapshot = TickerSnapshot {
        used: row.try_get("used")?,
        granted: row.try_get("granted")?,
        waited: row.try_get("waited")?,
        user_disabled: row.try_get("user_disabled")?,
        next_run_at: row.try_get("next_run_at")?,
        last_tick_at: row.try_get("last_tick_at")?,
        enabled: row.try_get("enabled")?,
        disarm_reason: row.try_get("disarm_reason")?,
        pending_entry: row.try_get("pending_entry")?,
        pending_entry_free: row.try_get("pending_entry_free")?,
        pending_entry_actor_id: row.try_get("pending_entry_actor_id")?,
        pending_entry_trigger: row.try_get("pending_entry_trigger")?,
    };
    let issue_id: Uuid = row.try_get("issue_id")?;
    let issue = sqlx::query("SELECT state_id, project_id FROM issues WHERE id = $1")
        .bind(issue_id)
        .fetch_optional(&mut **tx)
        .await?;
    let (state_id, project_id): (Option<Uuid>, Option<Uuid>) = match issue {
        Some(issue) => (issue.try_get("state_id")?, issue.try_get("project_id")?),
        None => (None, None),
    };
    let project = match project_id {
        Some(pid) => sqlx::query_as(
            r#"SELECT agent_default_max_ticks, agent_ticking_enabled,
                      agent_default_interval_seconds,
                      agent_review_default_interval_seconds,
                      agent_test_default_interval_seconds
               FROM projects WHERE id = $1"#,
        )
        .bind(pid)
        .fetch_optional(&mut **tx)
        .await?
        .unwrap_or_default(),
        None => ProjectTickCols::default(),
    };
    Ok(Some(ClaimRead {
        snapshot,
        issue_id,
        state_id,
        project,
    }))
}

/// Claim-time read: the snapshot plus the ids/columns the seam needs.
#[derive(Debug, Clone)]
pub struct ClaimRead {
    pub snapshot: TickerSnapshot,
    pub issue_id: Uuid,
    pub state_id: Option<Uuid>,
    pub project: ProjectTickCols,
}

impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for ProjectTickCols {
    fn from_row(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            pool: row.try_get("agent_default_max_ticks")?,
            ticking_enabled: row.try_get("agent_ticking_enabled")?,
            interval_default: row.try_get("agent_default_interval_seconds")?,
            interval_review: row.try_get("agent_review_default_interval_seconds")?,
            interval_test: row.try_get("agent_test_default_interval_seconds")?,
        })
    }
}

/// Pre-claim cap-disarm write (`agent_ticker.py:126-144`): `enabled`,
/// `disarm_reason`, the four pending columns, `updated_at`. `used`,
/// `next_run_at` and `last_tick_at` are untouched.
async fn write_cap_disarm(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ticker_id: Uuid,
    disarm_reason: TickerDisarmReason,
    now: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE issue_agent_ticker
           SET enabled = FALSE, disarm_reason = $1,
               pending_entry = FALSE, pending_entry_free = FALSE,
               pending_entry_actor_id = NULL, pending_entry_trigger = '',
               updated_at = $2
           WHERE id = $3"#,
    )
    .bind(disarm_reason.as_str())
    .bind(now)
    .bind(ticker_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Clock-advance write (`agent_ticker.py:229-242`): `used`, `last_tick_at`,
/// `next_run_at`, `enabled`, `disarm_reason`, the four pending columns,
/// `updated_at`.
async fn write_claim(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ticker_id: Uuid,
    ticket: &ClaimTicket,
    now: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE issue_agent_ticker
           SET "used" = $1, last_tick_at = $2, next_run_at = $3,
               enabled = $4, disarm_reason = $5,
               pending_entry = FALSE, pending_entry_free = FALSE,
               pending_entry_actor_id = NULL, pending_entry_trigger = '',
               updated_at = $6
           WHERE id = $7"#,
    )
    .bind(ticket.used)
    .bind(now)
    .bind(ticket.next_run_at)
    .bind(ticket.enabled)
    .bind(&ticket.disarm_reason)
    .bind(now)
    .bind(ticker_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Rollback write (`agent_ticker.py:279-291`): every `prev_*` field.
/// `last_tick_at` is *not* written — the claim-time stamp stays.
async fn write_rollback(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ticker_id: Uuid,
    ticket: &ClaimTicket,
    now: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE issue_agent_ticker
           SET "used" = $1, next_run_at = $2,
               enabled = $3, disarm_reason = $4,
               pending_entry = $5, pending_entry_free = $6,
               pending_entry_actor_id = $7, pending_entry_trigger = $8,
               updated_at = $9
           WHERE id = $10"#,
    )
    .bind(ticket.prev_used)
    .bind(ticket.prev_next_run_at)
    .bind(ticket.prev_enabled)
    .bind(&ticket.prev_disarm_reason)
    .bind(ticket.prev_pending_entry)
    .bind(ticket.prev_pending_entry_free)
    .bind(ticket.prev_pending_entry_actor_id)
    .bind(&ticket.prev_pending_entry_trigger)
    .bind(now)
    .bind(ticker_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Per-ticker worker (`agent_ticker.py:77-306`).
///
/// Returns `true` iff a continuation run was dispatched. `now` is the
/// firing instant (`timezone.now()`); the jitter draw comes from `rng`
/// (tests replay with a fixed `0.0` draw through [`decide`]).
pub async fn fire_tick(
    pool: &PgPool,
    seam: &Arc<dyn FireTickSeam>,
    ticker_id: Uuid,
    now: DateTime<Utc>,
    rng: &mut impl rand08::Rng,
) -> Result<bool, Error> {
    let mut tx = pool.begin().await?;
    let read = match read_claim(&mut tx, ticker_id).await? {
        Some(read) => read,
        None => return Ok(false),
    };
    let snapshot = &read.snapshot;
    let pool_size = pool_size_or_default(read.project.pool);
    let ticking_enabled = read.project.ticking_enabled.unwrap_or(true);

    match precheck(snapshot, pool_size, &now) {
        PreDecision::Skip => Ok(false),
        PreDecision::CapDisarm { disarm_reason } => {
            write_cap_disarm(&mut tx, ticker_id, disarm_reason, &now).await?;
            tx.commit().await?;
            Ok(false)
        }
        PreDecision::Proceed { free_claim, cap } => {
            if !seam.is_ticking_state(read.state_id) {
                return Ok(false);
            }
            if seam.has_active_run(read.issue_id).await? {
                tracing::info!(
                    "agent_ticker.fire_tick: skip issue={} reason=active-run-exists",
                    read.issue_id,
                );
                return Ok(false);
            }
            if !seam.has_prior_run(read.issue_id).await? {
                tracing::info!(
                    "agent_ticker.fire_tick: skip issue={} reason=no-prior-run",
                    read.issue_id,
                );
                return Ok(false);
            }
            let interval = seam.tick_interval_seconds(read.state_id, &read.project);
            let jitter = jitter_seconds(interval, rng);
            let answers = SeamAnswers {
                ticking_state: true,
                active_run: false,
                prior_run: true,
                interval_seconds: interval,
                jitter_seconds: jitter,
            };
            let ticket =
                match build_claim(snapshot, ticking_enabled, &now, &answers, free_claim, cap) {
                    Some(ticket) => ticket,
                    None => return Ok(false),
                };
            write_claim(&mut tx, ticker_id, &ticket, &now).await?;
            tx.commit().await?;

            // Dispatch outside the transaction so the ticker write is
            // visible to other tx-bounded readers (`:244-247`).
            let run = seam
                .dispatch_continuation_run(read.issue_id, &ticket.claim_trigger, ticket.claim_actor)
                .await?;
            match run {
                None => {
                    let mut rollback_tx = pool.begin().await?;
                    let exists: Option<(Uuid,)> = sqlx::query_as(
                        "SELECT id FROM issue_agent_ticker WHERE id = $1 FOR UPDATE",
                    )
                    .bind(ticker_id)
                    .fetch_optional(&mut *rollback_tx)
                    .await?;
                    if exists.is_some() {
                        write_rollback(&mut rollback_tx, ticker_id, &ticket, &now).await?;
                    }
                    rollback_tx.commit().await?;
                    tracing::info!(
                        "agent_ticker.fire_tick: dispatch returned None issue={}; rolled back claim",
                        read.issue_id,
                    );
                    Ok(false)
                }
                Some(run_id) => {
                    tracing::info!(
                        "agent_ticker.fire_tick: dispatched run={} issue={} trigger={} used={} free={} cap_hit={}",
                        run_id,
                        read.issue_id,
                        ticket.claim_trigger,
                        ticket.used,
                        py_bool(ticket.free_claim),
                        py_bool(ticket.cap_hit_now),
                    );
                    Ok(true)
                }
            }
        }
    }
}

/// Parse the `fire_tick.delay(str(ticker_id))` payload: `args=[str]`
/// (`agent_ticker.py:71`). Anything else is a poison message — park it,
/// never retry it.
pub fn parse_fire_tick_arg(job: &JobRow) -> Result<Uuid, String> {
    let args = job
        .args
        .as_array()
        .ok_or_else(|| format!("{FIRE_TICK_TASK}: args is not an array: {}", job.args))?;
    let first = args
        .first()
        .ok_or_else(|| format!("{FIRE_TICK_TASK}: args is empty"))?;
    let raw = first
        .as_str()
        .ok_or_else(|| format!("{FIRE_TICK_TASK}: args[0] is not a string: {first}"))?;
    Uuid::parse_str(raw)
        .map_err(|e| format!("{FIRE_TICK_TASK}: args[0] is not a UUID ({raw}): {e}"))
}

/// Register the local `fire_tick` handler, flipping ownership of
/// `pi_dash.bgtasks.agent_ticker.fire_tick` from the Python plane to this
/// binary (the coexistence rule: a registered name runs locally, everything
/// else forwards — [`crate::worker::route_for`]).
pub fn register_fire_tick(registry: &mut Registry, pool: PgPool, seam: Arc<dyn FireTickSeam>) {
    registry.register(
        FIRE_TICK_TASK,
        Arc::new(move |job: JobRow| {
            let pool = pool.clone();
            let seam = seam.clone();
            let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                Box::pin(async move {
                    let ticker_id = match parse_fire_tick_arg(&job) {
                        Ok(id) => id,
                        Err(error) => return Ok(Verdict::Fail { error }),
                    };
                    let now = Utc::now();
                    // `StdRng` (not `thread_rng`): the handler future must be `Send`.
                    let mut rng = rand08::rngs::StdRng::from_entropy();
                    fire_tick(&pool, &seam, ticker_id, now, &mut rng)
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
    use super::super::scan::fire_message;
    use super::*;
    use crate::worker::{route_for, Route};
    use chrono::TimeZone;
    use serde_json::{json, Value};

    /// The frozen instant the fixtures record (`ticker/fire.before_after.json "now"`).
    fn frozen() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 28, 12, 0, 0).unwrap()
    }

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/tasks_ticker")
    }

    fn fixture(name: &str) -> Value {
        let text = std::fs::read_to_string(fixtures_dir().join(name))
            .unwrap_or_else(|e| panic!("read fixture {name}: {e}"));
        serde_json::from_str(&text).expect("fixture is valid JSON")
    }

    fn opt_dt(value: &Value) -> Option<DateTime<Utc>> {
        match value {
            Value::Null => None,
            Value::String(s) => Some(s.parse().unwrap_or_else(|e| panic!("parse dt {s}: {e}"))),
            other => panic!("expected dt-or-null, got {other}"),
        }
    }

    fn opt_uuid(value: &Value) -> Option<Uuid> {
        match value {
            Value::Null => None,
            Value::String(s) => Some(Uuid::parse_str(s).expect("fixture uuid")),
            other => panic!("expected uuid-or-null, got {other}"),
        }
    }

    fn snapshot_from(side: &Value) -> TickerSnapshot {
        TickerSnapshot {
            used: side["used"].as_i64().unwrap() as i32,
            granted: side["granted"].as_i64().unwrap() as i32,
            waited: side["waited"].as_i64().unwrap() as i32,
            user_disabled: side["user_disabled"].as_bool().unwrap(),
            next_run_at: opt_dt(&side["next_run_at"]),
            last_tick_at: opt_dt(&side["last_tick_at"]),
            enabled: side["enabled"].as_bool().unwrap(),
            disarm_reason: side["disarm_reason"].as_str().unwrap().to_owned(),
            pending_entry: side["pending_entry"].as_bool().unwrap(),
            pending_entry_free: side["pending_entry_free"].as_bool().unwrap(),
            pending_entry_actor_id: opt_uuid(&side["pending_entry_actor_id"]),
            pending_entry_trigger: side["pending_entry_trigger"].as_str().unwrap().to_owned(),
        }
    }

    /// The firing seam: ticking state, no active run, a prior run, the
    /// recorded 3 h interval, jitter mocked to `0.0` — exactly the
    /// fixture's `_method` line.
    fn firing_answers() -> SeamAnswers {
        SeamAnswers {
            ticking_state: true,
            active_run: false,
            prior_run: true,
            interval_seconds: 10800,
            jitter_seconds: 0.0,
        }
    }

    fn assert_snapshot_eq(actual: &TickerSnapshot, expected: &TickerSnapshot, ctx: &str) {
        assert_eq!(actual.used, expected.used, "{ctx}: used");
        assert_eq!(actual.granted, expected.granted, "{ctx}: granted");
        assert_eq!(actual.waited, expected.waited, "{ctx}: waited");
        assert_eq!(
            actual.user_disabled, expected.user_disabled,
            "{ctx}: user_disabled"
        );
        assert_eq!(
            actual.next_run_at, expected.next_run_at,
            "{ctx}: next_run_at"
        );
        assert_eq!(
            actual.last_tick_at, expected.last_tick_at,
            "{ctx}: last_tick_at"
        );
        assert_eq!(actual.enabled, expected.enabled, "{ctx}: enabled");
        assert_eq!(
            actual.disarm_reason, expected.disarm_reason,
            "{ctx}: disarm_reason"
        );
        assert_eq!(
            actual.pending_entry, expected.pending_entry,
            "{ctx}: pending_entry"
        );
        assert_eq!(
            actual.pending_entry_free, expected.pending_entry_free,
            "{ctx}: pending_entry_free"
        );
        assert_eq!(
            actual.pending_entry_actor_id, expected.pending_entry_actor_id,
            "{ctx}: pending_entry_actor_id"
        );
        assert_eq!(
            actual.pending_entry_trigger, expected.pending_entry_trigger,
            "{ctx}: pending_entry_trigger"
        );
    }

    // FX-TICKER-04 (`bgtasks/agent_ticker.py:77-306`): every recorded case
    // replays through [`decide`] to the recorded after-row, the recorded
    // `fire_result`, and (on dispatch) the recorded trigger. `True` comes
    // out only when a run was dispatched.
    #[test]
    fn fx_ticker_04_replays_green() {
        let fx = fixture("ticker/fire.before_after.json");
        assert_eq!(fx["now"].as_str().unwrap(), "2026-09-28T12:00:00+00:00");
        let now = frozen();
        let cases = fx["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 12);
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let before = snapshot_from(&case["before"]);
            let after = snapshot_from(&case["after"]);
            let pool = case["before"]["pool"].as_i64().unwrap() as i32;
            let fire_result = case["fire_result"].as_bool().unwrap();
            // recheck-missing (`:104-105`) never reaches the decision: the
            // locked read finds no row, the probe row stays untouched.
            if name == "recheck-missing" {
                assert!(!fire_result, "{name}: fires False");
                assert_snapshot_eq(&after, &before, name);
                continue;
            }
            let mut answers = firing_answers();
            if name == "preclaim-active-run" {
                answers.active_run = true;
            }
            if name == "preclaim-no-prior-run" {
                answers.prior_run = false;
            }
            let decision = decide(&before, pool, true, &now, &answers);
            match decision {
                FireDecision::Skip => {
                    assert!(!fire_result, "{name}: skip fires False");
                    assert_snapshot_eq(&after, &before, name);
                }
                FireDecision::CapDisarm { disarm_reason } => {
                    assert!(!fire_result, "{name}: cap disarm fires False");
                    assert!(!after.enabled, "{name}: disarmed");
                    assert_eq!(
                        after.disarm_reason,
                        disarm_reason.as_str(),
                        "{name}: reason"
                    );
                    assert!(!after.pending_entry, "{name}: pending cleared");
                    assert!(!after.pending_entry_free, "{name}: pending-free cleared");
                    assert_eq!(after.pending_entry_actor_id, None, "{name}: actor cleared");
                    assert_eq!(after.pending_entry_trigger, "", "{name}: trigger cleared");
                    // Everything else untouched: no budget spent, no clock move.
                    assert_eq!(after.used, before.used, "{name}: used");
                    assert_eq!(after.next_run_at, before.next_run_at, "{name}: next_run_at");
                    assert_eq!(
                        after.last_tick_at, before.last_tick_at,
                        "{name}: last_tick_at"
                    );
                }
                FireDecision::Claimed(ticket) => {
                    // The rollback image is the before-row on every field.
                    assert_eq!(ticket.prev_used, before.used, "{name}: prev_used");
                    assert_eq!(
                        ticket.prev_next_run_at, before.next_run_at,
                        "{name}: prev_next"
                    );
                    assert_eq!(ticket.prev_enabled, before.enabled, "{name}: prev_enabled");
                    assert_eq!(
                        ticket.prev_disarm_reason, before.disarm_reason,
                        "{name}: prev_disarm"
                    );
                    assert_eq!(
                        ticket.prev_pending_entry, before.pending_entry,
                        "{name}: prev_pending"
                    );
                    assert_eq!(
                        ticket.prev_pending_entry_actor_id, before.pending_entry_actor_id,
                        "{name}: prev_actor"
                    );
                    assert_eq!(
                        ticket.prev_pending_entry_trigger, before.pending_entry_trigger,
                        "{name}: prev_trigger"
                    );
                    if name == "dispatch-none-rollback" {
                        // Mocked dispatch `None` (`:254-296`): every field
                        // restored except `last_tick_at`, which stays at the
                        // claim time (`:257-258`).
                        assert!(!fire_result, "{name}: rollback fires False");
                        let rolled = rollback_snapshot(&before, &ticket, now);
                        assert_snapshot_eq(&rolled, &after, name);
                        assert_eq!(after.last_tick_at, Some(now), "{name}: last_tick_at kept");
                        assert_eq!(after.disarm_reason, "", "{name}: disarm NONE restored");
                        continue;
                    }
                    assert!(fire_result, "{name}: dispatch fires True");
                    let dispatched = case["dispatched_run"]["trigger"].as_str().unwrap();
                    assert_eq!(ticket.claim_trigger, dispatched, "{name}: trigger");
                    // The after-row is the ticket, stamped and de-queued.
                    assert_eq!(after.used, ticket.used, "{name}: used");
                    assert_eq!(after.next_run_at, Some(ticket.next_run_at), "{name}: next");
                    assert_eq!(after.enabled, ticket.enabled, "{name}: enabled");
                    assert_eq!(after.disarm_reason, ticket.disarm_reason, "{name}: disarm");
                    assert_eq!(after.last_tick_at, Some(now), "{name}: last_tick_at");
                    assert!(!after.pending_entry, "{name}: pending cleared");
                    assert!(!after.pending_entry_free, "{name}: pending-free cleared");
                    // Branch pins from the trace lines.
                    match name {
                        name if name == "happy-tick" || name == "user-disabled-disarm" => {
                            assert_eq!(after.used, before.used + 1, "{name}: charged");
                        }
                        _ => {}
                    }
                    if name == "free-claim-spent-pool" {
                        assert!(ticket.free_claim, "{name}: free");
                        assert_eq!(after.used, before.used, "{name}: free run unspent");
                        assert!(ticket.cap_hit_now, "{name}: cap_hit");
                        assert_eq!(after.disarm_reason, "pool_spent", "{name}: not cap_hit");
                    }
                    if name == "clock-advance-cap-edge" {
                        assert!(ticket.cap_hit_now, "{name}: cap_hit");
                        assert_eq!(after.disarm_reason, "cap_hit", "{name}");
                    }
                }
            }
        }
    }

    // The fixture has no non-ticking-state case; the Python early return
    // (`:150-151`) still applies and leaves the row untouched.
    #[test]
    fn non_ticking_state_skips_without_touching_row() {
        let before = TickerSnapshot {
            used: 0,
            granted: 0,
            waited: 0,
            user_disabled: false,
            next_run_at: Some(frozen() - chrono::Duration::seconds(1)),
            last_tick_at: None,
            enabled: true,
            disarm_reason: String::new(),
            pending_entry: false,
            pending_entry_free: false,
            pending_entry_actor_id: None,
            pending_entry_trigger: String::new(),
        };
        let mut answers = firing_answers();
        answers.ticking_state = false;
        assert_eq!(
            decide(&before, 10, true, &frozen(), &answers),
            FireDecision::Skip
        );
    }

    // `-1` is infinite: no cap gate, no cap-hit disarm, whatever `used`
    // says (`effective_max_ticks`, `cap_reached`).
    #[test]
    fn infinite_pool_never_disarms() {
        let before = TickerSnapshot {
            used: 1_000_000,
            granted: 0,
            waited: 0,
            user_disabled: false,
            next_run_at: Some(frozen() - chrono::Duration::seconds(1)),
            last_tick_at: None,
            enabled: true,
            disarm_reason: String::new(),
            pending_entry: false,
            pending_entry_free: false,
            pending_entry_actor_id: None,
            pending_entry_trigger: String::new(),
        };
        let decision = decide(
            &before,
            INFINITE_MAX_TICKS,
            true,
            &frozen(),
            &firing_answers(),
        );
        let ticket = match decision {
            FireDecision::Claimed(ticket) => ticket,
            other => panic!("infinite pool must claim, got {other:?}"),
        };
        assert_eq!(ticket.used, 1_000_001);
        assert!(ticket.enabled);
        assert_eq!(ticket.disarm_reason, "");
        assert!(!ticket.cap_hit_now);
    }

    // Re-tick grants and waits extend the cap arm for arm with the scanner SQL.
    #[test]
    fn granted_and_waited_extend_the_cap() {
        let mut before = TickerSnapshot {
            used: 12,
            granted: 5,
            waited: 0,
            user_disabled: false,
            next_run_at: Some(frozen() - chrono::Duration::seconds(1)),
            last_tick_at: None,
            enabled: true,
            disarm_reason: String::new(),
            pending_entry: false,
            pending_entry_free: false,
            pending_entry_actor_id: None,
            pending_entry_trigger: String::new(),
        };
        // Cap is 10 + 5 = 15: used=12 still claims.
        assert!(matches!(
            decide(&before, 10, true, &frozen(), &firing_answers()),
            FireDecision::Claimed(_)
        ));
        // Without the grant the same row disarms as cap_hit.
        before.granted = 0;
        assert_eq!(
            decide(&before, 10, true, &frozen(), &firing_answers()),
            FireDecision::CapDisarm {
                disarm_reason: TickerDisarmReason::CapHit
            }
        );
        assert_eq!(effective_max_ticks(10, 5, 3), 18);
        assert_eq!(
            effective_max_ticks(INFINITE_MAX_TICKS, 0, 0),
            INFINITE_MAX_TICKS
        );
    }

    // A project with ticking switched off fires the queued entry once, then
    // disarms with NONE (not USER_DISABLED); a user-disabled clock disarms
    // with USER_DISABLED (`agent_ticker.py:221-227`).
    #[test]
    fn project_ticking_off_disarms_with_none() {
        let before = TickerSnapshot {
            used: 0,
            granted: 0,
            waited: 0,
            user_disabled: false,
            next_run_at: Some(frozen() - chrono::Duration::seconds(1)),
            last_tick_at: None,
            enabled: true,
            disarm_reason: String::new(),
            pending_entry: false,
            pending_entry_free: false,
            pending_entry_actor_id: None,
            pending_entry_trigger: String::new(),
        };
        let ticket = match decide(&before, 10, false, &frozen(), &firing_answers()) {
            FireDecision::Claimed(ticket) => ticket,
            other => panic!("queued entry must fire once, got {other:?}"),
        };
        assert!(!ticket.enabled);
        assert_eq!(ticket.disarm_reason, "");
    }

    #[test]
    fn log_bools_match_python_capitalisation() {
        assert_eq!(py_bool(true), "True");
        assert_eq!(py_bool(false), "False");
    }

    // Task name + payload are wire-identical to the Celery v2
    // `fire_tick.delay(str(ticker_id))` call: exact name, `args=[str]`,
    // empty kwargs, and the body/headers shape the worker forward path
    // publishes.
    #[test]
    fn fire_tick_message_is_celery_wire_identical() {
        assert_eq!(FIRE_TICK_TASK, "pi_dash.bgtasks.agent_ticker.fire_tick");
        assert_eq!(super::super::scan::FIRE_TICK_TASK, FIRE_TICK_TASK);
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let message = fire_message(FIRE_TICK_TASK, &id);
        assert_eq!(message.task, FIRE_TICK_TASK);
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
        assert_eq!(headers["task"], FIRE_TICK_TASK);
        assert_eq!(headers["id"], message.id);
    }

    fn bad_pool() -> PgPool {
        sqlx::pool::PoolOptions::<sqlx::Postgres>::new()
            .connect_lazy("postgres://127.0.0.1:1/none")
            .expect("lazy pool needs no server")
    }

    fn job_with_args(args: Value) -> JobRow {
        let now = frozen();
        JobRow {
            id: 1,
            celery_id: "test-id".to_owned(),
            task: FIRE_TICK_TASK.to_owned(),
            args,
            kwargs: json!({}),
            queue: "default".to_owned(),
            status: "queued".to_owned(),
            attempts: 0,
            max_retries: 3,
            visible_at: now,
            claimed_at: None,
            claimed_by: None,
            created_at: now,
            last_error: None,
        }
    }

    // Payload parsing never touches the database: poison messages park.
    #[test]
    fn parse_rejects_non_uuid_payloads() {
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        assert_eq!(
            parse_fire_tick_arg(&job_with_args(json!([id.to_string()]))),
            Ok(id)
        );
        for bad in [
            json!({"id": id.to_string()}),
            json!([]),
            json!([42]),
            json!(["not-a-uuid"]),
        ] {
            assert!(parse_fire_tick_arg(&job_with_args(bad)).is_err());
        }
    }

    // Registering flips exactly this task to local ownership; the handler
    // parks poison payloads without a database round-trip and reports
    // database failures as retryable errors.
    #[tokio::test]
    async fn registry_owns_fire_tick_after_register() {
        struct NoSeam;
        impl FireTickSeam for NoSeam {
            fn is_ticking_state(&self, _state: Option<Uuid>) -> bool {
                true
            }
            fn tick_interval_seconds(
                &self,
                _state: Option<Uuid>,
                _project: &ProjectTickCols,
            ) -> i64 {
                10800
            }
            fn has_active_run(
                &self,
                _issue: Uuid,
            ) -> Pin<Box<dyn Future<Output = Result<bool, sqlx::Error>> + Send + '_>> {
                Box::pin(async { Ok(false) })
            }
            fn has_prior_run(
                &self,
                _issue: Uuid,
            ) -> Pin<Box<dyn Future<Output = Result<bool, sqlx::Error>> + Send + '_>> {
                Box::pin(async { Ok(true) })
            }
            fn dispatch_continuation_run(
                &self,
                _issue: Uuid,
                _trigger: &str,
                _actor: Option<Uuid>,
            ) -> Pin<Box<dyn Future<Output = Result<Option<Uuid>, sqlx::Error>> + Send + '_>>
            {
                Box::pin(async { Ok(None) })
            }
        }
        let pool = bad_pool();
        let mut registry = Registry::new();
        assert_eq!(route_for(&registry, FIRE_TICK_TASK), Route::PythonOwned);
        register_fire_tick(&mut registry, pool, Arc::new(NoSeam));
        assert!(registry.owns(FIRE_TICK_TASK));
        assert_eq!(route_for(&registry, FIRE_TICK_TASK), Route::Local);
        let handler = registry.get(FIRE_TICK_TASK).unwrap().clone();

        // Poison payload parks without touching the (unreachable) database.
        let verdict = handler(job_with_args(json!(["not-a-uuid"])))
            .await
            .expect("poison parks, never errors");
        assert!(matches!(verdict, Verdict::Fail { .. }));

        // A well-formed id reaches the database: unreachable here, so the
        // worker loop would retry with budget.
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let verdict = handler(job_with_args(json!([id.to_string()]))).await;
        assert!(verdict.is_err());
    }
}
