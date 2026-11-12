//! What every operation runs with: the pool, the ORM connection, and
//! the hooks the domain writes call.
//!
//! The hooks are the same object-safe traits the v1 drivers register on
//! the domain [`HookRegistry`](crate::domain::HookRegistry)
//! ([`IssueTransitionHook`], [`TaskEnqueueHook`]); the binary builds the
//! `Arc`s once (the public `pidash_jobs::hooks` implementations over
//! the binary's deps) and shares them with every operation.
//! Operations that need more (the move's post-commit drain and creation
//! deps) take it as extra parameters.

use std::sync::Arc;

use sqlx::PgPool;

use crate::domain::activity::TaskEnqueueHook;
use crate::domain::transition::IssueTransitionHook;

/// The shared operation dependencies.
#[derive(Clone)]
pub struct Deps {
    /// Reads, probes and row locks run on the pool; the domain write
    /// itself runs on [`Deps::orm`].
    pub pool: PgPool,
    /// The SeaORM connection the [`domain`](crate::domain) writes run on
    /// (built from the primary pool; opens no connections of its own).
    pub orm: sea_orm::DatabaseConnection,
    /// The post-save orchestration (completion comment, transition fire).
    pub transition: Arc<dyn IssueTransitionHook>,
    /// Best-effort task enqueue (activity, webhooks, crawls). Infallible:
    /// failures warn inside the implementation and the response stands,
    /// exactly like the handler-local `enqueue_best_effort` twins the
    /// operations replace.
    pub enqueue: Arc<dyn TaskEnqueueHook>,
}

impl Deps {
    /// Build the shared deps over caller-provided hooks. Callers that
    /// want OSS's own hooks (exactly the side effects the v1 endpoint
    /// gets) do not call this directly: they build the public
    /// `pidash_jobs::hooks::HookDeps` bundle once and call its
    /// `operation_deps`, which installs the OSS hooks through this
    /// constructor. The wiring cannot live here (services cannot name
    /// the hook implementations — the crate graph is `services` below
    /// `jobs`) and the hook implementations cannot live here either
    /// (they need the jobs queue and transition core).
    pub fn live(
        pool: PgPool,
        orm: sea_orm::DatabaseConnection,
        transition: Arc<dyn IssueTransitionHook>,
        enqueue: Arc<dyn TaskEnqueueHook>,
    ) -> Self {
        Self {
            pool,
            orm,
            transition,
            enqueue,
        }
    }
}
