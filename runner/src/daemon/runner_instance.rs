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

use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::approval::router::ApprovalRouter;
use crate::cloud::http::{AckEntry, InboundEnvelope, RunnerCloudClient};
use crate::cloud::protocol::Envelope;
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
    pub client: Option<RunnerCloudClient>,
    pub out: RunnerOut,
    pub mailbox_tx: mpsc::Sender<InboundEnvelope>,
    pub mailbox_rx: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<InboundEnvelope>>>>,
    pub ack_tx: mpsc::UnboundedSender<AckEntry>,
    pub ack_rx: Arc<tokio::sync::Mutex<Option<mpsc::UnboundedReceiver<AckEntry>>>>,
    /// Latched when this instance is being torn down (today: only on
    /// `ServerMsg::RemoveRunner`). Background tasks subscribe and exit
    /// even if they weren't parked at the exact moment the signal was
    /// sent, preventing zombie heartbeats after per-runner teardown.
    pub remove_tx: watch::Sender<bool>,
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
        Self::new_with_out(config, paths, RunnerOut::new(runner_id, out_tx), None)
    }

    pub fn new_http(config: RunnerConfig, paths: &Paths, client: RunnerCloudClient) -> Self {
        Self::new_with_out(
            config,
            paths,
            RunnerOut::new_http(client.clone()),
            Some(client),
        )
    }

    pub fn new_offline(config: RunnerConfig, paths: &Paths) -> Self {
        let runner_id = config.runner_id;
        Self::new_with_out(config, paths, RunnerOut::offline(runner_id), None)
    }

    fn new_with_out(
        config: RunnerConfig,
        paths: &Paths,
        out: RunnerOut,
        client: Option<RunnerCloudClient>,
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
            cli: None,
        });
        let approvals = ApprovalRouter::new();
        let (mailbox_tx, mailbox_rx) = mpsc::channel(INSTANCE_MAILBOX_DEPTH);
        let (ack_tx, ack_rx) = mpsc::unbounded_channel();
        let (remove_tx, _remove_rx) = watch::channel(false);
        Self {
            runner_id,
            name,
            config,
            paths: runner_paths,
            state,
            approvals,
            client,
            out,
            mailbox_tx,
            mailbox_rx: Arc::new(tokio::sync::Mutex::new(Some(mailbox_rx))),
            ack_tx,
            ack_rx: Arc::new(tokio::sync::Mutex::new(Some(ack_rx))),
            remove_tx,
        }
    }

    /// Take the mailbox receiver. Called once by the supervisor when it
    /// spawns this instance's `RunnerLoop`. Subsequent calls return None
    /// — the receiver is single-consumer.
    pub async fn take_mailbox_rx(&self) -> Option<mpsc::Receiver<InboundEnvelope>> {
        self.mailbox_rx.lock().await.take()
    }

    pub async fn take_ack_rx(&self) -> Option<mpsc::UnboundedReceiver<AckEntry>> {
        self.ack_rx.lock().await.take()
    }
}
