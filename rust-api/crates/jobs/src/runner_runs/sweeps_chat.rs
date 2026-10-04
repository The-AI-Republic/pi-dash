//! Dedupe + chat sweeps + stall/terminal reconcile (D-15 L6b).
//!
//! Ports `runner/tasks.py:205-353` (PIDASHCONV-540, fixture FX-RUN-07):
//!
//! * `runner.sweep_run_message_dedupe` (`:205-213`) and
//!   `runner.sweep_chat_message_dedupe` (`:216-224`): delete idempotency
//!   rows older than the TTL.
//! * `runner.sweep_agent_chat_state` (`:227-244`): `sweep_active_turns()`
//!   + `sweep_empty_sessions()` from L5.
//! * `runner.reconcile_stalled_runs` (`:246-330`): the 3-conjunct stall
//!   watchdog, reaping via L4 `finalize_run_terminal`.
//! * `runner.apply_agent_run_terminal_effects` (`:333-337`) +
//!   `runner.reconcile_agent_run_terminal_effects` (`:340-353`, one unit):
//!   the single apply plus the batch-of-100 `.delay` fan-out.
//!
//! # Layering: executors over L4/L5 planners
//!
//! Every statement comes from a planner: L5 `chat` (sweep selects, the
//! fail-turn plan, event inserts), L4 `lifecycle`/`finalization`
//! (terminal-update values, finalize lock/update, the terminal-effects
//! plan, comment + scheduler-hook SQL). This module adds the candidate
//! selects Django builds at the task call sites (the stall 3-conjunct
//! query, the reconcile batch query, the two dedupe deletes), the row
//! mapping, the transaction/savepoint structure, and the post-commit
//! effect firing. Settings resolve from the environment under the same
//! names as `settings/common.py`, with the Django defaults.
//!
//! # Seams
//!
//! Calls owned by other planes go through [`RunnerRunsSeams`]
//! (`FireTickSeam`/`ExecuteSeams` precedent): D-14 matcher drains, the
//! D-11 `dispatch_waiting` call, D-12 orchestration + handoff +
//! agent-system-user, and the chat-event Redis publish (the jobs crate
//! has no Redis dependency; `pidash_db::redis::RedisHandle` has no
//! publish method, so a direct publish would be a foundation change).
//! Celery `.delay` fan-out needs no seam: it enqueues into
//! `rust_job_queue`, which routes local when a handler is registered
//! and forwards to the Python plane over AMQP otherwise.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * Both dedupe sweeps read `RUN_MESSAGE_DEDUPE_TTL_SECS` — the chat
//!   sweep shares the run key (`tasks.py:208,219`).
//! * The stall loop's `if run.runner is None: continue` guard is kept
//!   although the `INNER JOIN`s make it dead.
//! * `finalize_run_terminal`'s docstring promises a post-commit drain
//!   re-fire, but the code ends with log-and-return
//!   (`run_lifecycle.py:458-463`): no drain is ported.
//! * A pending-entry `.delay` that fails after the hooks commit is lost:
//!   the hooks marker is already set, so the retry skips the hooks phase
//!   and never re-registers the fire. Kept as-is.
//! * A malformed `apply_agent_run_terminal_effects` arg parks the job
//!   (`Verdict::Fail`) instead of spending the retry budget — the
//!   `fire_tick` precedent. Same terminal state as Celery's
//!   retry-then-dead-letter, without the futile retries.
//! * Garbage in a settings env var falls back to the Django default
//!   (the F-09 `env_secs` precedent) instead of failing the task like
//!   Python's `int(getattr(...))` would. Zero/negative values pass
//!   through unclamped, exactly like `int()`.
//! * The dedupe deletes take Django's collector fast path (leaf tables,
//!   no signals): a single `DELETE`, no candidate fetch. The executor
//!   runs that statement directly, in autocommit rather than the
//!   collector's one-statement transaction — same rows, same count.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_db::runner_enroll::columns::{dev_machine, runner as runner_cols};
use pidash_db::runner_runs::{
    agent_run, chat_dedupe, chat_event, chat_message, chat_session, run_dedupe, AgentChatMessage,
    AgentChatSession,
};
use pidash_db::tasks_ticker::models::scheduler_binding;
use pidash_services::integrations::serializers::isoformat;
use pidash_services::runner_runs::chat::{self, ChatEffect, NewEventInputs};
use pidash_services::runner_runs::finalization::{self, TerminalEffectsInputs};
use pidash_services::runner_runs::lifecycle::{self, LifecycleError, LiveStateUsageFacts};
use pidash_services::runner_runs::scheduler_hook::{self, BindingFacts, SchedulerHookPlan};
use pidash_services::runner_runs::{LifecycleEffect, SetValue};
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::{
    classify_run_error, run_message_dedupe_ttl_secs, AgentChatMessageRole, AgentChatMessageStatus,
    AgentChatSessionStatus, AgentRunStatus, DevMachineInfo, RunErrorKind, RunnerInfo,
    TERMINAL_RUN_STATUSES,
};

use crate::queue::{enqueue, JobRow, NewJob};
use crate::tasks_ticker::scan::FIRE_TICK_TASK;
use crate::worker::{HandlerError, Registry, Verdict};

// ---------------------------------------------------------------------------
// Task names
// ---------------------------------------------------------------------------

/// `runner.sweep_run_message_dedupe` (`tasks.py:205`).
pub const SWEEP_RUN_MESSAGE_DEDUPE_TASK: &str = "runner.sweep_run_message_dedupe";
/// `runner.sweep_chat_message_dedupe` (`tasks.py:216`).
pub const SWEEP_CHAT_MESSAGE_DEDUPE_TASK: &str = "runner.sweep_chat_message_dedupe";
/// `runner.sweep_agent_chat_state` (`tasks.py:227`).
pub const SWEEP_AGENT_CHAT_STATE_TASK: &str = "runner.sweep_agent_chat_state";
/// `runner.reconcile_stalled_runs` (`tasks.py:246`).
pub const RECONCILE_STALLED_RUNS_TASK: &str = "runner.reconcile_stalled_runs";
/// `runner.reconcile_agent_run_terminal_effects` (`tasks.py:340`).
pub const RECONCILE_TERMINAL_EFFECTS_TASK: &str = "runner.reconcile_agent_run_terminal_effects";

/// Re-export the L4 wire name for the single apply
/// (`runner.apply_agent_run_terminal_effects`, `tasks.py:333`).
pub use finalization::TERMINAL_EFFECTS_TASK;

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/// `RUNNER_AGENT_STALL_THRESHOLD_SECS` default (`tasks.py:268`).
pub const STALL_THRESHOLD_SECS_DEFAULT: i64 = 360;
/// `RUNNER_AGENT_OBSERVABILITY_STALE_SECS` default (`tasks.py:269`).
pub const OBSERVABILITY_STALE_SECS_DEFAULT: i64 = 90;

/// Resolve a seconds setting: the env var under Django's setting name,
/// or `default` when unset or unparsable (the F-09 `env_secs`
/// precedent). No clamping — `0`/negative pass through like `int()`.
pub fn setting_secs(name: &str, default: i64) -> i64 {
    parse_setting_secs(std::env::var(name).ok().as_deref(), default)
}

/// The pure half of [`setting_secs`], unit-tested without env access.
pub fn parse_setting_secs(raw: Option<&str>, default: i64) -> i64 {
    raw.and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(default)
}

/// `RUN_MESSAGE_DEDUPE_TTL_SECS` (`tasks.py:208,219`): shared by both
/// dedupe sweeps, via the L1 resolver.
pub fn dedupe_ttl_secs() -> i64 {
    parse_dedupe_ttl(std::env::var("RUN_MESSAGE_DEDUPE_TTL_SECS").ok().as_deref())
}

/// The pure half of [`dedupe_ttl_secs`], unit-tested without env access.
pub fn parse_dedupe_ttl(raw: Option<&str>) -> i64 {
    run_message_dedupe_ttl_secs(raw.and_then(|value| value.parse::<i64>().ok()))
}

/// `RUNNER_AGENT_STALL_THRESHOLD_SECS` (`tasks.py:268`).
pub fn stall_threshold_secs() -> i64 {
    setting_secs(
        "RUNNER_AGENT_STALL_THRESHOLD_SECS",
        STALL_THRESHOLD_SECS_DEFAULT,
    )
}

/// `RUNNER_AGENT_OBSERVABILITY_STALE_SECS` (`tasks.py:269`).
pub fn observability_stale_secs() -> i64 {
    setting_secs(
        "RUNNER_AGENT_OBSERVABILITY_STALE_SECS",
        OBSERVABILITY_STALE_SECS_DEFAULT,
    )
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure a sweeps executor reports. Handlers render it as the
/// job's `last_error` (a retry) or park text.
#[derive(Debug)]
pub enum SweepError {
    /// A database failure (`sqlx::Error`, incl. a missing
    /// capacity-re-read row — Django's `DoesNotExist`).
    Db(sqlx::Error),
    /// A row the planner cannot consume (unknown enum text).
    BadRow(&'static str),
    /// The terminal-update planner rejected the inputs (unreachable
    /// for the stall path: `FAILED` with a string detail).
    Lifecycle(LifecycleError),
    /// A seam call failed where the source propagates (capacity
    /// dispatch/drains, the pending-entry enqueue).
    Seam(String),
}

impl std::fmt::Display for SweepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SweepError::Db(error) => write!(f, "database error: {error}"),
            SweepError::BadRow(what) => write!(f, "unparseable row value: {what}"),
            SweepError::Lifecycle(error) => write!(f, "terminal plan failed: {error}"),
            SweepError::Seam(error) => write!(f, "seam call failed: {error}"),
        }
    }
}

impl std::error::Error for SweepError {}

impl From<sqlx::Error> for SweepError {
    fn from(error: sqlx::Error) -> Self {
        SweepError::Db(error)
    }
}

impl From<LifecycleError> for SweepError {
    fn from(error: LifecycleError) -> Self {
        SweepError::Lifecycle(error)
    }
}

// ---------------------------------------------------------------------------
// Seams
// ---------------------------------------------------------------------------

/// A seam failure. Plain text, like [`HandlerError`].
pub type SeamError = String;

