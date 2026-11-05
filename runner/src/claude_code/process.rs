//! `claude` CLI subprocess wrapper. Mirrors the shape of
//! `codex::app_server::AppServer` but speaks newline-delimited stream-JSON
//! instead of JSON-RPC.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{Mutex, mpsc, watch};

use crate::agent::tracked::TrackedChild;
use crate::agent::{
    AgentProcessHandle, ExitSnapshot, STDERR_RING_LINES, StderrBuffer, StderrRing, StderrSnapshot,
};
use crate::claude_code::schema::StreamEvent;
use crate::util::shell::{is_benign_login_shell_warning, login_shell_command};

/// Handles the `claude --print --output-format stream-json` subprocess
/// lifecycle. Owns stdin so callers can push one or more user turns and
/// exposes an mpsc receiver of parsed events coming off stdout.
///
/// Mirrors `AppServer`'s child-ownership split: `Child` is owned by an
/// internal wait task, and `ClaudeProcess` retains side-channels
/// (`kill_tx`, `exit_rx`). The `pid` is captured at spawn for SIGINT
/// delivery and observability. See `.ai_design/runner_agent_bridge` §4.4.
pub struct ClaudeProcess {
    pid: Option<u32>,
    /// `Option` so we can `take()` stdin when shutting down the process.
    stdin: Option<ChildStdin>,
    pub inbound: mpsc::Receiver<StreamEvent>,
    child: TrackedChild,
    exit_rx: watch::Receiver<Option<ExitSnapshot>>,
    stderr_ring: StderrRing,
}

/// Arguments passed to `claude` for a subprocess. The defaults below match the
/// MVP policy: non-interactive, bypass permissions, stream-json I/O.
pub struct SpawnArgs<'a> {
    pub binary: &'a str,
    pub cwd: &'a Path,
    pub model: Option<&'a str>,
    pub resume_session_id: Option<&'a str>,
    /// When `true`, pass `--permission-mode bypassPermissions`. Always `true`
    /// for MVP; wiring a real permission prompt needs an MCP bridge.
    pub bypass_permissions: bool,
}

impl ClaudeProcess {
    pub async fn spawn(args: SpawnArgs<'_>) -> Result<Self> {
        // Route through the platform spawn helper so the probe and real
        // launch behavior stay aligned. See
        // `util::shell::login_shell_command` for why.
        let mut argv: Vec<&str> = vec![
            "--print",
            "--verbose",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--include-partial-messages",
        ];
        if args.bypass_permissions {
            argv.extend(["--permission-mode", "bypassPermissions"]);
        }
        if let Some(model) = args.model {
            argv.extend(["--model", model]);
        }
        if let Some(session_id) = args.resume_session_id.filter(|s| !s.is_empty()) {
            argv.extend(["--resume", session_id]);
        }
        let cmd = login_shell_command(args.binary, &argv, Some(args.cwd));
        Self::spawn_command(cmd).await
    }

    /// Test-friendly constructor: any `Command` that writes newline-delimited
    /// stream-JSON to stdout works (e.g. a shell script fake). Forces the
    /// stdio disposition that the reader task expects.
    pub async fn spawn_command(mut cmd: Command) -> Result<Self> {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (mut child, tree) =
            crate::agent::tracked::spawn(&mut cmd).context("spawning claude subprocess")?;
        let stdin = child.stdin.take().context("claude stdin missing")?;
        let stdout = child.stdout.take().context("claude stdout missing")?;
        let stderr = child.stderr.take().context("claude stderr missing")?;
        let pid = child.id();

        let (tx, rx) = mpsc::channel(128);
        let stderr_ring: StderrRing = Arc::new(Mutex::new(StderrBuffer::new(STDERR_RING_LINES)));
        tokio::spawn(read_events(stdout, tx.clone()));
        tokio::spawn(drain_stderr(stderr, stderr_ring.clone()));

        let (exit_tx, exit_rx) = watch::channel::<Option<ExitSnapshot>>(None);
        let child = TrackedChild::supervise(child, tree, exit_tx);

        Ok(Self {
            pid,
            stdin: Some(stdin),
            inbound: rx,
            child,
            exit_rx,
            stderr_ring,
        })
    }

    /// Snapshot the recent stderr lines (mirrors `AppServer::recent_stderr`).
    pub async fn recent_stderr(&self) -> StderrSnapshot {
        self.stderr_ring.lock().await.snapshot()
    }

    /// True once the leader has exited and its process group is gone.
    pub fn is_gone(&self) -> bool {
        self.child.is_gone()
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

    /// Send one JSON line (plus a newline) to Claude's stdin.
    pub async fn send_line(&mut self, line: &str) -> Result<()> {
        let stdin = self.stdin.as_mut().context("claude stdin already closed")?;
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    /// Close stdin. Safe to call more than once.
    pub fn close_stdin(&mut self) {
        self.stdin.take();
    }

    /// Best-effort interrupt: send SIGINT if we can, otherwise fall back to
    /// SIGKILL via the kill channel. Unlike Codex there's no in-protocol
    /// `turn/interrupt` — the only lever we have is the OS.
    pub async fn interrupt(&mut self) -> Result<()> {
        #[cfg(unix)]
        {
            use nix::sys::signal::{Signal, kill};
            use nix::unistd::Pid;
            if let Some(pid) = self.pid {
                let pid_t = Pid::from_raw(pid as i32);
                // SIGINT is usually enough; the kill_on_drop on the child
                // catches any process that ignores it when we later drop.
                match kill(pid_t, Signal::SIGINT) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        // Reaped, permission denied, PID-namespace mismatch —
                        // don't pretend the interrupt succeeded. Fall through
                        // to SIGKILL so the caller still gets a best effort.
                        tracing::warn!("claude SIGINT failed ({e}); falling back to SIGKILL");
                    }
                }
            }
        }
        // Non-unix, pid-already-reaped, or SIGINT failure above.
        self.child
            .kill_leader()
            .await
            .context("failed to send kill request to claude wait task")?;
        Ok(())
    }

    /// Stop the agent and everything it started: close stdin, wait for the
    /// leader up to `grace`, then finish its process group. Replaces the old
    /// `shutdown`; see `agent::tracked`.
    pub async fn finish(mut self, grace: std::time::Duration) -> Result<()> {
        self.close_stdin();
        let outcome = self.child.finish(grace).await;
        tracing::debug!(?outcome, "claude process tree finished");
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
                // value as `Unknown` so upstream changes don't crash the
                // daemon.
                let ev = match serde_json::from_str::<StreamEvent>(trimmed) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!("claude emitted unparsable stream-json ({e}): {trimmed}");
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
                tracing::warn!("claude stdout read error: {e}");
                break;
            }
        }
    }
}

async fn drain_stderr(stderr: tokio::process::ChildStderr, ring: StderrRing) {
    // Claude surfaces auth, model, and runtime errors here. At the default
    // `info` level these would be invisible, so every non-empty line is
    // logged at `warn!` AND buffered into the per-process ring for
    // RunFailed-detail enrichment.
    //
    // The login-shell wrapper (see `util::shell`) always emits two TTY-less
    // diagnostics before exec'ing claude; suppress those so logs aren't
    // noisy on every spawn.
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
                tracing::warn!(target: "claude.stderr", "{trimmed}");
                ring.lock().await.push(trimmed);
            }
            Err(e) => {
                tracing::warn!("claude stderr read error: {e}");
                break;
            }
        }
    }
}
