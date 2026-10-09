#![forbid(unsafe_code)]

//! API-side D-12 creation runtime: factory + post-commit drain (PIDASHCONV-743).
//!
//! The D-12 creation drivers (`orchestration::creation`) run against the
//! pool [`CreationSeam`][pidash_services::orchestration::creation::CreationSeam]
//! and [`FinalizeAgentRunSeam`][pidash_services::orchestration::creation::FinalizeAgentRunSeam]
//! twin that lives in `project_move_handoff` (the api-side twin of
//! `jobs::LiveCreationStore`). Handlers that create runs inside their
//! own transaction span — the D-26 move (`app_issues`) now, the D-18
//! move later — consume the twin through this module instead of
//! copying it:
//!
//! - [`SpanCreationDeps::from_state`] builds the runtime facts
//!   (executor settings, admission cache, clock) from the request's
//!   [`AppState`].
//! - [`handoff_store`] lends the twin a caller-owned span
//!   transaction; [`split_outboxes`] takes the span back plus the
//!   collected [`CreationOutboxes`].
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

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_db::tx::Transaction as DbTransaction;
use pidash_jobs::dispatch::{dispatch_agent_run, DispatchDecision};
use pidash_services::dispatch::{consume_admission_token, DeferredConsume};
use pidash_services::runner_runs::finalization as finalize_kernel;

use crate::project_move_handoff::{drain_publish_effects, HandoffStore, RedisAdmissionCache};
use crate::runner_enroll::teardown::redis_client;
use crate::state::AppState;

/// The creation runtime facts a span store is built from: executor
/// settings, the admission cache, and the unix clock for the
/// capacity/admission verdicts.
#[derive(Clone)]
pub(crate) struct SpanCreationDeps {
    pub(crate) cloud: CloudAgentSettings,
    pub(crate) managed: ManagedRunnerSettings,
    pub(crate) cache: RedisAdmissionCache,
    pub(crate) now_unix_secs: i64,
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

/// Lend the twin seam a caller-owned span transaction. The twin runs
/// the D-12 driver on the span (it sees the caller's uncommitted
/// writes); the caller takes the span back through
/// [`split_outboxes`] — on driver failure too, so its own abort still
/// rolls the span back.
pub(crate) fn handoff_store<'t, 'p>(
    tx: DbTransaction<'t>,
    pool: &'p PgPool,
    deps: &SpanCreationDeps,
) -> HandoffStore<'t, 'p> {
    HandoffStore::new(
        tx,
        pool,
        deps.cloud.clone(),
        deps.managed.clone(),
        deps.cache.clone(),
        deps.now_unix_secs,
    )
}

/// Take the span transaction back plus the collected outboxes.
pub(crate) fn split_outboxes<'t>(
    store: HandoffStore<'t, '_>,
) -> (DbTransaction<'t>, CreationOutboxes) {
    let (tx, dispatches, deferred, terminal_effects, cache) = store.into_parts();
    (
        tx,
        CreationOutboxes {
            dispatches,
            deferred,
            terminal_effects,
            cache,
        },
    )
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
/// caller's own D-14 pod-drain recipe.
pub(crate) async fn drain_creation_outboxes<F, Fut, E>(
    pool: &PgPool,
    state: &AppState,
    outboxes: CreationOutboxes,
    drain_pod: F,
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
        drain_publish_effects(pool, state, pairs).await;
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
