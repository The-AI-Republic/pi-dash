//! `claude` CLI subprocess wrapper. Mirrors the shape of
//! `codex::app_server::AppServer` but speaks newline-delimited stream-JSON
//! instead of JSON-RPC.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;

use crate::claude_code::schema::StreamEvent;
use crate::util::shell::{is_benign_login_shell_warning, login_shell_command};

/// Handles the `claude --print --output-format stream-json` subprocess
/// lifecycle. Owns stdin (so we can push the user turn + signal EOF) and
/// exposes an mpsc receiver of parsed events coming off stdout.
pub struct ClaudeProcess {
    child: Child,
    /// `Option` so we can `take()` stdin when the caller wants to half-close
    /// it (signalling end-of-input to Claude, which causes it to process the
    /// queued turn and exit).
    stdin: Option<ChildStdin>,
    pub inbound: mpsc::Receiver<StreamEvent>,
}

/// Arguments passed to `claude` for a run. The defaults below match the MVP
/// policy: non-interactive, bypass permissions, stream-json I/O.
pub struct SpawnArgs<'a> {
    pub binary: &'a str,
    pub cwd: &'a Path,
    pub model: Option<&'a str>,
    /// When `true`, pass `--permission-mode bypassPermissions`. Always `true`
    /// for MVP; wiring a real permission prompt needs an MCP bridge.
    pub bypass_permissions: bool,
}

impl ClaudeProcess {
    pub async fn spawn(args: SpawnArgs<'_>) -> Result<Self> {
        // Route through a login bash so the agent binary is found via the
        // user's interactive PATH (nvm/pyenv/asdf/brew). See
        // `util::shell::login_shell_command` for why.
        let mut argv: Vec<&str> = vec![
            "--print",
            "--verbose",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
        ];
        if args.bypass_permissions {
            argv.extend(["--permission-mode", "bypassPermissions"]);
        }
        if let Some(model) = args.model {
            argv.extend(["--model", model]);
        }
        let mut cmd = login_shell_command(args.binary, &argv);
        cmd.current_dir(args.cwd);
        Self::spawn_command(cmd).await
    }

    /// Test-friendly constructor: any `Command` that writes newline-delimited
    /// stream-JSON to stdout works (e.g. a shell script fake). Forces the
    /// stdio disposition that the reader task expects.
    pub async fn spawn_command(mut cmd: Command) -> Result<Self> {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().context("spawning claude subprocess")?;
        let stdin = child.stdin.take().context("claude stdin missing")?;
        let stdout = child.stdout.take().context("claude stdout missing")?;
        let stderr = child.stderr.take().context("claude stderr missing")?;

        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(read_events(stdout, tx.clone()));
        tokio::spawn(drain_stderr(stderr));
        Ok(Self {
            child,
            stdin: Some(stdin),
            inbound: rx,
        })
    }

    /// Send one JSON line (plus a newline) to Claude's stdin.
    pub async fn send_line(&mut self, line: &str) -> Result<()> {
        let stdin = self.stdin.as_mut().context("claude stdin already closed")?;
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    /// Close stdin so Claude knows no further input is coming and processes
    /// the queued turn to completion. Safe to call more than once.
    pub fn close_stdin(&mut self) {
        self.stdin.take();
    }

    /// Best-effort interrupt: send SIGINT if we can, otherwise fall back to
    /// SIGKILL via `start_kill`. Unlike Codex there's no in-protocol
    /// `turn/interrupt` — the only lever we have is the OS.
    pub async fn interrupt(&mut self) -> Result<()> {
        #[cfg(unix)]
        {
            use nix::sys::signal::{Signal, kill};
            use nix::unistd::Pid;
            if let Some(pid) = self.child.id() {
                let pid = Pid::from_raw(pid as i32);
                // SIGINT is usually enough; the kill_on_drop on the child
                // catches any process that ignores it when we later drop.
                match kill(pid, Signal::SIGINT) {
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
            .start_kill()
            .context("failed to kill claude subprocess")?;
        Ok(())
    }

    pub async fn shutdown(mut self, grace: std::time::Duration) -> Result<()> {
        self.close_stdin();
        match tokio::time::timeout(grace, self.child.wait()).await {
            Ok(Ok(status)) => {
                tracing::debug!(?status, "claude exited gracefully");
            }
            _ => {
                tracing::warn!("claude did not exit within grace; sending SIGKILL");
                self.child.start_kill().ok();
                self.child.wait().await.ok();
            }
        }
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

async fn drain_stderr(stderr: tokio::process::ChildStderr) {
    // Claude surfaces auth, model, and runtime errors here. At the default
    // `info` level these would be invisible, so every non-empty line is
    // logged at `warn!`. Operators still have to dig through logs — a future
    // follow-up can buffer the last N lines into the `Failed` event detail —
    // but at least nothing is silently swallowed.
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
            }
            Err(e) => {
                tracing::warn!("claude stderr read error: {e}");
                break;
            }
        }
    }
}
