// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The one `pidash` daemon this computer runs, shared with the CLI.
//!
//! Earlier builds ran a private daemon per workspace, each with its own config
//! under the app's `managed/` tree. Every one of them was a separate dev
//! machine to the server, so one computer showed up several times, and the
//! user's own `pidash` install was yet another. Now the app drives the
//! user-installed `pidash` with its **default** config and data paths — the
//! same daemon `pidash install` sets up as a service — and talks to it at the
//! default IPC socket.
//!
//! Lifecycle, as [`ensure`] runs it:
//!
//! 1. Make sure the CLI is installed (from the app bundle when possible).
//! 2. Move any legacy per-workspace config into the default one: stop any
//!    daemon an earlier build left running on it, `pidash __managed adopt`
//!    each dir, and delete a dir only once adopt reports every runner in it
//!    moved. A dir whose runners belong to a workspace the CLI is not signed
//!    in to is kept (its token is their only credential) and reported, so the
//!    UI can say so; a later launch adopts them once the CLI is signed in to
//!    that workspace.
//! 3. If a daemon already answers at the default socket, use it.
//! 4. Otherwise install and start the OS service (`pidash install`,
//!    `pidash start`). If that fails, run the same daemon as a child of this
//!    app with `PIDASH_SUPERVISOR=desktop` — online only while the app is
//!    open — and report it, so the UI can offer a Repair that retries the
//!    service.
//!
//! On quit only a child daemon this app started is stopped. A service daemon
//! belongs to the OS and keeps serving the user's runners.
//!
//! Security posture follows `pidash_cli`: every command here builds its own
//! argv. The webview can ask for "ensure" or "repair" and nothing else; it
//! cannot name a program, an argument, or an environment variable.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::managed_runner::ManagedPaths;

/// Env var the daemon reads to report who supervises it (contract §6).
const SUPERVISOR_ENV: &str = "PIDASH_SUPERVISOR";

/// Overrides the app must never pass on: they would point the CLI at a
/// private tree instead of the shared default config.
const PATH_OVERRIDES: [&str; 2] = ["PIDASH_CONFIG_DIR", "PIDASH_DATA_DIR"];

/// How long a daemon gets to bind its socket after a start.
const SOCKET_WAIT: Duration = Duration::from_secs(10);

/// Default timeout for a `pidash` invocation. Service install and sign-in can
/// touch the network and the service manager, so this is generous; it exists
/// so a wedged child can never hang a Tauri command forever.
pub(crate) const CLI_TIMEOUT: Duration = Duration::from_secs(90);

/// Who runs the shared daemon, as the UI needs to know it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonMode {
    /// The OS service (launchd / systemd / Windows) runs it.
    Service,
    /// This app runs it as a child process: online only while the app is open.
    Child,
    /// Nothing to run yet: the CLI is not signed in.
    Idle,
    /// No daemon could be started; `message` says why.
    Unavailable,
}

/// What [`ensure`] / [`repair`] report to the overlay.
#[derive(Debug, Clone, Serialize)]
pub struct DaemonStatus {
    pub mode: DaemonMode,
    /// The installed CLI predates the commands this app needs (contract §6).
    pub cli_outdated: bool,
    pub message: Option<String>,
    /// Runners still in a legacy per-workspace config because adopt could not
    /// move them (signed in to another workspace, or not at all). They cannot
    /// run until the CLI is signed in to their workspace.
    pub unadopted: Vec<UnadoptedRunner>,
}

impl DaemonStatus {
    fn new(mode: DaemonMode) -> Self {
        Self {
            mode,
            cli_outdated: false,
            message: None,
            unadopted: Vec::new(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            mode: DaemonMode::Unavailable,
            cli_outdated: false,
            message: Some(message.into()),
            unadopted: Vec::new(),
        }
    }
}

/// A runner `pidash __managed adopt` left in its legacy config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnadoptedRunner {
    pub name: String,
    /// The runner's workspace slug as adopt reports it, else the legacy
    /// dir's name (also a slug); empty when neither is known.
    pub workspace: String,
    /// The CLI's reason: `workspace_mismatch`, `not_signed_in`, …
    pub reason: String,
}

/// Process-wide state for the shared daemon.
#[derive(Default)]
pub struct SharedDaemon {
    /// The daemon this app started as a child, when the service was
    /// unavailable. `None` when the OS service runs the daemon — that one is
    /// never ours to stop.
    child: Mutex<Option<Child>>,
    /// The default IPC socket, as the CLI resolves it (`pidash status --json`).
    socket: Mutex<Option<PathBuf>>,
    /// The installed CLI, once found.
    cli: Mutex<Option<PathBuf>>,
    /// Name of the built-in engine runner local chat talks to. A shared daemon
    /// can host several runners, and the daemon only picks one by itself when
    /// it hosts exactly one.
    chat_runner: Mutex<Option<String>>,
    /// Serialises ensure/repair so two callers never start two daemons.
    lifecycle: tokio::sync::Mutex<()>,
}

/// A failed `pidash` invocation, as the overlay sees it.
///
/// `code` is the CLI's own error code (`{"error": "<code>"}` on stdout, per
/// the contract) where it gave one, or one of ours: `cli_missing`,
/// `cli_outdated`, `cli_failed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CliError {
    pub code: String,
    pub message: String,
}

