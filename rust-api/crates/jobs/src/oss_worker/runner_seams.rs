#![forbid(unsafe_code)]

//! Live runner seam providers for the worker (PIDASHCONV-859).
//!
//! [`RunnerSeams`] is the shared [`RunnerRunsSeams`][seams],
//! [`SweepsRunsOutbox`] and [`InlineTerminalEffects`] provider the two
//! runner `register` fns take. Every method is a thin delegation to a
//! ported piece — the 862 executors, the `db` outbox verbs,
//! `services::pubsub`, the L6b inline apply — plus the mechanical
//! adaptations (`Uuid`↔`&str`, `usize`↔`i64`, `Value`→`Map`, error
//! `Display`→`String`). No port logic lives here: the matcher drains
//! come from `services::runner_sessions::live_drains` and the D-12
//! trio from `jobs::orchestration::runner_seams` (PIDASHCONV-862).
//!
//! [seams]: crate::runner_runs::sweeps_chat::RunnerRunsSeams

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::Utc;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::dispatch::dispatch_waiting;
use crate::orchestration::runner_seams::{
    complete_project_move_handoff_live, disarm_on_terminal_signal as disarm_executor,
    maybe_apply_deferred_pause_live,
};
use crate::runner_runs::sweeps_chat::{
    apply_agent_run_terminal_effects, RunnerRunsSeams, SeamError,
};
use crate::runner_runs::sweeps_runs::{
    InlineTerminalEffects, SendOfflineError, SweepsError, SweepsRunsOutbox,
};
use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings, RunnerSettings, Settings};
use pidash_db::orchestration::workpad::get_agent_system_user;
use pidash_db::runner_sessions::outbox::{self as session_outbox, OutboxError};
use pidash_services::extensions::{AssistantModelSeam, CloudAgentToolsetsSeam};
use pidash_services::runner_sessions::live_drains::{
    drain_for_runner_by_id, drain_pod_by_id, PoolPubsubStore,
};
use pidash_services::runner_sessions::pubsub as pubsub_kernel;

/// Shared live providers for the runner tasks (PIDASHCONV-859, moved
/// from the binary to this library module in PIDASHCONV-918 so both
/// binaries compose the same providers).
/// Construct with [`RunnerSeams::new`] — no I/O.
#[derive(Debug, Clone)]
pub struct RunnerSeams {
    pool: PgPool,
    redis: Option<redis::Client>,
    runner: RunnerSettings,
    cloud: CloudAgentSettings,
    managed: ManagedRunnerSettings,
    toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    assistant_seam: Arc<dyn AssistantModelSeam>,
}

impl RunnerSeams {
    /// Build the providers from the worker pool, the shared Redis
    /// client (the `LivePorts::new` shape — `None` when `REDIS_URL`
    /// is unset or unparsable), the resolved [`Settings`], and the
    /// injected cloud-agent toolsets and assistant seams. No I/O:
    /// everything is stored for the per-call effects.
    pub fn new(
        pool: PgPool,
        redis: Option<redis::Client>,
        settings: &Settings,
        toolsets: Arc<dyn CloudAgentToolsetsSeam>,
        assistant_seam: Arc<dyn AssistantModelSeam>,
    ) -> Self {
        Self {
            pool,
            redis,
            runner: settings.runner.clone(),
            cloud: settings.cloud_agent.clone(),
            managed: settings.managed_runner.clone(),
            toolsets,
            assistant_seam,
        }
    }
}

// ---------------------------------------------------------------------------
// Chat event publish (`services/chat.py:140-147`)
//
// Moved verbatim from `pidash_api::runner_runs` (PIDASHCONV-918): the
// worker seam below is its second caller, and `pidash-jobs` cannot
// depend on `pidash-api` (the crate graph is `types` → `db` →
// `services` → `api`). The API crate re-exports this under its old
// path, so the chat handler is untouched.
// ---------------------------------------------------------------------------

/// `publish_event`: PUBLISH the serialized event on the session channel.
/// A missing client is a no-op (`redis_instance()` returning `None`).
/// The caller swallows every publish failure with a log line — the
/// source's `except Exception: logger.exception(...)`
/// (`services/chat.py:140-147`) never 500s. The caller passes
/// `RunnerPorts::redis_client`.
pub async fn publish_chat_event(
    client: Option<&redis::Client>,
    channel: &str,
    payload: &str,
) -> Result<(), redis::RedisError> {
    let Some(client) = client else {
        return Ok(());
    };
    use redis::AsyncCommands;
    let mut connection = client.get_multiplexed_async_connection().await?;
    let _: () = connection.publish(channel, payload).await?;
    Ok(())
}

