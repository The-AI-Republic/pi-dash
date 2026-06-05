//! `cursor-agent` CLI subprocess wrapper. Mirrors the shape of
//! `claude_code::process::ClaudeProcess` but with one structural difference:
//! cursor-agent in print mode takes the user prompt as a **positional CLI
//! argument** (`cursor-agent -p "<prompt>" --output-format stream-json`) and
//! runs the turn to completion, rather than reading newline-delimited
//! stream-JSON turns off stdin. There is therefore no stdin to feed — the
//! process is one-shot per run, spawned by the bridge only once the prompt is
//! known.
//!
//! Because the bridge spawns lazily (at `run`, not at construction), the
//! exit-watch channel and stderr ring are created up front by the bridge and
//! handed in here, so the supervisor's `process_handle()` — captured before
//! the first run — observes the real subprocess once it starts.

use anyhow::{Context, Result};
use chrono::Utc;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, watch};

use crate::agent::{ExitSnapshot, StderrRing};
use crate::cursor_agent::schema::StreamEvent;
use crate::util::shell::is_benign_login_shell_warning;

/// Handles the `cursor-agent --print --output-format stream-json` subprocess
/// lifecycle. Unlike `ClaudeProcess` it owns no stdin (the prompt rides in
/// argv); it exposes an mpsc receiver of parsed events coming off stdout and a
/// kill channel for interrupt/shutdown.
pub struct CursorProcess {
    pid: Option<u32>,
    pub inbound: mpsc::Receiver<StreamEvent>,
    kill_tx: mpsc::Sender<KillRequest>,
}

#[derive(Debug, Clone, Copy)]
enum KillRequest {
    Graceful,
    Force,
}

impl CursorProcess {
    /// Spawn the subprocess from a fully-built `Command`. The command's argv
    /// (`--print --output-format stream-json [--force] [--model ..] [--resume
    /// ..] <prompt>`) is assembled by `cursor_agent::bridge::Bridge`; tests
    /// inject a shell-script fake here. Forces the stdio disposition the reader
    /// task expects.
    pub async fn spawn_command(
        mut cmd: Command,
        exit_tx: watch::Sender<Option<ExitSnapshot>>,
        stderr_ring: StderrRing,
    ) -> Result<Self> {
        // No stdin: cursor-agent print mode takes the prompt from argv.
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().context("spawning cursor-agent subprocess")?;
        let stdout = child.stdout.take().context("cursor-agent stdout missing")?;
        let stderr = child.stderr.take().context("cursor-agent stderr missing")?;
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
    /// SIGKILL via the kill channel. cursor-agent has no in-protocol cancel,
    /// so the OS signal is the only lever.
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
                        tracing::warn!("cursor-agent SIGINT failed ({e}); falling back to SIGKILL");
                    }
                }
            }
        }
        self.kill_tx
            .send(KillRequest::Force)
            .await
            .context("failed to send kill request to cursor-agent wait task")?;
        Ok(())
    }

    pub async fn shutdown(
        self,
        grace: std::time::Duration,
        mut exit_rx: watch::Receiver<Option<ExitSnapshot>>,
    ) -> Result<()> {
        // Already exited (the one-shot turn finished, or a prior kill landed):
        // there is nothing to wait on, and the wait task has already published
        // its final snapshot and ended — so a `changed()` here would never fire.
        // Return immediately instead of blocking the full `grace`.
        if exit_rx.borrow().is_some() {
            return Ok(());
        }
        let _ = self.kill_tx.send(KillRequest::Graceful).await;
        match tokio::time::timeout(grace, exit_rx.changed()).await {
            Ok(Ok(())) => {
                let snap = exit_rx.borrow().clone();
                tracing::debug!(?snap, "cursor-agent exited gracefully");
            }
            _ => {
                tracing::warn!("cursor-agent did not exit within grace; sending SIGKILL");
                let _ = self.kill_tx.send(KillRequest::Force).await;
                // Bounded wait: the wait task may have already ended (so the
                // snapshot is never republished); never let shutdown hang
                // indefinitely on a `changed()` that will not come.
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
                        // All senders dropped — the owning `CursorProcess` (and
                        // its kill channel) is gone, i.e. the bridge was dropped
                        // without a graceful shutdown. The only way to reach here
                        // with the child still alive is a mid-run drop, so kill
                        // it rather than leaving an orphaned `cursor-agent`
                        // running to completion. `start_kill` is a no-op if it
                        // already exited; we then `wait` to reap it (no zombie).
                        // Breaking out of the loop here also stops the biased
                        // select! from spinning on the now-closed recv channel.
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

async fn read_events(stdout: tokio::process::ChildStdout, tx: mpsc::Sender<StreamEvent>) {
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
                let ev = match serde_json::from_str::<StreamEvent>(trimmed) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!(
                            "cursor-agent emitted unparsable stream-json ({e}): {trimmed}"
                        );
                        match serde_json::from_str::<serde_json::Value>(trimmed) {
                            Ok(v) => StreamEvent::Unknown(v),
                            Err(_) => continue,
                        }
                    }
                };
                if tx.send(ev).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                tracing::warn!("cursor-agent stdout read error: {e}");
                break;
            }
        }
    }
}

async fn drain_stderr(stderr: tokio::process::ChildStderr, ring: StderrRing) {
    // cursor-agent surfaces auth, model, and runtime errors here. At the
    // default `info` level these would be invisible, so every non-empty line is
    // logged at `warn!` AND buffered into the per-process ring for
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
                tracing::warn!(target: "cursor_agent.stderr", "{trimmed}");
                ring.lock().await.push(trimmed);
            }
            Err(e) => {
                tracing::warn!("cursor-agent stderr read error: {e}");
                break;
            }
        }
    }
}
