// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The app as a client of the computer's Pi Dash daemon
//! (`.ai_design/built_engine_as_runner/design.md` §4.1, §4.4).
//!
//! One computer has one daemon: the `pidash` CLI's, installed as a user
//! service, configured under the standard paths. The app finds it at the
//! default control socket — the same path an override-less `pidash` CLI
//! resolves (`runner/src/util/paths.rs`) — and never starts, stops or
//! re-roots it. Nothing here sets `PIDASH_CONFIG_DIR` / `PIDASH_DATA_DIR`.
//!
//! **Discovery** ([`daemon_discover`]) sends `Hello` and classifies the answer:
//!
//! | Answer                                   | Outcome                 |
//! | ---------------------------------------- | ----------------------- |
//! | nothing listens on the socket            | [`Discovery::NoDaemon`] |
//! | an error, or no `Hello` (pre-v5 daemon)  | [`Discovery::TooOld`]   |
//! | `ipc_version` below [`MIN_DAEMON_IPC`]   | [`Discovery::TooOld`]   |
//! | `ipc_version` above [`MAX_DAEMON_IPC`]   | [`Discovery::AppTooOld`]|
//! | no account signed in                     | [`Discovery::SignedOut`]|
//! | another account or cloud than the app's  | [`Discovery::AccountMismatch`] |
//! | the app's account                        | [`Discovery::Compatible`] |
//!
//! Until the outcome is `Compatible` the app's engine features are off; the
//! daemon and its runners keep working whatever the app decides.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pidash_ipc::protocol::{
    EngineRunnerInfo, EngineSettings, EngineStatusInfo, HelloAccount, HelloInfo, Request, Response,
    RunnerChanges, StatusSnapshot,
};
use serde::{Deserialize, Serialize};
use tauri::Url;
use uuid::Uuid;

/// Oldest daemon protocol the app works with: v6 adds the engine requests
/// (`EngineStatus`, `EngineRunnerAdd`, `EngineSettings*`) the app relies on.
pub const MIN_DAEMON_IPC: u32 = 6;
/// Newest daemon protocol this build understands.
pub const MAX_DAEMON_IPC: u32 = pidash_ipc::IPC_VERSION;

/// How long a `Hello` may take before the daemon counts as not answering.
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// Ordinary round-trips.
const CALL_TIMEOUT: Duration = Duration::from_secs(20);
/// `EngineRunnerAdd` / `Remove` call the server and may wait for a runner to
/// start.
const ENROL_TIMEOUT: Duration = Duration::from_secs(90);

// ---------------------------------------------------------------------------
// Where the daemon lives
// ---------------------------------------------------------------------------

/// The standard daemon's directories, resolved exactly as the CLI resolves
/// them with no override: `ProjectDirs("so", "pidash", "pidash")`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub runtime_dir: PathBuf,
}

impl DaemonPaths {
    pub fn standard() -> Result<Self, String> {
        let dirs = directories::ProjectDirs::from("so", "pidash", "pidash")
            .ok_or("unable to resolve the standard Pi Dash directories")?;
        let data_dir = dirs.data_dir().to_path_buf();
        let runtime_dir = dirs
            .runtime_dir()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| data_dir.join("runtime"));
        Ok(Self {
            config_dir: dirs.config_dir().to_path_buf(),
            data_dir,
            runtime_dir,
        })
    }

    /// `<runtime_dir>/pidash.sock`, byte-identical to the daemon's
    /// `Paths::ipc_socket_path` (a named pipe derived from it on Windows).
    pub fn socket(&self) -> PathBuf {
        self.runtime_dir.join("pidash.sock")
    }

    /// The daemon's engine home for `account`:
    /// `<daemon data>/engine/<account>/codex-home` (design §5), keyed like
    /// the runner's `engine::codex_config::account_dir_name`.
    pub fn engine_home(&self, account: Option<&str>) -> PathBuf {
        self.data_dir
            .join("engine")
            .join(account_dir_name(account))
            .join("codex-home")
    }
}

/// Mirror of `runner/src/engine/codex_config.rs::account_dir_name`. Kept
/// byte-identical so the migration moves a legacy engine home to exactly the
/// directory the daemon points the engine at.
pub fn account_dir_name(account: Option<&str>) -> String {
    let Some(raw) = account.map(str::trim).filter(|s| !s.is_empty()) else {
        return "default".to_string();
    };
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@') {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .take(96)
        .collect();
    let cleaned = cleaned.trim_matches('.').to_string();
    if cleaned.is_empty() {
        "default".to_string()
    } else {
        cleaned
    }
}

fn socket() -> Result<PathBuf, String> {
    Ok(DaemonPaths::standard()?.socket())
}