impl CliError {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    pub(crate) fn outdated() -> Self {
        Self::new(
            "cli_outdated",
            "The installed pidash CLI is too old for this version of Pi Dash. Update the Pi Dash CLI.",
        )
    }

    pub(crate) fn is_outdated(&self) -> bool {
        self.code == "cli_outdated"
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Captured result of one `pidash` run.
#[derive(Debug, Clone)]
pub(crate) struct CliOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl CliOutput {
    /// Turn a failed run into a [`CliError`]: the CLI's own JSON error when
    /// it printed one, `cli_outdated` when the CLI rejected the command line
    /// itself, otherwise its stderr.
    pub(crate) fn error(&self, what: &str) -> CliError {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(self.stdout.trim())
            && let Some(code) = value.get("error").and_then(|v| v.as_str())
        {
            let message = value
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or(code)
                .to_string();
            return CliError::new(code, message);
        }
        if self.rejected_command_line() {
            return CliError::outdated();
        }
        let detail = self.stderr.trim();
        let detail = if detail.is_empty() { self.stdout.trim() } else { detail };
        // Keep the tail: clap and anyhow put the useful line last.
        let tail: Vec<&str> = detail.lines().rev().take(6).collect();
        let detail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
        let status = self
            .code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".to_string());
        CliError::new(
            "cli_failed",
            if detail.is_empty() {
                format!("pidash {what} failed (exit {status})")
            } else {
                format!("pidash {what} failed: {detail}")
            },
        )
    }

    /// Whether clap refused the argv — an older CLI that lacks a subcommand
    /// or flag this app passes. clap exits 2 for usage errors.
    pub(crate) fn rejected_command_line(&self) -> bool {
        self.code == Some(2)
            && [
                "unexpected argument",
                "unrecognized subcommand",
                "unrecognized argument",
                "Found argument",
            ]
            .iter()
            .any(|needle| self.stderr.contains(needle))
    }
}

/// A `pidash` invocation at the default paths.
///
/// Every call goes through here, so no code path can accidentally aim the CLI
/// at a private tree: an inherited `PIDASH_CONFIG_DIR` / `PIDASH_DATA_DIR`
/// (or a stray supervisor marker) is removed.
pub(crate) fn cli_command(cli: &Path) -> Command {
    let mut cmd = Command::new(cli);
    for var in PATH_OVERRIDES {
        cmd.env_remove(var);
    }
    cmd.env_remove(SUPERVISOR_ENV);
    #[cfg(windows)]
    {
        // Keep the console window from flashing on every invocation.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Run `cli args…` to completion (or `timeout`), feeding `stdin` when given.
///
/// Blocking; call from a blocking task. stdout and stderr are drained on their
/// own threads so a chatty child can never deadlock on a full pipe.
pub(crate) fn run_cli(
    cli: &Path,
    args: &[&str],
    stdin: Option<&str>,
    timeout: Duration,
) -> Result<CliOutput, CliError> {
    let mut child = cli_command(cli)
        .args(args)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CliError::new("cli_missing", format!("running {}: {e}", cli.display())))?;
    if let Some(input) = stdin
        && let Some(mut pipe) = child.stdin.take()
    {
        // A child that exits without reading (an older CLI rejecting a flag)
        // breaks the pipe; its own exit status is the real answer.
        let _ = pipe.write_all(input.as_bytes());
    }
    let drain = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).into_owned()
        })
    };
    let out = drain(Box::new(child.stdout.take().expect("piped stdout")));
    let err = drain(Box::new(child.stderr.take().expect("piped stderr")));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CliError::new(
                    "cli_failed",
                    format!("pidash {} timed out", args.first().copied().unwrap_or_default()),
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(CliError::new("cli_failed", format!("waiting for pidash: {e}"))),
        }
    };
    Ok(CliOutput {
        success: status.success(),
        code: status.code(),
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

impl SharedDaemon {
    /// The installed CLI, installing it from the bundle when there is none.
    async fn cli<R: Runtime>(&self, app: &AppHandle<R>) -> Result<PathBuf, CliError> {
        if let Some(path) = self.cli.lock().unwrap().clone().filter(|p| p.is_file()) {
            return Ok(path);
        }
        let mut status = crate::pidash_cli::detect_pidash_cli().await;
        if !status.installed {
            crate::pidash_cli::install_cli(app)
                .await
                .map_err(|e| CliError::new("cli_missing", format!("installing the pidash CLI: {e}")))?;
            status = crate::pidash_cli::detect_pidash_cli().await;
        }
        let path = status.path.ok_or_else(|| {
            CliError::new("cli_missing", "The pidash CLI is not installed and could not be installed.")
        })?;
        *self.cli.lock().unwrap() = Some(path.clone());
        Ok(path)
    }

    /// Forget the cached CLI path, so the next call detects it again (after
    /// an update replaced it, say).
    pub(crate) fn forget_cli(&self) {
        *self.cli.lock().unwrap() = None;
    }

    pub(crate) fn chat_runner(&self) -> Option<String> {
        self.chat_runner.lock().unwrap().clone()
    }

    pub(crate) fn set_chat_runner(&self, name: String) {
        *self.chat_runner.lock().unwrap() = Some(name);
    }

    /// Whether the child daemon this app started is still running.
    fn child_alive(&self) -> bool {
        let mut guard = self.child.lock().unwrap();
        match guard.as_mut().map(|c| c.try_wait()) {
            Some(Ok(None)) => true,
            Some(_) => {
                *guard = None;
                false
            }
            None => false,
        }
    }
}

/// The installed CLI, for callers outside this module.
pub(crate) async fn cli_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, CliError> {
    app.state::<SharedDaemon>().cli(app).await
}

/// Run the installed CLI on the blocking pool.
pub(crate) async fn run<R: Runtime>(
    app: &AppHandle<R>,
    args: Vec<String>,
    stdin: Option<String>,
) -> Result<CliOutput, CliError> {
    let cli = cli_path(app).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        run_cli(&cli, &args, stdin.as_deref(), CLI_TIMEOUT)
    })
    .await
    .map_err(|e| CliError::new("cli_failed", format!("pidash task panicked: {e}")))?
}