impl RunnerRunsSeams for RunnerSeams {
    fn drain_runner(
        &self,
        runner_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
        Box::pin(async move {
            drain_for_runner_by_id(&self.pool, self.redis.as_ref(), &self.runner, runner_id)
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }

    fn drain_pod(
        &self,
        pod_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
        Box::pin(async move {
            drain_pod_by_id(&self.pool, self.redis.as_ref(), &self.runner, pod_id)
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }

    fn dispatch_waiting(
        &self,
        workspace_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
        Box::pin(async move {
            dispatch_waiting(&self.pool, &self.cloud, workspace_id, Utc::now())
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }

    fn complete_project_move_handoff(
        &self,
        run_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
        Box::pin(async move {
            complete_project_move_handoff_live(
                &self.pool,
                &self.cloud,
                &self.managed,
                self.redis.as_ref(),
                &self.runner,
                run_id,
                self.toolsets.clone(),
                self.assistant_seam.clone(),
            )
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
        })
    }

    fn disarm_on_terminal_signal(
        &self,
        run_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
        Box::pin(async move {
            disarm_executor(&self.pool, run_id)
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }

    fn apply_deferred_pause(
        &self,
        run_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
        Box::pin(async move {
            maybe_apply_deferred_pause_live(
                &self.pool,
                &self.cloud,
                &self.managed,
                run_id,
                self.toolsets.clone(),
            )
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
        })
    }

    fn agent_system_user(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Uuid, SeamError>> + Send + '_>> {
        Box::pin(async move {
            // Own transaction: `get_agent_system_user` issues
            // `SAVEPOINT`s on the insert path, so it must run inside
            // one (its docs' "call inside one").
            let mut tx = self.pool.begin().await.map_err(|error| error.to_string())?;
            let (user, _) = get_agent_system_user(&mut tx, Utc::now())
                .await
                .map_err(|error| error.to_string())?;
            tx.commit().await.map_err(|error| error.to_string())?;
            Ok(user.id)
        })
    }

    fn publish_chat_event(
        &self,
        channel: String,
        payload: String,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
        Box::pin(async move {
            publish_chat_event(self.redis.as_ref(), &channel, &payload)
                .await
                .map_err(|error| error.to_string())
        })
    }
}

impl SweepsRunsOutbox for RunnerSeams {
    async fn send_to_runner(
        &self,
        runner_id: Uuid,
        message: Value,
    ) -> Result<(), SendOfflineError> {
        // The `SessionPorts::send_to_runner` shape
        // (`api::runner_sessions::runner_open`): a non-object frame
        // is warn-swallowed, the 862 [`PoolPubsubStore`] carries the
        // send, swallowed failures log, and only the offline error
        // raises.
        let Value::Object(map) = message else {
            tracing::warn!(%runner_id, "send_to_runner: non-object frame; swallowed");
            return Ok(());
        };
        let store =
            PoolPubsubStore::new(self.pool.clone(), self.redis.clone(), self.runner.clone());
        match pubsub_kernel::send_to_runner(&store, runner_id, &map).await {
            Ok(outcome) => {
                for warning in &outcome.warnings {
                    tracing::warn!("{warning}");
                }
                Ok(())
            }
            Err(OutboxError::RunnerOffline {
                runner_id,
                message_type,
            }) => Err(SendOfflineError {
                runner_id,
                message_type,
            }),
            Err(error) => {
                tracing::warn!(%error, %runner_id, "send_to_runner: enqueue failed; swallowed");
                Ok(())
            }
        }
    }

    async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), SweepsError> {
        session_outbox::clear_session_marker(self.redis.as_ref(), &session_id.to_string())
            .await
            .map_err(|error| SweepsError(error.to_string()))
    }

    async fn publish_session_eviction(
        &self,
        runner_id: Uuid,
        old_session_id: Uuid,
        new_session_id: &str,
    ) -> Result<(), SweepsError> {
        session_outbox::publish_session_eviction(
            self.redis.as_ref(),
            &runner_id.to_string(),
            Some(&old_session_id.to_string()),
            new_session_id,
        )
        .await
        .map_err(|error| SweepsError(error.to_string()))
    }

    async fn safe_trim_runner_stream(
        &self,
        runner_id: Uuid,
        time_cutoff_id: &str,
    ) -> Result<Option<i64>, SweepsError> {
        session_outbox::safe_trim_runner_stream(
            self.redis.as_ref(),
            &runner_id.to_string(),
            time_cutoff_id,
        )
        .await
        .map(|removed| removed.map(|count| count as i64))
        .map_err(|error| SweepsError(error.to_string()))
    }

    async fn reap_idle_consumers(
        &self,
        runner_id: Uuid,
        keep_consumers: &HashSet<String>,
    ) -> Result<i64, SweepsError> {
        // Default floor: the task passes no `min_idle_ms`
        // (`tasks.py:186-189`).
        session_outbox::reap_idle_consumers(
            self.redis.as_ref(),
            &self.runner,
            &runner_id.to_string(),
            keep_consumers,
            None,
        )
        .await
        .map(|count| count as i64)
        .map_err(|error| SweepsError(error.to_string()))
    }

    async fn due_runners_for_stream_cleanup(&self) -> Result<Vec<String>, SweepsError> {
        session_outbox::due_runners_for_stream_cleanup(self.redis.as_ref())
            .await
            .map_err(|error| SweepsError(error.to_string()))
    }

    async fn delete_runner_stream(&self, runner_id: &str) -> Result<(), SweepsError> {
        session_outbox::delete_runner_stream(self.redis.as_ref(), runner_id)
            .await
            .map_err(|error| SweepsError(error.to_string()))
    }

    async fn remove_stream_cleanup_marker(&self, runner_id: &str) -> Result<(), SweepsError> {
        session_outbox::remove_stream_cleanup_marker(self.redis.as_ref(), runner_id)
            .await
            .map_err(|error| SweepsError(error.to_string()))
    }
}

impl InlineTerminalEffects for RunnerSeams {
    async fn apply_inline(&self, run_id: Uuid) -> Result<bool, SweepsError> {
        apply_agent_run_terminal_effects(&self.pool, self, &run_id)
            .await
            .map_err(|error| SweepsError(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_chat_event_without_client_is_noop() {
        // Mirrors `redis_instance()` returning `None`: no client, no
        // I/O, no error.
        publish_chat_event(None, "channel", "payload")
            .await
            .expect("missing client is a no-op");
    }
}
