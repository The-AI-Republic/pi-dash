#![forbid(unsafe_code)]

//! Per-issue ticking clock: reconcile + handlers + primitives + thin senders (D-12 L5, stage 5).
//!
//! Port of the clock core of `apps/api/pi_dash/orchestration/scheduling.py`:
//!
//! * `PAUSED_STATE_NAME` (`:46`) — reused from L1, never re-ported.
//! * `is_paused_state` (`:205-207`) → [`is_paused_state`].
//! * `_project_ticking_enabled` (`:210-212`) → [`project_ticking_enabled`].
//! * `_clock_allowed` (`:215-217`) → [`clock_allowed`].
//! * `_compute_next_run_at` (`:220-222`) → [`compute_next_run_at`].
//! * `_issue_has_active_run` (`:225-228`) — a one-line wrapper over L2's
//!   [`active_run_for`](pidash_db::orchestration::runs::active_run_for); the
//!   handler resolves it and passes [`bool`] (never re-ported).
//! * `_lock_ticker` (`:231-251`) → [`lock_ticker_sql`] (read half) +
//!   [`create_ticker_sql`] / [`new_ticker_row`] (create half).
//! * `_TICKER_CLOCK_FIELDS` (`:254-265`) → [`TICKER_CLOCK_FIELDS`].
//! * `_save_clock` (`:268-269`) → [`SAVE_CLOCK_SQL`] (+ [`ClockWrite`]).
//! * `_clear_pending` (`:272-276`) → [`clear_pending`].
//! * `_stop_clock` (`:279-282`) → [`stop_clock`].
//! * `_queue_entry` (`:285-303`) → [`queue_entry`].
//! * `_stop_for_switch` (`:306-310`) → [`stop_for_switch`].
//! * `_retime_clock` (`:313-330`) → [`retime_clock`].
//! * `reconcile` (`:374-397`) → [`reconcile`].
//! * `_on_enter_or_move` (`:400-448`) → [`on_enter_or_move`].
//! * `_on_left_bucket` (`:451-459`) → [`on_left_bucket`].
//! * `_on_run_ended` (`:462-518`) → [`on_run_ended`].
//! * `_on_human_run_requested` (`:521-534`) → [`on_human_run_requested`].
//! * `_on_retick` (`:537-580`) → [`on_retick`].
//! * `arm_ticker` (`:598-606`) → [`arm_ticker`].
//! * `disarm_ticker` (`:609-629`) → [`disarm_ticker`].
//! * `maybe_disarm_on_terminal_signal` (`:632-637`) → [`maybe_disarm_on_terminal_signal`].
//! * `reset_ticker_after_comment_and_run` (`:640-647`) → [`reset_ticker_after_comment_and_run`].
//!
//! The services crate carries no `sqlx` dependency, so — per the D-27/D-30/D-36
//! `queries.rs` precedent and the sibling [`super::blockers`] / [`super::relations`]
//! modules — SQL here is text plus symbolic `:name` placeholders; handlers
//! translate each `:name` to a positional `$n` in the statement's `*_PARAMS`
//! order (first appearance) when binding via `sqlx`. Row effects are pure
//! mutations over the merged [`IssueAgentTicker`] row plus a [`ClockWrite`]
//! telling the handler which statement to execute; see [`reconcile`]'s
//! handler contract for the lock → handler → save transaction the `:386`
//! `transaction.atomic()` becomes, and [`reconcile_log_line`] for the `:388`
//! post-commit log line.
//!
//! Read seams (reused, never re-ported): L1 [`TickerEvent`] / [`TickerDecision`]
//! / outcome vocabulary / [`StateRef`] phase registry (`pidash_types::orchestration`),
//! [`kind_for`](crate::prompting::recipes::kind_for) (`prompting::recipes`),
//! budget/interval helpers on [`IssueAgentTicker`] (`pidash_db::tasks_ticker`),
//! [`AgentRunStatus`] (`pidash_db::dispatch`). Events carry ids and handlers
//! take re-read rows — the stateless shape L1's docs prescribe.
//!
//! Fixture: FX-ORCH-05 (`rust-api/fixtures/orchestration/fx05_clock/`:
//! `reconcile.enter_move.before_after.json`, `reconcile.left_bucket.before_after.json`,
//! `reconcile.run_ended.before_after.json`, `reconcile.human_retick.before_after.json`,
//! `clock_primitives.golden.json`, `thin_senders.golden.json`). The replay below pins
//! every decision, every after-row, and the SQL shapes; retime timestamps pin the
//! stage cadence window (fixture) plus exact math at fixed jitter (port) — never the
//! fixture's exact stamps, which pin CPython's RNG stream (see [`compute_next_run_at`]).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Existing quirks ported as-is (translation, don't redesign):
//!
//! 1. `reconcile` never writes `used` — only `fire_tick`'s claim does. The
//!    UPDATE projects exactly [`TICKER_CLOCK_FIELDS`]; the test greps it.
//! 2. Stop reasons verbatim: `CAP_HIT` is never produced here (only the timer
//!    tick stamps it), `POOL_SPENT` parks without auto-pausing, and
//!    `TERMINAL_SIGNAL` is unreachable via [`disarm_ticker`] (refused with the
//!    `:616-619` `ValueError` string byte-verbatim, em dash included).
//! 3. The retick reasons use underscores (`no_ticker`, `not_ticking_state`,
//!    `budget_not_exhausted`) while the sibling handlers use hyphens
//!    (`no-ticker`, `not-in-bucket`) — ported as written.
//! 4. The unreachable infinite-pool guard (`:560-563`) is kept: `cap_reached()`
//!    is never true on an infinite pool so the budget guard above always
//!    returns first, but the sentinel must never leak into `granted`.
//! 5. Jitter is caller-drawn: Python draws inside `_compute_next_run_at` via
//!    `random.uniform`, whose stream no Rust RNG reproduces (the D-10
//!    `jitter_seconds` decision) — handlers take `jitter_secs` and the
//!    handler draws it via `pidash_db::tasks_ticker::jitter_seconds`.
//! 6. A created row is INSERTed once in its final shape instead of Python's
//!    INSERT-then-UPDATE — same end state, one round trip; `used` stays 0.

use chrono::{DateTime, Utc};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use pidash_db::dispatch::status::AgentRunStatus;
use pidash_db::tasks_ticker::{
    models::issue_agent_ticker::{
        IssueAgentTicker, COLUMNS as TICKER_COLUMNS, TABLE as TICKER_TABLE,
    },
    TickerDisarmReason, DEFAULT_MAX_TICKS, INFINITE_MAX_TICKS,
};
use pidash_types::orchestration::{
    cadence_fields_for, default_outcome_for_kind, is_ticking_state, outcome_for_run,
    template_name_for, RunOutcomeRef, StateRef, TickerDecision, TickerEvent, TickerEventKind,
    OUTCOME_PROGRESSED, OUTCOME_WAITING_ON_HUMAN, STOPPING_OUTCOMES,
};

use crate::prompting::recipes::{kind_for, WORK_KIND_CODING};

// ---------------------------------------------------------------------------
// Clock field list + handler-executed SQL
// ---------------------------------------------------------------------------

/// `_TICKER_CLOCK_FIELDS` (`scheduling.py:254-265`): the Django field names
/// `reconcile` ever writes, in source order. `used` is never here — only
/// `fire_tick`'s claim writes it. Pinned by P9.
pub const TICKER_CLOCK_FIELDS: &[&str] = &[
    "enabled",
    "disarm_reason",
    "next_run_at",
    "pending_entry",
    "pending_entry_free",
    "pending_entry_actor",
    "pending_entry_trigger",
    "granted",
    "resume_parent_run",
    "updated_at",
];

/// Bind order for [`lock_ticker_sql`].
pub const LOCK_TICKER_PARAMS: &[&str] = &["issue_id"];

/// `_lock_ticker` read half (`scheduling.py:236`): the issue's ticker row,
/// locked. Django renders the default-manager soft-delete scope
/// (`deleted_at IS NULL`) plus the pk ordering `.first()` adds on an
/// unordered model (`ORDER BY id ASC LIMIT 1`), then `FOR UPDATE` — kept
/// though unobservable (the issue FK is one-to-one).
pub fn lock_ticker_sql() -> String {
    format!(
        "SELECT {} FROM {TICKER_TABLE} WHERE issue_id = :issue_id AND deleted_at IS NULL ORDER BY id ASC LIMIT 1 FOR UPDATE",
        TICKER_COLUMNS.join(", "),
    )
}

/// Bind order for [`create_ticker_sql`] (first appearance in [`TICKER_COLUMNS`] order).
pub const CREATE_TICKER_PARAMS: &[&str] = &["now", "created_by_id", "id", "issue_id"];

/// `_lock_ticker` create half (`scheduling.py:238-246`): the disabled,
/// unarmed, zero-budget shape, as the handler-executed INSERT. Fixed values
/// are literals (Django binds them; the row is identical either way); `:now`
/// stamps both `created_at` and `updated_at` (Django calls `now()` twice —
/// unobservable, the L4 quirk-6 precedent); `:created_by_id` is the crum user
/// or NULL on system paths, while `updated_by_id` is always NULL on create
/// (`BaseModel.save`, `db/models/base.py:25-44`).
pub fn create_ticker_sql() -> String {
    format!(
        "INSERT INTO {TICKER_TABLE} ({}) VALUES (:now, :now, :created_by_id, NULL, NULL, :id, :issue_id, 0, 0, 0, FALSE, NULL, NULL, FALSE, '', FALSE, FALSE, NULL, '', NULL)",
        TICKER_COLUMNS.join(", "),
    )
}

/// `_save_clock` (`scheduling.py:268-269`) as the handler-executed UPDATE.
/// `SET` order follows Django `_meta` field order (the L2 `INGEST_UPDATE_SQL`
/// precedent), not `update_fields` order; the FK fields render as their
/// attnames (`pending_entry_actor_id`, `resume_parent_run_id`).
pub const SAVE_CLOCK_SQL: &str = "UPDATE issue_agent_ticker SET updated_at = :updated_at, granted = :granted, next_run_at = :next_run_at, enabled = :enabled, disarm_reason = :disarm_reason, pending_entry = :pending_entry, pending_entry_free = :pending_entry_free, pending_entry_actor_id = :pending_entry_actor_id, pending_entry_trigger = :pending_entry_trigger, resume_parent_run_id = :resume_parent_run_id WHERE id = :id";

/// Bind order for [`SAVE_CLOCK_SQL`] (first appearance).
pub const SAVE_CLOCK_PARAMS: &[&str] = &[
    "updated_at",
    "granted",
    "next_run_at",
    "enabled",
    "disarm_reason",
    "pending_entry",
    "pending_entry_free",
    "pending_entry_actor_id",
    "pending_entry_trigger",
    "resume_parent_run_id",
    "id",
];

// ---------------------------------------------------------------------------
// Input views (re-read rows at the handler boundary)
// ---------------------------------------------------------------------------

