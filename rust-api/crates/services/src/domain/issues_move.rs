//! Move a work item to another project: the domain entry point.
//!
//! [`move_to_project`] runs the shared [`move_work_item_to_project`]
//! kernel ([`crate::app_issues::issue_move`], `utils/issue_move.py:122-393`)
//! on the [`WriteTxn`]'s SeaORM transaction — guards, span and refetch
//! all on the one transaction — and queues the post-commit chain (the
//! handoff-creation drain, the cancel send, the pod drains, the
//! completion comment, the transition fire, the two activity enqueues)
//! onto the [`WriteTxn`] queue, in today's driver order. Both view
//! drivers (app + v1) call it with [`MoveOptions::django`]; the options
//! exist for callers the two views do not serve (downstream overlays).
//!
//! [`move_work_item_to_project`]: crate::app_issues::issue_move::move_work_item_to_project

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use uuid::Uuid;

use crate::dispatch::DeferredConsume;

/// The handoff-creation seam's post-commit outboxes, minus the admission
/// cache: dispatch ids (collected by `dispatch_after_commit`), deferred
/// admission consumes, and finalized-run ids awaiting their terminal
/// publish pairs. The api-side [`MovePostCommitHook`] implementation
/// supplies its own cache clone when it drains these through the
/// existing `drain_creation_outboxes`.
#[derive(Debug, Clone, Default)]
pub struct MoveCreationOutboxes {
    pub dispatches: Vec<Uuid>,
    pub deferred: Vec<DeferredConsume>,
    pub terminal_effects: Vec<Uuid>,
}

/// Why a post-commit drain leg failed. The hook implementation logs the
/// specific line (preserving today's driver warnings) and returns this
/// as the leg's failure; the chain records the stop and the driver
/// answers `ServerError` with the move standing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveDrainError(pub String);

impl std::fmt::Display for MoveDrainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for MoveDrainError {}

/// The move's post-commit drains as an object-safe hook (the
/// [`TaskEnqueueHook`](super::activity::TaskEnqueueHook) precedent:
/// boxed futures, no generics). The drains need the binary's pool,
/// pubsub and runner settings, so the api binary implements this over
/// its existing drain functions, the driver registers it on the
/// [`HookRegistry`](super::HookRegistry), and the move chain calls it
/// from its after-commit step.
pub trait MovePostCommitHook: Send + Sync {
    /// Drain the handoff-creation outboxes (the driver's
    /// `take_creation_outboxes` block): terminal publishes (best-effort),
    /// then one `dispatch_agent_run` per id with its pod drain.
    /// Fallible — [`ChainStop::CreationDrain`].
    ///
    /// [`ChainStop::CreationDrain`]: super::issues::ChainStop::CreationDrain
    fn drain_creation(
        &self,
        outboxes: MoveCreationOutboxes,
        terminal_created_by: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = Result<(), MoveDrainError>> + Send + '_>>;

    /// Drain one source pod (`MovePostCommit::DrainPod`). Fallible —
    /// [`ChainStop::PodDrain`].
    ///
    /// [`ChainStop::PodDrain`]: super::issues::ChainStop::PodDrain
    fn drain_pod(
        &self,
        pod_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), MoveDrainError>> + Send + '_>>;

    /// Send one cancel frame (`MovePostCommit::SendCancel`): best-effort
    /// with logged warnings, never fails (Django's
    /// `_send_project_move_cancel`, `:70-107`, never raises).
    fn send_cancel(
        &self,
        runner_id: Uuid,
        run_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// The hook the driver registered, for the move chain legs.
pub(crate) fn drain_hook(txn: &super::txn::WriteTxn) -> Arc<dyn MovePostCommitHook> {
    txn.hooks()
        .get::<dyn MovePostCommitHook>()
        .expect("driver registered MovePostCommitHook")
}
