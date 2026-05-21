//! Per-runner outbound transport handle.

use anyhow::{Result, anyhow};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::cloud::http::RunnerCloudClient;
use crate::cloud::protocol::{ClientMsg, Envelope};

#[derive(Clone)]
pub struct RunnerOut {
    runner_id: Uuid,
    inner: RunnerOutInner,
}

#[derive(Clone)]
enum RunnerOutInner {
    Ws(mpsc::Sender<Envelope<ClientMsg>>),
    Http(RunnerCloudClient),
    Offline,
}

impl RunnerOut {
    pub fn new(runner_id: Uuid, inner: mpsc::Sender<Envelope<ClientMsg>>) -> Self {
        Self {
            runner_id,
            inner: RunnerOutInner::Ws(inner),
        }
    }

    pub fn new_http(client: RunnerCloudClient) -> Self {
        Self {
            runner_id: client.runner_id(),
            inner: RunnerOutInner::Http(client),
        }
    }

    pub fn offline(runner_id: Uuid) -> Self {
        Self {
            runner_id,
            inner: RunnerOutInner::Offline,
        }
    }

    pub fn runner_id(&self) -> Uuid {
        self.runner_id
    }

    pub async fn send(&self, body: ClientMsg) -> Result<()> {
        let env = Envelope::for_runner(self.runner_id, body);
        match &self.inner {
            RunnerOutInner::Ws(tx) => tx
                .send(env)
                .await
                .map_err(|e| anyhow!("cloud sender dropped: {e}")),
            RunnerOutInner::Http(client) => {
                client.dispatch_client_msg(env).await.map_err(Into::into)
            }
            RunnerOutInner::Offline => Ok(()),
        }
    }

    pub async fn send_connection_scoped(&self, body: ClientMsg) -> Result<()> {
        match &self.inner {
            RunnerOutInner::Ws(tx) => tx
                .send(Envelope::new(body))
                .await
                .map_err(|e| anyhow!("cloud sender dropped: {e}")),
            RunnerOutInner::Http(_) => Err(anyhow!(
                "connection-scoped frames are unsupported on HTTP transport"
            )),
            RunnerOutInner::Offline => Ok(()),
        }
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