/// The shared daemon's control socket, as the installed CLI resolves it.
///
/// Asked of the CLI rather than re-derived here: the default runtime dir is
/// platform- and environment-dependent (`$XDG_RUNTIME_DIR` on Linux), and the
/// CLI is the one program that must agree with the daemon about it.
pub(crate) async fn socket<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let state = app.state::<SharedDaemon>();
    if let Some(socket) = state.socket.lock().unwrap().clone() {
        return Ok(socket);
    }
    let (socket, _) = probe_status(app).await.map_err(|e| e.message)?;
    *state.socket.lock().unwrap() = Some(socket.clone());
    Ok(socket)
}

/// `pidash status --json`: the socket path and whether a daemon answers on it.
async fn probe_status<R: Runtime>(app: &AppHandle<R>) -> Result<(PathBuf, bool), CliError> {
    let out = run(app, vec!["status".into(), "--json".into()], None).await?;
    parse_status(&out)
}

fn parse_status(out: &CliOutput) -> Result<(PathBuf, bool), CliError> {
    let value: serde_json::Value = serde_json::from_str(out.stdout.trim()).map_err(|_| out.error("status"))?;
    let socket = value
        .get("ipc_socket")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| CliError::new("cli_failed", "pidash status did not report its IPC socket"))?;
    let running = value.get("daemon").is_some_and(|d| !d.is_null());
    Ok((PathBuf::from(socket), running))
}

/// Whether the CLI supports this app's commands and is signed in.
///
/// `Ok(None)` means the CLI is too old to say (no `auth status --json`).
async fn signed_in<R: Runtime>(app: &AppHandle<R>) -> Result<Option<bool>, CliError> {
    match crate::account_sync::auth_status(app).await {
        Ok(status) => Ok(Some(status.signed_in)),
        Err(e) if e.is_outdated() => Ok(None),
        Err(e) => Err(e),
    }
}