// ---------------------------------------------------------------------------
// Round-trips
// ---------------------------------------------------------------------------

/// Why a request got no usable answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// Nothing listens on the socket: no daemon is running.
    Unreachable(String),
    /// A daemon may be there but did not take the connection or answer in
    /// time (a busy pipe, a slow daemon). Not "no daemon", not "too old".
    NoAnswer(String),
    /// The daemon answered with an error (its message), or not in time, or
    /// with something that is not the expected frame.
    Failed(String),
}

impl CallError {
    pub fn message(&self) -> String {
        match self {
            Self::Unreachable(e) => format!("Pi Dash is not running on this computer ({e})"),
            Self::NoAnswer(e) => format!("Pi Dash on this computer is not answering ({e})"),
            Self::Failed(e) => e.clone(),
        }
    }
}

/// One request, one answer, on a fresh connection.
pub async fn call_at(
    socket: &Path,
    req: Request,
    timeout: Duration,
) -> Result<Response, CallError> {
    let mut client = pidash_ipc::Client::connect(socket)
        .await
        .map_err(classify_connect_error)?;
    match tokio::time::timeout(timeout, client.call(req)).await {
        Err(_) => Err(CallError::NoAnswer(
            "the Pi Dash daemon did not answer in time".into(),
        )),
        Ok(Err(e)) => Err(CallError::Failed(format!("{e:#}"))),
        Ok(Ok(Response::Error(err))) => Err(CallError::Failed(err.message)),
        Ok(Ok(resp)) => Ok(resp),
    }
}

/// Only "nothing is there" (no socket or pipe, or a stale socket nobody
/// listens on) means no daemon. Anything else, such as a pipe that stays
/// busy or a permission error, is a daemon that did not answer.
fn classify_connect_error(e: anyhow::Error) -> CallError {
    let message = format!("{e:#}");
    let absent = e
        .chain()
        .filter_map(|c| c.downcast_ref::<std::io::Error>())
        .any(|io| {
            matches!(
                io.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            )
        });
    if absent {
        CallError::Unreachable(message)
    } else {
        CallError::NoAnswer(message)
    }
}

async fn call(req: Request, timeout: Duration) -> Result<Response, String> {
    call_at(&socket()?, req, timeout)
        .await
        .map_err(|e| e.message())
}

/// What a `Hello` probe found.
#[derive(Debug, Clone, PartialEq)]
pub enum HelloProbe {
    /// No daemon listens at the socket.
    Unreachable,
    /// Something listens but did not answer `Hello`: a daemon older than v5.
    NoHello(String),
    /// A daemon may be there but did not answer in time or took no
    /// connection (busy). Asked again later, not reported as too old.
    NoAnswer(String),
    Answered(Box<HelloInfo>),
}

pub async fn probe_hello(socket: &Path) -> HelloProbe {
    match call_at(socket, Request::Hello, HELLO_TIMEOUT).await {
        Err(CallError::Unreachable(_)) => HelloProbe::Unreachable,
        Err(CallError::NoAnswer(e)) => HelloProbe::NoAnswer(e),
        Err(CallError::Failed(e)) => HelloProbe::NoHello(e),
        Ok(Response::Hello(info)) => HelloProbe::Answered(Box::new(info)),
        Ok(other) => HelloProbe::NoHello(format!("unexpected answer to Hello: {other:?}")),
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// The app's signed-in account, as the webview knows it.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppIdentity {
    pub user_id: String,
    #[serde(default)]
    pub email: Option<String>,
    /// The cloud the app is signed in to (its `API_BASE_URL`).
    pub cloud_url: String,
}

/// The installed CLI, for the set-up and update offers.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct CliInfo {
    pub installed: bool,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    /// Whether the app can replace the installed CLI with the copy it
    /// carries ("Update Pi Dash on this computer"). `false` for an install
    /// under a system location (an MSI under `Program Files`, `/usr/…`).
    pub updatable: bool,
    /// Why it cannot, in the user's words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Discovery {
    /// No daemon on this computer: the app is a cloud client and offers set-up.
    NoDaemon {
        cli: CliInfo,
    },
    /// A daemon that predates the protocol the app needs.
    TooOld {
        #[serde(skip_serializing_if = "Option::is_none")]
        daemon_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        ipc_version: Option<u32>,
        cli: CliInfo,
    },
    /// A daemon that did not answer in time, or whose pipe stayed busy.
    /// Not "too old": the app asks again.
    NotAnswering {
        reason: String,
    },
    /// A daemon newer than this app understands: update the app.
    AppTooOld {
        daemon_version: String,
        ipc_version: u32,
    },
    /// The daemon runs but no workspace is signed in.
    SignedOut {
        hello: Box<HelloInfo>,
    },
    /// Signed in as someone else, or to another cloud (design §4.6, PO-13).
    AccountMismatch {
        /// How to name the daemon's account to the user.
        daemon_account: String,
        daemon_cloud_url: String,
        /// Whether the daemon hosts runners; "Sign Pi Dash on this computer
        /// in as me" is offered only when it hosts none.
        hosts_runners: bool,
    },
    Compatible {
        hello: Box<HelloInfo>,
    },
}

