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
use serde_json::{Map, Value};
use sqlx::PgPool;
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
        use pidash_db::entity::assistant_user_llm_config as cfg_entity;
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
        let db = pidash_db::pool::sea_orm_connection(pool);
        let row: Option<cfg_entity::Model> = cfg_entity::Entity::find()
            .filter(cfg_entity::Column::UserId.eq(creator_id))
            .limit(1)
            .one(&db)
            .await
            .map_err(pidash_db::orm_tx::into_sqlx)?;
        Ok(row.map(|model| LlmRow {
            provider_kind: model.provider_kind,
            base_url: model.base_url,
            model_name: model.model_name,
            api_key_encrypted: model.api_key_encrypted,
        }))
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
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        use pidash_db::entity::{agent_run as run_entity, agent_run_event as ev_entity};
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
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
        if AgentExecutorKind::from_value(&locked.executor_kind)
            == Some(AgentExecutorKind::CloudAgent)
        {
            let exists = ev_entity::Entity::find()
                .filter(ev_entity::Column::AgentRunId.eq(call.run_id))
                .filter(ev_entity::Column::Kind.eq("terminal"))
                .select_only()
                .column(ev_entity::Column::Id)
                .into_tuple::<i64>()
                .one(&conn)
                .await
                .map_err(seam_err)?
                .is_some();
            if !exists {
                let max: Option<i32> = ev_entity::Entity::find()
                    .filter(ev_entity::Column::AgentRunId.eq(call.run_id))
                    .select_only()
                    .column(ev_entity::Column::Seq)
                    .order_by_desc(ev_entity::Column::Seq)
                    .limit(1)
                    .into_tuple::<i32>()
                    .one(&conn)
                    .await
                    .map_err(seam_err)?;
                let plan = finalization::plan_terminal_event(
                    max,
                    new_status,
                    &finalization::finalize_error_code(&values),
                );
                use sea_orm::ActiveModelTrait;
                ev_entity::ActiveModel {
                    seq: sea_orm::Set(plan.seq),
                    kind: sea_orm::Set("terminal".to_owned()),
                    payload: sea_orm::Set(plan.payload),
                    created_at: sea_orm::Set(sea_orm::prelude::DateTimeWithTimeZone::from(now)),
                    agent_run_id: sea_orm::Set(call.run_id),
                    ..Default::default()
                }
                .insert(&conn)
                .await
                .map_err(seam_err)?;
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
