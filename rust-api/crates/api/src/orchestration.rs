#![forbid(unsafe_code)]

//! API-side D-12 creation runtime: factory + post-commit drain (PIDASHCONV-743).
//!
//! The D-12 creation drivers (`orchestration::creation`) run against the
//! pool [`CreationSeam`][pidash_services::orchestration::creation::CreationSeam]
//! and [`FinalizeAgentRunSeam`][pidash_services::orchestration::creation::FinalizeAgentRunSeam]
//! twin that lives in `project_move_handoff` (the api-side twin of
//! `jobs::LiveCreationStore`). The move domain service creates handoff
//! runs inside its own transaction span; its
//! [`crate::domain_hooks::AppMovePostCommitHook`] drains the collected
//! [`CreationOutboxes`] through this module instead of copying the
//! drain:
//!
//! - [`SpanCreationDeps::from_state`] builds the runtime facts
//!   (executor settings, admission cache, clock) from the request's
//!   [`AppState`].
//! - [`drain_creation_outboxes`] runs the outboxes after the caller's
//!   span commits, in the `complete_project_move_handoff` order: terminal publish
//!   pairs, dispatches, deferred admission consumes. A dispatch is
//!   `dispatch_agent_run` per id; a [`DispatchDecision::PodDrain`]
//!   drains through the caller's own pod-drain recipe (Python drains
//!   the target pod inline, `dispatch.py:61-70` — the decision is
//!   acted on, never dropped).
//!
//! The caller drains creation outboxes before its own post-commit
//! work: the creation registers its `on_commit` entries first (the
//! move's dispatch precedes its cancel send and pod drains,
//! `issue_move.py:353-372`).

use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings, RunnerSettings};
use pidash_db::runner_sessions::machine_outbox::{self, MachineOutboxError};
use pidash_db::runner_sessions::models::runner_session;
use pidash_db::runner_sessions::outbox as runner_outbox;
use pidash_db::runner_sessions::outbox::OutboxError;
use pidash_db::runner_sessions::RunnerSession;
use pidash_jobs::dispatch::{dispatch_agent_run, DispatchDecision};
use pidash_services::dispatch::{consume_admission_token, DeferredConsume};
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

use crate::project_move_handoff::drain_publish_effects;
use pidash_services::dispatch::admission::RedisAdmissionCache;
use crate::runner_enroll::teardown::redis_client;
use crate::state::AppState;
use pidash_jobs::orchestration::HandoffStore;

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
                client: redis_client(state),
            },
            now_unix_secs: unix_now_secs(),
            toolsets: state.toolsets().clone(),
            assistant_seam: state.assistant_seam().clone(),
        }
    }
}

/// The creation seam's post-commit outboxes: dispatch ids (collected
/// by `dispatch_after_commit`), deferred admission consumes, and
/// finalized-run ids awaiting their terminal publish pairs. The
/// admission cache rides along so the deferred drain needs no
/// rebuild.
pub(crate) struct CreationOutboxes {
    pub(crate) dispatches: Vec<Uuid>,
    pub(crate) deferred: Vec<DeferredConsume>,
    pub(crate) terminal_effects: Vec<Uuid>,
    pub(crate) cache: RedisAdmissionCache,
}

/// Why the creation drain failed. The caller's span already
/// committed — the caller answers 500 with its writes standing (the
/// enqueue precedent).
#[derive(Debug)]
pub(crate) enum CreationDrainError<E> {
    /// `dispatch_agent_run` failed.
    Dispatch(sqlx::Error),
    /// The caller's target-pod drain failed.
    PodDrain(E),
}

/// Drain the creation outboxes after the caller's span commits, in
/// the `complete_project_move_handoff` order: terminal publish pairs (isolated each,
/// infallible), dispatches (`dispatch_agent_run` per id, failures
/// propagate), deferred admission consumes (best-effort). A
/// [`DispatchDecision::PodDrain`] drains through `drain_pod`, the
/// caller's own D-14 pod-drain recipe. `terminal_created_by` is the
/// ambient crum request user for the inline terminal effects.
pub(crate) async fn drain_creation_outboxes<F, Fut, E>(
    pool: &PgPool,
    state: &AppState,
    outboxes: CreationOutboxes,
    drain_pod: F,
    terminal_created_by: Option<Uuid>,
) -> Result<(), CreationDrainError<E>>
where
    F: Fn(Uuid) -> Fut,
    Fut: Future<Output = Result<(), E>>,
{
    if !outboxes.terminal_effects.is_empty() {
        let mut pairs = Vec::with_capacity(outboxes.terminal_effects.len() * 2);
        for id in outboxes.terminal_effects {
            pairs.extend(finalize_kernel::plan_publish_effects(id));
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
    let cloud = state.settings().cloud_agent.clone();
    let now = now_micros();
    for id in outboxes.dispatches {
        match dispatch_agent_run(pool, &cloud, id, now).await {
            Ok(DispatchDecision::PodDrain { pod_id }) => {
                drain_pod(pod_id)
                    .await
                    .map_err(CreationDrainError::PodDrain)?;
            }
            Ok(_) => {}
            Err(error) => return Err(CreationDrainError::Dispatch(error)),
        }
    }
    for consume in &outboxes.deferred {
        consume_admission_token(&outboxes.cache, consume);
    }
    Ok(())
}

/// `timezone.now()` truncated to microseconds (the `project_move_handoff`
/// `now_micros` position).
fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).expect("micros in range")
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
