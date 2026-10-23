//! Live [`ExecuteSeams`] for the cloud-agent execute plane (PIDASHCONV-832).
//!
//! The D-11 execute drives (`register_execute_tasks`) are generic over
//! [`ExecuteSeams`][crate::dispatch_execute::ExecuteSeams], and the
//! owning planes have not provided live calls — the only implementations
//! repo-wide are `#[cfg(test)]` fakes. The seams live here, next to the
//! execute drives (moved from the binary in PIDASHCONV-918 so both
//! binaries compose the same providers), composing only public ported
//! pieces plus two small mirrored reads:
//!
//! * `finalize_agent_run`: the `run_finalize_transaction` recipe
//!   (`runner_runs/sweeps_chat.rs`, itself a port of
//!   `agent_run_finalization.py:47-87`) recomposed from the public
//!   `runner_runs::finalization` planners plus the effects enqueue.
//!   The inline terminal-effects leg is skipped (it needs a live
//!   `RunnerRunsSeams`, PIDASHCONV-859): the queued effects task is
//!   idempotent and covers the hooks, so this is outcome-equivalent
//!   while a Python worker lives, and a known switchover dependency
//!   on PIDASHCONV-840 afterwards.
//! * `normalize_usage`: the ported pure function, delegated.
//! * `has_usable_llm_config` / `creator_llm_facts`: one mirrored row
//!   read (`assistant_user_llm_config`, the `get_config` query) plus
//!   the ported presence gate and SSRF check.
//! * `invoke_model`: the live model runtime
//!   ([`model_runtime`][crate::oss_worker::model_runtime], PIDASHCONV-866): the
//!   pydantic-ai `Agent.run` port over the Anthropic +
//!   OpenAI-compatible lanes, closing over the [`RunTools`][crate::oss_worker::model_runtime::tools_exec::RunTools]
//!   executor. The registered run handler executes end to end — no
//!   forward override.

#![forbid(unsafe_code)]

use crate::dispatch_execute::{
    CreatorLlmFacts, ExecuteSeams, FinalizeCall, InvokeError, ModelInvocation, RawUsage, SeamError,
};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Live execute seams over the worker's primary pool.
///
/// `block_private_urls` is `ASSISTANT_BLOCK_PRIVATE_URLS`
/// (`CloudAgentSettings::block_private_urls`): the SSRF gate input.
/// `invoke` carries the model runtime's static inputs (tool context +
/// key resolver).
#[derive(Clone)]
pub struct LiveExecuteSeams {
    pool: PgPool,
    block_private_urls: bool,
    invoke: crate::oss_worker::model_runtime::invoke::InvokeCtx,
}

impl LiveExecuteSeams {
    pub fn new(
        pool: PgPool,
        block_private_urls: bool,
        invoke: crate::oss_worker::model_runtime::invoke::InvokeCtx,
    ) -> Self {
        Self {
            pool,
            block_private_urls,
            invoke,
        }
    }

    /// Mirror of `get_config` (`llm.py:67-68`): the creator's BYOK row,
    /// if any. `user_id` is one-to-one, so at most one row exists.
    /// `pub(crate)` for the model runtime (key resolve + transition
    /// presence prefetch), which reuses this read without forking it.
    pub(crate) async fn llm_row(
        pool: &PgPool,
        creator_id: Uuid,
    ) -> Result<Option<LlmRow>, sqlx::Error> {
        sqlx::query_as::<_, LlmRow>(
            "SELECT provider_kind, base_url, model_name, api_key_encrypted \
             FROM assistant_user_llm_config WHERE user_id = $1 LIMIT 1",
        )
        .bind(creator_id)
        .fetch_optional(pool)
        .await
    }
}

/// One `assistant_user_llm_config` row: the `UserLLMConfig` columns
/// the model resolve consumes (`models.py:238-263`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct LlmRow {
    pub(crate) provider_kind: String,
    pub(crate) base_url: String,
    pub(crate) model_name: String,
    pub(crate) api_key_encrypted: Option<Vec<u8>>,
}

impl LlmRow {
    /// `has_api_key` (`models.py:261-262`): the key column is set and
    /// non-empty.
    pub(crate) fn has_api_key(&self) -> bool {
        self.api_key_encrypted
            .as_ref()
            .is_some_and(|key| !key.is_empty())
    }
}