/// The project columns the clock reads (`scheduling.py` reaches them via
/// `issue.project`). Each is `Option` for the `getattr(..., default)` rows
/// that somehow lack the field; `None` answers the documented default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectClockPolicy {
    /// `agent_ticking_enabled` (default `true`; `_project_ticking_enabled`, `:210-212`).
    pub agent_ticking_enabled: Option<bool>,
    /// `agent_default_max_ticks` (default 10, `-1` infinite; `pool_size`).
    pub agent_default_max_ticks: Option<i32>,
    /// `agent_default_interval_seconds` (impl cadence column).
    pub agent_default_interval_seconds: Option<i64>,
    /// `agent_review_default_interval_seconds` (review cadence column).
    pub agent_review_default_interval_seconds: Option<i64>,
    /// `agent_test_default_interval_seconds` (test cadence column).
    pub agent_test_default_interval_seconds: Option<i64>,
}

/// The issue half the clock reads: its id, its current state, and its
/// project's clock policy. Python rebinds the caller's fresh issue onto the
/// locked row (`:247-251`) so the resolvers read the state the caller sees;
/// the handler re-reads its own rows and hands the values here instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockIssue<'a> {
    /// The issue id (only used for the create-shape row).
    pub issue_id: Uuid,
    /// The issue's current state (`None` when stateless).
    pub state: Option<StateRef<'a>>,
    /// The issue's project clock policy.
    pub policy: ProjectClockPolicy,
}

/// The run half `_on_run_ended` reads: status, kind, and done payload.
/// Handlers re-read the run row (L1 events carry only its id).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunEndedRef<'a> {
    /// The run's resting status.
    pub status: AgentRunStatus,
    /// The run's `phase_kind` (`""` when never set).
    pub phase_kind: &'a str,
    /// The run's `done_payload` (`None` when never yielded).
    pub done_payload: Option<&'a Value>,
}

// ---------------------------------------------------------------------------
// Pure clock primitives
// ---------------------------------------------------------------------------

/// `is_paused_state` (`scheduling.py:205-207`): the project's auto-pause
/// parking state — a name-only check (`Paused`, `backlog` group per migration
/// `0128_paused_state.py`); `None` is not paused.
pub fn is_paused_state(state: Option<&StateRef<'_>>) -> bool {
    state.is_some_and(|s| s.name == pidash_types::orchestration::PAUSED_STATE_NAME)
}

/// `_project_ticking_enabled` (`scheduling.py:210-212`): the project switch,
/// defaulting to `true` when the row lacks the field.
pub fn project_ticking_enabled(policy: &ProjectClockPolicy) -> bool {
    policy.agent_ticking_enabled.unwrap_or(true)
}

/// `pool_size` (`db/models/issue_agent_ticker.py:200-203`): the project's
/// per-issue pool, `-1` infinite, defaulting to 10.
pub fn pool_size(policy: &ProjectClockPolicy) -> i32 {
    policy.agent_default_max_ticks.unwrap_or(DEFAULT_MAX_TICKS)
}

/// `effective_interval_seconds` (`db/models/issue_agent_ticker.py:185-198`):
/// the issue's current stage interval — the stage's project column via
/// `cadence_fields_for`, falling back to the stage default.
pub fn effective_interval_seconds(
    state: Option<&StateRef<'_>>,
    policy: &ProjectClockPolicy,
) -> i64 {
    let fields = cadence_fields_for(state);
    let column = match fields.project_interval {
        "agent_default_interval_seconds" => policy.agent_default_interval_seconds,
        "agent_review_default_interval_seconds" => policy.agent_review_default_interval_seconds,
        "agent_test_default_interval_seconds" => policy.agent_test_default_interval_seconds,
        // A future stage this struct predates: exactly Python's
        // `getattr(project, column, fields.default_interval)` fallback.
        _ => None,
    };
    column.unwrap_or(fields.default_interval)
}

/// `_clock_allowed` (`scheduling.py:215-217`): may this issue's clock run at
/// all (user / project switches)?
pub fn clock_allowed(ticker: &IssueAgentTicker, policy: &ProjectClockPolicy) -> bool {
    !ticker.user_disabled && project_ticking_enabled(policy)
}

/// `_compute_next_run_at` (`scheduling.py:220-222`): `base + interval +
/// jitter`, the jitter rounded to whole microseconds as
/// `timedelta(seconds=...)` does. The caller draws `jitter_secs` via
/// `pidash_db::tasks_ticker::jitter_seconds` — the port cannot reproduce
/// CPython's RNG stream (the D-10 decision), so the draw stays at the call
/// boundary and the goldens below pin the math.
pub fn compute_next_run_at(
    interval_seconds: i64,
    base: DateTime<Utc>,
    jitter_secs: f64,
) -> DateTime<Utc> {
    let jitter_micros = (jitter_secs * 1_000_000.0).round() as i64;
    base + chrono::Duration::seconds(interval_seconds)
        + chrono::Duration::microseconds(jitter_micros)
}

/// `_clear_pending` (`scheduling.py:272-276`).
pub fn clear_pending(ticker: &mut IssueAgentTicker) {
    ticker.pending_entry = false;
    ticker.pending_entry_free = false;
    ticker.pending_entry_actor_id = None;
    ticker.pending_entry_trigger = String::new();
}

/// `_stop_clock` (`scheduling.py:279-282`): reason stamped, pending cleared,
/// `next_run_at` kept.
pub fn stop_clock(ticker: &mut IssueAgentTicker, reason: &str) {
    ticker.enabled = false;
    ticker.disarm_reason = reason.to_string();
    clear_pending(ticker);
}

/// `_queue_entry` (`scheduling.py:285-303`): owe an entry run for the current
/// stage. `next_run_at = now` so the scanner picks it up next pass; `enabled`
/// even on a spent pool or a disabled clock (a human asked for *this* run —
/// `fire_tick` re-applies the switch after the claim). A free entry remembers
/// actor + trigger; a counting one clears both.
pub fn queue_entry(
    ticker: &mut IssueAgentTicker,
    now: DateTime<Utc>,
    free: bool,
    actor: Option<Uuid>,
    trigger: &str,
) {
    ticker.enabled = true;
    ticker.disarm_reason = TickerDisarmReason::None.as_str().to_string();
    ticker.next_run_at = Some(now);
    ticker.pending_entry = true;
    ticker.pending_entry_free = free;
    ticker.pending_entry_actor_id = if free { actor } else { None };
    ticker.pending_entry_trigger = if free {
        trigger.to_string()
    } else {
        String::new()
    };
}

/// `_stop_for_switch` (`scheduling.py:306-310`): the user switch stamps
/// `USER_DISABLED`, the project switch the empty (armed-string) reason.
pub fn stop_for_switch(ticker: &mut IssueAgentTicker) {
    let reason = if ticker.user_disabled {
        TickerDisarmReason::UserDisabled.as_str()
    } else {
        TickerDisarmReason::None.as_str()
    };
    stop_clock(ticker, reason);
}

/// `_retime_clock` (`scheduling.py:313-330`): arm the clock for the current
/// stage's interval if the pool allows. A spent pool stops with `POOL_SPENT`
/// (not `CAP_HIT` — that reason is reserved for the timer tick that consumed
/// the last run, and it alone auto-pauses).
pub fn retime_clock(
    ticker: &mut IssueAgentTicker,
    issue: &ClockIssue<'_>,
    now: DateTime<Utc>,
    jitter_secs: f64,
) {
    if !clock_allowed(ticker, &issue.policy) {
        stop_for_switch(ticker);
        return;
    }
    if ticker.cap_reached(pool_size(&issue.policy)) {
        stop_clock(ticker, TickerDisarmReason::PoolSpent.as_str());
        return;
    }
    ticker.enabled = true;
    ticker.disarm_reason = TickerDisarmReason::None.as_str().to_string();
    clear_pending(ticker);
    ticker.next_run_at = Some(compute_next_run_at(
        effective_interval_seconds(issue.state.as_ref(), &issue.policy),
        now,
        jitter_secs,
    ));
}

/// `_lock_ticker` create shape (`scheduling.py:238-246`) as a built row:
/// disabled, unarmed, zero budget. The handler INSERTs it via
/// [`create_ticker_sql`]; `id` is handler-generated, `now` stamps both audit
/// columns, `created_by` is the crum user or `None` on system paths.
pub fn new_ticker_row(
    id: Uuid,
    issue_id: Uuid,
    now: DateTime<Utc>,
    created_by: Option<Uuid>,
) -> IssueAgentTicker {
    IssueAgentTicker {
        id,
        created_at: now,
        updated_at: now,
        created_by_id: created_by,
        updated_by_id: None,
        deleted_at: None,
        issue_id,
        used: 0,
        granted: 0,
        waited: 0,
        user_disabled: false,
        next_run_at: None,
        last_tick_at: None,
        enabled: false,
        disarm_reason: TickerDisarmReason::None.as_str().to_string(),
        pending_entry: false,
        pending_entry_free: false,
        pending_entry_actor_id: None,
        pending_entry_trigger: String::new(),
        resume_parent_run_id: None,
    }
}

// ---------------------------------------------------------------------------
// Decisions + writes
// ---------------------------------------------------------------------------

/// Which write the handler must execute after a pure clock call — the
/// handler-executed half of `_save_clock`, plus the create-shape INSERT.
/// `reconcile` never writes `used` on any path: the UPDATE projects only
/// [`TICKER_CLOCK_FIELDS`], and the INSERT only ever carries the zero-budget
/// create shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockWrite {
    /// No write: a guard returned before any mutation.
    None,
    /// INSERT the (already mutated) row via [`create_ticker_sql`].
    Insert,
    /// UPDATE the row via [`SAVE_CLOCK_SQL`] (`:updated_at` is `now`).
    Update,
}

/// A pure clock call's answer: the [`TickerDecision`] for the caller to act
/// on plus the [`ClockWrite`] the handler must execute in the same transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockOutcome {
    pub decision: TickerDecision,
    pub write: ClockWrite,
}

/// A guard outcome: nothing mutated, nothing to write.
fn untouched(ticker: Option<Uuid>, reason: &str) -> ClockOutcome {
    ClockOutcome {
        decision: TickerDecision {
            ticker,
            reason: reason.to_string(),
            ..TickerDecision::default()
        },
        write: ClockWrite::None,
    }
}

// ---------------------------------------------------------------------------
// Event handlers
// ---------------------------------------------------------------------------

