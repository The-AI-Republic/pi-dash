use anyhow::{Context, Result};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;

use crate::codex::jsonrpc::Incoming;

/// Handles the `codex app-server` subprocess lifecycle + JSON-RPC wire.
pub struct AppServer {
    child: Child,
    stdin: ChildStdin,
    next_id: AtomicU64,
    pub inbound: mpsc::Receiver<Incoming>,
}

impl AppServer {
    pub async fn spawn(binary: &str, cwd: &Path) -> Result<Self> {
        let mut cmd = Command::new(binary);
        cmd.arg("app-server").current_dir(cwd);
        Self::spawn_command(cmd).await
    }

    /// Test-friendly constructor that takes a fully-prepared Command. The
    /// stdio disposition (piped) and `kill_on_drop` are forced on.
    pub async fn spawn_command(mut cmd: Command) -> Result<Self> {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().context("spawning codex subprocess")?;
        let stdin = child.stdin.take().context("codex stdin missing")?;
        let stdout = child.stdout.take().context("codex stdout missing")?;
        let stderr = child.stderr.take().context("codex stderr missing")?;

        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(read_frames(stdout, tx.clone()));
        tokio::spawn(drain_stderr(stderr));
        Ok(Self {
            child,
            stdin,
            next_id: AtomicU64::new(1),
            inbound: rx,
        })
    }

    pub fn alloc_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub async fn send_raw(&mut self, line: &str) -> Result<()> {
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    pub async fn shutdown(mut self, grace: std::time::Duration) -> Result<()> {
        drop(self.stdin);
        match tokio::time::timeout(grace, self.child.wait()).await {
            Ok(Ok(status)) => {
                tracing::debug!(?status, "codex exited gracefully");
            }
            _ => {
                tracing::warn!("codex did not exit within grace; sending SIGKILL");
                self.child.start_kill().ok();
                self.child.wait().await.ok();
            }
        }
        Ok(())
    }
}

async fn read_frames(stdout: tokio::process::ChildStdout, tx: mpsc::Sender<Incoming>) {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_str::<Incoming>(trimmed) {
                    Ok(frame) => {
                        if tx.send(frame).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("codex emitted non-JSON line ({e}): {trimmed}");
                    }
                }
            }
            Err(e) => {
                tracing::warn!("codex stdout read error: {e}");
                break;
            }
        }
    }
}

async fn drain_stderr(stderr: tokio::process::ChildStderr) {
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => tracing::debug!(target: "codex.stderr", "{}", line.trim_end()),
            Err(e) => {
                tracing::warn!("codex stderr read error: {e}");
                break;
            }
        }
    }
}