/// Map one `updates` entry to its `SET` clause: the keys every
/// `FinalizeCall` producer emits (`fail_updates`, `cancel_updates`,
/// `refused_updates`, `completion_updates`, `expire_updates`). Text
/// columns bind
/// [`SetValue::Text`][pidash_services::runner_runs::SetValue], JSONB
/// columns [`SetValue::Json`][pidash_services::runner_runs::SetValue].
/// Anything else fails the job closed — a new producer key must extend
/// this map deliberately, never slip through as the wrong column type.
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

impl ExecuteSeams for LiveExecuteSeams {
    /// `finalize_agent_run` (`agent_run_finalization.py:47-87`),
    /// recomposed from the public planners exactly like
    /// `run_finalize_transaction`: lock the non-terminal row (plus the
    /// expected-status guard when carried), merge `done_payload`,
    /// write the values, insert the cloud-only terminal event, commit,
    /// then publish effects. A lock miss commits empty and loses
    /// (`Ok(false)`), like the source. The inline effects leg is
    /// skipped (see the module docs); the queued leg matches
    /// `reap_stalled_row`'s enqueue, including its log line.
    async fn finalize_agent_run(&self, call: FinalizeCall) -> Result<bool, SeamError> {
        use pidash_services::runner_runs::finalization;
        use pidash_types::dispatch::AgentExecutorKind;
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

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| SeamError(error.to_string()))?;
        let lock_sql =
            finalization::lock_run_for_finalize_sql(false, call.expected_status.is_some());
        let mut lock = sqlx::query(&lock_sql).bind(call.run_id);
        for status in TERMINAL_RUN_STATUSES {
            lock = lock.bind(status.value());
        }
        if let Some(expected) = call.expected_status {
            lock = lock.bind(expected);
        }
        let locked = lock
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| SeamError(error.to_string()))?;
        let Some(locked) = locked else {
            tx.commit()
                .await
                .map_err(|error| SeamError(error.to_string()))?;
            return Ok(false);
        };
        let stored_payload: Option<Value> = locked
            .try_get("done_payload")
            .map_err(|error| SeamError(error.to_string()))?;
        finalization::apply_done_payload_merge(
            &mut values,
            stored_payload.as_ref().unwrap_or(&Value::Null),
        );
        let update_sql = finalization::finalize_update_sql(&values);
        let mut update = sqlx::query(&update_sql);
        for clause in &values.clauses {
            update = match &clause.value {
                pidash_services::runner_runs::SetValue::Null => match clause.column {
                    "terminal_hooks_applied_at" | "terminal_capacity_released_at" => {
                        update.bind(None::<DateTime<Utc>>)
                    }
                    _ => {
                        return Err(SeamError("null finalize column".to_owned()));
                    }
                },
                pidash_services::runner_runs::SetValue::Now => update.bind(Utc::now()),
                pidash_services::runner_runs::SetValue::Text(text) => update.bind(text.clone()),
                pidash_services::runner_runs::SetValue::Json(value) => update.bind(value.clone()),
            };
        }
        update
            .bind(call.run_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| SeamError(error.to_string()))?;
        let executor_kind: String = locked
            .try_get("executor_kind")
            .map_err(|error| SeamError(error.to_string()))?;
        if AgentExecutorKind::from_value(&executor_kind) == Some(AgentExecutorKind::CloudAgent) {
            let exists = sqlx::query(finalization::terminal_event_exists_sql())
                .bind(call.run_id)
                .bind("terminal")
                .fetch_optional(&mut *tx)
                .await
                .map_err(|error| SeamError(error.to_string()))?
                .is_some();
            if !exists {
                let max: Option<i32> =
                    sqlx::query_scalar(finalization::terminal_event_max_seq_sql())
                        .bind(call.run_id)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(|error| SeamError(error.to_string()))?
                        .flatten();
                let plan = finalization::plan_terminal_event(
                    max,
                    new_status,
                    &finalization::finalize_error_code(&values),
                );
                sqlx::query(&finalization::terminal_event_insert_sql())
                    .bind(call.run_id)
                    .bind(plan.seq)
                    .bind("terminal")
                    .bind(plan.payload)
                    .bind(Utc::now())
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| SeamError(error.to_string()))?;
            }
        }
        tx.commit()
            .await
            .map_err(|error| SeamError(error.to_string()))?;

        for effect in finalization::plan_publish_effects(call.run_id) {
            match effect {
                pidash_services::runner_runs::LifecycleEffect::PublishTerminalEffects {
                    run_id,
                } => {
                    let job = crate::runner_runs::sweeps_chat::terminal_effects_job(&run_id);
                    if let Err(error) = crate::queue::enqueue(&self.pool, &job).await {
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

    /// `normalize_usage` (`usage.py:129-148`): the ported pure
    /// function, delegated.
    async fn normalize_usage(&self, report: Map<String, Value>) -> Result<Value, SeamError> {
        Ok(pidash_types::runner_runs::normalize_usage(&Value::Object(
            report,
        )))
    }

    /// `has_usable_llm_config` (`model_provider.py:45-53`): the row
    /// exists and holds a key. Pure gate over the mirrored read.
    async fn has_usable_llm_config(&self, creator_id: Uuid) -> Result<bool, SeamError> {
        let row = Self::llm_row(&self.pool, creator_id)
            .await
            .map_err(|error| SeamError(error.to_string()))?;
        Ok(pidash_services::assistant::seams::has_usable_llm_config(
            row.as_ref().is_some_and(|row| row.has_api_key()),
        ))
    }

    /// The creator's provider row as presence bits: the mirrored
    /// `get_config` read plus the execution-time SSRF re-check
    /// (`llm.py:78-84`). A missing row yields empty facts (no key),
    /// which the resolve gates map to `llm_config_missing`, exactly
    /// like the source's `cfg is None` arm.
    async fn creator_llm_facts(&self, creator_id: Uuid) -> Result<CreatorLlmFacts, SeamError> {
        let row = Self::llm_row(&self.pool, creator_id)
            .await
            .map_err(|error| SeamError(error.to_string()))?;
        let Some(row) = row else {
            return Ok(CreatorLlmFacts {
                has_api_key: false,
                model_name: String::new(),
                provider_kind: String::new(),
                base_url: String::new(),
                base_url_blocked: false,
            });
        };
        let blocked = !row.base_url.is_empty()
            && pidash_services::assistant::ssrf::is_blocked(
                &row.base_url,
                self.block_private_urls,
                &pidash_services::assistant::ssrf::SystemResolver,
            );
        Ok(CreatorLlmFacts {
            has_api_key: row.has_api_key(),
            model_name: row.model_name,
            provider_kind: row.provider_kind,
            base_url: row.base_url,
            base_url_blocked: blocked,
        })
    }

    /// The live model runtime (PIDASHCONV-866): delegate to the
    /// `Agent.run` port over the invocation's resolved model.
    async fn invoke_model(
        &self,
        invocation: ModelInvocation,
    ) -> Result<(pidash_types::dispatch::CloudAgentOutput, RawUsage), InvokeError> {
        crate::oss_worker::model_runtime::invoke::invoke(&self.invoke, invocation).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn llm_row_with(key: Option<Vec<u8>>) -> LlmRow {
        LlmRow {
            provider_kind: "anthropic".to_owned(),
            base_url: String::new(),
            model_name: "claude".to_owned(),
            api_key_encrypted: key,
        }
    }

    #[test]
    fn has_api_key_needs_a_non_empty_key_column() {
        assert!(!llm_row_with(None).has_api_key());
        assert!(!llm_row_with(Some(Vec::new())).has_api_key());
        assert!(llm_row_with(Some(vec![1, 2, 3])).has_api_key());
    }

    #[test]
    fn update_clause_binds_text_columns_as_text() {
        for column in ["error_code", "error", "refusal_category", "llm_model"] {
            let (bound, value) =
                update_clause(column, &Value::String("boom".to_owned())).expect("text binds");
            assert_eq!(bound, column);
            match value {
                pidash_services::runner_runs::SetValue::Text(text) => assert_eq!(text, "boom"),
                other => panic!("{column} must bind text, got {other:?}"),
            }
        }
    }

    #[test]
    fn update_clause_rejects_non_text_for_text_columns() {
        let error = update_clause("error", &Value::Null).expect_err("null must not bind text");
        assert!(error.0.contains("error"), "unexpected error: {error:?}");
    }

    #[test]
    fn update_clause_binds_payload_columns_as_json() {
        let payload = serde_json::json!({"ok": true});
        for column in ["done_payload", "usage"] {
            let (bound, value) = update_clause(column, &payload).expect("json binds");
            assert_eq!(bound, column);
            match value {
                pidash_services::runner_runs::SetValue::Json(value) => assert_eq!(value, payload),
                other => panic!("{column} must bind json, got {other:?}"),
            }
        }
    }

    #[test]
    fn update_clause_fails_closed_on_unknown_keys() {
        let error =
            update_clause("bogus", &Value::Null).expect_err("unknown keys must fail closed");
        assert!(error.0.contains("bogus"), "unexpected error: {error:?}");
    }
}
