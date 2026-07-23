//! `grok agent stdio` subprocess wrapper. Grok's ACP server is a long-lived
//! JSON-RPC endpoint on stdin/stdout, so this is modelled on
//! `codex::app_server::AppServer` (persistent, owns stdin for outbound
//! requests) rather than OpenClaw's one-shot argv process.
//!
//! Inbound NDJSON is surfaced as raw `serde_json::Value`s rather than parsed
//! `AcpMessage`s: the bridge needs the untyped value both to translate turns
//! (via [`crate::openclaw::schema::AcpMessage`]) *and* to read the JSON-RPC
//! `id` off a `session/request_permission` request so it can answer with a
//! matching response. Unparsable lines are dropped with a warning, keeping the
//! reader resilient to any non-JSON chatter grok might emit.
//!
//! `Child` is owned exclusively by an internal wait task (as in the Codex
//! wrapper) because `Child::wait()` and `start_kill()` both need `&mut self`;
//! the `GrokProcess` keeps only the side-channels to drive it.

use anyhow::{Context, Result};
use chrono::Utc;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, mpsc, watch};

use crate::agent::{
    AgentProcessHandle, ExitSnapshot, STDERR_RING_LINES, StderrBuffer, StderrRing, StderrSnapshot,
};
use crate::util::shell::{is_benign_login_shell_warning, login_shell_command};

/// Handles the `grok agent stdio` subprocess lifecycle + ACP JSON-RPC wire.
pub struct GrokProcess {
    pid: Option<u32>,
    stdin: ChildStdin,
    next_id: AtomicU64,
    pub inbound: mpsc::Receiver<serde_json::Value>,
    kill_tx: mpsc::Sender<KillRequest>,
    exit_rx: watch::Receiver<Option<ExitSnapshot>>,
    stderr_ring: StderrRing,
}

#[derive(Debug, Clone, Copy)]
enum KillRequest {
    /// Stdin was already half-closed by the caller; just await natural exit and
    /// force-kill on grace expiry.
    Graceful,
    /// Force-kill immediately (the grace-window fallback).
    Force,
}

impl GrokProcess {
    /// Spawn `grok agent stdio` in `cwd` via the platform login-shell helper so
    /// the probe (`pidash doctor`) and real launch stay aligned.
    pub async fn spawn(binary: &str, cwd: &Path) -> Result<Self> {
        let cmd = login_shell_command(binary, &["agent", "stdio"], Some(cwd));
        Self::spawn_command(cmd).await
    }

    /// Test-friendly constructor taking a fully-prepared `Command`. Forces the
    /// stdio disposition the reader / writer tasks expect.
    pub async fn spawn_command(mut cmd: Command) -> Result<Self> {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().context("spawning grok subprocess")?;
        let stdin = child.stdin.take().context("grok stdin missing")?;
        let stdout = child.stdout.take().context("grok stdout missing")?;
        let stderr = child.stderr.take().context("grok stderr missing")?;
        let pid = child.id();

        let (tx, rx) = mpsc::channel(128);
        let stderr_ring: StderrRing = Arc::new(Mutex::new(StderrBuffer::new(STDERR_RING_LINES)));
        tokio::spawn(read_frames(stdout, tx));
        tokio::spawn(drain_stderr(stderr, stderr_ring.clone()));

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
            stderr_ring,
        })
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Allocate a fresh monotonic JSON-RPC request id.
    pub fn alloc_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Write a single JSON-RPC line to grok's stdin (newline-framed).
    pub async fn send_raw(&mut self, line: &str) -> Result<()> {
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    pub fn process_handle(&self) -> AgentProcessHandle {
        AgentProcessHandle {
            pid: self.pid,
            exit_rx: self.exit_rx.clone(),
        }
    }

    pub async fn recent_stderr(&self) -> StderrSnapshot {
        self.stderr_ring.lock().await.snapshot()
    }

    /// Best-effort interrupt: SIGINT if we can, else force-kill. We don't hold
    /// a spec-level `session/cancel` channel in the MVP, so the OS signal is
    /// the lever (grok cleans up its ACP session on SIGINT).
    pub async fn interrupt(&mut self) -> Result<()> {
        #[cfg(unix)]
        {
            use nix::sys::signal::{Signal, kill};
            use nix::unistd::Pid;
            if let Some(pid) = self.pid {
                match kill(Pid::from_raw(pid as i32), Signal::SIGINT) {
                    Ok(()) => return Ok(()),
                    Err(e) => tracing::warn!("grok SIGINT failed ({e}); falling back to SIGKILL"),
                }
            }
        }
        self.kill_tx
            .send(KillRequest::Force)
            .await
            .context("failed to send kill request to grok wait task")?;
        Ok(())
    }

    pub async fn shutdown(mut self, grace: std::time::Duration) -> Result<()> {
        // Half-close stdin so grok notices end-of-input and can exit cleanly.
        drop(self.stdin);
        let _ = self.kill_tx.send(KillRequest::Graceful).await;
        match tokio::time::timeout(grace, self.exit_rx.changed()).await {
            Ok(Ok(())) => {
                let snap = self.exit_rx.borrow().clone();
                tracing::debug!(?snap, "grok exited gracefully");
            }
            _ => {
                tracing::warn!("grok did not exit within grace; sending SIGKILL");
                let _ = self.kill_tx.send(KillRequest::Force).await;
                let _ = tokio::time::timeout(grace, self.exit_rx.changed()).await;
            }
        }
        Ok(())
    }
}

/// Owns the `Child` exclusively. Awaits a kill request (force-kill) or natural
/// exit, then publishes an `ExitSnapshot` and terminates.
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
                    Some(KillRequest::Graceful) => {
                        // No-op: wait for natural exit.
                    }
                    None => {
                        // All senders dropped — recv resolves to None every
                        // iteration; stop polling it and just await exit.
                        let res = child.wait().await;
                        break exit_snapshot_from(res.ok());
                    }
                }
            }
            res = child.wait() => {
                break exit_snapshot_from(res.ok());
            }
        }
    };
    let _ = exit_tx.send(Some(snapshot));
}

fn exit_snapshot_from(status: Option<std::process::ExitStatus>) -> ExitSnapshot {
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.as_ref().and_then(|s| s.signal())
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    let status_code = status.as_ref().and_then(|s| s.code());
    ExitSnapshot {
        status_code,
        signal,
        observed_at: Utc::now(),
    }
}

async fn read_frames(stdout: tokio::process::ChildStdout, tx: mpsc::Sender<serde_json::Value>) {
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
                match serde_json::from_str::<serde_json::Value>(trimmed) {
                    Ok(v) => {
                        if tx.send(v).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("grok emitted non-JSON line ({e}): {trimmed}");
                    }
                }
            }
            Err(e) => {
                tracing::warn!("grok stdout read error: {e}");
                break;
            }
        }
    }
}

async fn drain_stderr(stderr: tokio::process::ChildStderr, ring: StderrRing) {
    // grok surfaces auth (missing/invalid XAI_API_KEY), model, and runtime
    // errors here. At the default level these would be invisible, so every
    // non-empty line is logged at `warn!` AND buffered into the per-process
    // ring for RunFailed-detail enrichment. The login-shell wrapper emits two
    // TTY-less diagnostics before exec; suppress those.
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
                tracing::warn!(target: "grok.stderr", "{trimmed}");
                ring.lock().await.push(trimmed);
            }
            Err(e) => {
                tracing::warn!("grok stderr read error: {e}");
                break;
            }
        }
    }
}
