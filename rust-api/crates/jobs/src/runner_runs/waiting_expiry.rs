#![forbid(unsafe_code)]

//! Managed wait expiry (`managed_runner.expire_waiting_runs`).
//!
//! Moved out of `dispatch::execute` in P3 step A (agent naming
//! convention): the only live survivor of the retired execute plane.
//! It serves Pi Dash Agent runs that never came online — QUEUED
//! managed rows older than the cutoff, first 500, finalized FAILED
//! with the `QUEUED` guard (`managed_runner/tasks.py:31-61`).
//!
//! The task name string does not change (section 8): it is shared
//! with Django's task set.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Select};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::config::PidashAgentSettings;
use pidash_db::entity::agent_run;
use pidash_db::orm_tx::into_sqlx;
use pidash_db::pool::sea_orm_connection;
use pidash_types::agent_runs::ManagedRunnerReason;

use crate::queue::{JobRow, NewJob};
use crate::task_message::TaskMessage;
use crate::worker::{HandlerError, Registry, Verdict};

// ---------------------------------------------------------------------------
// Celery wire: the task name + `.delay()` payloads
// ---------------------------------------------------------------------------

/// The managed wait expiry (`@shared_task(name=...)`,
/// `managed_runner/tasks.py:31`). No args.
pub const EXPIRE_WAITING_RUNS_TASK: &str = "managed_runner.expire_waiting_runs";

/// The `.delay()` message for the managed wait expiry.
pub fn expire_waiting_runs_message() -> TaskMessage {
    TaskMessage::new(EXPIRE_WAITING_RUNS_TASK, Vec::new(), Map::new())
}

/// The `.delay()` message lifted into its queue row (the beat fire shape).
pub fn expire_waiting_runs_job() -> NewJob {
    let message = expire_waiting_runs_message();
    NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    )
}

// ---------------------------------------------------------------------------
// Owned beat entry (`celery.py:176`; the reconcile entry is runner's)
// ---------------------------------------------------------------------------

/// Beat entry name (`celery.py:176`).
pub const EXPIRE_BEAT_NAME: &str = "managed-runner-expire-waiting-runs";
/// Beat cadence, seconds (`MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS`, default
/// 300).
pub const EXPIRE_INTERVAL_SECS: u64 = 300;
/// Settings key carrying the expiry cadence (`celery.py:178`).
pub const EXPIRE_SETTINGS_KEY: &str = "MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS";

/// The one owned beat entry: `(name, task, settings key)`, in
/// `celery.py` order — the golden the transcription test pins.
pub const OWNED_BEAT_ENTRIES: [(&str, &str, &str); 1] = [(
    EXPIRE_BEAT_NAME,
    EXPIRE_WAITING_RUNS_TASK,
    EXPIRE_SETTINGS_KEY,
)];

/// The expiry batch cap (`managed_runner/tasks.py:47`, `stale[:500]`).
pub const EXPIRE_BATCH_CAP: i64 = 500;

/// `managed_runner/tasks.py:54-56`.
pub const DETAIL_NEVER_CAME_ONLINE: &str =
    "Pi Dash Agent never came online for this run. Open the desktop app and run the issue again.";

/// The managed wait cutoff (`managed_runner/tasks.py:39`):
/// `now - MANAGED_RUNNER_QUEUED_MAX_AGE_SECS`.
pub fn expire_cutoff(now: DateTime<Utc>, queued_max_age_secs: i64) -> DateTime<Utc> {
    now - ChronoDuration::seconds(queued_max_age_secs)
}

/// The managed-expiry updates (`managed_runner/tasks.py:52-57`).
pub fn expire_updates() -> Map<String, Value> {
    Map::from_iter([
        (
            "error_code".to_owned(),
            Value::String(ManagedRunnerReason::NOT_CONNECTED.to_owned()),
        ),
        (
            "error".to_owned(),
            Value::String(DETAIL_NEVER_CAME_ONLINE.to_owned()),
        ),
    ])
}