/// Poll the socket until a daemon answers or [`SOCKET_WAIT`] passes.
async fn wait_for_socket(socket: &Path) -> bool {
    let deadline = Instant::now() + SOCKET_WAIT;
    while Instant::now() < deadline {
        if crate::ipc::daemon_status(socket).await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    false
}

/// Make sure the shared daemon runs. See the module docs for the steps.
pub(crate) async fn ensure<R: Runtime>(app: &AppHandle<R>) -> DaemonStatus {
    let state = app.state::<SharedDaemon>();
    let _lifecycle = state.lifecycle.lock().await;
    let mut status = ensure_locked(app, false).await;
    // Report an outdated CLI whatever else happened: the UI's "update" prompt
    // is the fix for most of what can go wrong after it.
    if !status.cli_outdated && matches!(signed_in(app).await, Ok(None)) {
        status.cli_outdated = true;
    }
    status
}

/// Retry the service: stop a child daemon this app runs, install and start
/// the service, and fall back to a child again only if that still fails.
pub(crate) async fn repair<R: Runtime>(app: &AppHandle<R>) -> DaemonStatus {
    let state = app.state::<SharedDaemon>();
    let _lifecycle = state.lifecycle.lock().await;
    state.forget_cli();
    *state.socket.lock().unwrap() = None;
    let child = state.child.lock().unwrap().take();
    if let Some(child) = child {
        tauri::async_runtime::spawn_blocking(move || stop_child(child, 10))
            .await
            .ok();
    }
    let mut status = ensure_locked(app, true).await;
    if !status.cli_outdated && matches!(signed_in(app).await, Ok(None)) {
        status.cli_outdated = true;
    }
    status
}

async fn ensure_locked<R: Runtime>(app: &AppHandle<R>, force_service: bool) -> DaemonStatus {
    let state = app.state::<SharedDaemon>();
    let cli = match state.cli(app).await {
        Ok(cli) => cli,
        Err(e) => return DaemonStatus::unavailable(e.message),
    };

    let mut cli_outdated = false;
    let mut unadopted = Vec::new();
    if let Ok(paths) = ManagedPaths::resolve(app) {
        let adopt_cli = cli.clone();
        let outcome = tauri::async_runtime::spawn_blocking(move || {
            if legacy_config_dirs(&paths.config_dir).is_empty() {
                return Ok(LegacyMigration::default());
            }
            stop_orphaned_daemons(&paths.runner);
            migrate_legacy(&paths.config_dir, |dir| adopt(&adopt_cli, dir))
        })
        .await;
        match outcome {
            Ok(Ok(migration)) => unadopted = migration.unadopted,
            Ok(Err(e)) => {
                cli_outdated |= e.is_outdated();
                eprintln!("legacy desktop config migration: {e}");
            }
            Err(_) => {}
        }
    }

    let (socket, running) = match probe_status(app).await {
        Ok(found) => found,
        Err(e) => {
            return DaemonStatus {
                cli_outdated: cli_outdated || e.is_outdated(),
                unadopted,
                ..DaemonStatus::unavailable(e.message)
            };
        }
    };
    *state.socket.lock().unwrap() = Some(socket.clone());
    let with_flag = |mut s: DaemonStatus| {
        s.cli_outdated |= cli_outdated;
        s.unadopted = unadopted.clone();
        s
    };

    if state.child_alive() {
        return with_flag(DaemonStatus::new(DaemonMode::Child));
    }
    if running {
        return with_flag(DaemonStatus::new(DaemonMode::Service));
    }
    // The daemon refuses to start without a signed-in config, so there is
    // nothing to run before sign-in. An older CLI cannot say; try anyway.
    if !force_service && matches!(signed_in(app).await, Ok(Some(false))) {
        return with_flag(DaemonStatus::new(DaemonMode::Idle));
    }

    let service_error = match start_service(app).await {
        Ok(()) if wait_for_socket(&socket).await => {
            return with_flag(DaemonStatus::new(DaemonMode::Service));
        }
        Ok(()) => "the service started but its daemon did not answer".to_string(),
        Err(e) => e.message,
    };
    eprintln!("pidash service unavailable ({service_error}); running the daemon as a child of the app");

    match spawn_child(app, &cli) {
        Ok(child) => {
            *state.child.lock().unwrap() = Some(child);
        }
        Err(e) => return with_flag(DaemonStatus::unavailable(e)),
    }
    if wait_for_socket(&socket).await || state.child_alive() {
        return with_flag(DaemonStatus {
            message: Some(service_error),
            ..DaemonStatus::new(DaemonMode::Child)
        });
    }
    with_flag(DaemonStatus::unavailable(format!(
        "The Pi Dash daemon could not be started ({service_error}). See desktop-daemon.log in the app's data folder."
    )))
}

/// `pidash install`, then `pidash start`.
async fn start_service<R: Runtime>(app: &AppHandle<R>) -> Result<(), CliError> {
    // `--skip-linger`: enabling boot start on Linux asks sudo for a password,
    // which a GUI app has no terminal to answer.
    let install = run(app, vec!["install".into(), "--skip-linger".into()], None).await?;
    if !install.success {
        return Err(install.error("install"));
    }
    let start = run(app, vec!["start".into()], None).await?;
    if !start.success {
        return Err(start.error("start"));
    }
    Ok(())
}

/// Start `pidash __run` at the default paths as a child of this app, with its
/// output in `managed/desktop-daemon.log`.
fn spawn_child<R: Runtime>(app: &AppHandle<R>, cli: &Path) -> Result<Child, String> {
    let paths = ManagedPaths::resolve(app)?;
    std::fs::create_dir_all(&paths.root).map_err(|e| format!("creating {}: {e}", paths.root.display()))?;
    spawn_daemon_process(cli, &paths.root.join("desktop-daemon.log"))
}

/// Spawn `pidash __run` with both output streams in `log`: the daemon's
/// tracing output goes to stdout, while panics and its fatal startup error go
/// to stderr.
fn spawn_daemon_process(cli: &Path, log: &Path) -> Result<Child, String> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| format!("opening daemon log: {e}"))?;
    let file_stdout = file.try_clone().map_err(|e| format!("opening daemon log: {e}"))?;
    cli_command(cli)
        .arg("__run")
        .env(SUPERVISOR_ENV, "desktop")
        .stdin(Stdio::null())
        .stdout(Stdio::from(file_stdout))
        .stderr(Stdio::from(file))
        .spawn()
        .map_err(|e| format!("spawning pidash __run: {e}"))
}

/// Ask a child daemon to finish its current run and exit, then make sure.
///
/// Unix gets SIGTERM and `grace_seconds` to finish; Windows has no way to ask
/// a console-less process to exit (see `CREATE_NO_WINDOW` in
/// [`cli_command`]), so the stop there is immediate.
fn stop_child(mut child: Child, grace_seconds: u64) {
    #[cfg(not(unix))]
    let _ = grace_seconds;
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(child.id() as i32, libc::SIGTERM);
        }
        // Poll rather than sleeping the full grace period: a daemon with
        // nothing in flight exits immediately.
        let deadline = Instant::now() + Duration::from_secs(grace_seconds);
        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Best-effort teardown for app exit: stop the child daemon if this app runs
/// one. A service daemon is left alone — it is the user's, not the app's.
pub fn shutdown<R: Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<SharedDaemon>() else {
        return;
    };
    let child = state.child.lock().ok().and_then(|mut guard| guard.take());
    if let Some(child) = child {
        stop_child(child, 5);
    }
}

// ---------------------------------------------------------------------------
// Legacy per-workspace configs
// ---------------------------------------------------------------------------

/// The legacy config dirs under `managed/pidash/`, in the order they must be
/// adopted: every per-workspace subdir first, then the flat root an even
/// older build wrote (deleting the root deletes the subdirs with it).
fn legacy_config_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path())
                .filter(|p| p.join("config.toml").is_file())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    if root.join("config.toml").is_file() {
        dirs.push(root.to_path_buf());
    }
    dirs
}