/// Classify a `Hello` probe for the app's account. Pure, so every outcome is
/// tested without a daemon.
pub fn evaluate(probe: HelloProbe, app: &AppIdentity, cli: CliInfo) -> Discovery {
    let hello = match probe {
        HelloProbe::Unreachable => return Discovery::NoDaemon { cli },
        HelloProbe::NoHello(_) => {
            return Discovery::TooOld {
                daemon_version: None,
                ipc_version: None,
                cli,
            };
        }
        HelloProbe::NoAnswer(reason) => return Discovery::NotAnswering { reason },
        HelloProbe::Answered(hello) => hello,
    };
    if hello.ipc_version < MIN_DAEMON_IPC {
        return Discovery::TooOld {
            daemon_version: Some(hello.daemon_version.clone()),
            ipc_version: Some(hello.ipc_version),
            cli,
        };
    }
    if hello.ipc_version > MAX_DAEMON_IPC {
        return Discovery::AppTooOld {
            daemon_version: hello.daemon_version.clone(),
            ipc_version: hello.ipc_version,
        };
    }
    let Some(account) = hello.account.clone() else {
        return Discovery::SignedOut { hello };
    };
    let daemon_cloud = if account.cloud_url.is_empty() {
        hello.cloud_url.clone()
    } else {
        account.cloud_url.clone()
    };
    if !same_cloud(&app.cloud_url, &daemon_cloud) || !same_account(app, &account) {
        return Discovery::AccountMismatch {
            daemon_account: account
                .email
                .clone()
                .or_else(|| account.user_id.clone())
                .unwrap_or_else(|| "another account".to_string()),
            daemon_cloud_url: daemon_cloud,
            hosts_runners: hello.workspaces.iter().any(|w| w.runners > 0),
        };
    }
    Discovery::Compatible { hello }
}

/// Same Pi Dash cloud: scheme, host and port, whatever the path or a
/// trailing slash.
pub fn same_cloud(a: &str, b: &str) -> bool {
    match (Url::parse(a.trim()), Url::parse(b.trim())) {
        (Ok(a), Ok(b)) => a.origin() == b.origin(),
        _ => a
            .trim()
            .trim_end_matches('/')
            .eq_ignore_ascii_case(b.trim().trim_end_matches('/')),
    }
}

/// Same person. The user id decides when the daemon recorded one; an e-mail
/// (case-insensitive) otherwise. A daemon that recorded neither (a sign-in
/// from before accounts were recorded) cannot be shown to differ, so it is
/// taken as the same account rather than locking the user out.
pub fn same_account(app: &AppIdentity, daemon: &HelloAccount) -> bool {
    if let Some(id) = daemon.user_id.as_deref().filter(|s| !s.is_empty()) {
        return id == app.user_id;
    }
    match (daemon.email.as_deref(), app.email.as_deref()) {
        (Some(d), Some(a)) if !d.is_empty() => d.trim().eq_ignore_ascii_case(a.trim()),
        (Some(d), None) if !d.is_empty() => false,
        _ => true,
    }
}

/// Whether the app may replace the CLI at `cli`: a user-writable directory
/// that is not a system location.
pub fn cli_update_offer(cli: Option<&Path>) -> (bool, Option<String>) {
    let Some(cli) = cli else {
        return (false, None);
    };
    let dir = cli.parent().unwrap_or(Path::new(""));
    let lower = dir.to_string_lossy().to_ascii_lowercase();
    let system = lower.contains("program files")
        || [
            "/usr/bin",
            "/usr/local/bin",
            "/bin",
            "/opt/",
            "/home/linuxbrew",
            "/opt/homebrew",
        ]
        .iter()
        .any(|p| lower.starts_with(p));
    if system || !dir_writable(dir) {
        return (
            false,
            Some(format!(
                "Pi Dash is installed in {}, which this app cannot update. Update it the way it was installed (for example `pidash update`, or a new installer).",
                dir.display()
            )),
        );
    }
    (true, None)
}

