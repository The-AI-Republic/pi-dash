use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use http::Request;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use crate::cloud::protocol::{ClientMsg, Envelope, RunnerStatus, ServerMsg};
use crate::config::schema::Credentials;
use crate::util::backoff::Backoff;

pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Connects once to the cloud and runs the read + write loops until the
/// WS closes or an error propagates. Higher-level reconnect behavior is
/// owned by [`ConnectionLoop`].
pub struct Connection {
    pub stream: WsStream,
}

impl Connection {
    pub async fn open(cloud_url: &str, creds: &Credentials) -> Result<Self> {
        let ws_url = http_to_ws(cloud_url)?;
        let full = format!("{}/ws/runner/", ws_url.trim_end_matches('/'));
        let req = Request::builder()
            .method("GET")
            .uri(&full)
            .header("Host", host_of(&full).unwrap_or_default())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .header("Authorization", format!("Bearer {}", creds.runner_secret))
            .header("X-Runner-Id", creds.runner_id.to_string())
            .header("X-Runner-Protocol", crate::PROTOCOL_VERSION.to_string())
            .body(())
            .context("building WS upgrade request")?;
        let (stream, resp) = tokio::time::timeout(
            Duration::from_secs(15),
            tokio_tungstenite::connect_async(req),
        )
        .await
        .context("timed out opening WS to cloud")?
        .with_context(|| format!("connect_async to {full}"))?;
        tracing::debug!(status = %resp.status(), "cloud WS handshake complete");
        Ok(Self { stream })
    }
}

pub struct ConnectionHandle {
    pub tx: mpsc::Sender<Envelope<ClientMsg>>,
    pub rx: mpsc::Receiver<Envelope<ServerMsg>>,
}

/// Spawns read + write tasks backing a connection. The caller owns reconnect
/// decisions — this function returns when the connection closes or errors.
pub async fn run_connection(
    mut conn: Connection,
    mut outbound: mpsc::Receiver<Envelope<ClientMsg>>,
    inbound: mpsc::Sender<Envelope<ServerMsg>>,
) -> Result<()> {
    loop {
        tokio::select! {
            msg = outbound.recv() => {
                match msg {
                    Some(frame) => {
                        let text = serde_json::to_string(&frame)?;
                        conn.stream.send(Message::Text(text.into())).await?;
                    }
                    None => {
                        let _ = conn.stream.close(None).await;
                        break;
                    }
                }
            }
            ws = conn.stream.next() => {
                match ws {
                    Some(Ok(Message::Text(t))) => {
                        match serde_json::from_str::<Envelope<ServerMsg>>(&t) {
                            Ok(env) => {
                                if inbound.send(env).await.is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::warn!("bad frame from cloud: {e}");
                            }
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        conn.stream.send(Message::Pong(p)).await.ok();
                    }
                    Some(Ok(Message::Close(_))) => {
                        tracing::info!("cloud closed WS");
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        tracing::warn!("WS error: {e}");
                        return Err(e.into());
                    }
                    None => break,
                }
            }
        }
    }
    Ok(())
}

/// Holds the long-lived reconnect loop. Messages sent on the `outbound` channel
/// are delivered on the current WS; messages received from the WS are pushed
/// onto the `inbound` channel. Reconnects are transparent.
pub struct ConnectionLoop {
    pub cloud_url: String,
    pub creds: Credentials,
    pub outbound: mpsc::Receiver<Envelope<ClientMsg>>,
    pub inbound: mpsc::Sender<Envelope<ServerMsg>>,
    pub status_snapshot: tokio::sync::watch::Receiver<RunnerStatus>,
    pub in_flight: tokio::sync::watch::Receiver<Option<Uuid>>,
}

impl ConnectionLoop {
    pub async fn run(mut self) -> Result<()> {
        let mut backoff = Backoff::new();
        loop {
            match Connection::open(&self.cloud_url, &self.creds).await {
                Ok(conn) => {
                    backoff.reset();
                    // Send Hello immediately.
                    let hello = ClientMsg::Hello {
                        runner_id: self.creds.runner_id,
                        version: crate::RUNNER_VERSION.to_string(),
                        os: std::env::consts::OS.to_string(),
                        arch: std::env::consts::ARCH.to_string(),
                        status: *self.status_snapshot.borrow(),
                        in_flight_run: *self.in_flight.borrow(),
                        protocol_version: crate::PROTOCOL_VERSION,
                    };
                    let (tx_frame, rx_frame) = mpsc::channel(64);
                    tx_frame.send(Envelope::new(hello)).await.ok();
                    let forward = {
                        let tx_frame = tx_frame.clone();
                        let mut outbound_rx = std::mem::replace(
                            &mut self.outbound,
                            mpsc::channel::<Envelope<ClientMsg>>(1).1,
                        );
                        tokio::spawn(async move {
                            while let Some(m) = outbound_rx.recv().await {
                                if tx_frame.send(m).await.is_err() {
                                    break;
                                }
                            }
                            outbound_rx
                        })
                    };
                    let result = run_connection(conn, rx_frame, self.inbound.clone()).await;
                    // Reclaim the outbound receiver so we can keep forwarding on reconnect.
                    if let Ok(rx) = forward.await {
                        self.outbound = rx;
                    }
                    if let Err(e) = result {
                        tracing::warn!("cloud WS loop ended with error: {e:#}");
                    }
                }
                Err(e) => {
                    tracing::warn!("cloud WS connect failed: {e:#}");
                }
            }
            let delay = backoff.next_delay();
            tracing::info!("reconnecting in {:?}", delay);
            tokio::time::sleep(delay).await;
        }
    }
}

fn http_to_ws(url: &str) -> Result<String> {
    let lower = url.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("https://") {
        Ok(format!("wss://{rest}"))
    } else if let Some(rest) = lower.strip_prefix("http://") {
        Ok(format!("ws://{rest}"))
    } else if lower.starts_with("wss://") || lower.starts_with("ws://") {
        Ok(lower)
    } else {
        anyhow::bail!("invalid cloud URL scheme: {url}")
    }
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    rest.split('/').next().map(|h| h.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_to_ws_mapping() {
        assert_eq!(http_to_ws("https://x.test").unwrap(), "wss://x.test");
        assert_eq!(http_to_ws("http://y.test").unwrap(), "ws://y.test");
    }
}