pub(crate) fn expire_ids_query(cutoff: DateTime<Utc>) -> Select<agent_run::Entity> {
    agent_run::Entity::find()
        .select_only()
        .column(agent_run::Column::Id)
        .filter(agent_run::Column::ExecutorKind.eq("managed_runner"))
        .filter(agent_run::Column::Status.eq("queued"))
        .filter(agent_run::Column::CreatedAt.lt(cutoff))
        .order_by_desc(agent_run::Column::CreatedAt)
        .limit(500)
}

/// Overdue managed waits (`managed_runner/tasks.py:40-44`).
pub async fn expire_ids(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<Vec<Uuid>, sqlx::Error> {
    let db = sea_orm_connection(pool);
    expire_ids_query(cutoff)
        .into_tuple::<Uuid>()
        .all(&db)
        .await
        .map_err(into_sqlx)
}

// ---------------------------------------------------------------------------
// Seams: the planes this module does not own
// ---------------------------------------------------------------------------

/// A seam failure (transport/decoding on the seam implementor's side).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeamError(pub String);

impl std::fmt::Display for SeamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SeamError {}

/// One `finalize_agent_run` call
/// (`runner/services/agent_run_finalization.py:48-86`): the terminal
/// status, the updates dict, and the optional expected-status guard.
/// The seam owns the first-writer-wins transition, the `done_payload`
/// merge, the cloud `terminal` event row, and the effects publish —
/// returning whether this call won the race.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizeCall {
    pub run_id: Uuid,
    pub new_status: &'static str,
    pub updates: Map<String, Value>,
    pub expected_status: Option<&'static str>,
}

/// The store boundary for this module: runner-owned finalization.
pub trait ExecuteSeams: Send + Sync {
    /// `finalize_agent_run` (runner-owned, D-13+).
    fn finalize_agent_run(
        &self,
        call: FinalizeCall,
    ) -> impl std::future::Future<Output = Result<bool, SeamError>> + Send;
}

impl ExecuteSeams for LiveExecuteSeams {
    fn finalize_agent_run(
        &self,
        call: FinalizeCall,
    ) -> impl std::future::Future<Output = Result<bool, SeamError>> + Send {
        let pool = self.pool.clone();
        async move {
            finalize_agent_run(&pool, call).await
        }
    }
}

/// A failure outside the drive: database or seam. The handler retries
/// on these (the `sweep.rs` precedent).
#[derive(Debug)]
pub enum RunError {
    Db(sqlx::Error),
    Seam(SeamError),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Db(error) => write!(f, "database error: {error}"),
            RunError::Seam(error) => write!(f, "seam error: {error}"),
        }
    }
}

impl std::error::Error for RunError {}

impl From<sqlx::Error> for RunError {
    fn from(error: sqlx::Error) -> Self {
        RunError::Db(error)
    }
}

impl From<SeamError> for RunError {
    fn from(error: SeamError) -> Self {
        RunError::Seam(error)
    }
}

// ---------------------------------------------------------------------------
// Live seams: the runner-owned finalization over the worker's pool
// ---------------------------------------------------------------------------

/// Live [`ExecuteSeams`] over the worker's primary pool. Only
/// finalization survives here; the cloud model seams died with the
/// execute plane (P3 step A).
pub struct LiveExecuteSeams {
    pool: PgPool,
}

