//! Native ACP server subprocess wrapper. An ACP server is a long-lived
//! JSON-RPC endpoint on stdin/stdout, so this is modelled on
//! `codex::app_server::AppServer` (persistent, owns stdin for outbound
//! requests) rather than OpenClaw's one-shot argv process.
//!
//! Inbound NDJSON is surfaced as raw `serde_json::Value`s rather than parsed
//! `AcpMessage`s: the bridge needs the untyped value both to translate turns
//! (via [`crate::openclaw::schema::AcpMessage`]) *and* to read the JSON-RPC
//! `id` off a `session/request_permission` request so it can answer with a
//! matching response. Unparsable lines are dropped with a warning, keeping the
//! reader resilient to any non-JSON chatter the server might emit.
//!
//! `Child` is owned exclusively by an internal wait task (as in the Codex
//! wrapper) because `Child::wait()` and `start_kill()` both need `&mut self`;
//! the `AcpProcess` keeps only the side-channels to drive it.

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

/// Handles one native ACP server subprocess lifecycle + JSON-RPC wire.
/// `label` names the agent in logs and error strings (`"grok"`,
/// `"antigravity"`); it carries no behaviour.
pub struct AcpProcess {
    label: &'static str,
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

impl AcpProcess {
    /// Spawn an ACP server `binary` with `args` in `cwd` via the platform
    /// login-shell helper so the probe (`pidash doctor`) and real launch stay
    /// aligned.
    pub async fn spawn(
        label: &'static str,
        binary: &str,
        args: &[&str],
        cwd: &Path,
    ) -> Result<Self> {
        let cmd = login_shell_command(binary, args, Some(cwd));
        Self::spawn_command(label, cmd).await
    }

    /// Test-friendly constructor taking a fully-prepared `Command`. Forces the
    /// stdio disposition the reader / writer tasks expect.
    pub async fn spawn_command(label: &'static str, mut cmd: Command) -> Result<Self> {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawning {label} ACP subprocess"))?;
        let stdin = child
            .stdin
            .take()
            .with_context(|| format!("{label} stdin missing"))?;
        let stdout = child
            .stdout
            .take()
            .with_context(|| format!("{label} stdout missing"))?;
        let stderr = child
            .stderr
            .take()
            .with_context(|| format!("{label} stderr missing"))?;
        let pid = child.id();

        let (tx, rx) = mpsc::channel(128);
        let stderr_ring: StderrRing = Arc::new(Mutex::new(StderrBuffer::new(STDERR_RING_LINES)));
        let label_reader = label;
        tokio::spawn(read_frames(label_reader, stdout, tx));
        let label_stderr = label;
        tokio::spawn(drain_stderr(label_stderr, stderr, stderr_ring.clone()));

        let (kill_tx, kill_rx) = mpsc::channel::<KillRequest>(2);
        let (exit_tx, exit_rx) = watch::channel::<Option<ExitSnapshot>>(None);
        tokio::spawn(wait_task(child, kill_rx, exit_tx));

        Ok(Self {
            label,
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

    /// Write a single JSON-RPC line to the server's stdin (newline-framed).
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

    /// Best-effort interrupt: SIGINT if we can, else force-kill. Servers that
    /// end the turn through the protocol-level `session/cancel` notification
    /// do so in the bridge (`Bridge::interrupt`), which runs before this;
    /// this stays the OS-level lever for servers that clean up their ACP
    /// session on SIGINT.
    pub async fn interrupt(&mut self) -> Result<()> {
        let label = self.label;
        #[cfg(unix)]
        {
            use nix::sys::signal::{Signal, kill};
            use nix::unistd::Pid;
            if let Some(pid) = self.pid {
                match kill(Pid::from_raw(pid as i32), Signal::SIGINT) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        tracing::warn!("{label} SIGINT failed ({e}); falling back to SIGKILL")
                    }
                }
            }
        }
        self.kill_tx
            .send(KillRequest::Force)
            .await
            .with_context(|| format!("failed to send kill request to {label} wait task"))?;
        Ok(())
    }

    pub async fn shutdown(mut self, grace: std::time::Duration) -> Result<()> {
        // Half-close stdin so the server notices end-of-input and can exit
        // cleanly.
        drop(self.stdin);
        let _ = self.kill_tx.send(KillRequest::Graceful).await;
        match tokio::time::timeout(grace, self.exit_rx.changed()).await {
            Ok(Ok(())) => {
                let snap = self.exit_rx.borrow().clone();
                tracing::debug!(label = self.label, ?snap, "ACP server exited gracefully");
            }
            _ => {
                tracing::warn!(
                    label = self.label,
                    "ACP server did not exit within grace; sending SIGKILL"
                );
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

async fn read_frames(
    label: &'static str,
    stdout: tokio::process::ChildStdout,
    tx: mpsc::Sender<serde_json::Value>,
) {
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
                        tracing::warn!("{label} emitted non-JSON line ({e}): {trimmed}");
                    }
                }
            }
            Err(e) => {
                tracing::warn!("{label} stdout read error: {e}");
                break;
            }
        }
    }
}

async fn drain_stderr(label: &'static str, stderr: tokio::process::ChildStderr, ring: StderrRing) {
    // Servers surface auth (missing/invalid credentials), model, and runtime
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
                tracing::warn!(target: "acp.stderr", label, "{trimmed}");
                ring.lock().await.push(trimmed);
            }
            Err(e) => {
                tracing::warn!("{label} stderr read error: {e}");
                break;
            }
        }
    }
}