/// `_on_enter_or_move` (`scheduling.py:400-448`): the issue entered the
/// bucket, or moved between its rooms. No teardown, no rebuild — the same row
/// keeps `used` / `granted`; `ticker` is that post-lock row. `has_active_run`
/// is the L2 [`active_run_for`](pidash_db::orchestration::runs::active_run_for)
/// verdict the handler resolved. The write is always [`ClockWrite::Update`] —
/// [`reconcile`] upgrades it to `Insert` when it created the row in this call.
pub fn on_enter_or_move(
    ticker: &mut IssueAgentTicker,
    issue: &ClockIssue<'_>,
    event: &TickerEvent,
    has_active_run: bool,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> ClockOutcome {
    if let Some(resume_parent) = event.resume_parent {
        ticker.resume_parent_run_id = Some(resume_parent);
    }
    let mut decision = TickerDecision {
        ticker: Some(ticker.id),
        ..TickerDecision::default()
    };
    let allowed = clock_allowed(ticker, &issue.policy);
    if event.moved_by_run.is_some() {
        if ticker.cap_reached(pool_size(&issue.policy)) {
            // Pool spent: nothing fires; the issue parks in its truthful
            // state. `POOL_SPENT`, not `CAP_HIT`: parking must not auto-Pause
            // the issue out from under the Re-tick the §5.4 comment points at.
            stop_clock(ticker, TickerDisarmReason::PoolSpent.as_str());
            decision.parked = true;
            decision.reason = "pool-spent".to_string();
        } else if !allowed {
            stop_for_switch(ticker);
            decision.reason = "ticking-disabled".to_string();
        } else {
            // The agent moves from inside its own run, so a direct dispatch
            // would hit the single-active-run guard. Queue the entry; it counts.
            queue_entry(ticker, now, false, None, "");
            decision.queued = true;
            decision.reason = "entry-queued".to_string();
        }
    } else if !event.want_run {
        retime_clock(ticker, issue, now, jitter_secs);
        decision.reason = "retimed".to_string();
    } else if has_active_run {
        queue_entry(ticker, now, true, event.actor, &event.trigger);
        decision.queued = true;
        decision.reason = "free-entry-queued".to_string();
    } else {
        // Human move: one free run, always — even into a spent pool.
        retime_clock(ticker, issue, now, jitter_secs);
        decision.dispatch_now = true;
        decision.reason = "dispatch-now".to_string();
    }
    ClockOutcome {
        decision,
        write: ClockWrite::Update,
    }
}

/// `_on_left_bucket` (`scheduling.py:451-459`): the clock goes dormant; the
/// pool (`used` / `granted`) is kept.
pub fn on_left_bucket(ticker: Option<&mut IssueAgentTicker>) -> ClockOutcome {
    let Some(ticker) = ticker else {
        return untouched(None, "no-ticker");
    };
    stop_clock(ticker, TickerDisarmReason::LeftTickingState.as_str());
    ticker.next_run_at = None;
    ClockOutcome {
        decision: TickerDecision {
            ticker: Some(ticker.id),
            reason: "dormant".to_string(),
            ..TickerDecision::default()
        },
        write: ClockWrite::Update,
    }
}

/// `_on_run_ended` (`scheduling.py:462-518`): a run on the issue reached a
/// resting status. `run` is the re-read run row (the event carries only its
/// id); `event.outcome` is used verbatim when present, else resolved from the
/// run's payload, else the per-status default.
pub fn on_run_ended(
    ticker: Option<&mut IssueAgentTicker>,
    issue: &ClockIssue<'_>,
    event: &TickerEvent,
    run: &RunEndedRef<'_>,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> ClockOutcome {
    let Some(ticker) = ticker else {
        return untouched(None, "no-ticker");
    };
    let ticker_id = ticker.id;
    if !is_ticking_state(issue.state.as_ref()) {
        return untouched(Some(ticker_id), "not-in-bucket");
    }
    // The guard: a run that moved the issue on and *then* reported `done`
    // must not stop the clock already set for the next stage.
    let current_kind = kind_for(template_name_for(issue.state.as_ref()), WORK_KIND_CODING);
    if !run.phase_kind.is_empty() && run.phase_kind != current_kind {
        return untouched(Some(ticker_id), "stage-moved-on");
    }
    let outcome = event.outcome.clone().or_else(|| {
        outcome_for_run(&RunOutcomeRef {
            done_payload: run.done_payload,
            phase_kind: run.phase_kind,
        })
        .map(str::to_string)
    });
    let outcome = outcome.unwrap_or_else(|| {
        // No yield. A `completed` run is read per kind; a paused one means
        // `waiting_on_human`; a failed / cancelled / refused one said nothing
        // about the stage — keep ticking so the next tick retries, rather
        // than stopping the clock on a crash (which would strand a review/test
        // issue with no Re-tick button, since the cap was never reached).
        if run.status == AgentRunStatus::Completed {
            let kind = if run.phase_kind.is_empty() {
                current_kind
            } else {
                run.phase_kind
            };
            default_outcome_for_kind(kind).to_string()
        } else if run.status == AgentRunStatus::PausedAwaitingInput {
            OUTCOME_WAITING_ON_HUMAN.to_string()
        } else {
            OUTCOME_PROGRESSED.to_string()
        }
    });
    if ticker.pending_entry {
        // Something already owes the next run on this clock; the finished
        // run's opinion does not override it.
        return untouched(Some(ticker_id), &format!("{outcome}:pending-entry-kept"));
    }
    if STOPPING_OUTCOMES.contains(&outcome.as_str()) {
        if ticker.enabled {
            // Only stop an armed clock — a prior `cap_hit` must survive so
            // the deferred auto-pause still fires (design §5.2).
            stop_clock(ticker, TickerDisarmReason::TerminalSignal.as_str());
            return ClockOutcome {
                decision: TickerDecision {
                    ticker: Some(ticker_id),
                    reason: format!("{outcome}:stopped"),
                    ..TickerDecision::default()
                },
                write: ClockWrite::Update,
            };
        }
        return untouched(Some(ticker_id), &format!("{outcome}:already-stopped"));
    }
    // progressed / waiting_on_external: keep ticking. `fire_tick` already
    // re-timed the clock at claim for tick-started runs; make sure a
    // human-started run leaves a live clock behind too.
    if ticker.enabled
        && ticker.next_run_at.is_none()
        && !ticker.cap_reached(pool_size(&issue.policy))
    {
        ticker.next_run_at = Some(compute_next_run_at(
            effective_interval_seconds(issue.state.as_ref(), &issue.policy),
            now,
            jitter_secs,
        ));
        return ClockOutcome {
            decision: TickerDecision {
                ticker: Some(ticker_id),
                reason: format!("{outcome}:keep-ticking"),
                ..TickerDecision::default()
            },
            write: ClockWrite::Update,
        };
    }
    untouched(Some(ticker_id), &format!("{outcome}:keep-ticking"))
}

/// `_on_human_run_requested` (`scheduling.py:521-534`): Run AI / Comment & Run —
/// one free run now; re-time only if budget.
pub fn on_human_run_requested(
    ticker: &mut IssueAgentTicker,
    issue: &ClockIssue<'_>,
    event: &TickerEvent,
    has_active_run: bool,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> ClockOutcome {
    let mut decision = TickerDecision {
        ticker: Some(ticker.id),
        ..TickerDecision::default()
    };
    if event.want_run && has_active_run {
        queue_entry(ticker, now, true, event.actor, &event.trigger);
        decision.queued = true;
        decision.reason = "free-entry-queued".to_string();
    } else {
        retime_clock(ticker, issue, now, jitter_secs);
        decision.dispatch_now = event.want_run;
        decision.reason = if event.want_run {
            "dispatch-now".to_string()
        } else {
            "retimed".to_string()
        };
    }
    ClockOutcome {
        decision,
        write: ClockWrite::Update,
    }
}

/// `_on_retick` (`scheduling.py:537-580`): grant a fresh project-sized pool,
/// then fire now. The grant is one whole per-issue pool per press; all guards
/// must hold or the call is a no-op.
pub fn on_retick(
    ticker: Option<&mut IssueAgentTicker>,
    issue: &ClockIssue<'_>,
    event: &TickerEvent,
    has_active_run: bool,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> ClockOutcome {
    let Some(ticker) = ticker else {
        return untouched(None, "no_ticker");
    };
    let ticker_id = ticker.id;
    let paused = is_paused_state(issue.state.as_ref());
    if !is_ticking_state(issue.state.as_ref()) && !paused {
        return untouched(Some(ticker_id), "not_ticking_state");
    }
    let pool = pool_size(&issue.policy);
    if !ticker.cap_reached(pool) {
        return untouched(Some(ticker_id), "budget_not_exhausted");
    }
    if pool == INFINITE_MAX_TICKS {
        // Unreachable given the cap guard above, but never fold the infinite
        // sentinel into `granted`.
        return untouched(Some(ticker_id), "budget_not_exhausted");
    }
    // `granted += max(0, pool)`; saturating (Python ints are unbounded but the
    // counter grows in pool-sized steps from a small base — overflow is unreachable).
    ticker.granted = ticker.granted.saturating_add(pool.max(0));
    let mut decision = TickerDecision {
        ticker: Some(ticker_id),
        granted: true,
        reason: "granted".to_string(),
        ..TickerDecision::default()
    };
    if paused {
        // The cap-hit auto-pause parked the issue outside the bucket. The
        // grant lands here; `re_tick_ticker` moves the issue back to In
        // Progress as a human move, which arms the clock and fires the run.
        decision.reason = "granted-from-paused".to_string();
        return ClockOutcome {
            decision,
            write: ClockWrite::Update,
        };
    }
    if event.want_run && has_active_run {
        queue_entry(ticker, now, true, event.actor, &event.trigger);
        decision.queued = true;
    } else {
        retime_clock(ticker, issue, now, jitter_secs);
        decision.dispatch_now = event.want_run;
    }
    ClockOutcome {
        decision,
        write: ClockWrite::Update,
    }
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

/// Failures of the pure clock layer.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClockError {
    /// `reconcile` on an unknown event kind — the `ValueError` at `:385`.
    #[error("unknown ticker event kind '{kind}'")]
    UnknownEventKind {
        /// The offending kind, verbatim.
        kind: String,
    },
    /// `RUN_ENDED` without the ended run's re-read row — unreachable from live
    /// callers (the event always carries the run); Python would crash
    /// dereferencing it (`outcome_for_run(None)`).
    #[error("run_ended event without its run row")]
    RunEndedWithoutRun,
    /// `disarm_ticker` refused `TERMINAL_SIGNAL` — the `ValueError` at
    /// `:616-619`, byte-verbatim (fixture S2d).
    #[error(
        "disarm_ticker overwrites disarm_reason — send a RUN_ENDED event (reconcile) for TERMINAL_SIGNAL so a prior CAP_HIT survives."
    )]
    TerminalSignalViaDisarm,
}

/// Whether the handler for `kind` locks-or-creates (`_lock_ticker(create=True)`,
/// `:231`). The handler INSERTs via [`create_ticker_sql`] when the lock misses
/// on these kinds; every other kind treats a miss as `no-ticker` / `no_ticker`.
pub fn creates_ticker(kind: &str) -> bool {
    kind == TickerEventKind::ENTERED_BUCKET
        || kind == TickerEventKind::MOVED_STAGE
        || kind == TickerEventKind::HUMAN_RUN_REQUESTED
}

/// `reconcile` (`scheduling.py:374-397`): apply one event to the issue's clock
/// and say what the caller should do.
///
/// Pure dispatcher over the event kind; unknown kinds answer
/// [`ClockError::UnknownEventKind`] (the `:385` `ValueError`).
///
/// Handler contract (the `:386` `transaction.atomic()` + `:388` log, which only
/// the executing handler can do): resolve `has_active_run` via L2's
/// [`active_run_for`](pidash_db::orchestration::runs::active_run_for) and `run`
/// by re-reading the event's run id, draw `jitter_secs` via
/// `pidash_db::tasks_ticker::jitter_seconds`, then inside ONE transaction — lock
/// the ticker row ([`lock_ticker_sql`]), call this with the row in `ticker`
/// (`None` when the lock missed), execute the returned [`ClockWrite`]
/// ([`create_ticker_sql`] on `Insert`, [`SAVE_CLOCK_SQL`] on `Update`), commit —
/// and after commit log [`reconcile_log_line`].
///
/// For create-kinds ([`creates_ticker`]) a missed lock builds the zero-budget
/// create-shape row in `ticker` (the `:238-246` INSERT half of `_lock_ticker`,
/// with `created_by` as the crum user) and the outcome write is `Insert`: one
/// INSERT of the final row instead of Python's INSERT-then-UPDATE — same end
/// state, one round trip, `used` still untouched (always 0 on this path).
#[allow(clippy::too_many_arguments)]
pub fn reconcile(
    ticker: &mut Option<IssueAgentTicker>,
    issue: &ClockIssue<'_>,
    event: &TickerEvent,
    run: Option<&RunEndedRef<'_>>,
    has_active_run: bool,
    now: DateTime<Utc>,
    jitter_secs: f64,
    created_by: Option<Uuid>,
) -> Result<ClockOutcome, ClockError> {
    let kind = event.kind.as_str();
    let created = creates_ticker(kind) && ticker.is_none();
    if created {
        *ticker = Some(new_ticker_row(
            Uuid::new_v4(),
            issue.issue_id,
            now,
            created_by,
        ));
    }
    let mut outcome = match kind {
        k if k == TickerEventKind::ENTERED_BUCKET || k == TickerEventKind::MOVED_STAGE => {
            let row = ticker
                .as_mut()
                .expect("enter/move always has a post-lock row");
            on_enter_or_move(row, issue, event, has_active_run, now, jitter_secs)
        }
        k if k == TickerEventKind::LEFT_BUCKET => on_left_bucket(ticker.as_mut()),
        k if k == TickerEventKind::RUN_ENDED => {
            let run = run.ok_or(ClockError::RunEndedWithoutRun)?;
            on_run_ended(ticker.as_mut(), issue, event, run, now, jitter_secs)
        }
        k if k == TickerEventKind::HUMAN_RUN_REQUESTED => {
            let row = ticker
                .as_mut()
                .expect("human_run_requested always has a post-lock row");
            on_human_run_requested(row, issue, event, has_active_run, now, jitter_secs)
        }
        k if k == TickerEventKind::RETICK => on_retick(
            ticker.as_mut(),
            issue,
            event,
            has_active_run,
            now,
            jitter_secs,
        ),
        _ => {
            return Err(ClockError::UnknownEventKind {
                kind: event.kind.clone(),
            });
        }
    };
    if created {
        outcome.write = ClockWrite::Insert;
    }
    Ok(outcome)
}

/// The `:388-396` post-commit log line, byte-verbatim (Python renders bools
/// `True`/`False`): `agent_ticker: reconcile issue=<uuid> event=<kind> ->
/// dispatch_now=<bool> queued=<bool> parked=<bool> reason=<reason>`.
pub fn reconcile_log_line(issue_id: &Uuid, event_kind: &str, decision: &TickerDecision) -> String {
    format!(
        "agent_ticker: reconcile issue={issue_id} event={event_kind} -> dispatch_now={} queued={} parked={} reason={}",
        py_bool(decision.dispatch_now),
        py_bool(decision.queued),
        py_bool(decision.parked),
        decision.reason,
    )
}

/// Python `str(bool)`.
fn py_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

// ---------------------------------------------------------------------------
// Thin senders — kept for callers that predate `reconcile`
// ---------------------------------------------------------------------------

/// `arm_ticker` (`scheduling.py:598-606`): human re-engagement without a run —
/// re-time the clock for the current stage if the pool allows, never zeroing
/// `used`. `ticker` is the post-lock row; the write is always
/// [`ClockWrite::Update`] (the caller upgrades to `Insert` when it created the
/// row, as in [`reconcile`]). Python's `dispatch_immediate` kwarg is a
/// caller-only signal the function ignores (`noqa: ARG001`) — there is no such
/// parameter here.
pub fn arm_ticker(
    ticker: &mut IssueAgentTicker,
    issue: &ClockIssue<'_>,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> ClockOutcome {
    let event = TickerEvent::human_run_requested(false, None, "");
    // `has_active_run` is unread: `want_run = false` short-circuits the busy
    // check (`:525`), as in Python.
    on_human_run_requested(ticker, issue, &event, false, now, jitter_secs)
}

/// `disarm_ticker` (`scheduling.py:609-629`): stop the clock with `reason`
/// (default `left_ticking_state`, passed by the caller — Rust has no default
/// args). Idempotent. `TERMINAL_SIGNAL` is refused with
/// [`ClockError::TerminalSignalViaDisarm`] so a prior `CAP_HIT` survives; the
/// default also clears `next_run_at`, a custom reason keeps it; no ticker
/// answers `None` (no write). Returns the row id; the handler executes
/// [`SAVE_CLOCK_SQL`] iff `Some`.
pub fn disarm_ticker(
    ticker: Option<&mut IssueAgentTicker>,
    reason: &str,
) -> Result<Option<Uuid>, ClockError> {
    if reason == TickerDisarmReason::TerminalSignal.as_str() {
        return Err(ClockError::TerminalSignalViaDisarm);
    }
    let Some(ticker) = ticker else {
        return Ok(None);
    };
    stop_clock(ticker, reason);
    if reason == TickerDisarmReason::LeftTickingState.as_str() {
        ticker.next_run_at = None;
    }
    Ok(Some(ticker.id))
}

/// `maybe_disarm_on_terminal_signal` (`scheduling.py:632-637`): send `RUN_ENDED`
/// for the run; `true` iff the clock was stopped (reason ends `:stopped`). A
/// run with no work item answers `(false, None)` without sending. `run` is the
/// re-read run row; the outcome (when sent) carries the handler's write.
pub fn maybe_disarm_on_terminal_signal(
    ticker: &mut Option<IssueAgentTicker>,
    issue: &ClockIssue<'_>,
    run: &RunEndedRef<'_>,
    work_item: Option<Uuid>,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> (bool, Option<ClockOutcome>) {
    if work_item.is_none() {
        return (false, None);
    }
    let event = TickerEvent::run_ended(None, None);
    // `has_active_run` is unread on this path (`RUN_ENDED` never checks it);
    // `RUN_ENDED` never creates, so `created_by` is moot. Infallible: the
    // kind is known and the run row is present.
    let outcome = reconcile(
        ticker,
        issue,
        &event,
        Some(run),
        false,
        now,
        jitter_secs,
        None,
    )
    .expect("RUN_ENDED with a run row is infallible");
    let stopped = outcome.decision.reason.ends_with(":stopped");
    (stopped, Some(outcome))
}

/// `reset_ticker_after_comment_and_run` (`scheduling.py:640-647`): Comment & Run /
/// Run AI where the caller dispatches the run itself. Historical name — nothing
/// is *reset* any more (human-started runs are free); re-times the clock if the
/// pool has budget. Same event as [`arm_ticker`] under its historical name.
pub fn reset_ticker_after_comment_and_run(
    ticker: &mut IssueAgentTicker,
    issue: &ClockIssue<'_>,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> ClockOutcome {
    let event = TickerEvent::human_run_requested(false, None, "");
    on_human_run_requested(ticker, issue, &event, false, now, jitter_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::tasks_ticker::JITTER_FRACTION;
    use serde_json::Value;

    // -- replay scaffolding --------------------------------------------------

    /// The fixture generator's frozen clock (`2026-06-01T12:00Z`).
    fn now() -> DateTime<Utc> {
        "2026-06-01T12:00:00Z".parse().expect("frozen now parses")
    }

    /// Fixed jitter for case replay: fractional (exercises the microsecond
    /// path) and binary-exact (`42.5 * 1e6` needs no rounding).
    const JITTER: f64 = 42.5;

    const TICKER_ID: Uuid = Uuid::from_u128(0x1111_1111_1111_1111_1111_1111_1111_1111);
    const ISSUE_ID: Uuid = Uuid::from_u128(0x2222_2222_2222_2222_2222_2222_2222_2222);
    const ACTOR_ID: Uuid = Uuid::from_u128(0x3333_3333_3333_3333_3333_3333_3333_3333);
    const RUN_ID: Uuid = Uuid::from_u128(0x4444_4444_4444_4444_4444_4444_4444_4444);

    fn fx(name: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/orchestration/fx05_clock/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).expect("fx05 fixture exists");
        serde_json::from_str(&text).expect("fx05 fixture parses")
    }

    /// Project `FX5`: ticking enabled, pool 10, impl/review/test 10800/3600/7200s.
    fn fx5_policy() -> ProjectClockPolicy {
        ProjectClockPolicy {
            agent_ticking_enabled: Some(true),
            agent_default_max_ticks: Some(10),
            agent_default_interval_seconds: Some(10800),
            agent_review_default_interval_seconds: Some(3600),
            agent_test_default_interval_seconds: Some(7200),
        }
    }

    fn in_progress() -> StateRef<'static> {
        StateRef {
            group: "started",
            name: "In Progress",
        }
    }

    fn in_review() -> StateRef<'static> {
        StateRef {
            group: "review",
            name: "In Review",
        }
    }

    fn done_state() -> StateRef<'static> {
        StateRef {
            group: "completed",
            name: "Done",
        }
    }

    /// The auto-pause parking state (`backlog` group per migration `0128`).
    fn paused_state() -> StateRef<'static> {
        StateRef {
            group: "backlog",
            name: "Paused",
        }
    }

    fn clock_issue(
        state: Option<StateRef<'static>>,
        policy: ProjectClockPolicy,
    ) -> ClockIssue<'static> {
        ClockIssue {
            issue_id: ISSUE_ID,
            state,
            policy,
        }
    }

    /// Build a ticker row from a fixture before/after object. Actor emails map
    /// to [`ACTOR_ID`] and `run-E*` labels to [`RUN_ID`] — the replay pins
    /// presence + equality, never the label text.
    fn ticker_from(row: &Value) -> IssueAgentTicker {
        IssueAgentTicker {
            id: TICKER_ID,
            created_at: now(),
            updated_at: now(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            issue_id: ISSUE_ID,
            used: row["used"].as_i64().expect("used") as i32,
            granted: row["granted"].as_i64().expect("granted") as i32,
            waited: row["waited"].as_i64().expect("waited") as i32,
            user_disabled: row["user_disabled"].as_bool().expect("user_disabled"),
            next_run_at: row["next_run_at"]
                .as_str()
                .map(|s| s.parse().expect("next_run_at parses")),
            last_tick_at: None,
            enabled: row["enabled"].as_bool().expect("enabled"),
            disarm_reason: row["disarm_reason"]
                .as_str()
                .expect("disarm_reason")
                .to_string(),
            pending_entry: row["pending_entry"].as_bool().expect("pending_entry"),
            pending_entry_free: row["pending_entry_free"]
                .as_bool()
                .expect("pending_entry_free"),
            pending_entry_actor_id: if row["pending_entry_actor"].is_null() {
                None
            } else {
                Some(ACTOR_ID)
            },
            pending_entry_trigger: row["pending_entry_trigger"]
                .as_str()
                .expect("pending_entry_trigger")
                .to_string(),
            resume_parent_run_id: if row["resume_parent_run"].is_null() {
                None
            } else {
                Some(RUN_ID)
            },
        }
    }

    /// The after-row, field for field — except `next_run_at`, which the caller
    /// passes explicitly (retime stamps pin the cadence window against the
    /// fixture plus exact math at fixed jitter, never the fixture's RNG-drawn
    /// stamp; see the module docs).
    #[allow(clippy::too_many_arguments)]
    fn assert_row(
        ticker: &IssueAgentTicker,
        expected: &Value,
        expected_next: Option<DateTime<Utc>>,
        ctx: &str,
    ) {
        assert_eq!(
            ticker.used,
            expected["used"].as_i64().expect("used") as i32,
            "{ctx}: used"
        );
        assert_eq!(
            ticker.granted,
            expected["granted"].as_i64().expect("granted") as i32,
            "{ctx}: granted"
        );
        assert_eq!(
            ticker.waited,
            expected["waited"].as_i64().expect("waited") as i32,
            "{ctx}: waited"
        );
        assert_eq!(
            ticker.user_disabled,
            expected["user_disabled"].as_bool().expect("user_disabled"),
            "{ctx}: user_disabled"
        );
        assert_eq!(
            ticker.enabled,
            expected["enabled"].as_bool().expect("enabled"),
            "{ctx}: enabled"
        );
        assert_eq!(
            ticker.disarm_reason,
            expected["disarm_reason"].as_str().expect("disarm_reason"),
            "{ctx}: disarm_reason"
        );
        assert_eq!(ticker.next_run_at, expected_next, "{ctx}: next_run_at");
        assert_eq!(ticker.last_tick_at, None, "{ctx}: last_tick_at");
        assert_eq!(
            ticker.pending_entry,
            expected["pending_entry"].as_bool().expect("pending_entry"),
            "{ctx}: pending_entry"
        );
        assert_eq!(
            ticker.pending_entry_free,
            expected["pending_entry_free"]
                .as_bool()
                .expect("pending_entry_free"),
            "{ctx}: pending_entry_free"
        );
        let expected_actor = if expected["pending_entry_actor"].is_null() {
            None
        } else {
            Some(ACTOR_ID)
        };
        assert_eq!(
            ticker.pending_entry_actor_id, expected_actor,
            "{ctx}: pending_entry_actor"
        );
        assert_eq!(
            ticker.pending_entry_trigger,
            expected["pending_entry_trigger"]
                .as_str()
                .expect("pending_entry_trigger"),
            "{ctx}: pending_entry_trigger"
        );
        let expected_resume = if expected["resume_parent_run"].is_null() {
            None
        } else {
            Some(RUN_ID)
        };
        assert_eq!(
            ticker.resume_parent_run_id, expected_resume,
            "{ctx}: resume_parent_run"
        );
    }

    fn assert_decision(
        decision: &TickerDecision,
        expected: &Value,
        ticker: Option<Uuid>,
        ctx: &str,
    ) {
        assert_eq!(
            decision.dispatch_now,
            expected["dispatch_now"].as_bool().expect("dispatch_now"),
            "{ctx}: dispatch_now"
        );
        assert_eq!(
            decision.queued,
            expected["queued"].as_bool().expect("queued"),
            "{ctx}: queued"
        );
        assert_eq!(
            decision.parked,
            expected["parked"].as_bool().expect("parked"),
            "{ctx}: parked"
        );
        assert_eq!(
            decision.granted,
            expected["granted"].as_bool().expect("granted"),
            "{ctx}: granted"
        );
        assert_eq!(
            decision.reason,
            expected["reason"].as_str().expect("reason"),
            "{ctx}: reason"
        );
        assert_eq!(decision.ticker, ticker, "{ctx}: decision.ticker");
    }

    /// Our retime stamp at fixed jitter, and the fixture's RNG-drawn stamp in
    /// the same stage's cadence window.
    fn assert_retimed(fixture_ts: &str, interval: i64, actual: Option<DateTime<Utc>>, ctx: &str) {
        let expected = now()
            + chrono::Duration::seconds(interval)
            + chrono::Duration::microseconds(42_500_000);
        assert_eq!(actual, Some(expected), "{ctx}: retimed stamp");
        let fixture: DateTime<Utc> = fixture_ts.parse().expect("fixture ts parses");
        let lo = now() + chrono::Duration::seconds(interval);
        let width = chrono::Duration::microseconds(interval * 1_000_000 / 10);
        assert_eq!(JITTER_FRACTION, 0.1, "window width is JITTER_FRACTION");
        assert!(
            fixture >= lo && fixture < lo + width,
            "{ctx}: fixture {fixture} not in [{lo}, {})",
            lo + width
        );
    }

    fn fixture_ts(row: &Value) -> &str {
        row["next_run_at"].as_str().expect("fixture next_run_at")
    }

    // -- SQL shapes + field lists --------------------------------------------

    #[test]
    fn clock_fields_match_p9_in_order() {
        let golden = fx("clock_primitives.golden.json");
        let expected = golden["primitives"]["P9_clock_fields"]["_TICKER_CLOCK_FIELDS"]
            .as_array()
            .expect("P9 list");
        let actual: Vec<Value> = TICKER_CLOCK_FIELDS
            .iter()
            .map(|f| Value::String(f.to_string()))
            .collect();
        assert_eq!(actual, *expected);
        assert_eq!(TICKER_CLOCK_FIELDS.len(), 10);
        assert!(
            !TICKER_CLOCK_FIELDS.contains(&"used"),
            "reconcile never writes used"
        );
    }

    #[test]
    fn lock_sql_pins_select_for_update_shape() {
        let sql = lock_ticker_sql();
        assert_eq!(
            sql,
            format!(
                "SELECT {} FROM issue_agent_ticker WHERE issue_id = :issue_id AND deleted_at IS NULL ORDER BY id ASC LIMIT 1 FOR UPDATE",
                TICKER_COLUMNS.join(", ")
            )
        );
        assert_eq!(TICKER_COLUMNS.len(), 20);
        assert_eq!(LOCK_TICKER_PARAMS, &["issue_id"]);
    }

    #[test]
    fn create_sql_pins_disabled_unarmed_zero_budget_shape() {
        let sql = create_ticker_sql();
        let columns = sql
            .strip_prefix("INSERT INTO issue_agent_ticker (")
            .expect("insert prefix");
        let end = columns.find(") VALUES (").expect("values sep");
        let listed: Vec<&str> = columns[..end].split(", ").collect();
        assert_eq!(listed, TICKER_COLUMNS);
        let values = &columns[end + ") VALUES (".len()..columns.len() - 1];
        let bound: Vec<&str> = values.split(", ").collect();
        // Fixed create shape per column (COLUMNS order): the two `:now` stamps,
        // the crum-or-NULL creator, NULL audit/future stamps, handler ids, the
        // zero budget, unarmed + disabled with the empty reason, no pending entry.
        assert_eq!(
            bound,
            [
                ":now",
                ":now",
                ":created_by_id",
                "NULL",
                "NULL",
                ":id",
                ":issue_id",
                "0",
                "0",
                "0",
                "FALSE",
                "NULL",
                "NULL",
                "FALSE",
                "''",
                "FALSE",
                "FALSE",
                "NULL",
                "''",
                "NULL"
            ]
        );
        assert_eq!(
            CREATE_TICKER_PARAMS,
            &["now", "created_by_id", "id", "issue_id"]
        );
    }

    #[test]
    fn save_sql_projects_clock_fields_in_meta_order() {
        let set = SAVE_CLOCK_SQL
            .strip_prefix("UPDATE issue_agent_ticker SET ")
            .expect("update prefix");
        let end = set.find(" WHERE id = :id").expect("where pk");
        let assigned: Vec<&str> = set[..end].split(", ").collect();
        // Django `_meta` field order (COLUMNS filtered to the clock fields),
        // FK fields as attnames — not `update_fields` order.
        let expected: Vec<String> = TICKER_COLUMNS
            .iter()
            .filter(|c| {
                let field = c.strip_suffix("_id").unwrap_or(c);
                TICKER_CLOCK_FIELDS.contains(c) || TICKER_CLOCK_FIELDS.contains(&field)
            })
            .map(|c| format!("{c} = :{c}"))
            .collect();
        assert_eq!(assigned, expected);
        assert_eq!(assigned.len(), 10);
        assert!(
            !SAVE_CLOCK_SQL.contains("used"),
            "reconcile never writes used"
        );
        assert_eq!(
            SAVE_CLOCK_PARAMS,
            &[
                "updated_at",
                "granted",
                "next_run_at",
                "enabled",
                "disarm_reason",
                "pending_entry",
                "pending_entry_free",
                "pending_entry_actor_id",
                "pending_entry_trigger",
                "resume_parent_run_id",
                "id"
            ]
        );
    }

    // -- primitive goldens ---------------------------------------------------

    #[test]
    fn lock_create_shape_matches_p1() {
        let golden = fx("clock_primitives.golden.json");
        let after = &golden["primitives"]["P1_lock_create_shape"]["after"];
        let row = new_ticker_row(TICKER_ID, ISSUE_ID, now(), None);
        assert_row(&row, after, None, "P1");
        assert_eq!(row.issue_id, ISSUE_ID, "P1: issue rebound");
        assert_eq!(row.created_by_id, None, "P1: no crum user");
        assert_eq!(row.updated_by_id, None, "P1: updated_by NULL on create");
    }

    #[test]
    fn compute_next_run_at_pins_p10_math() {
        // Exact math at fixed jitter (binary-exact fractions only).
        assert_eq!(
            compute_next_run_at(10800, now(), 0.0),
            now() + chrono::Duration::seconds(10800)
        );
        assert_eq!(
            compute_next_run_at(10800, now(), 42.5),
            now() + chrono::Duration::seconds(10800) + chrono::Duration::microseconds(42_500_000)
        );
        assert_eq!(
            compute_next_run_at(3600, now(), 0.25),
            now() + chrono::Duration::seconds(3600) + chrono::Duration::microseconds(250_000)
        );
        // P10: the fixture stamp sits in the drawn window (the draw itself is
        // CPython's stream, unreproducible by design).
        let golden = fx("clock_primitives.golden.json");
        let stamp = golden["primitives"]["P10_compute_next_run_at"]["interval_10800"]
            .as_str()
            .expect("P10 stamp");
        let fixture: DateTime<Utc> = stamp.parse().expect("P10 parses");
        let lo = now() + chrono::Duration::seconds(10800);
        assert!(
            fixture >= lo && fixture < lo + chrono::Duration::seconds(1080),
            "P10 window"
        );
    }

    #[test]
    fn retime_resolves_per_stage_cadence_p4() {
        let golden = fx("clock_primitives.golden.json");
        for (case, state, interval) in [
            ("P4_retime_impl", in_progress(), 10800),
            ("P4_retime_review", in_review(), 3600),
            (
                "P4_retime_test",
                StateRef {
                    group: "test",
                    name: "In Test",
                },
                7200,
            ),
        ] {
            let after = &golden["primitives"][case]["after"];
            let issue = clock_issue(Some(state), fx5_policy());
            let mut ticker = ticker_from(after);
            // Reset the clock half so the retime is observable; budget survives.
            ticker.enabled = false;
            ticker.next_run_at = None;
            retime_clock(&mut ticker, &issue, now(), JITTER);
            assert!(ticker.enabled, "{case}: armed");
            assert_eq!(ticker.disarm_reason, "", "{case}: armed string");
            assert_row(
                &ticker,
                after,
                Some(compute_next_run_at(interval, now(), JITTER)),
                case,
            );
            assert_retimed(fixture_ts(after), interval, ticker.next_run_at, case);
        }
    }

    #[test]
    fn retime_spent_and_disabled_p5_p6() {
        let golden = fx("clock_primitives.golden.json");
        // P5: spent pool stops with POOL_SPENT, next_run_at kept (was None).
        let case = &golden["primitives"]["P5_retime_spent_pool"];
        let issue = clock_issue(Some(in_progress()), fx5_policy());
        let mut ticker = ticker_from(&case["before"]);
        retime_clock(&mut ticker, &issue, now(), JITTER);
        assert_row(&ticker, &case["after"], None, "P5");
        // P6a: user switch stamps USER_DISABLED.
        let case = &golden["primitives"]["P6a_retime_user_disabled"];
        let mut ticker = ticker_from(&case["before"]);
        retime_clock(&mut ticker, &issue, now(), JITTER);
        assert_row(&ticker, &case["after"], None, "P6a");
        // P6b: project switch stamps the empty reason (stopped, not armed).
        let case = &golden["primitives"]["P6b_retime_project_disabled"];
        let mut disabled = fx5_policy();
        disabled.agent_ticking_enabled = Some(false);
        let issue = clock_issue(Some(in_progress()), disabled);
        let mut ticker = ticker_from(&case["before"]);
        retime_clock(&mut ticker, &issue, now(), JITTER);
        assert_row(&ticker, &case["after"], None, "P6b");
    }

    #[test]
    fn queue_free_vs_counting_p7() {
        let golden = fx("clock_primitives.golden.json");
        // P7a: a free entry remembers actor + trigger, next_run_at = now.
        let after = &golden["primitives"]["P7a_queue_free"]["after"];
        let mut ticker = ticker_from(after);
        ticker.enabled = false;
        ticker.pending_entry = false;
        queue_entry(&mut ticker, now(), true, Some(ACTOR_ID), "run_ai");
        assert_row(&ticker, after, Some(now()), "P7a");
        // P7b: a counting entry clears both even when handed them.
        let after = &golden["primitives"]["P7b_queue_counting"]["after"];
        let mut ticker = ticker_from(after);
        ticker.enabled = false;
        ticker.pending_entry = false;
        queue_entry(&mut ticker, now(), false, Some(ACTOR_ID), "run_ai");
        assert_row(&ticker, after, Some(now()), "P7b");
    }

    #[test]
    fn stop_clock_keeps_next_run_at_p8() {
        let golden = fx("clock_primitives.golden.json");
        let case = &golden["primitives"]["P8_stop_clock"];
        let mut ticker = ticker_from(&case["before"]);
        stop_clock(&mut ticker, TickerDisarmReason::CapHit.as_str());
        let kept: DateTime<Utc> = fixture_ts(&case["before"]).parse().expect("before ts");
        assert_row(&ticker, &case["after"], Some(kept), "P8");
    }

    #[test]
    fn paused_allowed_and_pool_checks() {
        assert!(is_paused_state(Some(&paused_state())));
        assert!(!is_paused_state(Some(&in_progress())));
        assert!(!is_paused_state(None));
        assert!(project_ticking_enabled(&fx5_policy()));
        assert!(
            project_ticking_enabled(&ProjectClockPolicy::default()),
            "getattr default true"
        );
        let mut off = fx5_policy();
        off.agent_ticking_enabled = Some(false);
        assert!(!project_ticking_enabled(&off));
        assert_eq!(pool_size(&fx5_policy()), 10);
        assert_eq!(pool_size(&ProjectClockPolicy::default()), 10);
        assert_eq!(
            pool_size(&ProjectClockPolicy {
                agent_default_max_ticks: Some(-1),
                ..Default::default()
            }),
            -1
        );
        let row = new_ticker_row(TICKER_ID, ISSUE_ID, now(), None);
        assert!(clock_allowed(&row, &fx5_policy()));
        let mut disabled = row.clone();
        disabled.user_disabled = true;
        assert!(!clock_allowed(&disabled, &fx5_policy()));
        assert!(!clock_allowed(&row, &off));
        // Cadence falls back to the impl pair outside the registry.
        assert_eq!(
            effective_interval_seconds(Some(&in_progress()), &fx5_policy()),
            10800
        );
        assert_eq!(
            effective_interval_seconds(Some(&in_review()), &fx5_policy()),
            3600
        );
        assert_eq!(
            effective_interval_seconds(Some(&done_state()), &fx5_policy()),
            10800
        );
        assert_eq!(effective_interval_seconds(None, &fx5_policy()), 10800);
    }

    // -- enter/move table ----------------------------------------------------

    /// Run one enter/move case through `reconcile` and pin decision + after-row + write.
    #[allow(clippy::too_many_arguments)]
    fn replay_enter_move(
        name: &str,
        event: &TickerEvent,
        state: StateRef<'static>,
        policy: ProjectClockPolicy,
        busy: bool,
        expected_write: ClockWrite,
        expected_next: Option<DateTime<Utc>>,
    ) {
        let golden = fx("reconcile.enter_move.before_after.json");
        let case = &golden["cases"][name];
        let issue = clock_issue(Some(state), policy);
        let mut slot = if case["before"].is_null() {
            None
        } else {
            Some(ticker_from(&case["before"]))
        };
        let outcome = reconcile(&mut slot, &issue, event, None, busy, now(), JITTER, None)
            .expect("known kind reconciles");
        let row = slot.as_ref().expect("enter/move always leaves a row");
        assert_eq!(outcome.write, expected_write, "{name}: write");
        assert_decision(&outcome.decision, &case["decision"], Some(row.id), name);
        assert_row(row, &case["after"], expected_next, name);
        if case["before"].is_null() {
            assert_eq!(row.issue_id, ISSUE_ID, "{name}: created for this issue");
            assert_eq!(row.created_by_id, None, "{name}: no crum user");
        } else {
            assert_eq!(row.id, TICKER_ID, "{name}: same row rebound");
        }
    }

    #[test]
    fn enter_move_table_replays_e1_e10() {
        let retimed = |interval: i64| Some(compute_next_run_at(interval, now(), JITTER));
        // E1: human enter, no ticker → dispatch-now + armed (created).
        replay_enter_move(
            "E1_human_enter_no_ticker",
            &TickerEvent::entered_bucket(None, None, true, None),
            in_progress(),
            fx5_policy(),
            false,
            ClockWrite::Insert,
            retimed(10800),
        );
        // E2: human enter while busy → free entry queued with actor + trigger.
        replay_enter_move(
            "E2_human_enter_busy",
            &TickerEvent::entered_bucket(None, None, true, Some(ACTOR_ID)),
            in_progress(),
            fx5_policy(),
            true,
            ClockWrite::Insert,
            Some(now()),
        );
        // E3: human move, no run wanted → retimed.
        replay_enter_move(
            "E3_human_move_no_run_wanted",
            &TickerEvent::moved_stage(None, None, false, None),
            in_progress(),
            fx5_policy(),
            false,
            ClockWrite::Update,
            retimed(10800),
        );
        // E4: human move into a spent pool → dispatch-now + POOL_SPENT stop.
        replay_enter_move(
            "E4_human_move_spent_pool",
            &TickerEvent::moved_stage(None, None, true, None),
            in_progress(),
            fx5_policy(),
            false,
            ClockWrite::Update,
            None,
        );
        // E5: user-disabled → dispatch-now + USER_DISABLED stop.
        replay_enter_move(
            "E5_human_move_user_disabled",
            &TickerEvent::entered_bucket(None, None, true, None),
            in_progress(),
            fx5_policy(),
            false,
            ClockWrite::Update,
            None,
        );
        // E6: project-disabled (FX5B) → dispatch-now + stopped, empty reason.
        let mut fx5b = fx5_policy();
        fx5b.agent_ticking_enabled = Some(false);
        replay_enter_move(
            "E6_human_move_project_disabled",
            &TickerEvent::entered_bucket(None, None, true, None),
            in_progress(),
            fx5b,
            false,
            ClockWrite::Insert,
            None,
        );
        // E7: agent move with budget → counting entry queued.
        replay_enter_move(
            "E7_agent_move_budget",
            &TickerEvent::moved_stage(Some(RUN_ID), None, true, None),
            in_progress(),
            fx5_policy(),
            false,
            ClockWrite::Update,
            Some(now()),
        );
        // E8: agent move, pool spent → parked.
        replay_enter_move(
            "E8_agent_move_pool_spent",
            &TickerEvent::moved_stage(Some(RUN_ID), None, true, None),
            in_progress(),
            fx5_policy(),
            false,
            ClockWrite::Update,
            None,
        );
        // E9: agent move while user-disabled → ticking-disabled.
        replay_enter_move(
            "E9_agent_move_disabled",
            &TickerEvent::moved_stage(Some(RUN_ID), None, true, None),
            in_progress(),
            fx5_policy(),
            false,
            ClockWrite::Update,
            None,
        );
        // E10: cross-stage move captures the resume parent; review cadence.
        replay_enter_move(
            "E10_move_remembers_resume_parent",
            &TickerEvent::moved_stage(None, Some(RUN_ID), true, None),
            in_review(),
            fx5_policy(),
            false,
            ClockWrite::Insert,
            retimed(3600),
        );
        // Retime cadence windows against the fixture stamps.
        let golden = fx("reconcile.enter_move.before_after.json");
        for (name, interval) in [
            ("E1_human_enter_no_ticker", 10800),
            ("E3_human_move_no_run_wanted", 10800),
            ("E10_move_remembers_resume_parent", 3600),
        ] {
            let after = &golden["cases"][name]["after"];
            assert_retimed(
                fixture_ts(after),
                interval,
                Some(compute_next_run_at(interval, now(), JITTER)),
                name,
            );
        }
        assert_eq!(
            golden["cases"].as_object().expect("cases").len(),
            10,
            "all E cases replayed"
        );
    }

    // -- left-bucket table ---------------------------------------------------

    #[test]
    fn left_bucket_table_replays_l1_l2() {
        let golden = fx("reconcile.left_bucket.before_after.json");
        // L1: dormant with the pool kept (used/granted survive, next cleared).
        let case = &golden["cases"]["L1_left_bucket_dormant_keeps_pool"];
        let issue = clock_issue(Some(done_state()), fx5_policy());
        let mut slot = Some(ticker_from(&case["before"]));
        let outcome = reconcile(
            &mut slot,
            &issue,
            &TickerEvent::left_bucket(),
            None,
            false,
            now(),
            JITTER,
            None,
        )
        .expect("known kind reconciles");
        let row = slot.as_ref().expect("L1 leaves a row");
        assert_eq!(outcome.write, ClockWrite::Update, "L1: write");
        assert_decision(&outcome.decision, &case["decision"], Some(TICKER_ID), "L1");
        assert_row(row, &case["after"], None, "L1");
        // L2: no ticker is a no-op.
        let case = &golden["cases"]["L2_left_bucket_no_ticker"];
        let mut slot: Option<IssueAgentTicker> = None;
        let outcome = reconcile(
            &mut slot,
            &issue,
            &TickerEvent::left_bucket(),
            None,
            false,
            now(),
            JITTER,
            None,
        )
        .expect("known kind reconciles");
        assert_eq!(outcome.write, ClockWrite::None, "L2: write");
        assert_decision(&outcome.decision, &case["decision"], None, "L2");
        assert!(slot.is_none(), "L2: no row created");
        assert_eq!(
            golden["cases"].as_object().expect("cases").len(),
            2,
            "all L cases replayed"
        );
    }

    // -- run-ended table -----------------------------------------------------

    /// Run one run-ended case through `reconcile` and pin decision + after-row + write.
    #[allow(clippy::too_many_arguments)]
    fn replay_run_ended(
        name: &str,
        state: StateRef<'static>,
        run: &RunEndedRef<'_>,
        outcome: Option<&str>,
        expected_write: ClockWrite,
        expected_next: Option<DateTime<Utc>>,
    ) {
        let golden = fx("reconcile.run_ended.before_after.json");
        let case = &golden["cases"][name];
        let issue = clock_issue(Some(state), fx5_policy());
        let mut slot = if case["before"].is_null() {
            None
        } else {
            Some(ticker_from(&case["before"]))
        };
        let event = TickerEvent::run_ended(Some(RUN_ID), outcome);
        let outcome = reconcile(
            &mut slot,
            &issue,
            &event,
            Some(run),
            false,
            now(),
            JITTER,
            None,
        )
        .expect("known kind reconciles");
        assert_eq!(outcome.write, expected_write, "{name}: write");
        match slot.as_ref() {
            None => assert_decision(&outcome.decision, &case["decision"], None, name),
            Some(row) => {
                assert_decision(&outcome.decision, &case["decision"], Some(TICKER_ID), name);
                assert_row(row, &case["after"], expected_next, name);
            }
        }
    }

    #[test]
    fn run_ended_table_replays_r1_r8() {
        let completed_review = RunEndedRef {
            status: AgentRunStatus::Completed,
            phase_kind: "review",
            done_payload: None,
        };
        let completed_coding = RunEndedRef {
            status: AgentRunStatus::Completed,
            phase_kind: "coding-task",
            done_payload: None,
        };
        // R1 × 3: stopping outcomes → TERMINAL_SIGNAL stop, next_run_at kept.
        let kept = Some(now());
        for (name, outcome) in [
            ("R1_stopping_done", "done"),
            ("R1_stopping_blocked", "blocked"),
            ("R1_stopping_waiting_on_human", "waiting_on_human"),
        ] {
            replay_run_ended(
                name,
                in_review(),
                &completed_review,
                Some(outcome),
                ClockWrite::Update,
                kept,
            );
        }
        // R2 × 2: continuing outcomes re-arm a dateless clock (impl cadence).
        let rearmed = Some(compute_next_run_at(10800, now(), JITTER));
        for (name, outcome) in [
            ("R2_continuing_progressed", "progressed"),
            ("R2_continuing_waiting_on_external", "waiting_on_external"),
        ] {
            replay_run_ended(
                name,
                in_progress(),
                &completed_coding,
                Some(outcome),
                ClockWrite::Update,
                rearmed,
            );
        }
        // R2c: continuing with next_run_at set → untouched, no save.
        replay_run_ended(
            "R2c_continuing_keeps_next_run_at",
            in_progress(),
            &completed_coding,
            Some("progressed"),
            ClockWrite::None,
            kept,
        );
        // R3: the kind guard — a coding run's `done` must not stop the
        // review clock; the queued next-stage entry survives.
        replay_run_ended(
            "R3_stage_moved_on_guard",
            in_review(),
            &completed_coding,
            Some("done"),
            ClockWrite::None,
            kept,
        );
        // R4a: completed coding-task without yield → progressed (kept ticking).
        replay_run_ended(
            "R4a_no_yield_completed_coding",
            in_progress(),
            &completed_coding,
            None,
            ClockWrite::None,
            kept,
        );
        // R4b: completed review without yield → done (stopped).
        replay_run_ended(
            "R4b_no_yield_completed_review",
            in_review(),
            &completed_review,
            None,
            ClockWrite::Update,
            kept,
        );
        // R4c: paused_awaiting_input without payload → waiting_on_human.
        // The fixture does not pin the run kind; `""` (the model default)
        // passes the guard via the falsy branch.
        let paused = RunEndedRef {
            status: AgentRunStatus::PausedAwaitingInput,
            phase_kind: "",
            done_payload: None,
        };
        replay_run_ended(
            "R4c_no_yield_paused_awaiting",
            in_progress(),
            &paused,
            None,
            ClockWrite::Update,
            kept,
        );
        // R4d: failed without payload → progressed (a crash never stops).
        let failed = RunEndedRef {
            status: AgentRunStatus::Failed,
            phase_kind: "",
            done_payload: None,
        };
        replay_run_ended(
            "R4d_no_yield_failed",
            in_progress(),
            &failed,
            None,
            ClockWrite::None,
            kept,
        );
        // R5: a queued entry outranks the finished run's opinion.
        replay_run_ended(
            "R5_pending_entry_kept",
            in_review(),
            &completed_review,
            Some("done"),
            ClockWrite::None,
            kept,
        );
        // R6: stopping on a disarmed clock preserves the prior cap_hit.
        replay_run_ended(
            "R6_prior_cap_hit_preserved",
            in_progress(),
            &completed_coding,
            Some("done"),
            ClockWrite::None,
            None,
        );
        // R7: outside the bucket → not-in-bucket (the run is unread).
        replay_run_ended(
            "R7_not_in_bucket",
            done_state(),
            &completed_coding,
            Some("done"),
            ClockWrite::None,
            None,
        );
        // R8: no ticker → no-ticker.
        replay_run_ended(
            "R8_no_ticker",
            in_progress(),
            &completed_coding,
            Some("done"),
            ClockWrite::None,
            None,
        );
        // Retime cadence windows against the fixture stamps.
        let golden = fx("reconcile.run_ended.before_after.json");
        for name in [
            "R2_continuing_progressed",
            "R2_continuing_waiting_on_external",
        ] {
            let after = &golden["cases"][name]["after"];
            assert_retimed(fixture_ts(after), 10800, rearmed, name);
        }
        assert_eq!(
            golden["cases"].as_object().expect("cases").len(),
            15,
            "all R cases replayed"
        );
    }

    #[test]
    fn run_ended_resolves_outcome_from_payload() {
        // Beyond the fixtures: `event.outcome = None` with a status-bearing
        // payload resolves through L1's `outcome_for_run` (the R-cases pin the
        // verbatim and the no-yield paths; L1 pins the parser itself).
        let payload = serde_json::json!({"status": "blocked"});
        let run = RunEndedRef {
            status: AgentRunStatus::Completed,
            phase_kind: "review",
            done_payload: Some(&payload),
        };
        let issue = clock_issue(Some(in_review()), fx5_policy());
        let mut row = new_ticker_row(TICKER_ID, ISSUE_ID, now(), None);
        row.enabled = true;
        row.next_run_at = Some(now());
        let mut slot = Some(row);
        let event = TickerEvent::run_ended(Some(RUN_ID), None);
        let outcome = reconcile(
            &mut slot,
            &issue,
            &event,
            Some(&run),
            false,
            now(),
            JITTER,
            None,
        )
        .expect("known kind reconciles");
        assert_eq!(outcome.decision.reason, "blocked:stopped");
        assert_eq!(outcome.write, ClockWrite::Update);
        let row = slot.as_ref().expect("row survives");
        assert_eq!(row.disarm_reason, "terminal_signal");
        assert_eq!(row.next_run_at, Some(now()), "stopping keeps next_run_at");
    }

    // -- human-requested + retick table ---------------------------------------

    /// Run one human/retick case through `reconcile` and pin decision + after-row + write.
    #[allow(clippy::too_many_arguments)]
    fn replay_human_retick(
        name: &str,
        event: &TickerEvent,
        state: StateRef<'static>,
        policy: ProjectClockPolicy,
        busy: bool,
        expected_write: ClockWrite,
        expected_next: Option<DateTime<Utc>>,
    ) {
        let golden = fx("reconcile.human_retick.before_after.json");
        let case = &golden["cases"][name];
        let issue = clock_issue(Some(state), policy);
        let mut slot = if case["before"].is_null() {
            None
        } else {
            Some(ticker_from(&case["before"]))
        };
        let outcome = reconcile(&mut slot, &issue, event, None, busy, now(), JITTER, None)
            .expect("known kind reconciles");
        assert_eq!(outcome.write, expected_write, "{name}: write");
        match slot.as_ref() {
            None => assert_decision(&outcome.decision, &case["decision"], None, name),
            Some(row) => {
                let expected_id = if case["before"].is_null() {
                    row.id
                } else {
                    TICKER_ID
                };
                assert_decision(
                    &outcome.decision,
                    &case["decision"],
                    Some(expected_id),
                    name,
                );
                assert_row(row, &case["after"], expected_next, name);
            }
        }
    }

    #[test]
    fn human_retick_table_replays_h1_t5() {
        let retimed = Some(compute_next_run_at(10800, now(), JITTER));
        // H1: free human run → dispatch-now + re-armed (used untouched).
        replay_human_retick(
            "H1_human_run_free",
            &TickerEvent::human_run_requested(true, None, ""),
            in_progress(),
            fx5_policy(),
            false,
            ClockWrite::Update,
            retimed,
        );
        // H2: busy human run → free entry with actor + explicit trigger.
        replay_human_retick(
            "H2_human_run_busy",
            &TickerEvent::human_run_requested(true, Some(ACTOR_ID), "comment_and_run"),
            in_progress(),
            fx5_policy(),
            true,
            ClockWrite::Update,
            Some(now()),
        );
        // H3: no run wanted → retimed, no dispatch.
        replay_human_retick(
            "H3_human_run_no_run_wanted",
            &TickerEvent::human_run_requested(false, None, ""),
            in_progress(),
            fx5_policy(),
            false,
            ClockWrite::Update,
            retimed,
        );
        // T1: one fresh pool granted (0 → 10) + dispatch.
        replay_human_retick(
            "T1_retick_grant_dispatch",
            &TickerEvent::retick(true, None),
            in_progress(),
            fx5_policy(),
            false,
            ClockWrite::Update,
            retimed,
        );
        // T2a/b/c: the three guards (note the underscores).
        replay_human_retick(
            "T2a_retick_no_ticker",
            &TickerEvent::retick(true, None),
            in_progress(),
            fx5_policy(),
            false,
            ClockWrite::None,
            None,
        );
        replay_human_retick(
            "T2b_retick_not_ticking_state",
            &TickerEvent::retick(true, None),
            done_state(),
            fx5_policy(),
            false,
            ClockWrite::None,
            None,
        );
        replay_human_retick(
            "T2c_retick_budget_not_exhausted",
            &TickerEvent::retick(true, None),
            in_progress(),
            fx5_policy(),
            false,
            ClockWrite::None,
            None,
        );
        // T2d: infinite pool (FX5I) returns via the budget guard — the
        // sentinel check below it is unreachable by construction.
        let mut infinite = fx5_policy();
        infinite.agent_default_max_ticks = Some(-1);
        replay_human_retick(
            "T2d_retick_infinite_pool",
            &TickerEvent::retick(true, None),
            in_progress(),
            infinite,
            false,
            ClockWrite::None,
            None,
        );
        // T4: grant + free-entry queue while busy.
        replay_human_retick(
            "T4_retick_busy",
            &TickerEvent::retick(true, Some(ACTOR_ID)),
            in_progress(),
            fx5_policy(),
            true,
            ClockWrite::Update,
            Some(now()),
        );
        // T5: granted-from-paused — the grant lands, the clock is untouched.
        replay_human_retick(
            "T5_retick_from_paused",
            &TickerEvent::retick(true, None),
            paused_state(),
            fx5_policy(),
            false,
            ClockWrite::Update,
            None,
        );
        // Retime cadence windows against the fixture stamps.
        let golden = fx("reconcile.human_retick.before_after.json");
        for name in [
            "H1_human_run_free",
            "H3_human_run_no_run_wanted",
            "T1_retick_grant_dispatch",
        ] {
            let after = &golden["cases"][name]["after"];
            assert_retimed(fixture_ts(after), 10800, retimed, name);
        }
        assert_eq!(
            golden["cases"].as_object().expect("cases").len(),
            10,
            "all H/T cases replayed"
        );
    }

    // -- dispatcher ----------------------------------------------------------

    #[test]
    fn creates_ticker_table() {
        assert!(creates_ticker(TickerEventKind::ENTERED_BUCKET));
        assert!(creates_ticker(TickerEventKind::MOVED_STAGE));
        assert!(!creates_ticker(TickerEventKind::LEFT_BUCKET));
        assert!(!creates_ticker(TickerEventKind::RUN_ENDED));
        assert!(creates_ticker(TickerEventKind::HUMAN_RUN_REQUESTED));
        assert!(!creates_ticker(TickerEventKind::RETICK));
        assert!(!creates_ticker("bogus"));
    }

    #[test]
    fn dispatcher_rejects_unknown_kind() {
        let issue = clock_issue(Some(in_progress()), fx5_policy());
        let mut slot = Some(new_ticker_row(TICKER_ID, ISSUE_ID, now(), None));
        let event = TickerEvent {
            kind: "bogus".to_string(),
            moved_by_run: None,
            resume_parent: None,
            run: None,
            outcome: None,
            want_run: true,
            actor: None,
            trigger: String::new(),
        };
        let err = reconcile(&mut slot, &issue, &event, None, false, now(), JITTER, None)
            .expect_err("unknown kind fails");
        assert_eq!(
            err,
            ClockError::UnknownEventKind {
                kind: "bogus".to_string()
            }
        );
        assert_eq!(err.to_string(), "unknown ticker event kind 'bogus'");
    }

    #[test]
    fn dispatcher_requires_run_for_run_ended() {
        let issue = clock_issue(Some(in_progress()), fx5_policy());
        let mut slot = Some(new_ticker_row(TICKER_ID, ISSUE_ID, now(), None));
        let event = TickerEvent::run_ended(Some(RUN_ID), Some("done"));
        let err = reconcile(&mut slot, &issue, &event, None, false, now(), JITTER, None)
            .expect_err("run_ended without a run fails");
        assert_eq!(err, ClockError::RunEndedWithoutRun);
    }

    #[test]
    fn reconcile_log_line_verbatim() {
        let decision = TickerDecision {
            ticker: Some(TICKER_ID),
            dispatch_now: true,
            queued: false,
            parked: false,
            granted: false,
            reason: "dispatch-now".to_string(),
        };
        assert_eq!(
            reconcile_log_line(&ISSUE_ID, TickerEventKind::ENTERED_BUCKET, &decision),
            format!(
                "agent_ticker: reconcile issue={ISSUE_ID} event=entered_bucket -> dispatch_now=True queued=False parked=False reason=dispatch-now"
            )
        );
        let parked = TickerDecision {
            ticker: Some(TICKER_ID),
            parked: true,
            reason: "pool-spent".to_string(),
            ..TickerDecision::default()
        };
        assert_eq!(
            reconcile_log_line(&ISSUE_ID, TickerEventKind::MOVED_STAGE, &parked),
            format!(
                "agent_ticker: reconcile issue={ISSUE_ID} event=moved_stage -> dispatch_now=False queued=False parked=True reason=pool-spent"
            )
        );
    }

    // -- thin senders --------------------------------------------------------

    #[test]
    fn senders_replay_s1_s4() {
        let golden = fx("thin_senders.golden.json");
        let issue = clock_issue(Some(in_progress()), fx5_policy());
        let retimed = Some(compute_next_run_at(10800, now(), JITTER));
        // S1: arm re-times, `used` untouched.
        let case = &golden["cases"]["S1_arm"];
        let mut row = ticker_from(&case["before"]);
        let outcome = arm_ticker(&mut row, &issue, now(), JITTER);
        assert_eq!(outcome.write, ClockWrite::Update, "S1: write");
        assert_eq!(outcome.decision.ticker, Some(TICKER_ID), "S1: ticker");
        assert_row(&row, &case["after"], retimed, "S1");
        assert_retimed(fixture_ts(&case["after"]), 10800, retimed, "S1");
        // S2a: default disarm stamps LEFT_TICKING_STATE and clears next_run_at.
        let case = &golden["cases"]["S2a_disarm_default"];
        let mut row = ticker_from(&case["before"]);
        let id = disarm_ticker(
            Some(&mut row),
            TickerDisarmReason::LeftTickingState.as_str(),
        )
        .expect("default disarms");
        assert_eq!(id, Some(TICKER_ID), "S2a: ticker");
        assert_row(&row, &case["after"], None, "S2a");
        // S2b: a custom reason keeps next_run_at.
        let case = &golden["cases"]["S2b_disarm_custom_reason"];
        let mut row = ticker_from(&case["before"]);
        let kept: DateTime<Utc> = fixture_ts(&case["before"]).parse().expect("S2b before ts");
        let id = disarm_ticker(Some(&mut row), TickerDisarmReason::UserDisabled.as_str())
            .expect("custom disarms");
        assert_eq!(id, Some(TICKER_ID), "S2b: ticker");
        assert_row(&row, &case["after"], Some(kept), "S2b");
        // S2c: no ticker answers None.
        let id = disarm_ticker(None, TickerDisarmReason::LeftTickingState.as_str())
            .expect("no ticker ok");
        assert_eq!(id, None, "S2c");
        // S2d: TERMINAL_SIGNAL refused, string verbatim (em dash included).
        let err =
            disarm_ticker(None, TickerDisarmReason::TerminalSignal.as_str()).expect_err("refused");
        assert_eq!(err, ClockError::TerminalSignalViaDisarm);
        assert_eq!(
            err.to_string(),
            golden["cases"]["S2d_disarm_terminal_signal"]["value_error"]
                .as_str()
                .expect("S2d string")
        );
        // S3a: stopping run → true, clock stopped (completed review, no yield → done).
        let case = &golden["cases"]["S3a_maybe_disarm_stopping"];
        let stopping = RunEndedRef {
            status: AgentRunStatus::Completed,
            phase_kind: "review",
            done_payload: None,
        };
        let review_issue = clock_issue(Some(in_review()), fx5_policy());
        let mut slot = Some(ticker_from(&case["before"]));
        let (stopped, outcome) = maybe_disarm_on_terminal_signal(
            &mut slot,
            &review_issue,
            &stopping,
            Some(ISSUE_ID),
            now(),
            JITTER,
        );
        assert!(stopped, "S3a: stopped");
        let outcome = outcome.expect("S3a: outcome");
        assert_eq!(outcome.decision.reason, "done:stopped", "S3a: reason");
        assert_eq!(outcome.write, ClockWrite::Update, "S3a: write");
        assert_row(
            slot.as_ref().expect("S3a row"),
            &case["after"],
            Some(kept),
            "S3a",
        );
        // S3b: continuing run → false, clock untouched.
        let case = &golden["cases"]["S3b_maybe_disarm_continuing"];
        let continuing = RunEndedRef {
            status: AgentRunStatus::Completed,
            phase_kind: "coding-task",
            done_payload: None,
        };
        let mut slot = Some(ticker_from(&case["before"]));
        let (stopped, outcome) = maybe_disarm_on_terminal_signal(
            &mut slot,
            &issue,
            &continuing,
            Some(ISSUE_ID),
            now(),
            JITTER,
        );
        assert!(!stopped, "S3b: kept ticking");
        let outcome = outcome.expect("S3b: outcome");
        assert_eq!(
            outcome.decision.reason, "progressed:keep-ticking",
            "S3b: reason"
        );
        assert_eq!(outcome.write, ClockWrite::None, "S3b: write");
        assert_row(
            slot.as_ref().expect("S3b row"),
            &case["after"],
            Some(kept),
            "S3b",
        );
        // S3c: a run with no work item sends nothing.
        let mut slot = Some(ticker_from(&case["before"]));
        let (stopped, outcome) =
            maybe_disarm_on_terminal_signal(&mut slot, &issue, &continuing, None, now(), JITTER);
        assert!(!stopped, "S3c");
        assert!(outcome.is_none(), "S3c: no event sent");
        // S4a: reset re-times like arm.
        let case = &golden["cases"]["S4a_reset"];
        let mut row = ticker_from(&case["before"]);
        let outcome = reset_ticker_after_comment_and_run(&mut row, &issue, now(), JITTER);
        assert_eq!(outcome.write, ClockWrite::Update, "S4a: write");
        assert_row(&row, &case["after"], retimed, "S4a");
        // S4b: reset on a spent pool parks with POOL_SPENT.
        let case = &golden["cases"]["S4b_reset_spent_pool"];
        let mut row = ticker_from(&case["before"]);
        let outcome = reset_ticker_after_comment_and_run(&mut row, &issue, now(), JITTER);
        assert_eq!(outcome.write, ClockWrite::Update, "S4b: write");
        assert_row(&row, &case["after"], None, "S4b");
        assert_eq!(
            golden["cases"].as_object().expect("cases").len(),
            10,
            "all S cases replayed"
        );
    }

    // -- cross-case audits ---------------------------------------------------

    #[test]
    fn stop_reasons_verbatim_and_cap_hit_never_produced() {
        // Fixture-side half of the audit: every after-row reason is a known
        // disarm string, and `cap_hit` only ever survives from a before-row
        // (the timer tick owns it). The replay tests pin the port's reasons
        // exact against these rows, so together they prove the port never
        // produces a fresh `cap_hit`.
        let allowed = [
            "",
            "left_ticking_state",
            "pool_spent",
            "terminal_signal",
            "user_disabled",
            "cap_hit",
        ];
        for name in [
            "reconcile.enter_move.before_after.json",
            "reconcile.left_bucket.before_after.json",
            "reconcile.run_ended.before_after.json",
            "reconcile.human_retick.before_after.json",
        ] {
            let golden = fx(name);
            for (case_name, case) in golden["cases"].as_object().expect("cases") {
                let after = &case["after"];
                if after.is_null() {
                    continue;
                }
                let reason = after["disarm_reason"].as_str().expect("after reason");
                assert!(allowed.contains(&reason), "{name} {case_name}: {reason}");
                if reason == "cap_hit" {
                    assert_eq!(
                        case["before"]["disarm_reason"].as_str(),
                        Some("cap_hit"),
                        "{name} {case_name}: cap_hit must survive, never arise"
                    );
                }
            }
        }
    }

    #[test]
    fn any_in_range_draw_lands_in_window() {
        // The handler-side draw composes with the math: any draw in
        // `[0, width)` lands the stamp in-window. Draws are fixed values, not
        // live RNG output — `pidash-db` (rand 0.8) and this crate (rand 0.9)
        // resolve different `rand` majors, so no RNG crosses the call boundary;
        // the draw distribution is D-10's tested responsibility.
        for interval in [3600_i64, 7200, 10800] {
            let width_micros = interval * 1_000_000 / 10;
            for micros in [0, 1, 42_500_000, width_micros - 2] {
                let draw = micros as f64 / 1_000_000.0;
                let stamp = compute_next_run_at(interval, now(), draw);
                let lo = now() + chrono::Duration::seconds(interval);
                let width = chrono::Duration::microseconds(width_micros);
                assert!(
                    stamp >= lo && stamp < lo + width,
                    "interval {interval} draw {draw}"
                );
            }
        }
    }
}
