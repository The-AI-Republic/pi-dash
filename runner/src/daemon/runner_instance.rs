//! Per-runner state container.
//!
//! `RunnerInstance` bundles everything one logical runner needs while
//! the daemon hosts N of them: paths, config slice, approval router,
//! state handle, mailbox, and a `RunnerOut` for outbound frames. The
//! supervisor builds one per `config.runners` entry and the demux task
//! routes inbound frames to each instance's mailbox by `runner_id`.
//!
//! See `.ai_design/n_runners_in_same_machine/design.md` §6.2.

use std::sync::Arc;

use tokio::sync::mpsc;
use uuid::Uuid;

use crate::approval::router::ApprovalRouter;
use crate::cloud::protocol::{Envelope, ServerMsg};
use crate::config::schema::RunnerConfig;
use crate::daemon::runner_out::RunnerOut;
use crate::daemon::state::StateHandle;
use crate::util::paths::{Paths, RunnerPaths};

/// Inbound mailbox depth per instance. Sized big enough to absorb a
/// short burst of frames during a busy run without blocking the demux,
/// which is single-threaded and would otherwise stall every other
/// instance behind a slow consumer.
const INSTANCE_MAILBOX_DEPTH: usize = 64;

#[derive(Clone)]
pub struct RunnerInstance {
    pub runner_id: Uuid,
    pub name: String,
    pub config: RunnerConfig,
    pub paths: RunnerPaths,
    pub state: StateHandle,
    pub approvals: ApprovalRouter,
    pub out: RunnerOut,
    pub mailbox_tx: mpsc::Sender<Envelope<ServerMsg>>,
    pub mailbox_rx: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<Envelope<ServerMsg>>>>>,
    /// Notified by the `RunnerLoop` when this instance is being torn
    /// down (today: only on `ServerMsg::RemoveRunner`). Per-instance
    /// background tasks — heartbeat in particular — watch this and
    /// exit, preventing zombie traffic for a runner whose cloud-side
    /// row no longer exists. See `design.md` §11.4.
    pub remove_signal: Arc<tokio::sync::Notify>,
}

impl RunnerInstance {
    /// Build a `RunnerInstance` from its config slice and the daemon's
    /// shared out_tx. `state` is fresh (one StateHandle per instance);
    /// the daemon's old single-StateHandle gets replaced by N of them.
    pub fn new(
        config: RunnerConfig,
        paths: &Paths,
        out_tx: mpsc::Sender<Envelope<crate::cloud::protocol::ClientMsg>>,
    ) -> Self {
        let runner_id = config.runner_id;
        let name = config.name.clone();
        let runner_paths = paths.for_runner(runner_id);
        // Each instance gets its own StateHandle so per-runner status
        // (current_run, approvals_pending, etc.) doesn't bleed across
        // instances. The connection-level fields cached inside
        // (cloud_url, name) are still set from the daemon-level config.
        let state = StateHandle::new(crate::config::schema::Config {
            version: 2,
            daemon: Default::default(),
            runners: vec![config.clone()],
        });
        let approvals = ApprovalRouter::new();
        let out = RunnerOut::new(runner_id, out_tx);
        let (mailbox_tx, mailbox_rx) = mpsc::channel(INSTANCE_MAILBOX_DEPTH);
        Self {
            runner_id,
            name,
            config,
            paths: runner_paths,
            state,
            approvals,
            out,
            mailbox_tx,
            mailbox_rx: Arc::new(tokio::sync::Mutex::new(Some(mailbox_rx))),
            remove_signal: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Take the mailbox receiver. Called once by the supervisor when it
    /// spawns this instance's `RunnerLoop`. Subsequent calls return None
    /// — the receiver is single-consumer.
    pub async fn take_mailbox_rx(&self) -> Option<mpsc::Receiver<Envelope<ServerMsg>>> {
        self.mailbox_rx.lock().await.take()
    }
}