/// What one `pidash __managed adopt --from-config-dir <dir>` reported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AdoptOutcome {
    /// Every runner in the dir is now in the default config (adopted now or
    /// earlier). Only then may the dir — and the token in it, the only
    /// credential its runners have — be deleted.
    fully_adopted: bool,
    /// Runners left behind.
    skipped: Vec<SkippedRunner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkippedRunner {
    name: String,
    /// Adopt's `workspace_id`, which is the workspace's slug (may be null).
    workspace_id: Option<String>,
    workspace_slug: Option<String>,
    reason: String,
}

/// The result of one migration pass.
#[derive(Debug, Default)]
struct LegacyMigration {
    /// Dirs adopted and deleted.
    migrated: usize,
    /// Runners kept in their legacy dir, for the UI to explain.
    unadopted: Vec<UnadoptedRunner>,
}

/// Adopt every legacy config dir into the default config, deleting each one
/// adopt fully moved.
///
/// A dir is deleted only when its own adopt reported `fully_adopted`. One with
/// runners adopt left behind (another workspace than the CLI's, or a
/// signed-out CLI) is kept untouched — its token is the only credential those
/// runners have — and its runners are reported; adopt is idempotent, so the
/// next launch (or the next ensure, after a sign-in) picks them up. A failed
/// adopt, or output that cannot be read, stops the pass with nothing deleted
/// for that dir. The root, whose deletion would take its subdirs with it, is
/// removed only once every subdir is gone.
fn migrate_legacy(
    root: &Path,
    mut adopt: impl FnMut(&Path) -> Result<AdoptOutcome, CliError>,
) -> Result<LegacyMigration, CliError> {
    let mut migration = LegacyMigration::default();
    let mut kept_subdir = false;
    for dir in legacy_config_dirs(root) {
        let outcome = adopt(&dir)?;
        let is_root = dir == root;
        if !outcome.fully_adopted {
            // The CLI's `workspace_id` is the runner's workspace slug; a
            // subdir is named after one too, for a runner that has none.
            let dir_slug = (!is_root)
                .then(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
                .flatten();
            migration.unadopted.extend(
                outcome
                    .skipped
                    .into_iter()
                    .filter(|r| !r.reason.starts_with("already"))
                    .map(|r| UnadoptedRunner {
                        workspace: r
                            .workspace_slug
                            .or(r.workspace_id)
                            .or_else(|| dir_slug.clone())
                            .unwrap_or_default(),
                        name: r.name,
                        reason: r.reason,
                    }),
            );
            kept_subdir |= !is_root;
            continue;
        }
        if is_root && kept_subdir {
            // Adopt is idempotent: the root is adopted again (as a no-op) and
            // deleted on the launch that empties it.
            continue;
        }
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            // Adopt is idempotent, so a dir that survives is adopted again
            // (as a no-op) next launch and deleted then.
            eprintln!("removing adopted legacy config {}: {e}", dir.display());
            kept_subdir |= !is_root;
        }
        migration.migrated += 1;
    }
    Ok(migration)
}

/// `pidash __managed adopt --from-config-dir <dir>` at the default paths.
fn adopt(cli: &Path, dir: &Path) -> Result<AdoptOutcome, CliError> {
    let dir = dir.to_string_lossy();
    let out = run_cli(
        cli,
        &["__managed", "adopt", "--from-config-dir", &dir],
        None,
        CLI_TIMEOUT,
    )?;
    if !out.success {
        return Err(out.error("__managed adopt"));
    }
    parse_adopt(&out.stdout)
}

/// Read adopt's report: `{"adopted": [...], "skipped": [{"name", "workspace_id",
/// "reason"}], "fully_adopted": bool}`. Anything else is a CLI that exited 0
/// without saying what it did — an error, so nothing is deleted on its word.
/// A report without `fully_adopted` (an earlier CLI) counts as not fully
/// adopted for the same reason.
fn parse_adopt(stdout: &str) -> Result<AdoptOutcome, CliError> {
    let no_result = || CliError::new("cli_failed", "pidash __managed adopt printed no result");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).map_err(|_| no_result())?;
    if !value.get("adopted").is_some_and(|v| v.is_array()) {
        return Err(no_result());
    }
    let text = |entry: &serde_json::Value, key: &str| {
        entry
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let skipped = match value.get("skipped") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::Array(entries)) => entries
            .iter()
            .map(|entry| match entry {
                // The contract's first draft listed bare names.
                serde_json::Value::String(name) => Some(SkippedRunner {
                    name: name.clone(),
                    workspace_id: None,
                    workspace_slug: None,
                    reason: String::new(),
                }),
                serde_json::Value::Object(_) => Some(SkippedRunner {
                    name: text(entry, "name")?,
                    workspace_id: text(entry, "workspace_id"),
                    workspace_slug: text(entry, "workspace_slug"),
                    reason: text(entry, "reason").unwrap_or_default(),
                }),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(no_result)?,
        Some(_) => return Err(no_result()),
    };
    Ok(AdoptOutcome {
        fully_adopted: value.get("fully_adopted").and_then(|v| v.as_bool()) == Some(true),
        skipped,
    })
}

