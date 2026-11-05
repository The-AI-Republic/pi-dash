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
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::agent::tracked::TrackedChild;
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
    child: TrackedChild,
    /// Kept alive for the process lifetime; dropping it deletes the prompt
    /// file. `None` in tests that inject a fake command with no prompt file.
    _prompt_file: Option<PromptFile>,
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
            .stderr(Stdio::piped());
        let (mut child, tree) =
            crate::agent::tracked::spawn(&mut cmd).context("spawning muse exec subprocess")?;
        let stdout = child.stdout.take().context("muse exec stdout missing")?;
        let stderr = child.stderr.take().context("muse exec stderr missing")?;
        let pid = child.id();

        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(read_events(stdout, tx));
        tokio::spawn(drain_stderr(stderr, stderr_ring));
        let child = TrackedChild::supervise(child, tree, exit_tx);

        Ok(Self {
            pid,
            inbound: rx,
            child,
            _prompt_file: prompt_file,
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
        self.child
            .kill_leader()
            .await
            .context("failed to send kill request to muse exec wait task")?;
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
