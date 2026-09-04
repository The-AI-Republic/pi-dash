//! `muse exec` CLI subprocess wrapper. Mirrors the shape of
//! `cursor_agent::process::CursorProcess`: Muse Code's headless mode
//! (`muse exec --json`) runs one prompt to completion and exits, so the
//! process is one-shot per run, spawned by the bridge only once the prompt is
//! known.
//!
//! One structural difference from Cursor: Muse takes its prompt from a file
//! (`--prompt-file <PATH>`) rather than a positional argv element. The bridge
//! writes the turn's prompt to a temp file and hands the resulting
//! [`PromptFile`] guard here; the guard deletes the file when the process
//! wrapper drops, so the prompt (which can contain private repo context) never
//! outlives the run on disk.
//!
//! Because the bridge spawns lazily (at `run`, not at construction), the
//! exit-watch channel and stderr ring are created up front by the bridge and
//! handed in here, so the supervisor's `process_handle()` — captured before
//! the first run — observes the real subprocess once it starts.

use anyhow::{Context, Result};
use chrono::Utc;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::agent::{ExitSnapshot, StderrRing};
use crate::muse_code::schema::StreamEvent;
use crate::util::shell::is_benign_login_shell_warning;

/// A temp file holding one turn's prompt, deleted on drop. `muse exec` reads
/// its prompt from `--prompt-file <PATH>`; we materialize that file in the OS
/// temp dir (never inside the repo worktree, so it can't pollute git status)
/// and remove it once the run's process wrapper is torn down.
pub struct PromptFile {
    path: PathBuf,
}

impl PromptFile {
    /// Write `prompt` to a uniquely-named temp file and return a guard that
    /// owns its path. The file is created with the current process's default
    /// permissions; on Unix the OS temp dir is world-readable, so this is no
    /// weaker than the argv/stdin the other bridges use (both visible via
    /// `/proc`), and the guard bounds its lifetime to the run.
    pub fn create(prompt: &str) -> Result<Self> {
        let mut path = std::env::temp_dir();
        path.push(format!("pidash-muse-prompt-{}.md", Uuid::new_v4()));
        std::fs::write(&path, prompt)
            .with_context(|| format!("writing muse prompt file to {}", path.display()))?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for PromptFile {
    fn drop(&mut self) {
        // Best-effort cleanup: the run is over (or the bridge was dropped), so
        // a leftover temp file is the only cost of a failed unlink.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Handles the `muse exec --json --prompt-file <PATH>` subprocess lifecycle.
/// Owns no stdin (the prompt rides in a file); it exposes an mpsc receiver of
/// parsed events coming off stdout and a kill channel for interrupt/shutdown.
/// It also holds the [`PromptFile`] guard so the prompt file lives at least as
/// long as the process that reads it.
pub struct MuseProcess {
    pid: Option<u32>,
    pub inbound: mpsc::Receiver<StreamEvent>,
    kill_tx: mpsc::Sender<KillRequest>,
    /// Kept alive for the process lifetime; dropping it deletes the prompt
    /// file. `None` in tests that inject a fake command with no prompt file.
    _prompt_file: Option<PromptFile>,
}

#[derive(Debug, Clone, Copy)]
enum KillRequest {
    Graceful,
    Force,
}

impl MuseProcess {
    /// Spawn the subprocess from a fully-built `Command`. The command's argv
    /// (`exec --json --prompt-file <PATH> [--model ..] [--session-id ..]`) is
    /// assembled by `muse_code::bridge::Bridge`; tests inject a shell-script
    /// fake here (and pass `prompt_file: None`). Forces the stdio disposition
    /// the reader task expects.
    pub async fn spawn_command(
        mut cmd: Command,
        prompt_file: Option<PromptFile>,
        exit_tx: watch::Sender<Option<ExitSnapshot>>,
        stderr_ring: StderrRing,
    ) -> Result<Self> {
        // No stdin: muse exec takes the prompt from a file.
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().context("spawning muse exec subprocess")?;
        let stdout = child.stdout.take().context("muse exec stdout missing")?;
        let stderr = child.stderr.take().context("muse exec stderr missing")?;
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
            _prompt_file: prompt_file,
        })
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Best-effort interrupt: send SIGINT if we can, otherwise fall back to
    /// SIGKILL via the kill channel. muse exec has no in-protocol cancel, so
    /// the OS signal is the only lever.
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
                        tracing::warn!("muse exec SIGINT failed ({e}); falling back to SIGKILL");
                    }
                }
            }
        }
        self.kill_tx
            .send(KillRequest::Force)
            .await
            .context("failed to send kill request to muse exec wait task")?;
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
                tracing::debug!(?snap, "muse exec exited gracefully");
            }
            _ => {
                tracing::warn!("muse exec did not exit within grace; sending SIGKILL");
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
                        // All senders dropped — the owning `MuseProcess` (and
                        // its kill channel) is gone, i.e. the bridge was dropped
                        // without a graceful shutdown. Kill the child rather than
                        // leaving an orphaned `muse` running to completion.
                        // `start_kill` is a no-op if it already exited; we then
                        // `wait` to reap it (no zombie). Breaking out here also
                        // stops the biased select! from spinning on the
                        // now-closed recv channel.
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
                        tracing::warn!("muse exec emitted unparsable json ({e}): {trimmed}");
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
                tracing::warn!("muse exec stdout read error: {e}");
                break;
            }
        }
    }
}

async fn drain_stderr(stderr: tokio::process::ChildStderr, ring: StderrRing) {
    // muse exec surfaces auth, model, and runtime errors here. At the default
    // `info` level these would be invisible, so every non-empty line is logged
    // at `warn!` AND buffered into the per-process ring for RunFailed-detail
    // enrichment. The login-shell wrapper emits two TTY-less diagnostics before
    // exec; suppress those so logs aren't noisy on spawn.
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
                tracing::warn!(target: "muse_code.stderr", "{trimmed}");
                ring.lock().await.push(trimmed);
            }
            Err(e) => {
                tracing::warn!("muse exec stderr read error: {e}");
                break;
            }
        }
    }
}
