#![forbid(unsafe_code)]

//! API-side D-12 creation runtime: factory + post-commit drain (PIDASHCONV-743).
//!
//! The D-12 creation drivers (`orchestration::creation`) run against the
//! pool [`CreationSeam`][pidash_services::orchestration::creation::CreationSeam]
//! and [`FinalizeAgentRunSeam`][pidash_services::orchestration::creation::FinalizeAgentRunSeam]
//! twin that lives in `project_move_handoff` (the api-side twin of
//! `jobs::LiveCreationStore`). The move domain service creates handoff
//! runs inside its own transaction span; its
//! [`crate::domain_hooks::AppMovePostCommitHook`] drains them through the
//! public [`MoveHook`][pidash_jobs::hooks::MoveHook], which calls back
//! into this module for the api-only legs:
//!
//! - [`SpanCreationDeps::from_state`] builds the runtime facts
//!   (executor settings, admission cache, clock) from the request's
//!   [`AppState`].
//! - [`drain_terminal_effects`] runs finalized runs' terminal publish
//!   pairs after the caller's span commits, inline with the ambient
//!   user.
//!
//! The caller drains creation outboxes before its own post-commit
//! work: the creation registers its `on_commit` entries first (the
//! move's dispatch precedes its cancel send and pod drains,
//! `issue_move.py:353-372`).

use std::sync::Arc;

use chrono::Utc;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings, RunnerSettings};
use pidash_db::redis::client_from_settings;
use pidash_db::runner_sessions::machine_outbox::{self, MachineOutboxError};
use pidash_db::runner_sessions::models::runner_session;
use pidash_db::runner_sessions::outbox as runner_outbox;
use pidash_db::runner_sessions::outbox::OutboxError;
use pidash_db::runner_sessions::RunnerSession;
use pidash_services::domain::issues_move::MoveDrainError;
use pidash_services::extensions::{AssistantModelSeam, CloudAgentToolsetsSeam};
use pidash_services::runner_runs::finalization as finalize_kernel;
use pidash_services::runner_sessions::drain::{
    drain_pod_log, next_for_runner_sql, plan_assignment, AssignmentFacts, DrainEffect,
    ASSIGN_RUN_UPDATE_SQL, DRAIN_POD_BY_ID_LOOKUP_SQL, DRAIN_POD_IDLE_RUNNERS_SQL,
};
use pidash_services::runner_sessions::guards::alive_threshold;
use pidash_services::runner_sessions::pubsub::{
    send_to_runner, PubsubStore, CLOSE_ACTIVE_SESSIONS_SQL,
};

use crate::state::AppState;
use pidash_services::dispatch::admission::RedisAdmissionCache;

/// The creation runtime facts a span store is built from: executor
/// settings, the admission cache, and the unix clock for the
/// capacity/admission verdicts.
#[derive(Clone)]
pub(crate) struct SpanCreationDeps {
    pub(crate) cloud: CloudAgentSettings,
    pub(crate) managed: ManagedRunnerSettings,
    pub(crate) cache: RedisAdmissionCache,
    pub(crate) now_unix_secs: i64,
    pub(crate) toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    pub(crate) assistant_seam: Arc<dyn AssistantModelSeam>,
}

impl SpanCreationDeps {
    /// Build the runtime facts from the request state (the
    /// `complete_project_move_handoff` position).
    pub(crate) fn from_state(state: &AppState) -> Self {
        let settings = state.settings();
        Self {
            cloud: settings.cloud_agent.clone(),
            managed: settings.managed_runner.clone(),
            cache: RedisAdmissionCache {
                client: client_from_settings(state.settings()),
            },
            now_unix_secs: unix_now_secs(),
            toolsets: state.toolsets().clone(),
            assistant_seam: state.assistant_seam().clone(),
        }
    }
}

/// Drain finalized runs' terminal publish pairs after the caller's
/// span commits: one emit + inline apply per run, each isolated with
/// its failure log (the old `drain_creation_outboxes` terminal leg;
/// the dispatch + deferred legs moved to the public
/// [`MoveHook`][pidash_jobs::hooks::MoveHook], OVL-16).
/// `terminal_created_by` is the ambient crum request user for the
/// inline terminal effects.
pub(crate) async fn drain_terminal_effects(
    pool: &PgPool,
    state: &AppState,
    run_ids: &[Uuid],
    terminal_created_by: Option<Uuid>,
) {
    if run_ids.is_empty() {
        return;
    }
    let mut pairs = Vec::with_capacity(run_ids.len() * 2);
    for id in run_ids {
        pairs.extend(finalize_kernel::plan_publish_effects(*id));
    }
    let ports = crate::runner_runs::LivePorts::new(pool.clone(), state);
    pidash_jobs::orchestration::drain_publish_effects(pairs, |effect| {
        crate::runner_runs::run_endpoints::drain_lifecycle_effects(
            pool,
            &ports,
            vec![effect],
            terminal_created_by,
        )
    })
    .await;
}

/// `int(time.time())` (the `project_move_handoff` `unix_now_secs` position).
fn unix_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

// Shared with both move drivers (app + v1): one copy of the twins that
// lived in each move handler file. Failures answer `MoveDrainError`
// (every site was `Denial::ServerError` in both twins); the move chain
// maps them to its `PodDrain` stop.
// ---------------------------------------------------------------------------
// Move post-commit: cancel send + pod drain
// ---------------------------------------------------------------------------

/// The 552 [`PubsubStore`] over the pool + Redis (the delete-cmds
/// shape): same verbs, same outbox helpers, no runner-enroll
/// dependency.
pub(crate) struct MovePubsub<'p> {
    pub(crate) pool: &'p PgPool,
    pub(crate) redis: Option<&'p redis::Client>,
    pub(crate) runner: &'p RunnerSettings,
}