/// Stop the daemons earlier builds ran on their legacy config dirs.
///
/// Those builds stopped their daemons on quit, so normally there is none — but
/// one that crashed (or was killed) left an orphan that would keep serving
/// runners from a config about to be deleted. They are recognisable: earlier
/// builds ran the *bundled* binary as `<bundle>/bin/pidash __run`, while the
/// shared daemon — the service's, or this app's child — runs the installed
/// CLI. This build never starts the bundled binary as a daemon, so any such
/// process is an orphan. The daemon writes no PID file to go by.
#[cfg(unix)]
fn stop_orphaned_daemons(bundled: &Path) {
    let Ok(ps) = Command::new("ps").args(["-axo", "pid=,command="]).output() else {
        return;
    };
    let pids = orphan_pids(&String::from_utf8_lossy(&ps.stdout), bundled);
    for &pid in &pids {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
    // Give a run in flight a moment to finish, then make sure.
    let deadline = Instant::now() + Duration::from_secs(10);
    let alive = |pid: i32| unsafe { libc::kill(pid, 0) } == 0;
    while Instant::now() < deadline && pids.iter().any(|&pid| alive(pid)) {
        std::thread::sleep(Duration::from_millis(200));
    }
    for &pid in pids.iter().filter(|&&pid| alive(pid)) {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
}

/// Windows builds of the old app ran their daemons the same way, but there is
/// no `ps` to find an orphan by its command line; a crashed one goes away at
/// the next sign-out or reboot.
#[cfg(not(unix))]
fn stop_orphaned_daemons(_bundled: &Path) {}

/// PIDs in `ps -axo pid=,command=` output whose command is exactly
/// `<bundled> __run`.
#[cfg(any(unix, test))]
fn orphan_pids(ps: &str, bundled: &Path) -> Vec<i32> {
    let wanted = format!("{} __run", bundled.display());
    ps.lines()
        .filter_map(|line| {
            let (pid, command) = line.trim_start().split_once(' ')?;
            (command.trim() == wanted).then(|| pid.parse().ok()).flatten()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Make sure the shared daemon runs (installing the CLI and migrating legacy
/// config first) and report who runs it.
#[tauri::command]
pub async fn daemon_ensure<R: Runtime>(app: AppHandle<R>) -> Result<DaemonStatus, String> {
    Ok(ensure(&app).await)
}

/// The Repair action behind "Runs only while the app is open": retry the
/// service install.
#[tauri::command]
pub async fn daemon_repair<R: Runtime>(app: AppHandle<R>) -> Result<DaemonStatus, String> {
    Ok(repair(&app).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(code: i32, stdout: &str, stderr: &str) -> CliOutput {
        CliOutput {
            success: code == 0,
            code: Some(code),
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn json_errors_keep_the_cli_code() {
        let e = output(1, r#"{"error": "no_handoff_key", "message": "sign in again"}"#, "").error("x");
        assert_eq!(e, CliError::new("no_handoff_key", "sign in again"));
    }

    #[test]
    fn a_rejected_command_line_means_the_cli_is_outdated() {
        for stderr in [
            "error: unrecognized subcommand '__desktop'\n\nUsage: pidash <COMMAND>",
            "error: unexpected argument '--json' found\n",
        ] {
            assert!(output(2, "", stderr).error("x").is_outdated(), "{stderr}");
        }
        // Exit 2 alone is not enough: only clap's usage errors count.
        assert!(!output(2, "", "network unreachable").error("x").is_outdated());
        assert!(!output(1, "", "error: unexpected argument").error("x").is_outdated());
    }

    #[test]
    fn other_failures_carry_the_tail_of_stderr() {
        let stderr = (1..=10).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
        let e = output(1, "", &stderr).error("install");
        assert_eq!(e.code, "cli_failed");
        assert!(e.message.starts_with("pidash install failed:"));
        assert!(e.message.contains("line 10") && !e.message.contains("line 4\n"));
    }

    #[test]
    fn status_reports_the_socket_and_a_running_daemon() {
        let up = output(0, r#"{"service":"active","daemon":{"x":1},"ipc_socket":"/run/pidash/pidash.sock"}"#, "");
        assert_eq!(parse_status(&up).unwrap(), (PathBuf::from("/run/pidash/pidash.sock"), true));
        let down = output(0, r#"{"service":"inactive","daemon":null,"ipc_socket":"/s.sock"}"#, "");
        assert_eq!(parse_status(&down).unwrap(), (PathBuf::from("/s.sock"), false));
        assert!(parse_status(&output(2, "", "error: unexpected argument '--json'")).unwrap_err().is_outdated());
    }

    fn legacy(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::write(dir.join("config.toml"), "[daemon]\n").unwrap();
        dir
    }

    #[test]
    fn legacy_dirs_are_subdirs_with_a_config_then_the_flat_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("pidash");
        assert!(legacy_config_dirs(&root).is_empty(), "a missing root is nothing to migrate");
        legacy(&root, "beta");
        legacy(&root, "acme");
        std::fs::create_dir_all(root.join("data")).unwrap(); // no config: not a legacy dir
        assert_eq!(legacy_config_dirs(&root), vec![root.join("acme"), root.join("beta")]);
        std::fs::write(root.join("config.toml"), "").unwrap();
        assert_eq!(legacy_config_dirs(&root).last(), Some(&root));
    }

    fn fully() -> AdoptOutcome {
        AdoptOutcome { fully_adopted: true, skipped: Vec::new() }
    }

    fn skipped(name: &str, workspace_id: &str, reason: &str) -> SkippedRunner {
        SkippedRunner {
            name: name.into(),
            workspace_id: Some(workspace_id.into()),
            workspace_slug: None,
            reason: reason.into(),
        }
    }

    #[test]
    fn each_legacy_dir_is_deleted_only_after_its_adopt_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("pidash");
        let acme = legacy(&root, "acme");
        let beta = legacy(&root, "beta");
        let gamma = legacy(&root, "gamma");
        std::fs::write(root.join("config.toml"), "").unwrap();

        let mut seen = Vec::new();
        let result = migrate_legacy(&root, |dir| {
            seen.push(dir.to_path_buf());
            if dir == beta { Err(CliError::outdated()) } else { Ok(fully()) }
        });
        assert!(result.unwrap_err().is_outdated());
        assert_eq!(seen, vec![acme.clone(), beta.clone()]);
        assert!(!acme.exists(), "adopted dir must be removed");
        assert!(beta.exists() && gamma.exists(), "unadopted dirs must survive");
        assert!(root.join("config.toml").exists(), "the root goes only after everything in it");

        // Next launch, with a CLI that can adopt: everything goes, root last.
        let mut order = Vec::new();
        let migration = migrate_legacy(&root, |dir| {
            order.push(dir.to_path_buf());
            Ok(fully())
        })
        .unwrap();
        assert_eq!(migration.migrated, 3);
        assert!(migration.unadopted.is_empty());
        assert_eq!(order, vec![beta, gamma, root.clone()]);
        assert!(!root.exists());
        // And then it is a no-op.
        assert_eq!(migrate_legacy(&root, |_| panic!("nothing to adopt")).unwrap().migrated, 0);
    }

    #[test]
    fn a_partly_adopted_dir_is_kept_and_its_runners_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("pidash");
        let acme = legacy(&root, "acme");
        let beta = legacy(&root, "beta");
        std::fs::write(root.join("config.toml"), "").unwrap();

        // Signed in to acme: beta's runner belongs elsewhere, the root is fine.
        let migration = migrate_legacy(&root, |dir| {
            Ok(if dir == beta {
                AdoptOutcome {
                    fully_adopted: false,
                    skipped: vec![
                        skipped("desktop-b", "beta-ws", "workspace_mismatch"),
                        SkippedRunner { workspace_id: None, ..skipped("desktop-c", "", "workspace_unknown") },
                        skipped("desktop-a", "acme", "already_present"),
                    ],
                }
            } else {
                fully()
            })
        })
        .unwrap();
        assert!(!acme.exists());
        assert!(beta.join("config.toml").exists(), "its token is the runner's only credential");
        assert!(root.join("config.toml").exists(), "deleting the root would take beta with it");
        assert_eq!(migration.migrated, 1);
        assert_eq!(
            migration.unadopted,
            vec![
                UnadoptedRunner {
                    name: "desktop-b".into(),
                    workspace: "beta-ws".into(),
                    reason: "workspace_mismatch".into(),
                },
                UnadoptedRunner {
                    name: "desktop-c".into(),
                    workspace: "beta".into(),
                    reason: "workspace_unknown".into(),
                },
            ]
        );

        // The root's own runners are named by the slug adopt reports.
        std::fs::remove_dir_all(&beta).unwrap();
        let migration = migrate_legacy(&root, |_| {
            Ok(AdoptOutcome {
                fully_adopted: false,
                skipped: vec![skipped("old", "gamma", "not_signed_in")],
            })
        })
        .unwrap();
        assert!(root.join("config.toml").exists());
        assert_eq!(migration.unadopted[0].workspace, "gamma");

        // A later sign-in to that workspace: the next pass adopts and deletes.
        assert_eq!(migrate_legacy(&root, |_| Ok(fully())).unwrap().migrated, 1);
        assert!(!root.exists());
    }

    #[test]
    fn adopt_output_is_trusted_only_when_it_says_fully_adopted() {
        let full = parse_adopt(
            r#"{"adopted":[{"name":"a","workspace_id":"w1"}],"skipped":[],"fully_adopted":true}"#,
        )
        .unwrap();
        assert!(full.fully_adopted && full.skipped.is_empty());

        let partial = parse_adopt(
            r#"{"adopted":[],"skipped":[{"name":"b","workspace_id":"w2","reason":"workspace_mismatch"},{"name":"c","workspace_id":null,"reason":"workspace_unknown"}],"fully_adopted":false}"#,
        )
        .unwrap();
        assert!(!partial.fully_adopted);
        assert_eq!(partial.skipped[0], skipped("b", "w2", "workspace_mismatch"));
        assert_eq!(partial.skipped[1].workspace_id, None);
        // A failed adopt (exit 1) never gets this far; its error object is no report either.
        assert!(parse_adopt(r#"{"error":"adopt_failed","message":"x"}"#).is_err());

        // An earlier CLI without the flag never gets a dir deleted.
        let old = parse_adopt(r#"{"adopted":["a"],"skipped":["b"]}"#).unwrap();
        assert!(!old.fully_adopted);
        assert_eq!(old.skipped[0].name, "b");

        for bad in [
            "",
            "adopted 1 runner",
            r#"{"fully_adopted":true}"#,
            r#"{"adopted":{},"fully_adopted":true}"#,
            r#"{"adopted":[],"skipped":"b","fully_adopted":true}"#,
            r#"{"adopted":[],"skipped":[{"reason":"x"}],"fully_adopted":true}"#,
        ] {
            assert_eq!(parse_adopt(bad).unwrap_err().code, "cli_failed", "{bad}");
        }
    }

    #[test]
    fn orphans_are_only_the_bundled_binary_running_as_a_daemon() {
        let bundled = Path::new("/Applications/Pi Dash.app/Contents/Resources/bin/pidash");
        let ps = "    1 /sbin/launchd\n\
                  412 /Applications/Pi Dash.app/Contents/Resources/bin/pidash __run\n\
                  413 /Users/me/.local/bin/pidash __run\n\
                  414 /Applications/Pi Dash.app/Contents/Resources/bin/pidash __managed model-token --file x\n\
                 1415 /Applications/Pi Dash.app/Contents/Resources/bin/pidash __run --offline\n\
                 1416 /Applications/Pi Dash.app/Contents/Resources/bin/pidash __run\n";
        assert_eq!(orphan_pids(ps, bundled), vec![412, 1416]);
    }

    /// A stand-in `pidash` that records argv and env, for checking what the
    /// app actually runs.
    #[cfg(unix)]
    fn fake_cli(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("pidash");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Run `f`, retrying past ETXTBSY: a concurrently forking test can briefly
    /// inherit a just-written script's write handle.
    #[cfg(unix)]
    fn retry_busy<T>(mut f: impl FnMut() -> Result<T, String>) -> T {
        loop {
            match f() {
                Err(e) if e.contains("Text file busy") => std::thread::sleep(Duration::from_millis(20)),
                other => break other.unwrap(),
            }
        }
    }

    /// [`retry_busy`] for calls whose failure is the result under test.
    #[cfg(unix)]
    fn retry_busy_cli<T>(mut f: impl FnMut() -> Result<T, CliError>) -> Result<T, CliError> {
        loop {
            match f() {
                Err(e) if e.message.contains("Text file busy") => std::thread::sleep(Duration::from_millis(20)),
                other => break other,
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn adopt_runs_at_default_paths_and_needs_a_result() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = fake_cli(
            tmp.path(),
            "echo \"$* cfg=${PIDASH_CONFIG_DIR-unset} data=${PIDASH_DATA_DIR-unset}\" > \"$(dirname \"$0\")/argv\"\n\
             echo '{\"adopted\":[{\"name\":\"desktop-a\",\"workspace_id\":\"w\"}],\"skipped\":[],\"fully_adopted\":true}'",
        );
        let legacy = tmp.path().join("managed/pidash/acme");
        // SAFETY: tests in this binary never read these variables concurrently.
        unsafe { std::env::set_var("PIDASH_CONFIG_DIR", "/should/not/leak") };
        let result = retry_busy_cli(|| adopt(&cli, &legacy));
        unsafe { std::env::remove_var("PIDASH_CONFIG_DIR") };
        assert!(result.unwrap().fully_adopted);
        let argv = std::fs::read_to_string(tmp.path().join("argv")).unwrap();
        assert_eq!(
            argv.trim(),
            format!("__managed adopt --from-config-dir {} cfg=unset data=unset", legacy.display())
        );

        let silent = fake_cli(tmp.path(), "exit 0");
        let e = retry_busy_cli(|| adopt(&silent, &legacy)).unwrap_err();
        assert_eq!(e.code, "cli_failed", "exit 0 without a result must not count as adopted");
    }

    /// The child-process fallback runs the same daemon at the default paths,
    /// marked as desktop-supervised, with stdout and stderr both in the log.
    #[cfg(unix)]
    #[test]
    fn child_daemon_is_marked_desktop_supervised_and_logged() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = fake_cli(
            tmp.path(),
            "echo \"stdout from $1 supervisor=$PIDASH_SUPERVISOR data=${PIDASH_DATA_DIR-unset}\"\n\
             echo \"stderr from $1\" >&2",
        );
        let log = tmp.path().join("desktop-daemon.log");
        let mut child = retry_busy(|| spawn_daemon_process(&cli, &log));
        assert!(child.wait().unwrap().success());
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("stdout from __run supervisor=desktop data=unset"), "{text:?}");
        assert!(text.contains("stderr from __run"), "{text:?}");
    }

    #[cfg(unix)]
    #[test]
    fn run_cli_feeds_stdin_and_times_out() {
        let tmp = tempfile::tempdir().unwrap();
        let cat = fake_cli(tmp.path(), "cat");
        let out = retry_busy_cli(|| run_cli(&cat, &[], Some("secret"), CLI_TIMEOUT)).unwrap();
        assert!(out.success);
        assert_eq!(out.stdout, "secret");

        let slow = tmp.path().join("slow");
        std::fs::create_dir_all(&slow).unwrap();
        let sleeper = fake_cli(&slow, "sleep 5");
        let started = Instant::now();
        let e = retry_busy_cli(|| run_cli(&sleeper, &["install"], None, Duration::from_millis(300))).unwrap_err();
        assert!(e.message.contains("timed out"), "{e:?}");
        assert!(started.elapsed() < Duration::from_secs(4));
    }
}
