//! `acpx` subprocess wrapper for the OpenClaw bridge. Structurally identical
//! to `cursor_agent::process::CursorProcess`: `acpx --format json openclaw
//! exec -- "<prompt>"` takes the prompt as a **positional CLI argument**, runs
//! the turn to completion, streams raw ACP JSON-RPC NDJSON on stdout, and
//! exits. There is no stdin to feed — the process is one-shot per run, spawned
//! by the bridge only once the prompt is known.
//!
//! The exit-watch channel and stderr ring are created up front by the bridge
//! and handed in here, so the supervisor's `process_handle()` — captured
//! before the first run — observes the real subprocess once it starts.

use anyhow::{Context, Result};
use chrono::Utc;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, watch};

use crate::agent::{ExitSnapshot, StderrRing};
use crate::openclaw::schema::AcpMessage;
use crate::util::shell::is_benign_login_shell_warning;

/// Handles the `acpx ... openclaw exec` subprocess lifecycle. Owns no stdin
/// (the prompt rides in argv); exposes an mpsc receiver of parsed ACP messages
/// coming off stdout and a kill channel for interrupt/shutdown.
pub struct OpenClawProcess {
    pid: Option<u32>,
    pub inbound: mpsc::Receiver<AcpMessage>,
    kill_tx: mpsc::Sender<KillRequest>,
}

#[derive(Debug, Clone, Copy)]
enum KillRequest {
    Graceful,
    Force,
}

impl OpenClawProcess {
    /// Spawn the subprocess from a fully-built `Command`. The command's argv is
    /// assembled by `openclaw::bridge::Bridge`; tests inject a shell-script
    /// fake here. Forces the stdio disposition the reader task expects.
    pub async fn spawn_command(
        mut cmd: Command,
        exit_tx: watch::Sender<Option<ExitSnapshot>>,
        stderr_ring: StderrRing,
    ) -> Result<Self> {
        // No stdin: acpx exec takes the prompt from argv.
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().context("spawning acpx (openclaw) subprocess")?;
        let stdout = child.stdout.take().context("acpx stdout missing")?;
        let stderr = child.stderr.take().context("acpx stderr missing")?;
        let pid = child.id();

        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(read_events(stdout, tx));
        tokio::spawn(drain_stderr(stderr, stderr_ring));

        let (kill_tx, kill_rx) = mpsc::channel::<KillRequest>(2);
        tokio::spawn(wait_task(child, kill_rx, exit_tx));

        Ok(Self {
            pid,
            inbound: rx,
            kill_tx,
        })
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Best-effort interrupt: send SIGINT if we can, otherwise fall back to
    /// SIGKILL via the kill channel. We drive `acpx` one-shot and don't hold a
    /// live ACP session to send `session/cancel` over, so the OS signal is the
    /// only lever (acpx translates SIGINT into a clean ACP cancel on its end).
    pub async fn interrupt(&mut self) -> Result<()> {
        #[cfg(unix)]
        {
            use nix::sys::signal::{Signal, kill};
            use nix::unistd::Pid;
            if let Some(pid) = self.pid {
                let pid_t = Pid::from_raw(pid as i32);
                match kill(pid_t, Signal::SIGINT) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        tracing::warn!("acpx SIGINT failed ({e}); falling back to SIGKILL");
                    }
                }
            }
        }
        self.kill_tx
            .send(KillRequest::Force)
            .await
            .context("failed to send kill request to acpx wait task")?;
        Ok(())
    }

    pub async fn shutdown(
        self,
        grace: std::time::Duration,
        mut exit_rx: watch::Receiver<Option<ExitSnapshot>>,
    ) -> Result<()> {
        // Already exited (the one-shot turn finished, or a prior kill landed):
        // nothing to wait on, and the wait task has already published its final
        // snapshot and ended — so a `changed()` here would never fire.
        if exit_rx.borrow().is_some() {
            return Ok(());
        }
        let _ = self.kill_tx.send(KillRequest::Graceful).await;
        match tokio::time::timeout(grace, exit_rx.changed()).await {
            Ok(Ok(())) => {
                let snap = exit_rx.borrow().clone();
                tracing::debug!(?snap, "acpx exited gracefully");
            }
            _ => {
                tracing::warn!("acpx did not exit within grace; sending SIGKILL");
                let _ = self.kill_tx.send(KillRequest::Force).await;
                let _ = tokio::time::timeout(grace, exit_rx.changed()).await;
            }
        }
        Ok(())
    }
}

/// Owns the `Child` exclusively. Awaits either a kill request (force-kill the
/// subprocess) or the child's natural exit, then publishes an `ExitSnapshot`
/// and terminates.
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
                        // All senders dropped — the owning `OpenClawProcess`
                        // (and its kill channel) is gone, i.e. the bridge was
                        // dropped without a graceful shutdown. Kill the child
                        // rather than leaving an orphaned `acpx` running, then
                        // `wait` to reap it (no zombie).
                        let _ = child.start_kill();
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

async fn read_events(stdout: tokio::process::ChildStdout, tx: mpsc::Sender<AcpMessage>) {
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
                // Try structured parse first; fall back to stashing the raw
                // value as `Unknown` so upstream changes don't crash the daemon.
                let ev = match serde_json::from_str::<AcpMessage>(trimmed) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!("acpx emitted unparsable ACP NDJSON ({e}): {trimmed}");
                        match serde_json::from_str::<serde_json::Value>(trimmed) {
                            Ok(v) => AcpMessage::Unknown(v),
                            Err(_) => continue,
                        }
                    }
                };
                if tx.send(ev).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                tracing::warn!("acpx stdout read error: {e}");
                break;
            }
        }
    }
}

async fn drain_stderr(stderr: tokio::process::ChildStderr, ring: StderrRing) {
    // acpx / openclaw surface auth, gateway, model, and runtime errors here. At
    // the default `info` level these would be invisible, so every non-empty
    // line is logged at `warn!` AND buffered into the per-process ring for
    // RunFailed-detail enrichment. The login-shell wrapper emits two TTY-less
    // diagnostics before exec; suppress those so logs aren't noisy on spawn.
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
                tracing::warn!(target: "openclaw.stderr", "{trimmed}");
                ring.lock().await.push(trimmed);
            }
            Err(e) => {
                tracing::warn!("acpx stderr read error: {e}");
                break;
            }
        }
    }
}
