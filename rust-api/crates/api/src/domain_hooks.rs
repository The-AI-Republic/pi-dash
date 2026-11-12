#![forbid(unsafe_code)]

//! The api binary's domain-write hooks (ORM-03, PIDASHCONV-1521).
//!
//! Thin same-signature wrappers over the public
//! [`hooks`][pidash_jobs::hooks] bindings: [`AppTaskEnqueueHook`] is the
//! outbox [`EnqueueHook`][pidash_jobs::hooks::EnqueueHook],
//! [`AppMovePostCommitHook`] the [`MoveHook`][pidash_jobs::hooks::MoveHook]
//! over this binary's inline terminal drain and move pod drain, and
//! [`AppMoveAdmissionCache`] the
//! [`LiveMoveAdmissionCache`][pidash_jobs::hooks::LiveMoveAdmissionCache].
//! Handlers that drive domain writes register them on the
//! [`WriteTxn`][pidash_services::domain::WriteTxn] hook registry so domain
//! chains can reach the binary's pool, pubsub and runner settings without
//! depending on the jobs crate or the api crate.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use pidash_db::redis::client_from_settings;
use pidash_jobs::hooks::{
    EnqueueHook, LiveMoveAdmissionCache, MoveHook, OutboxTaskSink, TerminalEffectsDrain,
};
use pidash_jobs::issue_transition::PodDrain;
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::issues_move::{
    MoveAdmissionCache, MoveCreationOutboxes, MoveDrainError, MovePostCommitHook,
};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::orchestration::{drain_pod_by_id, drain_terminal_effects, MovePubsub};
use crate::project_move_handoff::RedisAdmissionCache;
use crate::state::AppState;

/// `task.delay(*args, **kwargs)` through the jobs kernel: unported tasks
/// forward Celery-format, like every other port. Enqueue failures log and
/// the response stands (Django's broker publish is equally fire-and-forget
/// from the view's perspective).
pub struct AppTaskEnqueueHook {
    hook: EnqueueHook,
}

impl AppTaskEnqueueHook {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            hook: EnqueueHook::new(pool, Arc::new(OutboxTaskSink)),
        }
    }
}

impl TaskEnqueueHook for AppTaskEnqueueHook {
    fn enqueue_task(
        &self,
        task: &str,
        args: Vec<Value>,
        kwargs: Map<String, Value>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.hook.enqueue_task(task, args, kwargs)
    }
}

/// The api terminal-effects drain: the request applies finalized runs'
/// terminal pairs synchronously with the ambient user (the old
/// `drain_creation_outboxes` terminal leg).
struct ApiTerminalEffectsDrain {
    pool: PgPool,
    state: AppState,
}

impl TerminalEffectsDrain for ApiTerminalEffectsDrain {
    fn drain(
        &self,
        run_ids: &[Uuid],
        created_by: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let pool = self.pool.clone();
        let state = self.state.clone();
        let run_ids = run_ids.to_vec();
        Box::pin(async move {
            drain_terminal_effects(&pool, &state, &run_ids, created_by).await;
        })
    }
}

/// The api move pod drain over the move's executor. Failures warn with
/// the executor's detail (the old path formatted it into the drain
/// error the chain leg logged) and erase to `()`.
struct ApiMovePodDrain {
    pool: PgPool,
    state: AppState,
}

impl PodDrain for ApiMovePodDrain {
    fn drain_pod(&self, pod_id: Uuid) -> Pin<Box<dyn Future<Output = Result<(), ()>> + Send + '_>> {
        let pool = self.pool.clone();
        let state = self.state.clone();
        Box::pin(async move {
            let redis = client_from_settings(state.settings());
            let pubsub = MovePubsub {
                pool: &pool,
                redis: redis.as_ref(),
                runner: &state.settings().runner,
            };
            drain_pod_by_id(&pool, &pubsub, pod_id)
                .await
                .map_err(|error| {
                    tracing::warn!(%error, %pod_id, "move pod drain failed");
                })
        })
    }
}

/// The move's post-commit drains (ORM-03, PIDASHCONV-1521 part D): the
/// handoff-creation drain, the source-pod drain, and the cancel send,
/// over the binary's pool, state and admission cache. Both move drivers
/// register one; the move chain calls it from its after-commit step, in
/// the drivers' old order.
pub struct AppMovePostCommitHook {
    hook: MoveHook,
}

impl AppMovePostCommitHook {
    pub fn new(pool: &PgPool, state: &AppState, cache: RedisAdmissionCache) -> Self {
        let settings = state.settings();
        let terminal = Arc::new(ApiTerminalEffectsDrain {
            pool: pool.clone(),
            state: state.clone(),
        });
        let pods = Arc::new(ApiMovePodDrain {
            pool: pool.clone(),
            state: state.clone(),
        });
        Self {
            hook: MoveHook::new(
                pool,
                &settings.cloud_agent,
                cache,
                client_from_settings(settings),
                &settings.runner,
                terminal,
                pods,
            ),
        }
    }
}

impl MovePostCommitHook for AppMovePostCommitHook {
    fn drain_creation(
        &self,
        outboxes: MoveCreationOutboxes,
        terminal_created_by: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = Result<(), MoveDrainError>> + Send + '_>> {
        self.hook.drain_creation(outboxes, terminal_created_by)
    }

    fn drain_pod(
        &self,
        pod_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), MoveDrainError>> + Send + '_>> {
        self.hook.drain_pod(pod_id)
    }

    fn send_cancel(
        &self,
        runner_id: Uuid,
        run_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.hook.send_cancel(runner_id, run_id)
    }
}

/// The admission cache over the binary's Redis cache: the two reads the
/// execution resolve needs, with string errors. Both move drivers pass
/// one through their creation facts.
#[derive(Clone)]
pub struct AppMoveAdmissionCache {
    inner: LiveMoveAdmissionCache,
}

impl AppMoveAdmissionCache {
    pub fn new(cache: RedisAdmissionCache) -> Self {
        Self {
            inner: LiveMoveAdmissionCache::new(cache),
        }
    }
}

impl MoveAdmissionCache for AppMoveAdmissionCache {
    fn bucket_count(&self, key: &str) -> Result<Option<i64>, String> {
        self.inner.bucket_count(key)
    }

    fn add_or_incr(&self, key: &str, timeout_secs: i64) -> Result<(), String> {
        self.inner.add_or_incr(key, timeout_secs)
    }
}