impl LiveExecuteSeams {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Map one `updates` entry to its column clause (`finalize_agent_run`
/// write mapping, moved from `oss_worker::execute_seams` in P3 step A).
fn update_clause(
    column: &str,
    value: &Value,
) -> Result<(&'static str, pidash_services::runner_runs::SetValue), SeamError> {
    use pidash_services::runner_runs::SetValue;
    let text = |column: &'static str, value: &Value| match value {
        Value::String(text) => Ok((column, SetValue::Text(text.clone()))),
        other => Err(SeamError(format!(
            "finalize_agent_run: {column} must bind text, got {other}"
        ))),
    };
    match column {
        "error_code" => text("error_code", value),
        "error" => text("error", value),
        "refusal_category" => text("refusal_category", value),
        "llm_model" => text("llm_model", value),
        "done_payload" => Ok(("done_payload", SetValue::Json(value.clone()))),
        "usage" => Ok(("usage", SetValue::Json(value.clone()))),
        unknown => Err(SeamError(format!(
            "finalize_agent_run: unmapped update key {unknown}"
        ))),
    }
}

/// `finalize_agent_run` (`agent_run_finalization.py:47-87`),
/// recomposed from the public planners exactly like
/// `run_finalize_transaction`: lock the non-terminal row (plus the
/// expected-status guard when carried), merge `done_payload`,
/// write the values, commit, then publish effects. (P3 step C:
/// the cloud-only terminal-event append is gone.) A lock miss
/// commits empty and loses (`Ok(false)`), like the source. The
/// inline effects leg is skipped (see the module docs); the queued
/// leg matches `reap_stalled_row`'s enqueue, including its log line.
async fn finalize_agent_run(pool: &PgPool, call: FinalizeCall) -> Result<bool, SeamError> {
    use pidash_services::runner_runs::finalization;
    use pidash_types::runner_runs::{AgentRunStatus, TERMINAL_RUN_STATUSES};

    let new_status = AgentRunStatus::from_value(call.new_status)
        .filter(|status| status.is_terminal())
        .ok_or_else(|| SeamError("new_status must be terminal".to_owned()))?;
    let mut extras = Vec::with_capacity(call.updates.len());
    for (column, value) in &call.updates {
        extras.push(update_clause(column.as_str(), value)?);
    }
    let mut values = finalization::plan_finalize_values(new_status, &extras)
        .map_err(|error| SeamError(error.to_string()))?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|error| SeamError(error.to_string()))?;
    let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
    use pidash_db::entity::agent_run as run_entity;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
    let seam_err = |error: sea_orm::DbErr| SeamError(error.to_string());
    let mut lock_query = run_entity::Entity::find_by_id(call.run_id)
        .filter(
            run_entity::Column::Status.is_not_in(
                TERMINAL_RUN_STATUSES
                    .iter()
                    .map(|s| s.value())
                    .collect::<Vec<_>>(),
            ),
        )
        .lock_exclusive();
    if let Some(expected) = call.expected_status {
        lock_query = lock_query.filter(run_entity::Column::Status.eq(expected));
    }
    let locked = lock_query.one(&conn).await.map_err(seam_err)?;
    let Some(locked) = locked else {
        tx.commit()
            .await
            .map_err(|error| SeamError(error.to_string()))?;
        return Ok(false);
    };
    finalization::apply_done_payload_merge(
        &mut values,
        locked.done_payload.as_ref().unwrap_or(&Value::Null),
    );
    // The `error_details` clause merges into the locked row's bag
    // in Rust (the SQL version did it server-side; same tx, same
    // row, no extra read).
    let merged_error_details = finalization::error_details_patch(&values).map(|patch| {
        let mut merged = locked
            .error_details
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(patch) = patch.as_object() {
            for (key, value) in patch {
                let key = key.as_str();
                let truthy = match value {
                    Value::Null => false,
                    Value::Bool(flag) => *flag,
                    Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
                    Value::String(text) => !text.is_empty(),
                    Value::Array(items) => !items.is_empty(),
                    Value::Object(map) => !map.is_empty(),
                };
                if truthy {
                    merged.insert(key.to_owned(), value.clone());
                } else {
                    merged.remove(key);
                }
            }
        }
        Value::Object(merged)
    });
    let now = chrono::Utc::now();
    let mut update = run_entity::Entity::update_many();
    for clause in &values.clauses {
        update = match clause.column {
            "status" => update.col_expr(
                run_entity::Column::Status,
                sea_orm::sea_query::Expr::value(match &clause.value {
                    pidash_services::runner_runs::SetValue::Text(text) => text.clone(),
                    _ => return Err(SeamError("status finalize clause".to_owned())),
                }),
            ),
            "ended_at" => update.col_expr(
                run_entity::Column::EndedAt,
                sea_orm::sea_query::Expr::value(sea_orm::prelude::DateTimeWithTimeZone::from(
                    now,
                )),
            ),
            "terminal_hooks_applied_at" | "terminal_capacity_released_at" => {
                match clause.column {
                    "terminal_hooks_applied_at" => update.col_expr(
                        run_entity::Column::TerminalHooksAppliedAt,
                        sea_orm::sea_query::Expr::value(
                            None::<sea_orm::prelude::DateTimeWithTimeZone>,
                        ),
                    ),
                    _ => update.col_expr(
                        run_entity::Column::TerminalCapacityReleasedAt,
                        sea_orm::sea_query::Expr::value(
                            None::<sea_orm::prelude::DateTimeWithTimeZone>,
                        ),
                    ),
                }
            }
            // `error_code` / `error` are folded into `error_details`
            // (`ERROR_DETAILS_COLUMN`); `refusal_category` is a
            // generated column. Only `llm_model` is an own column.
            "error_code" | "error" => update,
            "refusal_category" | "llm_model" => {
                let text = match &clause.value {
                    pidash_services::runner_runs::SetValue::Text(text) => text.clone(),
                    pidash_services::runner_runs::SetValue::Null => String::new(),
                    _ => return Err(SeamError("text finalize clause".to_owned())),
                };
                match clause.column {
                    "refusal_category" => update.col_expr(
                        run_entity::Column::RefusalCategory,
                        sea_orm::sea_query::Expr::value(text),
                    ),
                    _ => update.col_expr(
                        run_entity::Column::LlmModel,
                        sea_orm::sea_query::Expr::value(text),
                    ),
                }
            }
            "done_payload" | "usage" => match (&clause.value, clause.column) {
                (pidash_services::runner_runs::SetValue::Json(value), "done_payload") => update
                    .col_expr(
                        run_entity::Column::DonePayload,
                        sea_orm::sea_query::Expr::value(value.clone()),
                    ),
                (pidash_services::runner_runs::SetValue::Json(value), "usage") => update
                    .col_expr(
                        run_entity::Column::Usage,
                        sea_orm::sea_query::Expr::value(value.clone()),
                    ),
                _ => return Err(SeamError("json finalize clause".to_owned())),
            },
            "error_details" => update.col_expr(
                run_entity::Column::ErrorDetails,
                sea_orm::sea_query::Expr::value(
                    merged_error_details
                        .clone()
                        .unwrap_or_else(|| Value::Object(Default::default())),
                ),
            ),
            unknown => {
                return Err(SeamError(format!(
                    "finalize_agent_run: unmapped finalize column {unknown}"
                )))
            }
        };
    }
    update
        .filter(run_entity::Column::Id.eq(call.run_id))
        .exec(&conn)
        .await
        .map_err(seam_err)?;
    // P3 step C: the cloud-only terminal-event append died with the
    // retired plane; this finalize writes values only.
    tx.commit()
        .await
        .map_err(|error| SeamError(error.to_string()))?;