/// Cross-plane calls the sweeps need, one method per Python call
/// (`FireTickSeam` precedent: `Pin<Box<dyn Future>>` so the trait stays
/// object-safe). Isolation matches the source: drains after a chat
/// release share one guard, orchestration hooks are each isolated, the
/// capacity dispatch/drains and the pending-entry enqueue propagate.
pub trait RunnerRunsSeams: Send + Sync {
    /// `matcher.drain_for_runner_by_id` (D-14).
    fn drain_runner(
        &self,
        runner_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>>;
    /// `matcher.drain_pod_by_id` (D-14).
    fn drain_pod(
        &self,
        pod_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>>;
    /// `cloud_agent.dispatch.dispatch_waiting` (D-11). Unisolated: a
    /// failure propagates and the capacity marker stays unset.
    fn dispatch_waiting(
        &self,
        workspace_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>>;
    /// `orchestration.service.complete_project_move_handoff` (D-12):
    /// after the hooks transaction, isolated.
    fn complete_project_move_handoff(
        &self,
        run_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>>;
    /// `orchestration.scheduling.maybe_disarm_on_terminal_signal`
    /// (D-12), isolated.
    fn disarm_on_terminal_signal(
        &self,
        run_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>>;
    /// `orchestration.scheduling.maybe_apply_deferred_pause` (D-12),
    /// isolated.
    fn apply_deferred_pause(
        &self,
        run_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>>;
    /// `orchestration.workpad.get_agent_system_user` (D-12): the
    /// failure-comment actor. A failure rolls back the comment
    /// savepoint, like any other comment SQL failure.
    fn agent_system_user(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Uuid, SeamError>> + Send + '_>>;
    /// Redis `PUBLISH <channel> <event JSON>` for one chat event. A
    /// missing client is a silent no-op; a publish failure is
    /// swallowed and logged by the caller, never raised
    /// (`chat.py:140-147`).
    fn publish_chat_event(
        &self,
        channel: String,
        payload: String,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>>;
}

// ---------------------------------------------------------------------------
// SQL builders owned by this layer
// ---------------------------------------------------------------------------

/// `"table"."a", "table"."b", …` for a column list.
fn qualified_columns(table: &str, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|column| format!("\"{table}\".\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Position of `column` in `columns` (`None` when the L2 list drifted).
fn col_index(columns: &[&str], column: &str) -> Option<usize> {
    columns.iter().position(|name| *name == column)
}

/// The dedupe `DELETE` (`tasks.py:210,221`): Django's collector
/// fast-deletes these leaf tables (nothing references them — the FKs
/// point from the dedupe rows at runs/sessions — and no delete
/// signals are registered), so the source emits exactly this one
/// `DELETE`. Param: `$1` the TTL cutoff.
pub fn dedupe_delete_sql(table: &str) -> String {
    format!("DELETE FROM \"{table}\" WHERE \"{table}\".\"created_at\" < $1")
}

/// The TTL cutoff both dedupe sweeps delete before
/// (`timezone.now() - timedelta(seconds=ttl)`, `:209,220`).
pub fn dedupe_cutoff(now: DateTime<Utc>, ttl_secs: i64) -> DateTime<Utc> {
    now - chrono::Duration::seconds(ttl_secs)
}

/// The stall cutoffs (`tasks.py:270-272`): the agent-silence cutoff
/// first, the snapshot-freshness cutoff second.
pub fn stall_cutoffs(
    now: DateTime<Utc>,
    threshold_secs: i64,
    freshness_secs: i64,
) -> (DateTime<Utc>, DateTime<Utc>) {
    (
        now - chrono::Duration::seconds(threshold_secs),
        now - chrono::Duration::seconds(freshness_secs),
    )
}

/// The stall candidate select (`tasks.py:292-301`): full `agent_run` +
/// full `runner` (`select_related("runner")`) over the two `INNER
/// JOIN`s. The `WHERE` order is Django's: the `status__in` filter,
/// then the three live-state conjuncts in Q-sorted (alphabetical)
/// order — `last_event_at`, `observed_run_id`, `updated_at` — then the
/// `approvals_pending` OR. Params: `$1`/`$2` the active statuses
/// (`assigned`, `running`), `$3` the silence cutoff, `$4` the snapshot
/// cutoff, `$5` zero. `NULL` `last_event_at` rows never match (`__lt`
/// excludes `NULL`); `NULL` `approvals_pending` rows do (the `IS NULL`
/// arm). Fixture: FX-RUN-07 `candidate_sql`.
pub fn stalled_candidate_sql() -> String {
    format!(
        "SELECT {}, {} FROM \"agent_run\" \
         INNER JOIN \"runner\" ON (\"agent_run\".\"runner_id\" = \"runner\".\"id\") \
         INNER JOIN \"runner_live_state\" ON (\"runner\".\"id\" = \"runner_live_state\".\"runner_id\") \
         WHERE (\"agent_run\".\"status\" IN ($1, $2) \
         AND \"runner_live_state\".\"last_event_at\" < $3 \
         AND \"runner_live_state\".\"observed_run_id\" = (\"agent_run\".\"id\") \
         AND \"runner_live_state\".\"updated_at\" >= $4 \
         AND (\"runner_live_state\".\"approvals_pending\" = $5 \
         OR \"runner_live_state\".\"approvals_pending\" IS NULL)) \
         ORDER BY \"agent_run\".\"created_at\" DESC",
        qualified_columns(agent_run::TABLE, agent_run::COLUMNS),
        qualified_columns(runner_cols::TABLE, runner_cols::COLUMNS),
    )
}

/// The terminal-effects batch select (`tasks.py:345-350`): terminal
/// runs missing at least one marker, oldest `ended_at` first, capped
/// at 100. Params: `$1..$5` the terminal values in L1 tuple order.
/// Fixture: FX-RUN-07 `reconcile_terminal_effects.sql`.
pub fn reconcile_terminal_candidate_sql() -> String {
    "SELECT \"agent_run\".\"id\" FROM \"agent_run\" \
     WHERE (\"agent_run\".\"status\" IN ($1, $2, $3, $4, $5) \
     AND (\"agent_run\".\"terminal_hooks_applied_at\" IS NULL \
     OR \"agent_run\".\"terminal_capacity_released_at\" IS NULL)) \
     ORDER BY \"agent_run\".\"ended_at\" ASC LIMIT 100"
        .to_string()
}

/// A dev-machine label read for error enrichment (`tasks.py` never
/// issues it directly — it mirrors Django's lazy FK fetch when
/// `enrich_run_error` reaches `_runner_location`, i.e. for
/// auth-kind details only). Full row, like the lazy fetch. Param:
/// `$1` the machine id.
fn dev_machine_by_id_sql() -> String {
    format!(
        "SELECT {} FROM \"dev_machine\" WHERE \"dev_machine\".\"id\" = $1 LIMIT 1",
        qualified_columns(dev_machine::TABLE, dev_machine::COLUMNS)
    )
}

// ---------------------------------------------------------------------------
// Row mapping (L2 structs have no `FromRow`; joins need index mapping)
// ---------------------------------------------------------------------------

/// `AgentRun` id + the runner enrichment columns off a stall
/// candidate row (`agent_run` 41 cols, then `runner` cols).
struct StallCandidate {
    run_id: Uuid,
    runner_id: Option<Uuid>,
    runner_name: String,
    runner_host_label: String,
    runner_capabilities: Value,
    runner_dev_machine_id: Option<Uuid>,
}

fn map_stall_candidate(row: &sqlx::postgres::PgRow) -> Result<StallCandidate, SweepError> {
    let base = agent_run::COLUMNS.len();
    let at = |column: &str| -> Result<usize, SweepError> {
        col_index(runner_cols::COLUMNS, column)
            .map(|index| base + index)
            .ok_or(SweepError::BadRow("runner column"))
    };
    Ok(StallCandidate {
        run_id: row.try_get(0).map_err(SweepError::Db)?,
        runner_id: row.try_get(at("id")?).map_err(SweepError::Db)?,
        runner_name: row.try_get(at("name")?).map_err(SweepError::Db)?,
        runner_host_label: row.try_get(at("host_label")?).map_err(SweepError::Db)?,
        runner_capabilities: row.try_get(at("capabilities")?).map_err(SweepError::Db)?,
        runner_dev_machine_id: row.try_get(at("dev_machine_id")?).map_err(SweepError::Db)?,
    })
}

/// The locked finalize row's decision columns (`agent_run` only).
struct FinalizeLocked {
    executor_kind: AgentExecutorKind,
    done_payload: Option<Value>,
}

fn map_finalize_locked(row: &sqlx::postgres::PgRow) -> Result<FinalizeLocked, SweepError> {
    let at = |column: &str| -> Result<usize, SweepError> {
        col_index(agent_run::COLUMNS, column).ok_or(SweepError::BadRow("agent_run column"))
    };
    let executor_text: String = row.try_get(at("executor_kind")?).map_err(SweepError::Db)?;
    Ok(FinalizeLocked {
        executor_kind: AgentExecutorKind::from_value(&executor_text)
            .ok_or(SweepError::BadRow("executor_kind"))?,
        done_payload: row.try_get(at("done_payload")?).map_err(SweepError::Db)?,
    })
}

/// The usage/model snapshot for `_usage_updates`
/// (`live_state_by_runner_sql` column order, `lifecycle.rs:73-85`:
/// `usage` 9th, `llm_model` 10th — verified against the SQL text in
/// `live_state_facts_index_matches_planner_sql`).
fn map_live_state_facts(row: &sqlx::postgres::PgRow) -> Result<LiveStateUsageFacts, SweepError> {
    Ok(LiveStateUsageFacts {
        usage: row.try_get(8).map_err(SweepError::Db)?,
        llm_model: row.try_get(9).map_err(SweepError::Db)?,
    })
}

/// A full chat session row (`session_lock_open_sql` column order).
fn map_chat_session(row: &sqlx::postgres::PgRow) -> Result<AgentChatSession, SweepError> {
    let at = |column: &str| -> Result<usize, SweepError> {
        col_index(chat_session::COLUMNS, column).ok_or(SweepError::BadRow("chat session column"))
    };
    let status_text: String = row.try_get(at("status")?).map_err(SweepError::Db)?;
    Ok(AgentChatSession {
        id: row.try_get(at("id")?).map_err(SweepError::Db)?,
        workspace_id: row.try_get(at("workspace_id")?).map_err(SweepError::Db)?,
        runner_id: row.try_get(at("runner_id")?).map_err(SweepError::Db)?,
        created_by_id: row.try_get(at("created_by_id")?).map_err(SweepError::Db)?,
        pod_id: row.try_get(at("pod_id")?).map_err(SweepError::Db)?,
        status: AgentChatSessionStatus::from_value(&status_text)
            .ok_or(SweepError::BadRow("chat session status"))?,
        agent_kind: row.try_get(at("agent_kind")?).map_err(SweepError::Db)?,
        local_thread_id: row
            .try_get(at("local_thread_id")?)
            .map_err(SweepError::Db)?,
        local_session_id: row
            .try_get(at("local_session_id")?)
            .map_err(SweepError::Db)?,
        cwd: row.try_get(at("cwd")?).map_err(SweepError::Db)?,
        model: row.try_get(at("model")?).map_err(SweepError::Db)?,
        active_turn_id: row.try_get(at("active_turn_id")?).map_err(SweepError::Db)?,
        active_message_id: row
            .try_get(at("active_message_id")?)
            .map_err(SweepError::Db)?,
        close_requested: row
            .try_get(at("close_requested")?)
            .map_err(SweepError::Db)?,
        last_message_at: row
            .try_get(at("last_message_at")?)
            .map_err(SweepError::Db)?,
        closed_at: row.try_get(at("closed_at")?).map_err(SweepError::Db)?,
        error: row.try_get(at("error")?).map_err(SweepError::Db)?,
        created_at: row.try_get(at("created_at")?).map_err(SweepError::Db)?,
        updated_at: row.try_get(at("updated_at")?).map_err(SweepError::Db)?,
    })
}

/// A full chat message row (explicit-lookup / assistant-probe order).
fn map_chat_message(row: &sqlx::postgres::PgRow) -> Result<AgentChatMessage, SweepError> {
    let at = |column: &str| -> Result<usize, SweepError> {
        col_index(chat_message::COLUMNS, column).ok_or(SweepError::BadRow("chat message column"))
    };
    let role_text: String = row.try_get(at("role")?).map_err(SweepError::Db)?;
    let status_text: String = row.try_get(at("status")?).map_err(SweepError::Db)?;
    Ok(AgentChatMessage {
        id: row.try_get(at("id")?).map_err(SweepError::Db)?,
        session_id: row.try_get(at("session_id")?).map_err(SweepError::Db)?,
        role: AgentChatMessageRole::from_value(&role_text)
            .ok_or(SweepError::BadRow("chat message role"))?,
        content: row.try_get(at("content")?).map_err(SweepError::Db)?,
        content_parts: row.try_get(at("content_parts")?).map_err(SweepError::Db)?,
        status: AgentChatMessageStatus::from_value(&status_text)
            .ok_or(SweepError::BadRow("chat message status"))?,
        local_item_id: row.try_get(at("local_item_id")?).map_err(SweepError::Db)?,
        local_turn_id: row.try_get(at("local_turn_id")?).map_err(SweepError::Db)?,
        seq: row.try_get(at("seq")?).map_err(SweepError::Db)?,
        created_at: row.try_get(at("created_at")?).map_err(SweepError::Db)?,
        completed_at: row.try_get(at("completed_at")?).map_err(SweepError::Db)?,
    })
}

/// The event re-read columns `_publish_event_by_id` needs
/// (`chat_event` column order).
struct ChatEventWire {
    id: i64,
    session_id: Uuid,
    message_id: Option<Uuid>,
    seq: i32,
    kind: String,
    payload: Value,
    created_at: DateTime<Utc>,
}

fn map_chat_event_wire(row: &sqlx::postgres::PgRow) -> Result<ChatEventWire, SweepError> {
    let at = |column: &str| -> Result<usize, SweepError> {
        col_index(chat_event::COLUMNS, column).ok_or(SweepError::BadRow("chat event column"))
    };
    Ok(ChatEventWire {
        id: row.try_get(at("id")?).map_err(SweepError::Db)?,
        session_id: row.try_get(at("session_id")?).map_err(SweepError::Db)?,
        message_id: row.try_get(at("message_id")?).map_err(SweepError::Db)?,
        seq: row.try_get(at("seq")?).map_err(SweepError::Db)?,
        kind: row.try_get(at("kind")?).map_err(SweepError::Db)?,
        payload: row.try_get(at("payload")?).map_err(SweepError::Db)?,
        created_at: row.try_get(at("created_at")?).map_err(SweepError::Db)?,
    })
}

/// The locked effects row's hook facts (`lock_run_for_effects_sql`:
/// `agent_run` cols first, then issues/projects/states, then the
/// scheduler binding last). Binding columns ride at the tail, so only
/// the binding length (a public L2 const) is needed — never the
/// middle-join widths.
struct EffectsLocked {
    status: AgentRunStatus,
    run_config: Value,
    error: String,
    refusal_category: String,
    scheduler_binding_id: Option<Uuid>,
    hooks_applied: bool,
    binding: Option<BindingFacts>,
}

fn map_effects_locked(row: &sqlx::postgres::PgRow) -> Result<EffectsLocked, SweepError> {
    let at = |column: &str| -> Result<usize, SweepError> {
        col_index(agent_run::COLUMNS, column).ok_or(SweepError::BadRow("agent_run column"))
    };
    let status_text: String = row.try_get(at("status")?).map_err(SweepError::Db)?;
    let hooks_at: Option<DateTime<Utc>> = row
        .try_get(at("terminal_hooks_applied_at")?)
        .map_err(SweepError::Db)?;
    let binding_base = row
        .len()
        .checked_sub(scheduler_binding::COLUMNS.len())
        .ok_or(SweepError::BadRow("effects row width"))?;
    let binding_at = |column: &str| -> Result<usize, SweepError> {
        col_index(scheduler_binding::COLUMNS, column)
            .map(|index| binding_base + index)
            .ok_or(SweepError::BadRow("scheduler binding column"))
    };
    let binding_id: Option<Uuid> = row.try_get(binding_at("id")?).map_err(SweepError::Db)?;
    let binding = match binding_id {
        Some(id) => Some(BindingFacts {
            id,
            last_error: row
                .try_get(binding_at("last_error")?)
                .map_err(SweepError::Db)?,
        }),
        None => None,
    };
    Ok(EffectsLocked {
        status: AgentRunStatus::from_value(&status_text).ok_or(SweepError::BadRow("status"))?,
        run_config: row.try_get(at("run_config")?).map_err(SweepError::Db)?,
        error: row.try_get(at("error")?).map_err(SweepError::Db)?,
        refusal_category: row
            .try_get(at("refusal_category")?)
            .map_err(SweepError::Db)?,
        scheduler_binding_id: row
            .try_get(at("scheduler_binding_id")?)
            .map_err(SweepError::Db)?,
        hooks_applied: hooks_at.is_some(),
        binding,
    })
}

/// The capacity re-read facts (`select_run_for_capacity_sql`: only
/// `agent_run` columns are consumed).
struct CapacityFacts {
    executor_kind: AgentExecutorKind,
    runner_id: Option<Uuid>,
    pod_id: Option<Uuid>,
    workspace_id: Uuid,
    capacity_released: bool,
}

fn map_capacity_facts(row: &sqlx::postgres::PgRow) -> Result<CapacityFacts, SweepError> {
    let at = |column: &str| -> Result<usize, SweepError> {
        col_index(agent_run::COLUMNS, column).ok_or(SweepError::BadRow("agent_run column"))
    };
    let executor_text: String = row.try_get(at("executor_kind")?).map_err(SweepError::Db)?;
    let released_at: Option<DateTime<Utc>> = row
        .try_get(at("terminal_capacity_released_at")?)
        .map_err(SweepError::Db)?;
    Ok(CapacityFacts {
        executor_kind: AgentExecutorKind::from_value(&executor_text)
            .ok_or(SweepError::BadRow("executor_kind"))?,
        runner_id: row.try_get(at("runner_id")?).map_err(SweepError::Db)?,
        pod_id: row.try_get(at("pod_id")?).map_err(SweepError::Db)?,
        workspace_id: row.try_get(at("workspace_id")?).map_err(SweepError::Db)?,
        capacity_released: released_at.is_some(),
    })
}

// ---------------------------------------------------------------------------
// Dedupe sweeps (`tasks.py:205-224`)
// ---------------------------------------------------------------------------

/// Delete rows of one dedupe table older than the TTL cutoff.
/// Returns the deleted count.
async fn sweep_dedupe_table(
    pool: &PgPool,
    table: &str,
    now: &DateTime<Utc>,
    ttl_secs: i64,
) -> Result<u64, SweepError> {
    let cutoff = dedupe_cutoff(*now, ttl_secs);
    let done = sqlx::query(&dedupe_delete_sql(table))
        .bind(cutoff)
        .execute(pool)
        .await?;
    Ok(done.rows_affected())
}

/// `runner.sweep_run_message_dedupe` (`tasks.py:205-213`).
pub async fn sweep_run_message_dedupe(
    pool: &PgPool,
    now: &DateTime<Utc>,
    ttl_secs: i64,
) -> Result<u64, SweepError> {
    let deleted = sweep_dedupe_table(pool, run_dedupe::TABLE, now, ttl_secs).await?;
    if deleted > 0 {
        tracing::info!("sweep_run_message_dedupe deleted {deleted} row(s)");
    }
    Ok(deleted)
}

/// `runner.sweep_chat_message_dedupe` (`tasks.py:216-224`): same TTL
/// key as the run sweep, ported as-is.
pub async fn sweep_chat_message_dedupe(
    pool: &PgPool,
    now: &DateTime<Utc>,
    ttl_secs: i64,
) -> Result<u64, SweepError> {
    let deleted = sweep_dedupe_table(pool, chat_dedupe::TABLE, now, ttl_secs).await?;
    if deleted > 0 {
        tracing::info!("sweep_chat_message_dedupe deleted {deleted} row(s)");
    }
    Ok(deleted)
}

// ---------------------------------------------------------------------------
// Chat-state sweep (`tasks.py:227-244`)
// ---------------------------------------------------------------------------

/// `runner.sweep_agent_chat_state`: stale active turns plus empty
/// sessions (`chat.py` via L5).
pub async fn sweep_agent_chat_state(
    pool: &PgPool,
    seams: &dyn RunnerRunsSeams,
    now: &DateTime<Utc>,
) -> Result<u64, SweepError> {
    Ok(sweep_active_turns(pool, seams, now).await? + sweep_empty_sessions(pool, now).await?)
}

/// `sweep_active_turns` (`chat.py:403-448`): fail every stale OPEN
/// session's active turn. One transaction per row; a lock miss skips
/// the row uncounted. Post-commit, in registration order: one event
/// publish per inserted event, then the drain effect.
async fn sweep_active_turns(
    pool: &PgPool,
    seams: &dyn RunnerRunsSeams,
    now: &DateTime<Utc>,
) -> Result<u64, SweepError> {
    let cutoff = chat::sweep_active_cutoff(*now);
    let ids: Vec<Uuid> = sqlx::query_scalar(&chat::sweep_active_ids_sql(cutoff))
        .fetch_all(pool)
        .await?;
    let mut count = 0u64;
    for session_id in ids {
        if sweep_active_row(pool, seams, &session_id).await? {
            count += 1;
        }
    }
    Ok(count)
}

/// One `sweep_active_turns` row (`chat.py:411-447`). Returns whether
/// the row was swept (a lock miss returns `false`).
async fn sweep_active_row(
    pool: &PgPool,
    seams: &dyn RunnerRunsSeams,
    session_id: &Uuid,
) -> Result<bool, SweepError> {
    let mut tx = pool.begin().await?;
    let locked = sqlx::query(&chat::session_lock_open_sql(*session_id))
        .fetch_optional(&mut *tx)
        .await?;
    let Some(locked) = locked else {
        tx.commit().await?;
        return Ok(false);
    };
    let session = map_chat_session(&locked)?;
    // The explicit finalize target (`chat.py:343`): the session's
    // active message (the sweep passes no override).
    let target = chat::finalize_target_message_id(None, session.active_message_id);
    let explicit = match target {
        Some(message_id) => {
            sqlx::query(&chat::finalize_explicit_lookup_sql(session.id, message_id))
                .fetch_optional(&mut *tx)
                .await?
                .map(|row| map_chat_message(&row))
                .transpose()?
        }
        None => None,
    };
    // The streaming assistant (`chat.py:322-332`): the turn-scoped
    // probe only runs when the session has an active turn; the
    // fallback runs exactly when the scoped probe missed or never ran
    // ([`chat::select_active_assistant`] laziness).
    let scoped = if session.active_turn_id.is_empty() {
        None
    } else {
        sqlx::query(&chat::assistant_turn_scoped_sql(
            session.id,
            &session.active_turn_id,
        ))
        .fetch_optional(&mut *tx)
        .await?
        .map(|row| map_chat_message(&row))
        .transpose()?
    };
    let fallback = if scoped.is_none() {
        sqlx::query(&chat::assistant_fallback_sql(session.id))
            .fetch_optional(&mut *tx)
            .await?
            .map(|row| map_chat_message(&row))
            .transpose()?
    } else {
        None
    };
    let assistant = chat::select_active_assistant(&session.active_turn_id, scoped.as_ref(), || {
        fallback.as_ref()
    });
    let mut clock = Utc::now;
    let plan = chat::plan_sweep_active_row(&session, explicit.as_ref(), assistant, &mut clock);
    for update in &plan.finalize.updates {
        sqlx::query(&chat::message_status_update_sql(update))
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(&chat::fail_session_update_sql(
        session.id,
        &plan,
        Utc::now(),
    ))
    .execute(&mut *tx)
    .await?;
    let mut event_ids = Vec::with_capacity(plan.events.len());
    for event in &plan.events {
        if append_event_is_duplicate(&mut tx, &session.id, event).await? {
            continue;
        }
        event_ids.push(insert_chat_event(&mut tx, &session.id, event).await?);
    }
    tx.commit().await?;
    for event_id in event_ids {
        publish_chat_event_by_id(pool, seams, event_id).await?;
    }
    if plan.queue_drain {
        run_sweep_drain(seams, &session).await;
    }
    Ok(true)
}

/// The `append_event_locked` dedupe probe (`chat.py:158-161`).
/// Sweep rows always carry an empty `source_key`, so this never
/// probes — the planner guard is kept so the call shape stays the
/// `append_event_locked` shape.
async fn append_event_is_duplicate(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: &Uuid,
    event: &NewEventInputs,
) -> Result<bool, SweepError> {
    if !chat::append_checks_dedupe(&event.source_key) {
        return Ok(false);
    }
    let hit = sqlx::query(&chat::append_dedupe_check_sql(
        *session_id,
        &event.source_key,
    ))
    .fetch_optional(&mut **tx)
    .await?;
    Ok(hit.is_some())
}

/// The `append_event_locked` insert (`chat.py:162-169`): `MAX(seq)+1`
/// under the session row lock, `created_at` from `auto_now_add`.
/// Returns the new event id for the post-commit publish.
async fn insert_chat_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: &Uuid,
    event: &NewEventInputs,
) -> Result<i64, SweepError> {
    let max: Option<i32> = sqlx::query_scalar(&chat::next_event_seq_sql(*session_id))
        .fetch_one(&mut **tx)
        .await?;
    let seq = chat::next_seq_after_max(max);
    let event_id: i64 =
        sqlx::query_scalar(&chat::insert_event_sql(*session_id, seq, event, Utc::now()))
            .fetch_one(&mut **tx)
            .await?;
    Ok(event_id)
}

/// `_publish_event_by_id` (`chat.py:174-177`): re-read the event row
/// (a miss publishes nothing) and publish the serialized frame. A
/// lookup database failure propagates — only the Redis half is
/// swallowed (`chat.py:144-147`).
async fn publish_chat_event_by_id(
    pool: &PgPool,
    seams: &dyn RunnerRunsSeams,
    event_id: i64,
) -> Result<(), SweepError> {
    let row = sqlx::query(&chat::publish_event_lookup_sql(event_id))
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let event = map_chat_event_wire(&row)?;
    let payload_json = chat::dumps_value(&event.payload);
    let session_id = event.session_id.to_string();
    let message_id = event.message_id.map(|id| id.to_string());
    let created_at = isoformat(&event.created_at);
    let parts = chat::EventParts {
        id: event.id,
        session_id: &session_id,
        message_id: message_id.as_deref(),
        seq: event.seq,
        kind: &event.kind,
        payload_json: &payload_json,
        created_at: &created_at,
    };
    let (channel, body) =
        chat::publish_frame(event.session_id, &chat::serialize_event_json(&parts));
    if let Err(error) = seams.publish_chat_event(channel, body).await {
        tracing::error!(
            %error,
            "publish chat event failed for session {}",
            event.session_id
        );
    }
    Ok(())
}

/// The sweep row's drain effect (`chat.py:445-446`): both matcher
/// drains inside ONE swallow-and-log guard, so a runner-drain failure
/// skips the pod drain (`chat.py:98-107`).
async fn run_sweep_drain(seams: &dyn RunnerRunsSeams, session: &AgentChatSession) {
    let mut effect = None;
    chat::drain_tasks_after_chat_release(
        Some(session.runner_id),
        Some(session.pod_id),
        &mut |drain| effect = Some(drain),
    );
    if let Some(ChatEffect::DrainTasks { runner_id, pod_id }) = effect {
        let drained = async {
            seams.drain_runner(runner_id).await?;
            if let Some(pod_id) = pod_id {
                seams.drain_pod(pod_id).await?;
            }
            Ok::<(), SeamError>(())
        }
        .await;
        if let Err(error) = drained {
            tracing::error!(
                %error,
                "failed to drain task queue after chat release for runner {runner_id}"
            );
        }
    }
}

/// `sweep_empty_sessions` (`chat.py:483-497`): close OPEN sessions
/// older than 24h with zero messages, in one `UPDATE`. No ids, no
/// write (`chat.py:492-493`).
async fn sweep_empty_sessions(pool: &PgPool, now: &DateTime<Utc>) -> Result<u64, SweepError> {
    let cutoff = chat::sweep_empty_cutoff(*now);
    let ids: Vec<Uuid> = sqlx::query_scalar(&chat::sweep_empty_ids_sql(cutoff))
        .fetch_all(pool)
        .await?;
    if ids.is_empty() {
        return Ok(0);
    }
    let done = sqlx::query(&chat::sweep_empty_close_sql(&ids, Utc::now()))
        .execute(pool)
        .await?;
    Ok(done.rows_affected())
}

// ---------------------------------------------------------------------------
// Stall watchdog (`tasks.py:246-330`)
// ---------------------------------------------------------------------------

/// The stall detail (`tasks.py:316`).
pub fn stall_error_detail(threshold_secs: i64) -> String {
    format!("agent stalled: no events for >{threshold_secs}s")
}

/// `runner.reconcile_stalled_runs`: reap silent `ASSIGNED`/`RUNNING`
/// runs as `FAILED` via L4. Per-row guard: a failed reap logs and the
/// sweep continues (`tasks.py:311-323`); a late (already-closed) row
/// still counts as reaped (`:318` runs unconditionally).
pub async fn reconcile_stalled_runs(
    pool: &PgPool,
    seams: &dyn RunnerRunsSeams,
    now: &DateTime<Utc>,
    threshold_secs: i64,
    freshness_secs: i64,
) -> Result<u64, SweepError> {
    let (cutoff, snapshot_cutoff) = stall_cutoffs(*now, threshold_secs, freshness_secs);
    let rows = sqlx::query(&stalled_candidate_sql())
        .bind(AgentRunStatus::Assigned.value())
        .bind(AgentRunStatus::Running.value())
        .bind(cutoff)
        .bind(snapshot_cutoff)
        .bind(0i32)
        .fetch_all(pool)
        .await?;
    let mut reaped = 0u64;
    for row in &rows {
        // The id is for the poison log line only; the row is mapped
        // (and validated) inside the guarded reap.
        let run_id: Uuid = row.try_get(0).unwrap_or(Uuid::nil());
        match reap_stalled_row(pool, seams, row, threshold_secs).await {
            Ok(true) => reaped += 1,
            Ok(false) => {}
            Err(error) => tracing::error!(
                %error,
                "reconcile_stalled_runs: failed to reap run {run_id}"
            ),
        }
    }
    if reaped > 0 {
        tracing::info!(
            "reconcile_stalled_runs reaped {reaped} stalled run(s) (threshold={threshold_secs}s)"
        );
    }
    Ok(reaped)
}

/// One stall row: `finalize_run_terminal(runner, id, FAILED,
/// error_detail=...)` (`run_lifecycle.py:402-463`). Returns whether
/// the row counts as reaped — `false` only for the (dead, `INNER
/// JOIN`-excluded) runnerless guard.
async fn reap_stalled_row(
    pool: &PgPool,
    seams: &dyn RunnerRunsSeams,
    row: &sqlx::postgres::PgRow,
    threshold_secs: i64,
) -> Result<bool, SweepError> {
    let candidate = map_stall_candidate(row)?;
    let Some(runner_id) = candidate.runner_id else {
        return Ok(false);
    };
    let detail = stall_error_detail(threshold_secs);
    // `_matching_live_state` (`run_lifecycle.py:73-77`).
    let live_row = sqlx::query(&lifecycle::live_state_by_runner_sql())
        .bind(candidate.run_id)
        .bind(runner_id)
        .fetch_optional(pool)
        .await?;
    let live = live_row.map(|row| map_live_state_facts(&row)).transpose()?;
    // `RunnerInfo` mirrors `enrich_run_error`'s evaluation order
    // (`diagnostics.py:149-160`): runner attributes are touched only
    // for auth-kind details, the dev-machine FK only inside
    // `_runner_location`. The stall detail never classifies as auth,
    // so the machine fetch below never fires on this path — it exists
    // so the evaluation order stays the source's.
    let needs_runner = matches!(
        classify_run_error(Some(&detail)),
        Some(diagnostic) if diagnostic.kind == RunErrorKind::AgentAuthentication
    );
    let machine_labels = if needs_runner {
        match candidate.runner_dev_machine_id {
            Some(machine_id) => Some(fetch_dev_machine_labels(pool, &machine_id).await?),
            None => None,
        }
    } else {
        None
    };
    let machine_info = machine_labels
        .as_ref()
        .map(|(label, host_label)| DevMachineInfo {
            label: Some(label.as_str()),
            host_label: Some(host_label.as_str()),
        });
    let runner_info = if needs_runner {
        Some(RunnerInfo {
            name: Some(candidate.runner_name.as_str()),
            host_label: Some(candidate.runner_host_label.as_str()),
            capabilities: Some(&candidate.runner_capabilities),
            dev_machine: machine_info,
        })
    } else {
        None
    };
    let error_detail = Value::String(detail);
    let null = Value::Null;
    let inputs = lifecycle::TerminalUpdateInputs {
        status: AgentRunStatus::Failed,
        done_payload: &null,
        error_detail: &error_detail,
        refusal_category: &null,
        tokens: &null,
        model: &null,
        live: live.as_ref(),
        runner: runner_info.as_ref(),
    };
    let mut values = lifecycle::plan_terminal_finalize(&inputs)?;
    run_finalize_transaction(pool, &candidate.run_id, &runner_id, &mut values).await?;
    // `_publish_effects` (`agent_run_finalization.py:89-103`): the
    // Celery emit first, then the inline apply — each isolated, in
    // order.
    for effect in finalization::plan_publish_effects(candidate.run_id) {
        match effect {
            LifecycleEffect::PublishTerminalEffects { run_id } => {
                if let Err(error) = enqueue(pool, &terminal_effects_job(&run_id)).await {
                    tracing::error!(%error, "failed to publish terminal effects for run {run_id}");
                }
            }
            LifecycleEffect::ApplyTerminalEffectsInline { run_id } => {
                if let Err(error) = apply_agent_run_terminal_effects(pool, seams, &run_id).await {
                    tracing::error!(%error, "failed to apply terminal effects for run {run_id}");
                }
            }
            // Unreachable: `plan_publish_effects` emits exactly these two.
            _ => {}
        }
    }
    Ok(true)
}

/// The dev-machine labels `_runner_location` reads
/// (`diagnostics.py:133-136`). A set-but-missing id raises, like
/// Django's lazy FK fetch (`DoesNotExist` → the per-row guard).
async fn fetch_dev_machine_labels(
    pool: &PgPool,
    machine_id: &Uuid,
) -> Result<(String, String), SweepError> {
    let row = sqlx::query(&dev_machine_by_id_sql())
        .bind(*machine_id)
        .fetch_optional(pool)
        .await?
        .ok_or(SweepError::Db(sqlx::Error::RowNotFound))?;
    let at = |column: &str| -> Result<usize, SweepError> {
        col_index(dev_machine::COLUMNS, column).ok_or(SweepError::BadRow("dev_machine column"))
    };
    Ok((
        row.try_get(at("label")?).map_err(SweepError::Db)?,
        row.try_get(at("host_label")?).map_err(SweepError::Db)?,
    ))
}

/// `finalize_agent_run` (`agent_run_finalization.py:47-87`): lock the
/// non-terminal row, merge the done payload when carried, write the
/// values, insert the cloud-only terminal event. A lock miss commits
/// the empty transaction and logs the late-transition line
/// (`run_lifecycle.py:458-462`).
async fn run_finalize_transaction(
    pool: &PgPool,
    run_id: &Uuid,
    runner_id: &Uuid,
    values: &mut finalization::FinalizeValues,
) -> Result<(), SweepError> {
    let mut tx = pool.begin().await?;
    let lock_sql = finalization::lock_run_for_finalize_sql(true, false);
    let mut lock = sqlx::query(&lock_sql).bind(*run_id);
    for status in TERMINAL_RUN_STATUSES {
        lock = lock.bind(status.value());
    }
    lock = lock.bind(*runner_id);
    let locked = lock.fetch_optional(&mut *tx).await?;
    let Some(locked) = locked else {
        tx.commit().await?;
        tracing::info!("run_lifecycle: ignoring late terminal transition for closed run {run_id}");
        return Ok(());
    };
    let facts = map_finalize_locked(&locked)?;
    finalization::apply_done_payload_merge(
        values,
        facts.done_payload.as_ref().unwrap_or(&null_json()),
    );
    execute_finalize_update(&mut tx, values, run_id).await?;
    if facts.executor_kind == AgentExecutorKind::CloudAgent {
        let exists = sqlx::query(finalization::terminal_event_exists_sql())
            .bind(*run_id)
            .bind("terminal")
            .fetch_optional(&mut *tx)
            .await?
            .is_some();
        if !exists {
            let max: Option<i32> = sqlx::query_scalar(finalization::terminal_event_max_seq_sql())
                .bind(*run_id)
                .fetch_optional(&mut *tx)
                .await?
                .flatten();
            let plan = finalization::plan_terminal_event(
                max,
                AgentRunStatus::Failed,
                &finalization::finalize_error_code(values),
            );
            sqlx::query(&finalization::terminal_event_insert_sql())
                .bind(*run_id)
                .bind(plan.seq)
                .bind("terminal")
                .bind(plan.payload)
                .bind(Utc::now())
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

/// A shared JSON null for merges over a missing stored payload.
fn null_json() -> Value {
    Value::Null
}

/// The finalize `UPDATE` (`:71`): binds in clause order, the id last.
/// `NULL` columns bind typed nulls (`queue_position` is `int2`, the
/// markers are `timestamptz`); `Now` binds one `now()` per clause.
async fn execute_finalize_update(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    values: &finalization::FinalizeValues,
    run_id: &Uuid,
) -> Result<(), SweepError> {
    let update_sql = finalization::finalize_update_sql(values);
    let mut query = sqlx::query(&update_sql);
    for clause in &values.clauses {
        match &clause.value {
            SetValue::Null => match clause.column {
                "queue_position" => query = query.bind(None::<i16>),
                "terminal_hooks_applied_at" | "terminal_capacity_released_at" => {
                    query = query.bind(None::<DateTime<Utc>>);
                }
                _ => return Err(SweepError::BadRow("null finalize column")),
            },
            SetValue::Now => query = query.bind(Utc::now()),
            SetValue::Text(text) => query = query.bind(text.clone()),
            SetValue::Json(value) => query = query.bind(value.clone()),
        }
    }
    query = query.bind(*run_id);
    query.execute(&mut **tx).await?;
    Ok(())
}

/// `.delay(str(run_id))` for the terminal-effects task, as a queue row
/// (`agent_run_finalization.py:93-96`).
pub fn terminal_effects_job(run_id: &Uuid) -> NewJob {
    NewJob::new(
        TERMINAL_EFFECTS_TASK,
        json!([run_id.to_string()]),
        json!({}),
    )
}

// ---------------------------------------------------------------------------
// Terminal effects (`tasks.py:333-353` + `agent_run_finalization.py:105-199`)
// ---------------------------------------------------------------------------

/// `runner.apply_agent_run_terminal_effects` (`tasks.py:333-337`):
/// hooks once, then capacity at-least-once. Returns `false` when the
/// locked fetch finds no terminal row. Lock order is issue → run
/// (`:115-131`); the handoff completes after the hooks commit
/// (`:169-181`); capacity reads the row fresh (`:183`) and its
/// dispatch/drains propagate — a failure leaves the marker unset for
/// the reconciler.
pub async fn apply_agent_run_terminal_effects(
    pool: &PgPool,
    seams: &dyn RunnerRunsSeams,
    run_id: &Uuid,
) -> Result<bool, SweepError> {
    let mut tx = pool.begin().await?;
    let work_item_id: Option<Uuid> =
        sqlx::query_scalar(finalization::select_run_work_item_id_sql())
            .bind(*run_id)
            .fetch_optional(&mut *tx)
            .await?
            .flatten();
    if let Some(work_item_id) = work_item_id {
        sqlx::query(&finalization::lock_issue_sql())
            .bind(work_item_id)
            .fetch_optional(&mut *tx)
            .await?;
    }
    let lock_sql = finalization::lock_run_for_effects_sql();
    let mut lock = sqlx::query(&lock_sql).bind(*run_id);
    for status in TERMINAL_RUN_STATUSES {
        lock = lock.bind(status.value());
    }
    let locked = lock.fetch_optional(&mut *tx).await?;
    let Some(locked) = locked else {
        tx.commit().await?;
        return Ok(false);
    };
    let facts = map_effects_locked(&locked)?;
    // The composed plan, first with the locked-row facts (`agent_run`
    // leads the locked row, so the capacity mapper reads here too).
    // Only the hooks branch is consumed: capacity is re-planned after
    // commit from the fresh re-read, with the same hooks inputs.
    let locked_cap = map_capacity_facts(&locked)?;
    let hooks_outcome = finalization::plan_terminal_effects(&TerminalEffectsInputs {
        run_id: *run_id,
        status: facts.status,
        run_config: &facts.run_config,
        error: &facts.error,
        refusal_category: &facts.refusal_category,
        scheduler_binding: facts.binding.clone(),
        hooks_applied: facts.hooks_applied,
        capacity_released: locked_cap.capacity_released,
        executor_kind: locked_cap.executor_kind,
        runner_id: locked_cap.runner_id,
        pod_id: locked_cap.pod_id,
        workspace_id: locked_cap.workspace_id,
        work_item_present: work_item_id.is_some(),
    });
    let hooks_plan = match hooks_outcome {
        finalization::TerminalEffectsOutcome::Applied(plan) => plan,
        finalization::TerminalEffectsOutcome::NotTerminal => {
            tx.commit().await?;
            return Ok(false);
        }
    };
    let mut pending_fire = None;
    if let Some(hooks) = &hooks_plan.hooks {
        run_hooks_phase(
            &mut tx,
            seams,
            run_id,
            &work_item_id,
            facts.scheduler_binding_id.is_some(),
            hooks,
            &mut pending_fire,
        )
        .await?;
        sqlx::query(finalization::update_hooks_marker_sql())
            .bind(Utc::now())
            .bind(*run_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    // Post-commit, in source order: the pending-entry fire (a `.delay`
    // failure propagates — the hooks marker is already set, so the
    // retry never re-registers it), then the isolated handoff.
    if let Some(ticker_id) = pending_fire {
        enqueue(
            pool,
            &NewJob::new(FIRE_TICK_TASK, json!([ticker_id.to_string()]), json!({})),
        )
        .await?;
    }
    if hooks_plan.handoff_after_txn {
        if let Err(error) = seams.complete_project_move_handoff(*run_id).await {
            tracing::error!(%error, "failed to complete project-move handoff for run {run_id}");
        }
    }
    // The capacity re-read (`.get()` — a missing row raises).
    let cap_row = sqlx::query(&finalization::select_run_for_capacity_sql())
        .bind(*run_id)
        .fetch_one(pool)
        .await?;
    let cap = map_capacity_facts(&cap_row)?;
    let capacity_outcome = finalization::plan_terminal_effects(&TerminalEffectsInputs {
        run_id: *run_id,
        status: facts.status,
        run_config: &facts.run_config,
        error: &facts.error,
        refusal_category: &facts.refusal_category,
        scheduler_binding: facts.binding.clone(),
        hooks_applied: facts.hooks_applied,
        capacity_released: cap.capacity_released,
        executor_kind: cap.executor_kind,
        runner_id: cap.runner_id,
        pod_id: cap.pod_id,
        workspace_id: cap.workspace_id,
        work_item_present: work_item_id.is_some(),
    });
    // `NotTerminal` is unreachable here: the status is the locked
    // row's, unchanged since the first plan. Skipping capacity is the
    // safe direction (the marker stays unset for the reconciler).
    let capacity = match capacity_outcome {
        finalization::TerminalEffectsOutcome::Applied(plan) => plan.capacity,
        finalization::TerminalEffectsOutcome::NotTerminal => None,
    };
    if let Some(plan) = capacity {
        run_capacity_plan(seams, &plan).await?;
        sqlx::query(finalization::update_capacity_marker_sql())
            .bind(Utc::now())
            .bind(*run_id)
            .execute(pool)
            .await?;
    }
    Ok(true)
}

/// The hooks branch (`:134-167`): savepointed failure comment and
/// scheduler hook, inline orchestration (each isolated), and the
/// pending-entry ticker lookup (stashed for the post-commit fire).
async fn run_hooks_phase(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    seams: &dyn RunnerRunsSeams,
    run_id: &Uuid,
    work_item_id: &Option<Uuid>,
    has_binding: bool,
    hooks: &finalization::HooksPlan,
    pending_fire: &mut Option<Uuid>,
) -> Result<(), SweepError> {
    if hooks.failure_comment.is_some() {
        run_failure_comment_in_savepoint(tx, seams, run_id, hooks).await?;
    }
    if hooks.orchestration {
        if let Err(error) = seams.disarm_on_terminal_signal(*run_id).await {
            tracing::error!(%error, "orchestration.error: run-ended reconcile failed for run {run_id}");
        }
        if let Err(error) = seams.apply_deferred_pause(*run_id).await {
            tracing::error!(%error, "orchestration.error: deferred-pause failed for run {run_id}");
        }
        match fire_pending_entry_lookup(tx, work_item_id).await {
            Ok(found) => *pending_fire = found,
            Err(error) => tracing::error!(
                %error,
                "orchestration.error: pending-entry fire failed for run {run_id}"
            ),
        }
    }
    // The savepoint opens iff the run names a binding
    // (`run.scheduler_binding_id`, `:157`) — even when the binding row
    // itself is gone, in which case the hook inside no-ops.
    if has_binding {
        run_scheduler_hook_in_savepoint(tx, run_id, hooks).await?;
    }
    Ok(())
}

/// The failure comment (`:136-147` + `_post_failure_comment:341-399`)
/// behind a savepoint: success releases, failure rolls back to the
/// savepoint (Django's `needs_rollback` path — a caught-inside error
/// never poisons the enclosing transaction) and logs.
async fn run_failure_comment_in_savepoint(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    seams: &dyn RunnerRunsSeams,
    run_id: &Uuid,
    hooks: &finalization::HooksPlan,
) -> Result<(), SweepError> {
    sqlx::query("SAVEPOINT sp_failure_comment")
        .execute(&mut **tx)
        .await?;
    match post_failure_comment(tx, seams, run_id, hooks).await {
        Ok(()) => {
            sqlx::query("RELEASE SAVEPOINT sp_failure_comment")
                .execute(&mut **tx)
                .await?;
            Ok(())
        }
        Err(error) => {
            tracing::error!(%error, "run_lifecycle: failed to post failure comment for run {run_id}");
            sqlx::query("ROLLBACK TO SAVEPOINT sp_failure_comment")
                .execute(&mut **tx)
                .await?;
            Ok(())
        }
    }
}

/// Post the failure comment: re-read the run (a missing row or work
/// item means silence), the speaker dedupe check (a hit means
/// silence), then the comment + description inserts plus the
/// link-back.
async fn post_failure_comment(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    seams: &dyn RunnerRunsSeams,
    run_id: &Uuid,
    hooks: &finalization::HooksPlan,
) -> Result<(), SweepError> {
    let Some(comment) = hooks.failure_comment.as_ref() else {
        return Ok(());
    };
    let reread = sqlx::query(&lifecycle::failure_reread_sql())
        .bind(*run_id)
        .fetch_optional(&mut **tx)
        .await?;
    let Some(reread) = reread else {
        return Ok(());
    };
    let work_item_id: Option<Uuid> = reread
        .try_get(
            col_index(agent_run::COLUMNS, "work_item_id").ok_or(SweepError::BadRow("work_item"))?,
        )
        .map_err(SweepError::Db)?;
    if work_item_id.is_none() {
        return Ok(());
    }
    let ids = map_failure_comment_ids(&reread)?;
    let duplicate = sqlx::query(lifecycle::comment_dedupe_exists_sql())
        .bind(ids.issue_id)
        .bind(*run_id)
        .bind(lifecycle::CommentSpeaker::System.value())
        .fetch_optional(&mut **tx)
        .await?
        .is_some();
    if duplicate {
        return Ok(());
    }
    let actor_id = seams.agent_system_user().await.map_err(SweepError::Seam)?;
    let comment_id = Uuid::new_v4();
    let description_id = Uuid::new_v4();
    // `comment_insert_sql` positional binds (`lifecycle.rs:452-460`):
    // two `now()` reads, ambient user nulls (workers carry no `crum`
    // user), then the fetched ids plus the plan.
    sqlx::query(&lifecycle::comment_insert_sql())
        .bind(Utc::now())
        .bind(Utc::now())
        .bind(None::<Uuid>)
        .bind(None::<Uuid>)
        .bind(None::<DateTime<Utc>>)
        .bind(comment_id)
        .bind(ids.project_id)
        .bind(ids.workspace_id)
        .bind(comment.comment_stripped.clone())
        .bind(json!({}))
        .bind(comment.comment_html.clone())
        .bind(None::<Uuid>)
        .bind(Vec::<String>::new())
        .bind(Vec::<String>::new())
        .bind(ids.issue_id)
        .bind(actor_id)
        .bind("INTERNAL")
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(comment.speaker.value())
        .bind(comment.speaker_label.clone())
        .bind(comment.speaker_agent_run_id)
        .bind(None::<DateTime<Utc>>)
        .bind(None::<Uuid>)
        .execute(&mut **tx)
        .await?;
    sqlx::query(&lifecycle::description_insert_sql())
        .bind(Utc::now())
        .bind(Utc::now())
        .bind(None::<Uuid>)
        .bind(None::<Uuid>)
        .bind(None::<DateTime<Utc>>)
        .bind(description_id)
        .bind(ids.workspace_id)
        .bind(ids.project_id)
        .bind(json!({}))
        .bind(comment.comment_html.clone())
        .bind(None::<Vec<u8>>)
        .bind(comment.description_stripped.clone())
        .execute(&mut **tx)
        .await?;
    sqlx::query(lifecycle::comment_description_link_sql())
        .bind(description_id)
        .bind(comment_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// The ids the comment inserts need off the failure re-read row
/// (`agent_run` cols, then `issues` in L4 `_meta` order: `id` 6th,
/// `project_id` 7th, `workspace_id` 8th — verified against the SQL
/// text in `failure_comment_ids_match_planner_sql`).
struct FailureCommentIds {
    issue_id: Uuid,
    project_id: Uuid,
    workspace_id: Uuid,
}

fn map_failure_comment_ids(row: &sqlx::postgres::PgRow) -> Result<FailureCommentIds, SweepError> {
    let base = agent_run::COLUMNS.len();
    let issue_id: Option<Uuid> = row.try_get(base + 5).map_err(SweepError::Db)?;
    let Some(issue_id) = issue_id else {
        return Err(SweepError::BadRow("comment issue row"));
    };
    Ok(FailureCommentIds {
        issue_id,
        project_id: row.try_get(base + 6).map_err(SweepError::Db)?,
        workspace_id: row.try_get(base + 7).map_err(SweepError::Db)?,
    })
}

/// The scheduler hook (`:157-165`) behind a savepoint, same
/// release/rollback-to mechanics as the comment hook.
async fn run_scheduler_hook_in_savepoint(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: &Uuid,
    hooks: &finalization::HooksPlan,
) -> Result<(), SweepError> {
    sqlx::query("SAVEPOINT sp_scheduler_hook")
        .execute(&mut **tx)
        .await?;
    match run_scheduler_hook_update(tx, hooks).await {
        Ok(()) => {
            sqlx::query("RELEASE SAVEPOINT sp_scheduler_hook")
                .execute(&mut **tx)
                .await?;
            Ok(())
        }
        Err(error) => {
            tracing::error!(%error, "scheduler.terminate_hook: failed for run {run_id}");
            sqlx::query("ROLLBACK TO SAVEPOINT sp_scheduler_hook")
                .execute(&mut **tx)
                .await?;
            Ok(())
        }
    }
}

/// The scheduler binding write. A zero-row update raises in Django
/// (`update_fields` save on a missing row) — the executor maps it to
/// the savepoint rollback the same way.
async fn run_scheduler_hook_update(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    hooks: &finalization::HooksPlan,
) -> Result<(), SweepError> {
    let Some(plan) = hooks.scheduler.as_ref() else {
        return Ok(());
    };
    match plan {
        SchedulerHookPlan::Noop => Ok(()),
        SchedulerHookPlan::ClearError { binding_id } => {
            update_scheduler_binding(tx, binding_id, "").await
        }
        SchedulerHookPlan::SetError {
            binding_id,
            message,
        } => update_scheduler_binding(tx, binding_id, message).await,
    }
}

async fn update_scheduler_binding(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    binding_id: &Uuid,
    message: &str,
) -> Result<(), SweepError> {
    let done = sqlx::query(scheduler_hook::scheduler_binding_update_sql())
        .bind(message.to_owned())
        .bind(Utc::now())
        .bind(*binding_id)
        .execute(&mut **tx)
        .await?;
    if done.rows_affected() == 0 {
        return Err(SweepError::BadRow("scheduler binding row"));
    }
    Ok(())
}

/// `_fire_pending_entry` (`run_lifecycle.py:141-154`): the pending
/// ticker for the run's work item, if any. The `.delay` itself fires
/// post-commit (see [`apply_agent_run_terminal_effects`]).
async fn fire_pending_entry_lookup(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    work_item_id: &Option<Uuid>,
) -> Result<Option<Uuid>, SweepError> {
    let Some(work_item_id) = work_item_id else {
        return Ok(None);
    };
    let found: Option<Uuid> = sqlx::query_scalar(lifecycle::ticker_pending_entry_sql())
        .bind(*work_item_id)
        .fetch_optional(&mut **tx)
        .await?
        .flatten();
    Ok(found)
}

/// The capacity branch (`:184-198`): cloud runs dispatch waiting
/// work; local runs drain runner and/or pod. Unisolated — a failure
/// propagates and the marker write below never runs.
async fn run_capacity_plan(
    seams: &dyn RunnerRunsSeams,
    plan: &finalization::CapacityPlan,
) -> Result<(), SweepError> {
    match plan {
        finalization::CapacityPlan::Cloud { workspace_id } => seams
            .dispatch_waiting(*workspace_id)
            .await
            .map_err(SweepError::Seam),
        finalization::CapacityPlan::Local { runner_id, pod_id } => {
            if let Some(runner_id) = runner_id {
                seams
                    .drain_runner(*runner_id)
                    .await
                    .map_err(SweepError::Seam)?;
            }
            if let Some(pod_id) = pod_id {
                seams.drain_pod(*pod_id).await.map_err(SweepError::Seam)?;
            }
            Ok(())
        }
    }
}

/// `runner.reconcile_agent_run_terminal_effects` (`tasks.py:340-353`):
/// `.delay` the single apply for every terminal run missing a
/// marker, oldest `ended_at` first, capped at 100. Returns the batch
/// size.
pub async fn reconcile_agent_run_terminal_effects(pool: &PgPool) -> Result<u64, SweepError> {
    let candidate_sql = reconcile_terminal_candidate_sql();
    let mut query = sqlx::query_scalar(&candidate_sql);
    for status in TERMINAL_RUN_STATUSES {
        query = query.bind(status.value());
    }
    let ids: Vec<Uuid> = query.fetch_all(pool).await?;
    for run_id in &ids {
        enqueue(pool, &terminal_effects_job(run_id)).await?;
    }
    Ok(ids.len() as u64)
}

// ---------------------------------------------------------------------------
// Registry handlers
// ---------------------------------------------------------------------------

/// `apply_agent_run_terminal_effects(run_id)`: the first positional
/// arg as a run id. A malformed arg parks the job (`Verdict::Fail`)
/// — the `fire_tick` precedent for poison args.
pub fn parse_apply_arg(job: &JobRow) -> Result<Uuid, String> {
    job.args
        .as_array()
        .and_then(|args| args.first())
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{TERMINAL_EFFECTS_TASK}: expected args[0] to be a run id string"))
        .and_then(|raw| {
            Uuid::parse_str(raw)
                .map_err(|_| format!("{TERMINAL_EFFECTS_TASK}: invalid run id {raw:?}"))
        })
}

/// Register the six L6b handlers. Beat still fires these names on the
/// Django plane; the handlers run when the names route here.
pub fn register_runner_runs_tasks(
    registry: &mut Registry,
    pool: PgPool,
    seams: Arc<dyn RunnerRunsSeams>,
) {
    for task in [
        SWEEP_RUN_MESSAGE_DEDUPE_TASK,
        SWEEP_CHAT_MESSAGE_DEDUPE_TASK,
    ] {
        let pool = pool.clone();
        registry.register(
            task,
            Arc::new(move |_job: JobRow| {
                let pool = pool.clone();
                let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                    Box::pin(async move {
                        let now = Utc::now();
                        let ttl = dedupe_ttl_secs();
                        if task == SWEEP_RUN_MESSAGE_DEDUPE_TASK {
                            sweep_run_message_dedupe(&pool, &now, ttl)
                                .await
                                .map_err(|error| error.to_string())?;
                        } else {
                            sweep_chat_message_dedupe(&pool, &now, ttl)
                                .await
                                .map_err(|error| error.to_string())?;
                        }
                        Ok(Verdict::Ack)
                    });
                fut
            }),
        );
    }
    {
        let pool = pool.clone();
        let seams = seams.clone();
        registry.register(
            SWEEP_AGENT_CHAT_STATE_TASK,
            Arc::new(move |_job: JobRow| {
                let pool = pool.clone();
                let seams = seams.clone();
                let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                    Box::pin(async move {
                        sweep_agent_chat_state(&pool, seams.as_ref(), &Utc::now())
                            .await
                            .map_err(|error| error.to_string())?;
                        Ok(Verdict::Ack)
                    });
                fut
            }),
        );
    }
    {
        let pool = pool.clone();
        let seams = seams.clone();
        registry.register(
            RECONCILE_STALLED_RUNS_TASK,
            Arc::new(move |_job: JobRow| {
                let pool = pool.clone();
                let seams = seams.clone();
                let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                    Box::pin(async move {
                        reconcile_stalled_runs(
                            &pool,
                            seams.as_ref(),
                            &Utc::now(),
                            stall_threshold_secs(),
                            observability_stale_secs(),
                        )
                        .await
                        .map_err(|error| error.to_string())?;
                        Ok(Verdict::Ack)
                    });
                fut
            }),
        );
    }
    {
        let pool = pool.clone();
        let seams = seams.clone();
        registry.register(
            TERMINAL_EFFECTS_TASK,
            Arc::new(move |job: JobRow| {
                let pool = pool.clone();
                let seams = seams.clone();
                let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                    Box::pin(async move {
                        let run_id = match parse_apply_arg(&job) {
                            Ok(run_id) => run_id,
                            Err(error) => return Ok(Verdict::Fail { error }),
                        };
                        apply_agent_run_terminal_effects(&pool, seams.as_ref(), &run_id)
                            .await
                            .map_err(|error| error.to_string())?;
                        Ok(Verdict::Ack)
                    });
                fut
            }),
        );
    }
    {
        let pool = pool.clone();
        registry.register(
            RECONCILE_TERMINAL_EFFECTS_TASK,
            Arc::new(move |_job: JobRow| {
                let pool = pool.clone();
                let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                    Box::pin(async move {
                        reconcile_agent_run_terminal_effects(&pool)
                            .await
                            .map_err(|error| error.to_string())?;
                        Ok(Verdict::Ack)
                    });
                fut
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use pidash_types::runner_runs::{enrich_run_error, RUN_MESSAGE_DEDUPE_TTL_SECS_DEFAULT};
    use serde_json::json;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-07-tasks.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    /// Collapse all whitespace runs to single spaces for SQL comparison.
    fn squish(sql: &str) -> String {
        sql.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The `SELECT` list of a `SELECT … FROM` statement, one entry per
    /// top-level comma.
    fn select_list(sql: &str) -> Vec<String> {
        let upper = sql.to_ascii_uppercase();
        let from = upper.find(" FROM ").expect("FROM in select");
        let list = sql["SELECT ".len()..from].trim();
        list.split(',')
            .map(|entry| entry.trim().to_owned())
            .collect()
    }

    fn utc(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, min, sec)
            .single()
            .expect("valid timestamp")
    }

    // ------------------------------------------------------------------
    // Task names + settings
    // ------------------------------------------------------------------

    #[test]
    fn task_names_match_celery_names() {
        assert_eq!(
            SWEEP_RUN_MESSAGE_DEDUPE_TASK,
            "runner.sweep_run_message_dedupe"
        );
        assert_eq!(
            SWEEP_CHAT_MESSAGE_DEDUPE_TASK,
            "runner.sweep_chat_message_dedupe"
        );
        assert_eq!(SWEEP_AGENT_CHAT_STATE_TASK, "runner.sweep_agent_chat_state");
        assert_eq!(RECONCILE_STALLED_RUNS_TASK, "runner.reconcile_stalled_runs");
        assert_eq!(
            TERMINAL_EFFECTS_TASK,
            "runner.apply_agent_run_terminal_effects"
        );
        assert_eq!(
            RECONCILE_TERMINAL_EFFECTS_TASK,
            "runner.reconcile_agent_run_terminal_effects"
        );
    }

    #[test]
    fn setting_defaults_match_fixture() {
        let fx = fixture();
        let settings = fx.get("settings").expect("settings section");
        assert_eq!(
            settings
                .get("RUN_MESSAGE_DEDUPE_TTL_SECS")
                .and_then(Value::as_i64),
            Some(RUN_MESSAGE_DEDUPE_TTL_SECS_DEFAULT)
        );
        assert_eq!(
            settings
                .get("RUNNER_AGENT_STALL_THRESHOLD_SECS")
                .and_then(Value::as_i64),
            Some(STALL_THRESHOLD_SECS_DEFAULT)
        );
        assert_eq!(
            settings
                .get("RUNNER_AGENT_OBSERVABILITY_STALE_SECS")
                .and_then(Value::as_i64),
            Some(OBSERVABILITY_STALE_SECS_DEFAULT)
        );
        assert_eq!(RUN_MESSAGE_DEDUPE_TTL_SECS_DEFAULT, 604800);
        assert_eq!(STALL_THRESHOLD_SECS_DEFAULT, 360);
        assert_eq!(OBSERVABILITY_STALE_SECS_DEFAULT, 90);
    }

    #[test]
    fn setting_parse_defaults_and_passes_through() {
        assert_eq!(parse_setting_secs(None, 360), 360);
        assert_eq!(parse_setting_secs(Some("45"), 360), 45);
        // Garbage falls back (F-09 `env_secs` precedent); 0/negative
        // pass through like `int()`.
        assert_eq!(parse_setting_secs(Some("bogus"), 360), 360);
        assert_eq!(parse_setting_secs(Some(""), 360), 360);
        assert_eq!(parse_setting_secs(Some("0"), 360), 0);
        assert_eq!(parse_setting_secs(Some("-5"), 360), -5);
        assert_eq!(parse_dedupe_ttl(None), 604800);
        assert_eq!(parse_dedupe_ttl(Some("60")), 60);
        assert_eq!(parse_dedupe_ttl(Some("bogus")), 604800);
    }

    #[test]
    fn cutoffs_match_task_arithmetic() {
        let now = utc(2026, 10, 2, 22, 54, 9);
        assert_eq!(
            dedupe_cutoff(now, 604800),
            now - chrono::Duration::seconds(604800)
        );
        let (cutoff, snapshot) = stall_cutoffs(now, 360, 90);
        assert_eq!(cutoff, now - chrono::Duration::seconds(360));
        assert_eq!(snapshot, now - chrono::Duration::seconds(90));
        assert_eq!(
            stall_error_detail(360),
            "agent stalled: no events for >360s"
        );
    }

    #[test]
    fn stall_detail_never_enriches() {
        // The fixed stall text carries no auth signal, so the
        // `RunnerInfo` gate in the reap stays closed and enrichment is
        // the identity — verified here so the gate is pinned, not
        // assumed.
        for threshold in [0, 30, 360, 86400] {
            let detail = stall_error_detail(threshold);
            assert!(
                !matches!(
                    classify_run_error(Some(&detail)),
                    Some(diagnostic)
                        if diagnostic.kind == RunErrorKind::AgentAuthentication
                ),
                "stall detail must not classify as auth: {detail}"
            );
            assert_eq!(enrich_run_error(Some(&detail), None, None), detail);
        }
    }

    // ------------------------------------------------------------------
    // SQL pins
    // ------------------------------------------------------------------

    #[test]
    fn dedupe_deletes_match_fast_delete_shape() {
        assert_eq!(
            dedupe_delete_sql(run_dedupe::TABLE),
            "DELETE FROM \"run_message_dedupe\" WHERE \"run_message_dedupe\".\"created_at\" < $1"
        );
        assert_eq!(
            dedupe_delete_sql(chat_dedupe::TABLE),
            "DELETE FROM \"chat_message_dedupe\" WHERE \"chat_message_dedupe\".\"created_at\" < $1"
        );
        // The shared TTL key both sweeps read.
        assert_eq!(
            fixture()["dedupe_rule"],
            json!(
                "created_at < now-604800s deleted; both sweeps share RUN_MESSAGE_DEDUPE_TTL_SECS"
            )
        );
    }

    #[test]
    fn stall_candidate_select_lists_match_l2_order() {
        let entries = select_list(&stalled_candidate_sql());
        let mut expected = Vec::new();
        expected.extend(
            agent_run::COLUMNS
                .iter()
                .map(|column| format!("\"agent_run\".\"{column}\"")),
        );
        expected.extend(
            runner_cols::COLUMNS
                .iter()
                .map(|column| format!("\"runner\".\"{column}\"")),
        );
        assert_eq!(entries, expected);
        assert_eq!(agent_run::COLUMNS.len(), 41);
        assert_eq!(runner_cols::COLUMNS.len(), 30);
    }

    #[test]
    fn stall_candidate_matches_fixture_modulo_literals() {
        let recorded = fixture()["reconcile_stalled_runs"]["candidate_sql"][0]
            .as_str()
            .expect("candidate_sql[0]")
            .to_owned();
        // Literals → params in bind order: statuses, silence cutoff,
        // snapshot cutoff, zero.
        let normalized = recorded
            .replace("'assigned'", "$1")
            .replace("'running'", "$2")
            .replacen("'2026-10-02 22:48:09.124690+00:00'::timestamptz", "$3", 1)
            .replacen("'2026-10-02 22:52:39.124690+00:00'::timestamptz", "$4", 1)
            .replace(
                "\"runner_live_state\".\"approvals_pending\" = 0",
                "\"runner_live_state\".\"approvals_pending\" = $5",
            );
        assert_eq!(squish(&stalled_candidate_sql()), squish(&normalized));
        // The Q-sorted conjunct order survives normalization: silence
        // first, then observed, then freshness.
        let text = stalled_candidate_sql();
        let silence = text
            .find("\"runner_live_state\".\"last_event_at\" < $3")
            .expect("silence");
        let observed = text
            .find("\"runner_live_state\".\"observed_run_id\" = (\"agent_run\".\"id\")")
            .expect("observed");
        let fresh = text
            .find("\"runner_live_state\".\"updated_at\" >= $4")
            .expect("freshness");
        assert!(silence < observed && observed < fresh);
    }

    #[test]
    fn reconcile_candidate_matches_fixture_modulo_set_order() {
        let recorded = fixture()["reconcile_terminal_effects"]["sql"][0]
            .as_str()
            .expect("reconcile sql")
            .to_owned();
        // Django iterates a `set` for the `IN` list, so the recorded
        // order varies per process; the executor binds the L1 tuple
        // order (semantics are order-free — the L4 ported quirk).
        // Literals normalize to their L1 positions, preserving the
        // recorded order — so the comparison below is order-free too:
        // the same five params as a set, plus the same surrounding
        // statement once the IN list is masked.
        let mut normalized = recorded;
        for (index, status) in TERMINAL_RUN_STATUSES.iter().enumerate() {
            normalized =
                normalized.replace(&format!("'{}'", status.value()), &format!("${}", index + 1));
        }
        let ours = squish(&reconcile_terminal_candidate_sql());
        let theirs = squish(&normalized);
        fn in_list(sql: &str) -> Vec<&str> {
            let start = sql.find("IN (").expect("IN") + 4;
            let end = sql[start..].find(')').expect("close") + start;
            let mut items: Vec<&str> = sql[start..end].split(',').map(str::trim).collect();
            items.sort_unstable();
            items
        }
        assert_eq!(in_list(&ours), in_list(&theirs));
        fn mask(sql: &str) -> String {
            let start = sql.find("IN (").expect("IN") + 4;
            let end = sql[start..].find(')').expect("close") + start;
            format!("{}IN (?){}", &sql[..start], &sql[end..])
        }
        assert_eq!(mask(&ours), mask(&theirs));
    }

    #[test]
    fn live_state_facts_index_matches_planner_sql() {
        let entries = select_list(&lifecycle::live_state_by_runner_sql());
        assert_eq!(
            entries
                .iter()
                .position(|entry| entry == "\"runner_live_state\".\"usage\""),
            Some(8)
        );
        assert_eq!(
            entries
                .iter()
                .position(|entry| entry == "\"runner_live_state\".\"llm_model\""),
            Some(9)
        );
    }

    #[test]
    fn failure_comment_ids_match_planner_sql() {
        let entries = select_list(&lifecycle::failure_reread_sql());
        let base = agent_run::COLUMNS.len();
        assert_eq!(
            entries
                .iter()
                .position(|entry| entry == "\"issues\".\"id\""),
            Some(base + 5)
        );
        assert_eq!(
            entries
                .iter()
                .position(|entry| entry == "\"issues\".\"project_id\""),
            Some(base + 6)
        );
        assert_eq!(
            entries
                .iter()
                .position(|entry| entry == "\"issues\".\"workspace_id\""),
            Some(base + 7)
        );
    }

    #[test]
    fn scheduler_binding_tail_columns_exist() {
        assert!(col_index(scheduler_binding::COLUMNS, "id").is_some());
        assert!(col_index(scheduler_binding::COLUMNS, "last_error").is_some());
    }

    // ------------------------------------------------------------------
    // Fan-out shape + arg parsing
    // ------------------------------------------------------------------

    #[test]
    fn terminal_effects_job_matches_delay_shape() {
        let run_id = Uuid::parse_str("12345678-1234-5678-1234-567812345678").expect("uuid");
        let job = terminal_effects_job(&run_id);
        assert_eq!(job.task, TERMINAL_EFFECTS_TASK);
        assert_eq!(job.args, json!(["12345678-1234-5678-1234-567812345678"]));
        assert_eq!(job.kwargs, json!({}));
    }

    fn job_with_args(args: Value) -> JobRow {
        JobRow {
            id: 1,
            celery_id: "task-id-1".to_owned(),
            task: TERMINAL_EFFECTS_TASK.to_owned(),
            args,
            kwargs: json!({}),
            queue: "celery".to_owned(),
            status: "running".to_owned(),
            attempts: 0,
            max_retries: 3,
            visible_at: Utc::now(),
            claimed_at: None,
            claimed_by: None,
            created_at: Utc::now(),
            last_error: None,
        }
    }

    #[test]
    fn parse_apply_arg_accepts_uuid_shapes() {
        let hyphenated = "12345678-1234-5678-1234-567812345678";
        assert_eq!(
            parse_apply_arg(&job_with_args(json!([hyphenated]))).expect("hyphenated"),
            Uuid::parse_str(hyphenated).expect("uuid")
        );
        assert!(
            parse_apply_arg(&job_with_args(json!(["12345678123456781234567812345678"]))).is_ok()
        );
        assert!(parse_apply_arg(&job_with_args(json!([]))).is_err());
        assert!(parse_apply_arg(&job_with_args(json!([42]))).is_err());
        assert!(parse_apply_arg(&job_with_args(json!(["not-a-uuid"]))).is_err());
        assert!(parse_apply_arg(&job_with_args(json!({}))).is_err());
    }

    // ------------------------------------------------------------------
    // Registration + fixture replay
    // ------------------------------------------------------------------

    struct NoopSeams;

    impl RunnerRunsSeams for NoopSeams {
        fn drain_runner(
            &self,
            _runner_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            Box::pin(async move { Ok(()) })
        }
        fn drain_pod(
            &self,
            _pod_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            Box::pin(async move { Ok(()) })
        }
        fn dispatch_waiting(
            &self,
            _workspace_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            Box::pin(async move { Ok(()) })
        }
        fn complete_project_move_handoff(
            &self,
            _run_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            Box::pin(async move { Ok(()) })
        }
        fn disarm_on_terminal_signal(
            &self,
            _run_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            Box::pin(async move { Ok(()) })
        }
        fn apply_deferred_pause(
            &self,
            _run_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            Box::pin(async move { Ok(()) })
        }
        fn agent_system_user(
            &self,
        ) -> Pin<Box<dyn Future<Output = Result<Uuid, SeamError>> + Send + '_>> {
            Box::pin(async move { Ok(Uuid::nil()) })
        }
        fn publish_chat_event(
            &self,
            _channel: String,
            _payload: String,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            Box::pin(async move { Ok(()) })
        }
    }

    // Tokio: `PgPool::connect_lazy` spawns pool maintenance, so it
    // needs a runtime context even though it never connects.
    #[tokio::test]
    async fn registration_owns_all_six_names() {
        let pool =
            PgPool::connect_lazy("postgres://localhost/pidash_jobs_test").expect("lazy pool");
        let mut registry = Registry::new();
        register_runner_runs_tasks(&mut registry, pool, Arc::new(NoopSeams));
        for task in [
            SWEEP_RUN_MESSAGE_DEDUPE_TASK,
            SWEEP_CHAT_MESSAGE_DEDUPE_TASK,
            SWEEP_AGENT_CHAT_STATE_TASK,
            RECONCILE_STALLED_RUNS_TASK,
            TERMINAL_EFFECTS_TASK,
            RECONCILE_TERMINAL_EFFECTS_TASK,
        ] {
            assert!(registry.owns(task), "owns {task}");
            assert!(registry.get(task).is_some(), "handler for {task}");
        }
        assert!(!registry.owns("runner.sweep_old_streams"));
    }

    #[test]
    fn fixture_rows_pin_sweep_contracts() {
        let fx = fixture();
        // Dedupe: one old row gone per table, new rows kept.
        let dedupe = &fx["dedupe_sweeps"];
        assert_eq!(dedupe["run_returned"], json!(1));
        assert_eq!(dedupe["chat_returned"], json!(1));
        assert!(dedupe["run_old_gone"].as_bool().expect("bool"));
        assert!(dedupe["run_new_kept"].as_bool().expect("bool"));
        assert!(dedupe["chat_old_gone"].as_bool().expect("bool"));
        assert!(dedupe["chat_new_kept"].as_bool().expect("bool"));
        // Chat sweep: both service calls fire.
        assert_eq!(fx["sweep_agent_chat_state"]["calls"], json!([true, true]));
        // Stall: the reap set, the NULL exclusion, the poison survivor.
        let rows = &fx["reconcile_stalled_runs"]["rows"];
        for key in ["reap", "approvals_zero", "assigned"] {
            assert_eq!(rows[key]["status"], json!("failed"), "{key} reaped");
        }
        for key in [
            "observed_mismatch",
            "snapshot_stale",
            "event_fresh",
            "event_null",
            "approvals_pending",
            "poison",
        ] {
            assert_eq!(rows[key]["status"], json!("running"), "{key} survives");
        }
        assert_eq!(rows["event_null"]["error"], json!(""));
        assert_eq!(
            rows["awaiting_approval"]["status"],
            json!("awaiting_approval")
        );
        assert_eq!(fx["reconcile_stalled_runs"]["returned"], json!(3));
        // Terminal fan-out: full batch of 100.
        assert_eq!(fx["reconcile_terminal_effects"]["returned"], json!(100));
        assert_eq!(
            fx["reconcile_terminal_effects"]["delayed_count"],
            json!(100)
        );
        // Beat: the four scheduled names stay Django-owned; the apply
        // task is `.delay`-only.
        let beat = fx["beat_runner_entries"].as_object().expect("beat map");
        for task in [
            "runner.reconcile_stalled_runs",
            "runner.sweep_agent_chat_state",
            "runner.sweep_chat_message_dedupe",
            "runner.reconcile_agent_run_terminal_effects",
        ] {
            assert!(
                beat.values()
                    .any(|entry| entry.get("task").and_then(Value::as_str) == Some(task)),
                "beat still owns {task}"
            );
        }
        let unscheduled = fx["beat_not_scheduled"].as_array().expect("unscheduled");
        assert!(unscheduled
            .iter()
            .any(|name| name == "runner.apply_agent_run_terminal_effects (.delay only)"));
    }

    // ------------------------------------------------------------------
    // Live scenario runner (Django cross-check, ignored without a DB)
    // ------------------------------------------------------------------

    /// Recording seams for the live cross-check: every call is logged
    /// with raw ids; the runner resolves them to stable keys (run
    /// prompts, runner names) before writing the calls file.
    struct RecordingSeams {
        calls: std::sync::Mutex<Vec<Value>>,
        fail_actor: bool,
    }

    impl RecordingSeams {
        fn record(&self, value: Value) {
            self.calls.lock().expect("seam log").push(value);
        }
    }

    impl RunnerRunsSeams for RecordingSeams {
        fn drain_runner(
            &self,
            runner_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            self.record(json!({"fn": "drain_runner", "args": [runner_id.to_string()]}));
            Box::pin(async move { Ok(()) })
        }
        fn drain_pod(
            &self,
            pod_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            self.record(json!({"fn": "drain_pod", "args": [pod_id.to_string()]}));
            Box::pin(async move { Ok(()) })
        }
        fn dispatch_waiting(
            &self,
            workspace_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            self.record(json!({"fn": "dispatch_waiting", "args": [workspace_id.to_string()]}));
            Box::pin(async move { Ok(()) })
        }
        fn complete_project_move_handoff(
            &self,
            run_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            self.record(json!({"fn": "handoff", "args": [run_id.to_string()]}));
            Box::pin(async move { Ok(()) })
        }
        fn disarm_on_terminal_signal(
            &self,
            run_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            self.record(json!({"fn": "disarm", "args": [run_id.to_string()]}));
            Box::pin(async move { Ok(()) })
        }
        fn apply_deferred_pause(
            &self,
            run_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            self.record(json!({"fn": "deferred_pause", "args": [run_id.to_string()]}));
            Box::pin(async move { Ok(()) })
        }
        fn agent_system_user(
            &self,
        ) -> Pin<Box<dyn Future<Output = Result<Uuid, SeamError>> + Send + '_>> {
            // The failure-comment actor: a fixed probe id keeps the
            // dump joinable (the Django side resolves its own user;
            // the diff compares modulo actor PK).
            let fail = self.fail_actor;
            Box::pin(async move {
                if fail {
                    Err("c2 poison".to_owned())
                } else {
                    Ok(Uuid::parse_str("00000000-0000-0000-0000-000000540000").expect("probe"))
                }
            })
        }
        fn publish_chat_event(
            &self,
            channel: String,
            payload: String,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            self.record(json!({"publish": channel, "payload": payload}));
            Box::pin(async move { Ok(()) })
        }
    }

    /// Live cross-check runner, one scenario per invocation:
    ///
    /// ```sh
    /// export DATABASE_URL=postgres://localhost:5432/<seeded-scratch>
    /// export V540_SCENARIO=c V540_CALLS=/tmp/verify540_rs_calls_c.json
    /// cargo test -p pidash-jobs runner_runs::sweeps_chat::tests::live_run_scenario -- --ignored --nocapture
    /// ```
    ///
    /// The database must be migrated and seeded first
    /// (`/tmp/verify540_seed.py <scenario>`). Table state is dumped
    /// separately (`/tmp/verify540_dump.py`); this test writes only
    /// the executor's return value and resolved seam calls.
    /// Ignored by default so CI without a database stays green.
    #[tokio::test]
    #[ignore = "needs live scratch PG seeded for V540_SCENARIO via DATABASE_URL"]
    async fn live_run_scenario() {
        let url = std::env::var("DATABASE_URL").expect("export DATABASE_URL for the live test");
        let scenario = std::env::var("V540_SCENARIO").expect("export V540_SCENARIO=(a|b|c|c2|d|e)");
        let calls_path = std::env::var("V540_CALLS").expect("export V540_CALLS=<out json>");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .expect("connect scratch database");
        crate::queue::ensure_schema(&pool)
            .await
            .expect("ensure queue schema");
        let seams = Arc::new(RecordingSeams {
            calls: std::sync::Mutex::new(Vec::new()),
            fail_actor: scenario == "c2",
        });
        let now = Utc::now();
        let returned: Value = match scenario.as_str() {
            "a" => {
                let run = sweep_run_message_dedupe(&pool, &now, 604800)
                    .await
                    .expect("run dedupe");
                let chat = sweep_chat_message_dedupe(&pool, &now, 604800)
                    .await
                    .expect("chat dedupe");
                json!({"run": run, "chat": chat})
            }
            "b" => json!(sweep_agent_chat_state(&pool, seams.as_ref(), &now)
                .await
                .expect("chat sweep")),
            "c" => json!(reconcile_stalled_runs(&pool, seams.as_ref(), &now, 360, 90)
                .await
                .expect("stall sweep")),
            "c2" => {
                let id: Uuid = sqlx::query_scalar(
                    "SELECT \"agent_run\".\"id\" FROM \"agent_run\" WHERE \"agent_run\".\"prompt\" = $1",
                )
                .bind("540:c2-me")
                .fetch_one(&pool)
                .await
                .expect("seeded run");
                json!(apply_agent_run_terminal_effects(&pool, seams.as_ref(), &id)
                    .await
                    .expect("apply"))
            }
            "d" => json!(reconcile_agent_run_terminal_effects(&pool)
                .await
                .expect("reconcile")),
            "e" => {
                let id: Uuid = sqlx::query_scalar(
                    "SELECT \"agent_run\".\"id\" FROM \"agent_run\" WHERE \"agent_run\".\"prompt\" = $1",
                )
                .bind("540:apply-me")
                .fetch_one(&pool)
                .await
                .expect("seeded run");
                json!(apply_agent_run_terminal_effects(&pool, seams.as_ref(), &id)
                    .await
                    .expect("apply"))
            }
            other => panic!("unknown V540_SCENARIO={other}"),
        };
        // Resolve recorded ids to stable keys (run prompts, runner names).
        let run_rows: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT \"agent_run\".\"id\", \"agent_run\".\"prompt\" FROM \"agent_run\"",
        )
        .fetch_all(&pool)
        .await
        .expect("run map");
        let runner_rows: Vec<(Uuid, String)> =
            sqlx::query_as("SELECT \"runner\".\"id\", \"runner\".\"name\" FROM \"runner\"")
                .fetch_all(&pool)
                .await
                .expect("runner map");
        let pod_ids: Vec<Uuid> = sqlx::query_scalar("SELECT \"pod\".\"id\" FROM \"pod\"")
            .fetch_all(&pool)
            .await
            .expect("pod map");
        let mut calls = seams.calls.lock().expect("seam log").clone();
        for call in &mut calls {
            if let Some(args) = call.get_mut("args").and_then(Value::as_array_mut) {
                for arg in args.iter_mut() {
                    let raw = arg.as_str().unwrap_or("").to_owned();
                    let Ok(id) = Uuid::parse_str(&raw) else {
                        continue;
                    };
                    if let Some((_, prompt)) = run_rows.iter().find(|(rid, _)| *rid == id) {
                        *arg = Value::String(prompt.clone());
                    } else if let Some((_, name)) = runner_rows.iter().find(|(rid, _)| *rid == id) {
                        *arg = Value::String(name.clone());
                    } else if pod_ids.contains(&id) {
                        *arg = Value::String("pod".to_owned());
                    }
                }
            }
        }
        std::fs::write(
            &calls_path,
            serde_json::to_string_pretty(&json!({
                "returned": returned,
                "calls": calls,
            }))
            .expect("json"),
        )
        .expect("write calls file");
        println!("scenario {scenario}: returned={returned} calls={calls_path}");
    }
}
