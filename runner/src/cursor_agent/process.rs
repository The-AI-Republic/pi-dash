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
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

use crate::agent::tracked::TrackedChild;
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
    child: TrackedChild,
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
            .stderr(Stdio::piped());
        let (mut child, tree) =
            crate::agent::tracked::spawn(&mut cmd).context("spawning cursor-agent subprocess")?;
        let stdout = child.stdout.take().context("cursor-agent stdout missing")?;
        let stderr = child.stderr.take().context("cursor-agent stderr missing")?;
        let pid = child.id();

        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(read_events(stdout, tx));
        tokio::spawn(drain_stderr(stderr, stderr_ring));
        let child = TrackedChild::supervise(child, tree, exit_tx);

        Ok(Self {
            pid,
            inbound: rx,
            child,
        })
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// True once the leader has exited and its process group is gone.
    pub fn is_gone(&self) -> bool {
        self.child.is_gone()
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
        self.child
            .kill_leader()
            .await
            .context("failed to send kill request to cursor-agent wait task")?;
        Ok(())
    }

    /// Stop the agent and everything it started: close stdin (where there is
    /// one), wait for the leader up to `grace`, then finish its process
    /// group. Replaces the old `shutdown`; see `agent::tracked`.
    pub async fn finish(self, grace: std::time::Duration) -> Result<()> {
        let outcome = self.child.finish(grace).await;
        tracing::debug!(?outcome, "agent process tree finished");
        Ok(())
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