impl PubsubStore for MovePubsub<'_> {
    async fn enqueue_for_runner(
        &self,
        runner_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, OutboxError> {
        runner_outbox::enqueue_for_runner(self.redis, self.pool, self.runner, runner_id, message)
            .await
    }

    async fn enqueue_for_machine(
        &self,
        dev_machine_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, MachineOutboxError> {
        machine_outbox::enqueue_for_machine(
            self.redis,
            self.pool,
            self.runner,
            dev_machine_id,
            message,
        )
        .await
    }

    async fn active_runner_sessions(
        &self,
        runner_id: Uuid,
    ) -> Result<Vec<RunnerSession>, OutboxError> {
        let rows = sqlx::query(CLOSE_ACTIVE_SESSIONS_SQL)
            .bind(runner_id)
            .fetch_all(self.pool)
            .await?;
        rows.iter()
            .map(runner_session::runner_session_from_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(OutboxError::from)
    }

    async fn revoke_runner_session(
        &self,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), OutboxError> {
        pidash_services::domain::tokens::revoke_runner_session(
            self.pool,
            session_id,
            Utc::now(),
            reason,
        )
        .await
        .map_err(OutboxError::from)
    }

    async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), OutboxError> {
        runner_outbox::clear_session_marker(self.redis, &session_id.to_string()).await
    }

    async fn publish_session_eviction(
        &self,
        runner_id: Uuid,
        old_session_id: Uuid,
        new_session_id: &str,
    ) -> Result<(), OutboxError> {
        runner_outbox::publish_session_eviction(
            self.redis,
            &runner_id.to_string(),
            Some(&old_session_id.to_string()),
            new_session_id,
        )
        .await
    }
}

/// The `drain_pod_by_id` executor (`runner/services/matcher.py`,
/// through the 552 recipe): miss → 0 with no transaction; else one
/// transaction over the idle list, one `next_for_runner` claim per
/// runner, the `ASSIGN_RUN_UPDATE_SQL` write per claim — then, past
/// the commit, one `assign` send per claim. Effects dispatch in
/// idle-list order; the log line fires only when claims happened.
///
/// A dispatch failure propagates (the drain contract: the offline
/// error must not be swallowed) — rows already committed, mirroring
/// `on_commit` raising past the commit.
pub(crate) async fn drain_pod_by_id(
    pool: &PgPool,
    pubsub: &MovePubsub<'_>,
    pod_id: Uuid,
) -> Result<(), MoveDrainError> {
    let pod: Option<(Uuid,)> = sqlx::query_as(DRAIN_POD_BY_ID_LOOKUP_SQL)
        .bind(pod_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| MoveDrainError(error.to_string()))?;
    if pod.is_none() {
        return Ok(());
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| MoveDrainError(error.to_string()))?;
    let threshold = alive_threshold(Utc::now());
    let runners = sqlx::query(DRAIN_POD_IDLE_RUNNERS_SQL)
        .bind(threshold)
        .bind(pod_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|error| MoveDrainError(error.to_string()))?;
    let mut effects = Vec::new();
    for runner in &runners {
        let runner_id: Uuid = runner
            .try_get("id")
            .map_err(|error| MoveDrainError(error.to_string()))?;
        let owner_id: Uuid = runner
            .try_get("owner_id")
            .map_err(|error| MoveDrainError(error.to_string()))?;
        let provisioning: String = runner
            .try_get("provisioning")
            .map_err(|error| MoveDrainError(error.to_string()))?;
        let visibility: i32 = runner
            .try_get("visibility")
            .map_err(|error| MoveDrainError(error.to_string()))?;
        let Some(next_sql) = next_for_runner_sql(&provisioning, visibility) else {
            continue;
        };
        let run = sqlx::query(&next_sql)
            .bind(pod_id)
            .bind(runner_id)
            .bind(owner_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| MoveDrainError(error.to_string()))?;
        let Some(run) = run else {
            continue;
        };
        let run_id: Uuid = run
            .try_get("id")
            .map_err(|error| MoveDrainError(error.to_string()))?;
        let work_item_id: Option<Uuid> = run
            .try_get("work_item_id")
            .map_err(|error| MoveDrainError(error.to_string()))?;
        let prompt: String = run
            .try_get("prompt")
            .map_err(|error| MoveDrainError(error.to_string()))?;
        let run_config: Option<Value> = run
            .try_get("run_config")
            .map_err(|error| MoveDrainError(error.to_string()))?;
        let run_config = run_config
            .and_then(|config| config.as_object().cloned())
            .ok_or_else(|| {
                MoveDrainError("move pod drain: run_config is not an object".to_owned())
            })?;
        let plan = plan_assignment(&AssignmentFacts {
            run_id,
            runner_id,
            owner_id,
            work_item_id,
            prompt,
            run_config,
        });
        sqlx::query(ASSIGN_RUN_UPDATE_SQL)
            .bind(owner_id)
            .bind(runner_id)
            .bind(Utc::now())
            .bind(run_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| MoveDrainError(error.to_string()))?;
        effects.push(plan.after_commit);
    }
    tx.commit()
        .await
        .map_err(|error| MoveDrainError(error.to_string()))?;
    for effect in &effects {
        let DrainEffect::SendAssign { runner_id, message } = effect;
        send_to_runner(pubsub, *runner_id, message)
            .await
            .map_err(|error| MoveDrainError(error.to_string()))?;
    }
    if !effects.is_empty() {
        tracing::info!("{}", drain_pod_log(&pod_id, effects.len()));
    }
    Ok(())
}