#[cfg(unix)]
fn dir_writable(dir: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c` is a valid NUL-terminated path for the duration of the call.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

#[cfg(not(unix))]
fn dir_writable(dir: &Path) -> bool {
    std::fs::metadata(dir).is_ok_and(|m| !m.permissions().readonly())
}

fn cli_info() -> CliInfo {
    let status = crate::pidash_cli::detect_pidash_cli_sync();
    let (updatable, update_hint) = cli_update_offer(status.path.as_deref());
    CliInfo {
        installed: status.installed,
        path: status.path,
        version: status.version,
        updatable,
        update_hint,
    }
}

// ---------------------------------------------------------------------------
// The app's account, for native checks (PO-13)
// ---------------------------------------------------------------------------

/// The account the webview last asked discovery about: the app's signed-in
/// account. Native commands that sign the daemon in check against it, so
/// PO-13 does not rest on the UI alone.
static APP_IDENTITY: std::sync::Mutex<Option<AppIdentity>> = std::sync::Mutex::new(None);

fn remember_app_identity(identity: &AppIdentity) {
    *APP_IDENTITY.lock().unwrap_or_else(|e| e.into_inner()) = Some(identity.clone());
}

fn app_identity() -> Option<AppIdentity> {
    APP_IDENTITY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Whether the app may sign the daemon in now (design §4.6, PO-13): refused
/// when the daemon is signed in as another account or to another cloud,
/// unless `force` ("Sign Pi Dash on this computer in as me") and it hosts
/// no runners. Asked again right before the sign-in, so an account change
/// since the consent screen (a `pidash auth login` in a terminal) is caught.
pub fn login_allowed(probe: HelloProbe, app: &AppIdentity, force: bool) -> Result<(), String> {
    match evaluate(probe, app, CliInfo::default()) {
        Discovery::AccountMismatch {
            daemon_account,
            hosts_runners,
            ..
        } => {
            if force && !hosts_runners {
                return Ok(());
            }
            Err(if hosts_runners {
                format!(
                    "Pi Dash on this computer is signed in as {daemon_account} and hosts runners. \
                     Run `pidash auth logout` in a terminal first to use it with this account."
                )
            } else {
                format!(
                    "Pi Dash on this computer is signed in as {daemon_account}, not this account."
                )
            })
        }
        _ => Ok(()),
    }
}

/// [`login_allowed`] against the running daemon and the account the app
/// last reported. No check when the app has not reported one yet.
pub async fn check_login_allowed(force: bool) -> Result<(), String> {
    let Some(app) = app_identity() else {
        return Ok(());
    };
    login_allowed(probe_hello(&socket()?).await, &app, force)
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Find the computer's daemon and say whether the app can work with it.
#[tauri::command]
pub async fn daemon_discover(app_identity: AppIdentity) -> Result<Discovery, String> {
    remember_app_identity(&app_identity);
    let probe = probe_hello(&socket()?).await;
    let cli = tauri::async_runtime::spawn_blocking(cli_info)
        .await
        .map_err(|e| format!("detecting the CLI: {e}"))?;
    Ok(evaluate(probe, &app_identity, cli))
}

#[tauri::command]
pub async fn daemon_status() -> Result<StatusSnapshot, String> {
    match call(Request::StatusGet, CALL_TIMEOUT).await? {
        Response::Status(s) => Ok(s),
        other => Err(unexpected(&other)),
    }
}

#[tauri::command]
pub async fn daemon_engine_status() -> Result<EngineStatusInfo, String> {
    match call(Request::EngineStatus, CALL_TIMEOUT).await? {
        Response::EngineStatus(s) => Ok(s),
        other => Err(unexpected(&other)),
    }
}

/// The owner's "Pause the built-in engine" switch for this computer
/// (design §8). The daemon asks the server and answers with the engine
/// status, pause state included.
#[tauri::command]
pub async fn daemon_engine_set_paused(paused: bool) -> Result<EngineStatusInfo, String> {
    match call(Request::EngineSetPaused { paused }, CALL_TIMEOUT).await? {
        Response::EngineStatus(s) => Ok(s),
        other => Err(unexpected(&other)),
    }
}

#[tauri::command]
pub async fn daemon_engine_settings_get() -> Result<EngineSettings, String> {
    match call(Request::EngineSettingsGet, CALL_TIMEOUT).await? {
        Response::EngineSettings(s) => Ok(s),
        other => Err(unexpected(&other)),
    }
}

/// Change the task root (`None` = back to the default). The daemon validates
/// it (design §10); its refusal names the rule that failed and is passed to
/// the webview as the error.
#[tauri::command]
pub async fn daemon_engine_settings_set(
    task_root: Option<String>,
) -> Result<EngineSettings, String> {
    let task_root = task_root
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from);
    match call(Request::EngineSettingsSet { task_root }, CALL_TIMEOUT).await? {
        Response::EngineSettings(s) => Ok(s),
        other => Err(unexpected(&other)),
    }
}

/// Turn the built-in engine on for a project (design §5).
#[tauri::command]
pub async fn daemon_engine_runner_add(
    workspace: String,
    project: String,
    working_dir: Option<String>,
) -> Result<EngineRunnerInfo, String> {
    let req = Request::EngineRunnerAdd {
        workspace,
        project,
        working_dir: working_dir
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from),
    };
    match call(req, ENROL_TIMEOUT).await? {
        Response::EngineRunnerAdded(info) => Ok(info),
        other => Err(unexpected(&other)),
    }
}

#[tauri::command]
pub async fn daemon_engine_runner_remove(runner: String) -> Result<(), String> {
    match call(Request::EngineRunnerRemove { runner }, ENROL_TIMEOUT).await? {
        Response::Ack => Ok(()),
        other => Err(unexpected(&other)),
    }
}

/// Re-read the workspace tokens after the CLI signed one in or out.
#[tauri::command]
pub async fn daemon_credentials_reload() -> Result<(), String> {
    match call(Request::CredentialsReload, CALL_TIMEOUT).await? {
        Response::Ack => Ok(()),
        other => Err(unexpected(&other)),
    }
}

/// One `[[runner]]` block of the daemon, without anything secret.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalRunner {
    pub name: String,
    pub runner_id: Uuid,
    pub workspace_slug: Option<String>,
    pub project_slug: Option<String>,
    pub working_dir: PathBuf,
    /// `agent.kind`: `engine` for the built-in engine.
    pub agent: String,
}

/// Pick the runner blocks out of the daemon's `ConfigGet`. Only these fields
/// cross into the webview; tokens and the rest of the config never do.
pub fn local_runners(config: &serde_json::Value) -> Vec<LocalRunner> {
    let Some(blocks) = config.get("runner").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter_map(|b| {
            let s = |key: &str| b.get(key).and_then(|v| v.as_str()).map(str::to_string);
            Some(LocalRunner {
                name: s("name")?,
                runner_id: s("runner_id")?.parse().ok()?,
                workspace_slug: s("workspace_slug"),
                project_slug: s("project_slug"),
                working_dir: PathBuf::from(b.get("workspace")?.get("working_dir")?.as_str()?),
                agent: b
                    .get("agent")
                    .and_then(|a| a.get("kind"))
                    .and_then(|k| k.as_str())
                    .unwrap_or("codex")
                    .to_string(),
            })
        })
        .collect()
}

/// The daemon's runners: the engine's per-project folders for the settings
/// page, and this computer's engine runner ids for the nav (design §12.1).
#[tauri::command]
pub async fn daemon_runners() -> Result<Vec<LocalRunner>, String> {
    match call(Request::ConfigGet, CALL_TIMEOUT).await? {
        Response::Config(cfg) => Ok(local_runners(&cfg)),
        other => Err(unexpected(&other)),
    }
}

/// A fresh folder for `project` under `root`, following the daemon's
/// allocation rule (design §10): `<root>/<workspace>/<project>/`, with a
/// numeric suffix when that path is some runner's folder or exists and is not
/// empty.
pub fn allocate_folder(root: &Path, workspace: &str, project: &str, taken: &[PathBuf]) -> PathBuf {
    let base = root.join(workspace).join(project);
    let busy = |p: &Path| {
        taken.iter().any(|t| t == p)
            || std::fs::read_dir(p).is_ok_and(|mut entries| entries.next().is_some())
    };
    if !busy(&base) {
        return base;
    }
    (2..)
        .map(|n| root.join(workspace).join(format!("{project}-{n}")))
        .find(|p| !busy(p))
        .expect("an unbounded range always finds a free suffix")
}

/// "Use the default location" (design §10): rebind one engine runner to a
/// fresh folder under the current task root. The old folder stays on disk.
/// The daemon restarts only that runner, and refuses while it is busy.
#[tauri::command]
pub async fn daemon_engine_runner_rebind(runner: String) -> Result<PathBuf, String> {
    let settings = daemon_engine_settings_get().await?;
    let runners = daemon_runners().await?;
    let target = runners
        .iter()
        .find(|r| r.name == runner)
        .ok_or_else(|| format!("no runner named {runner:?} on this computer"))?;
    if target.working_dir.starts_with(&settings.task_root) {
        return Ok(target.working_dir.clone());
    }
    let taken: Vec<PathBuf> = runners.iter().map(|r| r.working_dir.clone()).collect();
    let folder = allocate_folder(
        &settings.task_root,
        target.workspace_slug.as_deref().unwrap_or("default"),
        target.project_slug.as_deref().unwrap_or(&target.name),
        &taken,
    );
    set_runner_working_dir(runner, folder.clone()).await?;
    Ok(folder)
}

/// Point `runner` at `folder` (per-runner reconfigure: only that runner
/// restarts, refused while it is busy). Callers confine `folder`.
pub(crate) async fn set_runner_working_dir(runner: String, folder: PathBuf) -> Result<(), String> {
    let changes = RunnerChanges {
        working_dir: Some(folder),
    };
    match call(Request::RunnerReconfigure { runner, changes }, CALL_TIMEOUT).await? {
        Response::Ack => Ok(()),
        other => Err(unexpected(&other)),
    }
}

fn unexpected(resp: &Response) -> String {
    format!("unexpected answer from the Pi Dash daemon: {resp:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_ipc::protocol::{EngineSignInState, EngineState, HelloWorkspace};

    fn app() -> AppIdentity {
        AppIdentity {
            user_id: "u-1".into(),
            email: Some("Dev@Example.com".into()),
            cloud_url: "https://pidash.example.com".into(),
        }
    }

    fn hello(ipc: u32, account: Option<HelloAccount>) -> HelloInfo {
        HelloInfo {
            daemon_version: "0.2.0".into(),
            ipc_version: ipc,
            cloud_url: "https://pidash.example.com/".into(),
            account,
            workspaces: vec![HelloWorkspace {
                slug: "acme".into(),
                signed_in: true,
                runners: 0,
            }],
            machine_id: Some(Uuid::nil()),
            engine: EngineState::Available,
            engine_detail: Some(EngineStatusInfo {
                state: EngineState::Available,
                reason_code: None,
                sign_in_state: EngineSignInState::Active,
                credential_expires_at: None,
                model: None,
                binary: None,
                profile_fetched_at: None,
                paused_by_owner: None,
                auto_pause_reason: None,
                pause_checked_at: None,
            }),
        }
    }

    fn account(user_id: Option<&str>, email: Option<&str>, cloud: &str) -> HelloAccount {
        HelloAccount {
            user_id: user_id.map(str::to_string),
            email: email.map(str::to_string),
            cloud_url: cloud.into(),
        }
    }

    fn answered(h: HelloInfo) -> HelloProbe {
        HelloProbe::Answered(Box::new(h))
    }

    #[test]
    fn no_socket_means_no_daemon_and_set_up_is_offered() {
        let cli = CliInfo {
            installed: true,
            ..Default::default()
        };
        let out = evaluate(HelloProbe::Unreachable, &app(), cli.clone());
        assert_eq!(out, Discovery::NoDaemon { cli });
    }

    #[test]
    fn a_daemon_that_does_not_answer_hello_is_too_old() {
        let out = evaluate(
            HelloProbe::NoHello("unknown method".into()),
            &app(),
            CliInfo::default(),
        );
        assert!(
            matches!(
                out,
                Discovery::TooOld {
                    daemon_version: None,
                    ..
                }
            ),
            "{out:?}"
        );
    }

    #[test]
    fn a_daemon_that_does_not_answer_in_time_is_not_reported_too_old() {
        let out = evaluate(
            HelloProbe::NoAnswer("did not answer in time".into()),
            &app(),
            CliInfo::default(),
        );
        assert!(matches!(out, Discovery::NotAnswering { .. }), "{out:?}");
    }

    #[test]
    fn only_a_missing_or_refused_endpoint_means_no_daemon() {
        let gone = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::NotFound))
            .context("connecting");
        assert!(matches!(
            classify_connect_error(gone),
            CallError::Unreachable(_)
        ));
        let refused =
            anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::ConnectionRefused));
        assert!(matches!(
            classify_connect_error(refused),
            CallError::Unreachable(_)
        ));
        // ERROR_PIPE_BUSY that outlasted the client's retries, a permission error…
        let busy = anyhow::Error::new(std::io::Error::from_raw_os_error(231)).context("connecting");
        assert!(matches!(
            classify_connect_error(busy),
            CallError::NoAnswer(_)
        ));
        let denied = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert!(matches!(
            classify_connect_error(denied),
            CallError::NoAnswer(_)
        ));
    }

    /// PO-13 natively: a sign-in is refused when the daemon is signed in as
    /// another account, unless forced on a daemon that hosts no runners.
    #[test]
    fn login_is_refused_for_another_account_unless_forced_on_an_empty_daemon() {
        let me = Some(account(Some("u-1"), None, "https://pidash.example.com"));
        assert!(login_allowed(answered(hello(MAX_DAEMON_IPC, me)), &app(), false).is_ok());
        assert!(login_allowed(HelloProbe::Unreachable, &app(), false).is_ok());
        assert!(login_allowed(answered(hello(MAX_DAEMON_IPC, None)), &app(), false).is_ok());

        let other = Some(account(
            Some("u-2"),
            Some("someone@example.com"),
            "https://pidash.example.com",
        ));
        let err = login_allowed(
            answered(hello(MAX_DAEMON_IPC, other.clone())),
            &app(),
            false,
        )
        .unwrap_err();
        assert!(err.contains("someone@example.com"), "{err}");
        assert!(
            login_allowed(answered(hello(MAX_DAEMON_IPC, other.clone())), &app(), true).is_ok()
        );

        let mut busy = hello(MAX_DAEMON_IPC, other);
        busy.workspaces[0].runners = 2;
        let err = login_allowed(answered(busy), &app(), true).unwrap_err();
        assert!(err.contains("hosts runners"), "{err}");
    }

    #[test]
    fn protocol_below_the_minimum_is_too_old_and_above_the_maximum_asks_for_an_app_update() {
        let me = Some(account(Some("u-1"), None, "https://pidash.example.com"));
        let out = evaluate(
            answered(hello(MIN_DAEMON_IPC - 1, me.clone())),
            &app(),
            CliInfo::default(),
        );
        assert!(
            matches!(out, Discovery::TooOld { ipc_version: Some(v), .. } if v == MIN_DAEMON_IPC - 1),
            "{out:?}"
        );
        let out = evaluate(
            answered(hello(MAX_DAEMON_IPC + 1, me)),
            &app(),
            CliInfo::default(),
        );
        assert!(matches!(out, Discovery::AppTooOld { .. }), "{out:?}");
    }

    #[test]
    fn the_same_account_is_compatible() {
        let me = Some(account(
            Some("u-1"),
            Some("other@example.com"),
            "https://pidash.example.com",
        ));
        let out = evaluate(
            answered(hello(MAX_DAEMON_IPC, me)),
            &app(),
            CliInfo::default(),
        );
        assert!(matches!(out, Discovery::Compatible { .. }), "{out:?}");
        // E-mail alone, any case.
        let me = Some(account(
            None,
            Some("dev@example.COM"),
            "https://pidash.example.com/",
        ));
        let out = evaluate(
            answered(hello(MAX_DAEMON_IPC, me)),
            &app(),
            CliInfo::default(),
        );
        assert!(matches!(out, Discovery::Compatible { .. }), "{out:?}");
    }

    #[test]
    fn another_account_is_refused_with_its_name() {
        let other = Some(account(
            Some("u-2"),
            Some("someone@example.com"),
            "https://pidash.example.com",
        ));
        let mut h = hello(MAX_DAEMON_IPC, other);
        h.workspaces[0].runners = 2;
        let out = evaluate(answered(h), &app(), CliInfo::default());
        assert_eq!(
            out,
            Discovery::AccountMismatch {
                daemon_account: "someone@example.com".into(),
                daemon_cloud_url: "https://pidash.example.com".into(),
                hosts_runners: true,
            }
        );
    }

    #[test]
    fn the_same_user_on_another_cloud_is_refused() {
        let elsewhere = Some(account(
            Some("u-1"),
            None,
            "https://other-cloud.example.org",
        ));
        let out = evaluate(
            answered(hello(MAX_DAEMON_IPC, elsewhere)),
            &app(),
            CliInfo::default(),
        );
        assert!(
            matches!(&out, Discovery::AccountMismatch { hosts_runners: false, daemon_cloud_url, .. } if daemon_cloud_url.contains("other-cloud")),
            "{out:?}"
        );
    }

    #[test]
    fn no_signed_in_account_is_signed_out() {
        let out = evaluate(
            answered(hello(MAX_DAEMON_IPC, None)),
            &app(),
            CliInfo::default(),
        );
        assert!(matches!(out, Discovery::SignedOut { .. }), "{out:?}");
    }

    #[test]
    fn discovery_serialises_with_a_kind_tag() {
        let out = evaluate(HelloProbe::Unreachable, &app(), CliInfo::default());
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["kind"], "no_daemon");
        assert_eq!(json["cli"]["installed"], false);
    }

    #[test]
    fn same_cloud_ignores_path_and_trailing_slash_but_not_host_or_scheme() {
        assert!(same_cloud(
            "https://a.example.com",
            "https://a.example.com/"
        ));
        assert!(same_cloud(
            "https://A.example.com/api",
            "https://a.example.com"
        ));
        assert!(!same_cloud("https://a.example.com", "http://a.example.com"));
        assert!(!same_cloud(
            "https://a.example.com",
            "https://b.example.com"
        ));
        assert!(!same_cloud(
            "http://localhost:8000",
            "http://localhost:3000"
        ));
    }

    #[test]
    fn account_dir_name_matches_the_runner() {
        // Same cases as runner/src/engine/codex_config.rs.
        assert_eq!(account_dir_name(Some("Dev@Example.com")), "dev@example.com");
        assert_eq!(account_dir_name(None), "default");
        assert_eq!(account_dir_name(Some("../../etc")), "_.._etc");
        let paths = DaemonPaths {
            config_dir: "/c".into(),
            data_dir: "/d".into(),
            runtime_dir: "/r".into(),
        };
        assert_eq!(
            paths.engine_home(Some("u-1")),
            PathBuf::from("/d/engine/u-1/codex-home")
        );
        assert_eq!(paths.socket(), PathBuf::from("/r/pidash.sock"));
    }

    #[test]
    fn local_runners_keeps_only_the_block_fields() {
        let cfg = serde_json::json!({
            "cli": { "token": "mt_secret" },
            "runner": [
                {
                    "name": "builtin-host-acme-web",
                    "runner_id": "00000000-0000-0000-0000-000000000001",
                    "workspace_slug": "acme",
                    "project_slug": "WEB",
                    "workspace": { "working_dir": "/tasks/acme/WEB" },
                    "agent": { "kind": "engine" }
                },
                {
                    "name": "mine",
                    "runner_id": "00000000-0000-0000-0000-000000000002",
                    "workspace": { "working_dir": "/src/mine" }
                },
                { "name": "broken" }
            ]
        });
        let runners = local_runners(&cfg);
        assert_eq!(runners.len(), 2);
        assert_eq!(runners[0].agent, "engine");
        assert_eq!(runners[0].project_slug.as_deref(), Some("WEB"));
        assert_eq!(runners[1].agent, "codex");
        let json = serde_json::to_string(&runners).unwrap();
        assert!(!json.contains("mt_secret"));
    }

    #[test]
    fn allocate_folder_suffixes_taken_or_non_empty_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert_eq!(
            allocate_folder(root, "acme", "WEB", &[]),
            root.join("acme/WEB")
        );
        // Some runner's folder.
        let taken = vec![root.join("acme/WEB")];
        assert_eq!(
            allocate_folder(root, "acme", "WEB", &taken),
            root.join("acme/WEB-2")
        );
        // Exists and not empty.
        std::fs::create_dir_all(root.join("acme/WEB-2")).unwrap();
        std::fs::write(root.join("acme/WEB-2/file"), "x").unwrap();
        assert_eq!(
            allocate_folder(root, "acme", "WEB", &taken),
            root.join("acme/WEB-3")
        );
        // An empty existing folder is free.
        std::fs::create_dir_all(root.join("acme/API")).unwrap();
        assert_eq!(
            allocate_folder(root, "acme", "API", &[]),
            root.join("acme/API")
        );
    }

    #[test]
    fn cli_update_is_not_offered_for_system_locations() {
        let (ok, hint) = cli_update_offer(Some(Path::new("/usr/local/bin/pidash")));
        assert!(!ok);
        assert!(hint.unwrap().contains("/usr/local/bin"));
        let (ok, _) = cli_update_offer(Some(Path::new(r"C:/Program Files/pidash/pidash.exe")));
        assert!(!ok);
        let tmp = tempfile::tempdir().unwrap();
        let (ok, hint) = cli_update_offer(Some(&tmp.path().join("pidash")));
        assert!(ok, "{hint:?}");
        assert_eq!(cli_update_offer(None), (false, None));
    }

    /// The probe over a real socket: a stub that answers like a pre-v5
    /// daemon (an `unknown method` error) is "no Hello"; a v6 stub answers.
    #[cfg(unix)]
    #[tokio::test]
    async fn probe_hello_over_the_socket() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixListener;

        async fn serve(socket: PathBuf, answer: serde_json::Value) {
            let listener = UnixListener::bind(&socket).unwrap();
            tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                assert!(line.contains("\"hello\""), "{line}");
                let mut bytes = serde_json::to_vec(&answer).unwrap();
                bytes.push(b'\n');
                reader.get_mut().write_all(&bytes).await.unwrap();
            });
        }

        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            probe_hello(&dir.path().join("none.sock")).await,
            HelloProbe::Unreachable
        );

        let old = dir.path().join("old.sock");
        serve(
            old.clone(),
            serde_json::json!({"result": "error", "data": {"code": 400, "message": "unknown method"}}),
        )
        .await;
        assert!(matches!(probe_hello(&old).await, HelloProbe::NoHello(_)));

        let new = dir.path().join("new.sock");
        let h = hello(MAX_DAEMON_IPC, None);
        serve(
            new.clone(),
            serde_json::to_value(Response::Hello(h.clone())).unwrap(),
        )
        .await;
        assert_eq!(probe_hello(&new).await, HelloProbe::Answered(Box::new(h)));
    }
}
