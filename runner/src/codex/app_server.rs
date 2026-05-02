use anyhow::{Context, Result};
use chrono::Utc;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, watch};

use crate::agent::{AgentProcessHandle, ExitSnapshot};
use crate::codex::jsonrpc::Incoming;
use crate::util::shell::{is_benign_login_shell_warning, login_shell_command};

/// Handles the `codex app-server` subprocess lifecycle + JSON-RPC wire.
///
/// `Child` is owned by an internal wait task spawned in `spawn_command`. The
/// `AppServer` holds only the side-channels needed to drive it: stdin for
/// outbound requests, `kill_tx` to request shutdown, and `exit_rx` to observe
/// exit. This split is required because `tokio::process::Child::wait()` and
/// `start_kill()` both take `&mut self`, so only one task can hold the child.
/// See `.ai_design/runner_agent_bridge/design.md` §4.4.
pub struct AppServer {
    pid: Option<u32>,
    stdin: ChildStdin,
    next_id: AtomicU64,
    pub inbound: mpsc::Receiver<Incoming>,
    kill_tx: mpsc::Sender<KillRequest>,
    exit_rx: watch::Receiver<Option<ExitSnapshot>>,
}

#[derive(Debug, Clone, Copy)]
enum KillRequest {
    /// Polite stdin-drop already happened on the AppServer side; just wait
    /// for natural exit and force-kill on grace expiry.
    Graceful,
    /// Force-kill immediately (used as the grace-window fallback).
    Force,
}

impl AppServer {
    pub async fn spawn(binary: &str, cwd: &Path) -> Result<Self> {
        // Route through a login bash so the agent binary is found via the
        // user's interactive PATH. See `util::shell::login_shell_command`.
        let cmd = login_shell_command(binary, &["app-server"], Some(cwd));
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
        let pid = child.id();

        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(read_frames(stdout, tx.clone()));
        tokio::spawn(drain_stderr(stderr));

        let (kill_tx, kill_rx) = mpsc::channel::<KillRequest>(2);
        let (exit_tx, exit_rx) = watch::channel::<Option<ExitSnapshot>>(None);
        tokio::spawn(wait_task(child, kill_rx, exit_tx));

        Ok(Self {
            pid,
            stdin,
            next_id: AtomicU64::new(1),
            inbound: rx,
            kill_tx,
            exit_rx,
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

    /// Bridge-owned observability handle. PID was captured at spawn time;
    /// `exit_rx` yields `Some(ExitSnapshot)` once the wait task observes
    /// termination.
    pub fn process_handle(&self) -> AgentProcessHandle {
        AgentProcessHandle {
            pid: self.pid,
            exit_rx: self.exit_rx.clone(),
        }
    }

    pub async fn shutdown(mut self, grace: std::time::Duration) -> Result<()> {
        // Half-close stdin so codex notices end-of-input.
        drop(self.stdin);
        // Tell the wait task to wait for natural exit; if the grace expires
        // we follow up with Force.
        let _ = self.kill_tx.send(KillRequest::Graceful).await;
        match tokio::time::timeout(grace, self.exit_rx.changed()).await {
            Ok(Ok(())) => {
                let snap = self.exit_rx.borrow().clone();
                tracing::debug!(?snap, "codex exited gracefully");
            }
            _ => {
                tracing::warn!("codex did not exit within grace; sending SIGKILL");
                let _ = self.kill_tx.send(KillRequest::Force).await;
                let _ = self.exit_rx.changed().await;
            }
        }
        Ok(())
    }
}

/// Owns the `Child` exclusively. Awaits either a kill request (force-kill
/// the subprocess) or the child's natural exit, then publishes an
/// `ExitSnapshot` and terminates.
async fn wait_task(
    mut child: Child,
    mut kill_rx: mpsc::Receiver<KillRequest>,
    exit_tx: watch::Sender<Option<ExitSnapshot>>,
) {
    let snapshot = loop {
        tokio::select! {
            biased;
            req = kill_rx.recv() => {
                match req {
                    Some(KillRequest::Force) => {
                        let _ = child.start_kill();
                    }
                    Some(KillRequest::Graceful) | None => {
                        // No-op: wait for natural exit.
                    }
                }
            }
            res = child.wait() => {
                let status = res.ok();
                #[cfg(unix)]
                let signal = {
                    use std::os::unix::process::ExitStatusExt;
                    status.as_ref().and_then(|s| s.signal())
                };
                #[cfg(not(unix))]
                let signal: Option<i32> = None;
                let status_code = status.as_ref().and_then(|s| s.code());
                break ExitSnapshot {
                    status_code,
                    signal,
                    observed_at: Utc::now(),
                };
            }
        }
    };
    let _ = exit_tx.send(Some(snapshot));
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
    // The login-shell wrapper (see `util::shell`) always emits two TTY-less
    // diagnostics before exec'ing codex; suppress those so the debug stream
    // only carries real codex output.
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim_end();
                if trimmed.is_empty() || is_benign_login_shell_warning(trimmed) {
                    continue;
                }
                tracing::debug!(target: "codex.stderr", "{trimmed}");
            }
            Err(e) => {
                tracing::warn!("codex stderr read error: {e}");
                break;
            }
        }
    }
}
