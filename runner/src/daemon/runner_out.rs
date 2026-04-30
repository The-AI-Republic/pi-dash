//! Per-runner outbound channel handle.
//!
//! Wraps the daemon's shared `mpsc::Sender<Envelope<ClientMsg>>` with a
//! baked-in `runner_id`. Every frame sent through `RunnerOut::send` is
//! automatically tagged with that id via `Envelope::for_runner`, so call
//! sites can't accidentally emit a connection-scoped frame when they
//! meant a per-runner one (or vice versa).
//!
//! With one runner per daemon today, this only ensures a stable
//! `rid` field on outbound traffic. It compounds when multi-runner lands:
//! each `RunnerInstance` carries its own `RunnerOut` cloned off the
//! shared `out_tx`, and the cloud's demux uses the field to route
//! responses to the right server-side runner record. See `design.md` §6.3.

use tokio::sync::mpsc;
use uuid::Uuid;

use crate::cloud::protocol::{ClientMsg, Envelope};

#[derive(Clone, Debug)]
pub struct RunnerOut {
    runner_id: Uuid,
    inner: mpsc::Sender<Envelope<ClientMsg>>,
}

impl RunnerOut {
    pub fn new(runner_id: Uuid, inner: mpsc::Sender<Envelope<ClientMsg>>) -> Self {
        Self { runner_id, inner }
    }

    pub fn runner_id(&self) -> Uuid {
        self.runner_id
    }

    /// Send a frame tagged with this runner's id. Returns `Err` if the
    /// receiver has been dropped (e.g. the cloud loop exited and the
    /// channel buffer drained); callers typically ignore the error and
    /// rely on the supervisor's shutdown path.
    pub async fn send(
        &self,
        body: ClientMsg,
    ) -> Result<(), mpsc::error::SendError<Envelope<ClientMsg>>> {
        self.inner
            .send(Envelope::for_runner(self.runner_id, body))
            .await
    }

    /// Escape hatch for callers that need to send a *connection-scoped*
    /// frame (no runner_id) over this same channel — e.g. the supervisor
    /// emitting `Bye { reason: "shutdown" }` when the daemon is exiting.
    /// Most call sites should prefer `send`.
    pub async fn send_connection_scoped(
        &self,
        body: ClientMsg,
    ) -> Result<(), mpsc::error::SendError<Envelope<ClientMsg>>> {
        self.inner.send(Envelope::new(body)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::protocol::RunnerStatus;
    use chrono::Utc;

    #[tokio::test]
    async fn send_tags_envelope_with_runner_id() {
        let id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(4);
        let out = RunnerOut::new(id, tx);
        out.send(ClientMsg::Heartbeat {
            ts: Utc::now(),
            status: RunnerStatus::Idle,
            in_flight_run: None,
        })
        .await
        .unwrap();
        let env = rx.recv().await.unwrap();
        assert_eq!(env.runner_id, Some(id));
    }

    #[tokio::test]
    async fn send_connection_scoped_does_not_tag() {
        let id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(4);
        let out = RunnerOut::new(id, tx);
        out.send_connection_scoped(ClientMsg::Bye {
            reason: "shutdown".into(),
        })
        .await
        .unwrap();
        let env = rx.recv().await.unwrap();
        assert_eq!(env.runner_id, None);
    }
}
