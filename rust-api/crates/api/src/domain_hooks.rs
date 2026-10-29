#![forbid(unsafe_code)]

//! The api binary's domain-write hooks (ORM-03, PIDASHCONV-1521).
//!
//! [`AppTaskEnqueueHook`] is the [`TaskEnqueueHook`][pidash_services::domain::activity::TaskEnqueueHook]
//! binding over `pidash_jobs::queue::enqueue`. [`AppMovePostCommitHook`] is the
//! [`MovePostCommitHook`][pidash_services::domain::issues_move::MovePostCommitHook]
//! binding over the move's drains, and [`AppMoveAdmissionCache`] the
//! [`MoveAdmissionCache`][pidash_services::domain::issues_move::MoveAdmissionCache]
//! binding over the binary's admission cache. Handlers that drive domain
//! writes register them on the [`WriteTxn`][pidash_services::domain::WriteTxn]
//! hook registry so domain chains can reach the binary's pool, pubsub and
//! runner settings without depending on the jobs crate or the api crate.

use std::future::Future;
use std::pin::Pin;

use pidash_db::redis::client_from_settings;
use pidash_jobs::queue::{self, NewJob};
use pidash_jobs::TaskMessage;
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::issues_move::{
    MoveAdmissionCache, MoveCreationOutboxes, MoveDrainError, MovePostCommitHook,
};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::orchestration::{
    drain_creation_outboxes, drain_pod_by_id, CreationDrainError, CreationOutboxes, MovePubsub,
};
use crate::project_move_handoff::RedisAdmissionCache;
use crate::state::AppState;

/// `task.delay(*args, **kwargs)` through the jobs kernel: unported tasks
/// forward Celery-format, like every other port. Enqueue failures log and
/// the response stands (Django's broker publish is equally fire-and-forget
/// from the view's perspective). Moved verbatim from the app-issue
/// handlers' local `enqueue_task`.
pub struct AppTaskEnqueueHook {
    pool: PgPool,
}

impl AppTaskEnqueueHook {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}

impl TaskEnqueueHook for AppTaskEnqueueHook {
    fn enqueue_task(
        &self,
        task: &str,
        args: Vec<Value>,
        kwargs: Map<String, Value>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let pool = self.pool.clone();
        let message = TaskMessage::new(task, args, kwargs);
        Box::pin(async move {
            let job = NewJob::new(
                message.task.clone(),
                Value::Array(message.args.clone()),
                Value::Object(message.kwargs.clone()),
            );
            if let Err(error) = queue::enqueue(&pool, &job).await {
                tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
            }
        })
    }
}

/// The move's post-commit drains (ORM-03, PIDASHCONV-1521 part D): the
/// handoff-creation drain, the source-pod drain, and the cancel send,
/// over the binary's pool, state and admission cache. Both move drivers
/// register one; the move chain calls it from its after-commit step, in
/// the drivers' old order.
pub struct AppMovePostCommitHook {
    pool: PgPool,
    state: AppState,
    cache: RedisAdmissionCache,
}

impl AppMovePostCommitHook {
    pub fn new(pool: &PgPool, state: &AppState, cache: RedisAdmissionCache) -> Self {
        Self {
            pool: pool.clone(),
            state: state.clone(),
            cache,
        }
    }

    fn pubsub<'a>(
        pool: &'a PgPool,
        redis: Option<&'a redis::Client>,
        state: &'a AppState,
    ) -> MovePubsub<'a> {
        MovePubsub {
            pool,
            redis,
            runner: &state.settings().runner,
        }
    }
}

impl MovePostCommitHook for AppMovePostCommitHook {
    fn drain_creation(
        &self,
        outboxes: MoveCreationOutboxes,
        terminal_created_by: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = Result<(), MoveDrainError>> + Send + '_>> {
        let pool = self.pool.clone();
        let state = self.state.clone();
        let outboxes = CreationOutboxes {
            dispatches: outboxes.dispatches,
            deferred: outboxes.deferred,
            terminal_effects: outboxes.terminal_effects,
            cache: self.cache.clone(),
        };
        Box::pin(async move {
            let redis = client_from_settings(state.settings());
            let pubsub = Self::pubsub(&pool, redis.as_ref(), &state);
            drain_creation_outboxes(
                &pool,
                &state,
                outboxes,
                |pod_id| drain_pod_by_id(&pool, &pubsub, pod_id),
                terminal_created_by,
            )
            .await
            .map_err(|error| match error {
                CreationDrainError::Dispatch(error) => {
                    MoveDrainError(format!("move creation dispatch failed: {error}"))
                }
                CreationDrainError::PodDrain(error) => {
                    MoveDrainError(format!("move creation pod drain failed: {error}"))
                }
            })
        })
    }

    fn drain_pod(
        &self,
        pod_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), MoveDrainError>> + Send + '_>> {
        let pool = self.pool.clone();
        let state = self.state.clone();
        Box::pin(async move {
            let redis = client_from_settings(state.settings());
            let pubsub = Self::pubsub(&pool, redis.as_ref(), &state);
            drain_pod_by_id(&pool, &pubsub, pod_id).await
        })
    }

    fn send_cancel(
        &self,
        runner_id: Uuid,
        run_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let pool = self.pool.clone();
        let state = self.state.clone();
        Box::pin(async move {
            let redis = client_from_settings(state.settings());
            let pubsub = Self::pubsub(&pool, redis.as_ref(), &state);
            let outcome =
                pidash_services::app_issues::send_project_move_cancel(&pubsub, runner_id, run_id)
                    .await;
            for warning in outcome.warnings {
                tracing::warn!("{warning}");
            }
        })
    }
}

/// The admission cache over the binary's Redis cache: the two reads the
/// execution resolve needs, with string errors. Both move drivers pass
/// one through their creation facts.
#[derive(Clone)]
pub struct AppMoveAdmissionCache {
    cache: RedisAdmissionCache,
}

impl AppMoveAdmissionCache {
    pub fn new(cache: RedisAdmissionCache) -> Self {
        Self { cache }
    }
}

impl MoveAdmissionCache for AppMoveAdmissionCache {
    fn bucket_count(&self, key: &str) -> Result<Option<i64>, String> {
        use pidash_services::dispatch::AdmissionCache;
        self.cache
            .bucket_count(key)
            .map_err(|error| error.to_string())
    }

    fn add_or_incr(&self, key: &str, timeout_secs: i64) -> Result<(), String> {
        use pidash_services::dispatch::AdmissionCache;
        self.cache
            .add_or_incr(key, timeout_secs)
            .map_err(|error| error.to_string())
    }
}