    for effect in finalization::plan_publish_effects(call.run_id) {
        match effect {
            pidash_services::runner_runs::LifecycleEffect::PublishTerminalEffects {
                run_id,
            } => {
                let job = crate::runner_runs::sweeps_chat::terminal_effects_job(&run_id);
                if let Err(error) = crate::queue::enqueue(pool, &job).await {
                    tracing::error!(%error, "failed to publish terminal effects for run {run_id}");
                }
            }
            pidash_services::runner_runs::LifecycleEffect::ApplyTerminalEffectsInline {
                run_id,
            } => {
                tracing::warn!(
                    "skipping inline terminal effects for run {run_id}: no live RunnerRunsSeams \
                     (PIDASHCONV-859); the queued effects task covers the hooks idempotently"
                );
            }
            _ => {}
        }
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// Drive
// ---------------------------------------------------------------------------

/// Fail managed runs that waited past the bound
/// (`managed_runner/tasks.py:31-61`): QUEUED managed rows older than
/// the cutoff, first 500, finalized FAILED with the `QUEUED` guard.
/// Returns how many finalizations won their race.
pub async fn drive_expire_waiting_runs<S: ExecuteSeams>(
    seams: &S,
    pool: &PgPool,
    managed: &PidashAgentSettings,
    now: DateTime<Utc>,
) -> Result<usize, RunError> {
    let cutoff = expire_cutoff(now, managed.queued_max_age_secs);
    let mut expired = 0;
    for run_id in expire_ids(pool, cutoff).await? {
        if seams
            .finalize_agent_run(FinalizeCall {
                run_id,
                new_status: pidash_db::agent_runs::AgentRunStatus::Failed.value(),
                updates: expire_updates(),
                expected_status: Some(pidash_db::agent_runs::AgentRunStatus::Queued.value()),
            })
            .await?
        {
            expired += 1;
            tracing::info!(run = %run_id, "managed_runner.queued_expired");
        }
    }
    Ok(expired)
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register the one owned handler (moved from
/// `register_execute_tasks` in P3 step A). The handler propagates
/// errors onto the worker's default retry budget (no task retry
/// policy set), like the scan/sweep handlers before it.
pub fn register_waiting_expiry_tasks<S: ExecuteSeams + 'static>(
    registry: &mut Registry,
    pool: PgPool,
    managed: PidashAgentSettings,
    seams: Arc<S>,
) {
    registry.register(
        EXPIRE_WAITING_RUNS_TASK,
        Arc::new(move |_job: JobRow| {
            let seams = seams.clone();
            let pool = pool.clone();
            let managed = managed.clone();
            let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                Box::pin(async move {
                    drive_expire_waiting_runs(seams.as_ref(), &pool, &managed, Utc::now())
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
    use chrono::TimeZone;
    use serde_json::json;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-07-execute.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn task_names_match_python_exactly() {
        assert_eq!(
            EXPIRE_WAITING_RUNS_TASK,
            "managed_runner.expire_waiting_runs"
        );
    }

    #[test]
    fn bare_message_and_job_carry_empty_args_and_kwargs() {
        let message = expire_waiting_runs_message();
        assert_eq!(message.task, EXPIRE_WAITING_RUNS_TASK);
        assert!(message.args.is_empty());
        assert!(message.kwargs.is_empty());
        let job = expire_waiting_runs_job();
        assert_eq!(job.task, EXPIRE_WAITING_RUNS_TASK);
        assert_eq!(job.args, json!([]));
        assert_eq!(job.kwargs, json!({}));
    }

    #[test]
    fn owned_beat_entries_match_the_fixture() {
        let beat = &fixture()["beat_entries_owned"];
        assert_eq!(
            beat["managed-runner-expire-waiting-runs"],
            json!({"task": "managed_runner.expire_waiting_runs", "settings_key": "MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS"})
        );
        for (name, task, key) in OWNED_BEAT_ENTRIES {
            assert_eq!(beat[name]["task"], json!(task), "task for {name}");
            assert_eq!(beat[name]["settings_key"], json!(key), "key for {name}");
        }
    }

    #[test]
    fn expire_cutoff_subtracts_queued_max_age() {
        let now = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap();
        assert_eq!(
            expire_cutoff(now, 43200),
            Utc.with_ymd_and_hms(2026, 9, 30, 12, 0, 0).unwrap()
        );
    }

    #[test]
    fn expire_updates_pin_code_and_detail() {
        let updates = expire_updates();
        assert_eq!(
            updates["error_code"],
            json!(ManagedRunnerReason::NOT_CONNECTED)
        );
        assert_eq!(updates["error"], json!(DETAIL_NEVER_CAME_ONLINE));
    }
}
