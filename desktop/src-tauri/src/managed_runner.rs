// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Provisioning and supervision of the built-in agent engine.
//!
//! The desktop app ships three things: this window, the `pidash` daemon, and
//! the agent engine. This module owns the engine — it writes its
//! configuration, registers its runners, keeps the model credential fresh on
//! disk, and tears the credential down on sign-out. The daemon itself is the
//! one shared with the user's `pidash` CLI (see `shared_daemon`), and the
//! engine's runners live in that daemon's default config next to the user's
//! own.
//!
//! **What this module deliberately does not do.** It never talks to the Pi
//! Dash API itself. The session lives in the webview's cookie jar, so the
//! overlay JS makes every authenticated call and hands the results here
//! through `invoke`. Replaying cookies from Rust would mean re-implementing
//! CSRF handling and would put a second copy of the session in play.
//!
//! It also never re-implements enrollment. Writing `config.toml`, registering
//! a runner and applying the response are `pidash`'s own code paths, reached
//! through the hidden `pidash __managed` subcommands of the installed CLI at
//! its default paths. A second implementation in a second language would
//! drift from the first the week after it shipped.
//!
//! See `pi-dash/.ai_design/managed_runner/design.md` §9.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::shared_daemon::{self, CliError, DaemonStatus};

/// Name the agent engine ships under.
///
/// Deliberately not `codex`: the engine is a component of this app, not a
/// program the user owns, and a binary called `codex` inside our bundle would
/// invite exactly the confusion with a personal install that the rest of this
/// design works to avoid.
#[cfg(windows)]
pub const ENGINE_BIN: &str = "pidash-agent-engine.exe";
#[cfg(not(windows))]
pub const ENGINE_BIN: &str = "pidash-agent-engine";

#[cfg(windows)]
pub const RUNNER_BIN: &str = "pidash.exe";
#[cfg(not(windows))]
pub const RUNNER_BIN: &str = "pidash";

/// The bundled engine binary path for a given Tauri `resource_dir`.
///
/// Pulled out of [`ManagedPaths::resolve`] so a test can pin that the engine
/// path is a pure function of `resource_dir` — which `resolve` re-reads on
/// every call — so after an app update the daemon is handed the newly bundled
/// engine, never a stale path from the previous install.
fn engine_bin_path(resource_dir: &Path) -> PathBuf {
    resource_dir.join("bin").join(ENGINE_BIN)
}

/// Absolute paths of the managed tree, derived once from Tauri's app-data dir.
///
/// What the built-in engine owns lives under `managed/`: its per-account
/// homes, the model credential, the task working copies and the local chat
/// store. The daemon's own config is no longer here — it is the shared
/// default one — but `config_dir` still names where builds before the shared
/// daemon kept their per-workspace configs, which `shared_daemon` adopts into
/// the default config and then deletes.
#[derive(Debug, Clone, Serialize)]
pub struct ManagedPaths {
    pub root: PathBuf,
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    /// Root of the per-account tree: `managed/accounts/<account-key>/`. The
    /// engine's home lives under it (see [`ManagedPaths::engine_home`]), so one
    /// account's transcripts are never in a directory another account's engine
    /// is pointed at.
    pub accounts_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub model_token_file: PathBuf,
    pub workdirs: PathBuf,
    /// Root of the per-account local chat history tree (see `chat_history`).
    /// Deliberately a sibling of `workdirs`, never a child, so a local chat's
    /// working copy can never be the same path as a managed run's.
    pub chat_dir: PathBuf,
    pub engine: PathBuf,
    pub runner: PathBuf,
    pub bin_dir: PathBuf,
}

impl ManagedPaths {
    pub fn resolve<R: Runtime>(app: &AppHandle<R>) -> Result<Self, String> {
        let base = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("resolving app data dir: {e}"))?;
        let root = base.join("managed");
        // Re-read the *current* bundle's resource dir on every call — nothing
        // here is cached. This is what makes an app update pick up the newly
        // bundled engine: once the update installs a new bundle, `resource_dir()`
        // points at it, so `engine` resolves to the new binary rather than a
        // stale path from the previous install. There is no auto-download; a new
        // engine only ever arrives with a new app build (see PDASHOSS01-158).
        let resource_dir = app
            .path()
            .resource_dir()
            .map_err(|e| format!("resolving resource dir: {e}"))?;
        let bin_dir = resource_dir.join("bin");
        Ok(Self {
            config_dir: root.join("pidash"),
            data_dir: root.join("pidash/data"),
            accounts_dir: root.join("accounts"),
            runtime_dir: root.join("runtime"),
            model_token_file: root.join("runtime/model.token"),
            workdirs: root.join("workdirs"),
            chat_dir: root.join("chat"),
            engine: engine_bin_path(&resource_dir),
            runner: bin_dir.join(RUNNER_BIN),
            bin_dir,
            root,
        })
    }

    /// Build a `ManagedPaths` rooted at an arbitrary directory, for tests that
    /// exercise the on-disk layout without a Tauri `AppHandle`.
    #[cfg(test)]
    pub fn for_test_root(root: PathBuf) -> Self {
        let bin_dir = root.join("bin");
        Self {
            config_dir: root.join("pidash"),
            data_dir: root.join("pidash/data"),
            accounts_dir: root.join("accounts"),
            runtime_dir: root.join("runtime"),
            model_token_file: root.join("runtime/model.token"),
            workdirs: root.join("workdirs"),
            chat_dir: root.join("chat"),
            engine: bin_dir.join(ENGINE_BIN),
            runner: bin_dir.join(RUNNER_BIN),
            bin_dir,
            root,
        }
    }

    /// The engine's `CODEX_HOME` for one account:
    /// `managed/accounts/<account-key>/codex-home`.
    ///
    /// Per account because the engine keeps its own copy of every conversation
    /// there — rollouts, `history.jsonl`, thread/log/memory databases — and
    /// isolation in the engine is by `CODEX_HOME` and nothing else. Keyed with
    /// the same [`account_key`](crate::chat_history::account_key) as the chat
    /// store, so the two copies of an account's history share one identity.
    pub(crate) fn engine_home(&self, account: &str) -> Result<PathBuf, String> {
        Ok(self
            .accounts_dir
            .join(crate::chat_history::account_key(account)?)
            .join("codex-home"))
    }

    /// Where builds before the per-account layout kept the one engine home
    /// every account shared.
    fn legacy_engine_home(&self) -> PathBuf {
        self.root.join("codex-home")
    }

    /// Create every directory the daemon expects to exist.
    fn ensure(&self) -> Result<(), String> {
        for dir in [
            &self.root,
            &self.config_dir,
            &self.data_dir,
            &self.runtime_dir,
            &self.workdirs,
        ] {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        }
        self.remove_legacy_engine_home();
        Ok(())
    }

    /// Create the account's engine home, owner-only, and return it.
    ///
    /// `CODEX_HOME` must exist as a directory before the engine starts — it
    /// refuses to run against a missing one rather than creating it, so a first
    /// launch would otherwise fail with a confusing error. The engine creates
    /// its own files with the default umask, so the directory is what keeps the
    /// transcripts inside it from being readable by other local users.
    pub(crate) fn ensure_engine_home(&self, account: &str) -> Result<PathBuf, String> {
        let home = self.engine_home(account)?;
        std::fs::create_dir_all(&home).map_err(|e| format!("creating {}: {e}", home.display()))?;
        if let Some(account_root) = home.parent() {
            crate::chat_history::restrict_dir(account_root);
        }
        crate::chat_history::restrict_dir(&home);
        Ok(home)
    }

    /// Delete the shared engine home left behind by an earlier build.
    ///
    /// It holds transcripts from every account that used this machine, in
    /// databases that cannot be split per account, so there is no owner to
    /// migrate it to: handing it to whoever signs in first would give them the
    /// other accounts' conversations. The app's own chat store is untouched, so
    /// every conversation is still listed and readable; only the engine's copy
    /// goes. Best-effort and cheap once the directory is gone — every managed
    /// command passes through [`ensure`](Self::ensure), so this runs before the
    /// fixed build first starts an engine.
    fn remove_legacy_engine_home(&self) {
        let legacy = self.legacy_engine_home();
        match std::fs::remove_dir_all(&legacy) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => eprintln!("removing legacy engine home {}: {e}", legacy.display()),
        }
    }
}

/// The engine's model endpoint, as reported by the cloud's agent-profile.
///
/// Mirrors the API response rather than re-deriving anything: the desktop is a
/// consumer of the server's provider resolution, never a second implementation
/// of it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentProfile {
    pub available: bool,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub reason_code: String,
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Report the managed paths so the overlay can show them in diagnostics.
#[tauri::command]
pub async fn managed_paths<R: Runtime>(app: AppHandle<R>) -> Result<ManagedPaths, String> {
    ManagedPaths::resolve(&app)
}

/// Register a runner for one project and write its `[[runner]]` block into
/// the shared default config.
///
/// Idempotent by design: the overlay calls this every time a project is
/// opened, and `__managed enroll` reuses an existing block rather than minting
/// a second runner. The CLI must already be signed in to `workspace` (the
/// overlay signs it in first); its machine token is what registers the runner.
///
/// `chat` marks the runner local chat talks to: the shared daemon may host
/// other runners too, so chat requests have to name this one.
#[tauri::command]
pub async fn managed_enroll<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    workspace: String,
    project: String,
    host_label: String,
    chat: Option<bool>,
) -> Result<String, CliError> {
    if !valid_component(&workspace) {
        return Err(CliError::new("invalid_input", "Invalid workspace slug"));
    }
    if !valid_component(&project) {
        return Err(CliError::new("invalid_input", "Invalid project identifier"));
    }
    let local = |e: String| CliError::new("cli_failed", e);
    let paths = ManagedPaths::resolve(&app).map_err(local)?;
    paths.ensure().map_err(local)?;
    let engine_home = paths.ensure_engine_home(&account).map_err(local)?;
    let working_dir = paths.workdirs.join(&workspace).join(&project);
    std::fs::create_dir_all(&working_dir)
        .map_err(|e| local(format!("creating {}: {e}", working_dir.display())))?;

    let lossy = |p: &Path| p.to_string_lossy().into_owned();
    let args = vec![
        "__managed".into(),
        "enroll".into(),
        "--workspace".into(),
        workspace,
        "--project".into(),
        project,
        "--host-label".into(),
        host_label,
        "--engine".into(),
        lossy(&paths.engine),
        "--codex-home".into(),
        lossy(&engine_home),
        "--working-dir".into(),
        lossy(&working_dir),
        "--path-prepend".into(),
        lossy(&paths.bin_dir),
        "--model-token-file".into(),
        lossy(&paths.model_token_file),
    ];
    let out = shared_daemon::run(&app, args, None).await?;
    if !out.success {
        return Err(out.error("__managed enroll"));
    }
    let stdout = out.stdout.trim().to_string();
    if chat.unwrap_or(false) {
        match enrolled_runner_name(&stdout) {
            Some(name) => app.state::<shared_daemon::SharedDaemon>().set_chat_runner(name),
            None => return Err(CliError::new("cli_failed", "pidash __managed enroll did not name its runner")),
        }
    }
    Ok(stdout)
}

/// The runner name in `__managed enroll`'s `{"ok": true, "name": ...}`.
fn enrolled_runner_name(stdout: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(stdout).ok()?;
    value.get("name")?.as_str().map(str::to_string)
}

/// Write the account's managed `CODEX_HOME/config.toml` from the cloud's
/// profile.
///
/// Rewritten whenever the profile changes (a settings save, a model switch)
/// and read by the engine at its next spawn, so a user changing their model in
/// Pi Dash settings does not have to restart anything.
#[tauri::command]
pub async fn managed_write_engine_config<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    profile: AgentProfile,
) -> Result<(), String> {
    let paths = ManagedPaths::resolve(&app)?;
    paths.ensure()?;
    let engine_home = paths.ensure_engine_home(&account)?;
    if !profile.available {
        return Err(format!(
            "profile unavailable ({}); refusing to write an engine config that cannot run",
            profile.reason_code
        ));
    }
    let config = render_engine_config(&profile, &paths.runner, &paths.model_token_file);
    atomic_write(&engine_home.join("config.toml"), config.as_bytes())
}

/// Store the short-lived model credential for the engine's auth helper.
///
/// Written atomically and `0600` on Unix. The engine's command-backed auth
/// helper re-reads it during a run, so rotation does not require a restart.
#[tauri::command]
pub async fn managed_write_model_token<R: Runtime>(
    app: AppHandle<R>,
    token: String,
) -> Result<(), String> {
    let paths = ManagedPaths::resolve(&app)?;
    paths.ensure()?;
    atomic_write(&paths.model_token_file, token.trim().as_bytes())
}

/// Point the built-in engine's runners at this build and account, then make
/// sure the shared daemon runs.
///
/// The rebind refreshes every engine runner's host-owned paths — the bundled
/// engine, the account's engine home, the model credential — even when
/// sessionStorage restores a workspace without reopening each project.
/// AppImage mounts are ephemeral; paths persisted by the previous launch are
/// invalid. The user's own runners are left alone (the CLI only touches blocks
/// that carry the managed engine fields).
#[tauri::command]
pub async fn managed_start_daemon<R: Runtime>(
    app: AppHandle<R>,
    account: String,
) -> Result<DaemonStatus, CliError> {
    let local = |e: String| CliError::new("cli_failed", e);
    let paths = ManagedPaths::resolve(&app).map_err(local)?;
    paths.ensure().map_err(local)?;
    let engine_home = paths.ensure_engine_home(&account).map_err(local)?;
    let lossy = |p: &Path| p.to_string_lossy().into_owned();
    let rebind = shared_daemon::run(
        &app,
        vec![
            "__managed".into(),
            "rebind".into(),
            "--engine".into(),
            lossy(&paths.engine),
            "--codex-home".into(),
            lossy(&engine_home),
            "--path-prepend".into(),
            lossy(&paths.bin_dir),
            "--model-token-file".into(),
            lossy(&paths.model_token_file),
        ],
        None,
    )
    .await?;
    if !rebind.success {
        return Err(rebind.error("__managed rebind"));
    }
    // Spawning is not the same as being reachable; `ensure` waits for the
    // socket to answer before reporting the daemon up, so the chat transport
    // never connects into the gap between spawn and bind.
    Ok(shared_daemon::ensure(&app).await)
}

/// What sign-out should do with the signed-out account's local chat history.
///
/// Absent (the default) means keep it: the approved behaviour is that history
/// stays on disk, scoped to the account, and is hidden until that same account
/// signs in again. Present means the user ticked "Delete chat history" in the
/// sign-out dialog, so we wipe that one account's history — the app's store and
/// the engine's own copy. Only the user-initiated sign-out ever sends it; the
/// automatic teardown paths (an expired session) never do.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ClearChatHistory {
    pub account: String,
}

/// Sign-out teardown: destroy the engine's local credential.
///
/// After this the bundled engine is inert — it has no model credential. The
/// shared daemon's machine token is the CLI's, and is revoked by
/// `pidash auth logout` (which the overlay runs first, after warning about
/// in-flight runs); the daemon picks that up and stops the runners itself.
/// The daemon process keeps running: it is the user's, not the app's.
///
/// Chat history is deliberately **not** removed here by default: it belongs to
/// the account and is kept, hidden, until that account signs in again. The only
/// time sign-out touches it is when the user asked to delete it
/// (`clear_chat_history` is `Some`). Task folders (`workdirs`) are never
/// removed here.
#[tauri::command]
pub async fn managed_sign_out<R: Runtime>(
    app: AppHandle<R>,
    clear_chat_history: Option<ClearChatHistory>,
) -> Result<(), String> {
    let paths = ManagedPaths::resolve(&app)?;
    sign_out_teardown(&paths, clear_chat_history)
}

/// The on-disk half of sign-out, once the daemon is stopped.
fn sign_out_teardown(
    paths: &ManagedPaths,
    clear_chat_history: Option<ClearChatHistory>,
) -> Result<(), String> {
    // Destroying the credentials is the security-critical half of sign-out and
    // must not be skipped because an optional history wipe failed (e.g. a busy
    // file on Windows). Run the wipe first but hold its result until *after*
    // the credentials are gone, then surface it.
    let history_result = match clear_chat_history {
        Some(request) => crate::chat_history::clear_account_history(paths, &request.account),
        None => Ok(()),
    };
    let _ = std::fs::remove_file(&paths.model_token_file);
    // Any legacy per-workspace config not yet adopted still holds a machine
    // token; it goes too.
    let _ = std::fs::remove_file(paths.config_dir.join("credentials.toml"));
    let _ = std::fs::remove_file(paths.config_dir.join("config.toml"));
    if let Ok(entries) = std::fs::read_dir(&paths.config_dir) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                for name in ["config.toml", "credentials.toml"] {
                    let path = entry.path().join(name);
                    match std::fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(format!("removing {}: {e}", path.display())),
                    }
                }
            }
        }
    }
    // Credentials are now gone regardless; surface any history-wipe failure.
    history_result
}

/// Whether the bundled binaries are present and runnable.
///
/// Surfaced as the app's "Pi Dash Agent needs repair" state: a partial install
/// or an interrupted update should say so, not fail at the first Run.
#[tauri::command]
pub async fn managed_doctor<R: Runtime>(app: AppHandle<R>) -> Result<serde_json::Value, String> {
    let paths = ManagedPaths::resolve(&app)?;
    paths.ensure()?;
    let identity = paths.root.join("device-id");
    let device_id = match std::fs::read_to_string(&identity) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let value = uuid::Uuid::new_v4().to_string();
            atomic_write(&identity, value.as_bytes())?;
            value
        }
        Err(e) => return Err(format!("reading desktop identity: {e}")),
    };
    Ok(serde_json::json!({
        "host_label": format!("desktop-{}", device_id.trim()),
        "runner_present": paths.runner.exists(),
        "engine_present": paths.engine.exists(),
        "token_present": paths.model_token_file.exists(),
        "accounts_dir": paths.accounts_dir.to_string_lossy(),
    }))
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

fn render_engine_config(profile: &AgentProfile, runner: &Path, token_file: &Path) -> String {
    // Command-backed auth refreshes during a running turn and after a 401.
    // No env_key or OpenAI login: both conflict with command-backed auth.
    // JSON string/array encoding is also valid TOML here, including Windows
    // backslashes and paths containing spaces or quotes.
    let command = serde_json::to_string(&runner.to_string_lossy()).expect("serialize helper path");
    let args = serde_json::to_string(&[
        "__managed",
        "model-token",
        "--file",
        &token_file.to_string_lossy(),
    ])
    .expect("serialize helper args");
    format!(
        r#"# Managed by Pi Dash Desktop. Edits are overwritten on launch.
model_provider = "pidash"
model = "{model}"
approval_policy = "on-request"
sandbox_mode = "workspace-write"
# Model gateways are not assumed to meter provider-hosted web-search tools.
# Leave task execution tools available without advertising that hosted tool.
web_search = "disabled"

[model_providers.pidash]
name = "Pi Dash"
base_url = "{base_url}"
wire_api = "responses"
[model_providers.pidash.auth]
command = {command}
args = {args}
timeout_ms = 5000
refresh_interval_ms = 30000
"#,
        model = toml_value(&profile.model),
        base_url = toml_value(&profile.base_url),
    )
}

/// Sanitise a server-supplied string for a TOML basic-string value.
///
/// These values come from our own API, so this is defence in depth rather
/// than a threat model — but the file it lands in controls the engine's
/// sandbox policy and credential source, so a value that could close its
/// quote and open a new key is worth one line to prevent. Quotes, backslashes
/// and anything that could start a new line are dropped rather than escaped:
/// no legitimate model name or URL contains them, so silently losing one is
/// preferable to emitting a file whose meaning depends on escaping rules.
fn toml_value(raw: &str) -> String {
    raw.chars()
        .filter(|c| *c != '"' && *c != '\\' && !c.is_control())
        .take(512)
        .collect()
}

/// Write via a temp file and rename, so a reader never sees a half-written
/// config or a truncated credential.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    let tmp = path.with_extension("tmp");
    {
        let mut file =
            std::fs::File::create(&tmp).map_err(|e| format!("creating {}: {e}", tmp.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        file.write_all(bytes)
            .map_err(|e| format!("writing {}: {e}", tmp.display()))?;
        file.sync_all().ok();
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("renaming into {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine path handed to the daemon is derived purely from the
    /// bundle's `resource_dir`, which `ManagedPaths::resolve` re-reads on every
    /// call. So after an app update installs a new bundle (a new resource dir),
    /// the daemon runs the newly bundled engine — never a stale path from the
    /// previous install. This pins that invariant (PDASHOSS01-158, item 2).
    #[test]
    fn engine_path_tracks_the_current_bundle_resource_dir() {
        let old = Path::new("/Applications/Pi Dash.app/Contents/Resources/v1");
        let new = Path::new("/Applications/Pi Dash.app/Contents/Resources/v2");

        // Fully determined by resource_dir: `<resource_dir>/bin/<engine>`.
        assert_eq!(engine_bin_path(old), old.join("bin").join(ENGINE_BIN));
        // A different install dir therefore yields a different engine path —
        // an update can never resolve to the old bundle's binary.
        assert_ne!(engine_bin_path(old), engine_bin_path(new));
        // And it always lives under the given bundle's bin/.
        assert!(engine_bin_path(new).starts_with(new.join("bin")));
        assert!(engine_bin_path(new).ends_with(ENGINE_BIN));
    }

    /// A transcript the way the pinned engine leaves one in its home.
    fn write_rollout(home: &Path, who: &str) -> PathBuf {
        let rollout = home.join(format!("sessions/2026/10/05/rollout-{who}.jsonl"));
        std::fs::create_dir_all(rollout.parent().unwrap()).unwrap();
        std::fs::write(&rollout, format!("{who} transcript")).unwrap();
        rollout
    }

    #[test]
    fn engine_home_is_per_account_under_the_accounts_root() {
        let root = tempfile::tempdir().unwrap();
        let paths = ManagedPaths::for_test_root(root.path().join("managed"));
        let a = paths.engine_home("user-a").unwrap();
        let b = paths.engine_home("user-b").unwrap();
        assert_ne!(a, b, "two accounts must never share an engine home");
        // `managed/accounts/<account-key>/codex-home`, keyed like the chat store.
        let key = crate::chat_history::account_key("user-a").unwrap();
        assert_eq!(a, paths.root.join("accounts").join(&key).join("codex-home"));
        assert!(!a.starts_with(paths.legacy_engine_home()));
        // No account, no engine home: nothing may fall back to a shared path.
        assert!(paths.engine_home("").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn engine_home_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let paths = ManagedPaths::for_test_root(root.path().join("managed"));
        let home = paths.ensure_engine_home("user-a").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&home), 0o700, "engine home must not be group/world readable");
        assert_eq!(mode(home.parent().unwrap()), 0o700, "account root must be 0700");
    }

    #[test]
    fn sign_out_keeps_engine_transcripts_unless_asked_to_delete_them() {
        let root = tempfile::tempdir().unwrap();
        let paths = ManagedPaths::for_test_root(root.path().join("managed"));
        paths.ensure().unwrap();
        let a = write_rollout(&paths.ensure_engine_home("user-a").unwrap(), "user-a");
        let b = write_rollout(&paths.ensure_engine_home("user-b").unwrap(), "user-b");
        let task = paths.workdirs.join("acme/web/notes.txt");
        std::fs::create_dir_all(task.parent().unwrap()).unwrap();
        std::fs::write(&task, "task folder").unwrap();
        atomic_write(&paths.model_token_file, b"tok").unwrap();

        // Default sign-out (and every automatic teardown): credentials go,
        // history stays where only that account's engine will look for it.
        sign_out_teardown(&paths, None).unwrap();
        assert!(!paths.model_token_file.exists());
        assert!(a.exists() && b.exists());

        // "Delete chat history" ticked: that account's engine folder goes as a
        // whole; the other account's and the task folders are untouched.
        sign_out_teardown(&paths, Some(ClearChatHistory { account: "user-a".into() })).unwrap();
        assert!(!paths.engine_home("user-a").unwrap().exists());
        assert!(b.exists(), "another account's transcripts must survive");
        assert!(task.exists(), "sign-out must never delete task folders");
    }

    #[test]
    fn legacy_shared_engine_home_is_removed() {
        let root = tempfile::tempdir().unwrap();
        let paths = ManagedPaths::for_test_root(root.path().join("managed"));
        let legacy = write_rollout(&paths.legacy_engine_home(), "everyone");
        let kept = write_rollout(&paths.ensure_engine_home("user-a").unwrap(), "user-a");
        paths.ensure().unwrap();
        assert!(!paths.legacy_engine_home().exists(), "{} survived", legacy.display());
        assert!(kept.exists());
        // And again with nothing left to remove.
        paths.ensure().unwrap();
    }

    #[test]
    fn workspace_and_project_paths_accept_identifiers_but_not_traversal() {
        for value in ["desktop-e2e", "PROJECT_1", "ÇŞĞIİÖÜ"] {
            assert!(valid_component(value));
        }
        for value in ["", "..", "../escape", "/absolute", "a/b", r"a\b"] {
            assert!(!valid_component(value));
        }
    }

    fn profile() -> AgentProfile {
        AgentProfile {
            available: true,
            base_url: "https://gateway.example.com/v1".into(),
            model: "gpt-5-codex".into(),
            reason_code: String::new(),
        }
    }

    #[test]
    fn engine_config_points_at_the_gateway_and_disables_login() {
        let text = render_engine_config(
            &profile(),
            Path::new("/bundle/pidash"),
            Path::new("/managed/model.token"),
        );
        assert!(text.contains(r#"base_url = "https://gateway.example.com/v1""#));
        assert!(text.contains(r#"model = "gpt-5-codex""#));
        // Responses is the only wire API the engine speaks.
        assert!(text.contains(r#"wire_api = "responses""#));
        assert!(text.contains(r#"web_search = "disabled""#));
        // No login screen, no auth.json, no ChatGPT OAuth.
        assert!(!text.contains("requires_openai_auth"));
        assert!(!text.contains("env_key"));
        assert!(text.contains("[model_providers.pidash.auth]"));
        assert!(text.contains(r#"command = "/bundle/pidash""#));
        assert!(
            text.contains(r#"args = ["__managed","model-token","--file","/managed/model.token"]"#)
        );
        assert!(text.contains("refresh_interval_ms = 30000"));
    }

    #[test]
    fn engine_config_cannot_be_broken_out_of_with_quotes() {
        let mut p = profile();
        p.model = r#"evil" 
sandbox_mode = "danger-full-access"#
            .into();
        let text = render_engine_config(
            &p,
            Path::new("/bundle/pidash"),
            Path::new("/managed/model.token"),
        );
        // The payload may survive *inside* the quoted value — that is inert.
        // What must not happen is a second key: exactly one `sandbox_mode`
        // assignment, and it must still be the policy we chose.
        let sandbox_keys: Vec<&str> = text
            .lines()
            .filter(|l| l.trim_start().starts_with("sandbox_mode"))
            .collect();
        assert_eq!(
            sandbox_keys,
            vec![r#"sandbox_mode = "workspace-write""#],
            "{text}"
        );
        // And the value must not have escaped its quotes or spanned lines.
        let model_lines: Vec<&str> = text
            .lines()
            .filter(|l| l.trim_start().starts_with("model ="))
            .collect();
        assert_eq!(model_lines.len(), 1, "{text}");
        assert!(
            model_lines[0].matches('"').count() == 2,
            "{}",
            model_lines[0]
        );
    }

    #[test]
    fn auth_helper_paths_preserve_windows_separators_and_spaces() {
        let runner = Path::new(r"C:\Program Files\Pi Dash\bin\pidash.exe");
        let token = Path::new(r"C:\Users\Jane Doe\Pi Dash\runtime\model.token");
        let config = render_engine_config(&profile(), runner, token);
        let value = |key: &str| {
            config
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .unwrap()
        };
        assert_eq!(
            serde_json::from_str::<String>(value("command = ")).unwrap(),
            runner.to_string_lossy()
        );
        let args: Vec<String> = serde_json::from_str(value("args = ")).unwrap();
        assert_eq!(
            args,
            [
                "__managed",
                "model-token",
                "--file",
                token.to_str().unwrap()
            ]
        );
    }

    #[test]
    #[ignore = "requires PIDASH_TEST_ENGINE and PIDASH_TEST_RUNNER binaries; offline fake gateway"]
    fn running_engine_refreshes_credentials_after_expiry() {
        let engine = std::env::var("PIDASH_TEST_ENGINE").expect("bundled engine path");
        let runner = std::env::var("PIDASH_TEST_RUNNER").expect("current runner path");
        let root = tempfile::tempdir().unwrap();
        let config_home = root.path().join("engine home with spaces");
        std::fs::create_dir(&config_home).unwrap();
        let token = config_home.join("model.token");
        let config = render_engine_config(&profile(), Path::new(&runner), &token);
        std::fs::write(config_home.join("config.toml"), config).unwrap();
        let output = std::process::Command::new("python3")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/engine_auth_smoke.py"))
            .arg("--engine")
            .arg(engine)
            .arg("--config-home")
            .arg(&config_home)
            .arg("--token-file")
            .arg(token)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        println!("{}", String::from_utf8_lossy(&output.stdout));
    }

    #[test]
    fn toml_value_strips_quotes_backslashes_and_control_characters() {
        assert_eq!(toml_value("gpt-5-codex"), "gpt-5-codex");
        assert_eq!(toml_value("a\"b"), "ab");
        assert_eq!(toml_value("a\\b"), "ab");
        assert_eq!(toml_value("a\nb\tc\rd"), "abcd");
        // Long values are capped rather than rejected: truncation produces a
        // provider error the user can read, an unbounded write does not.
        assert_eq!(toml_value(&"x".repeat(1000)).len(), 512);
    }

    #[test]
    fn atomic_write_sets_owner_only_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/model.token");
        atomic_write(&path, b"tok-abc").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "tok-abc");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "credential must not be group/world readable"
            );
        }
        // No temp file left behind for a scavenger to read.
        assert!(!path.with_extension("tmp").exists());
    }

    #[test]
    fn atomic_write_replaces_an_existing_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.token");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
    }

    #[test]
    fn enrolled_runner_name_is_read_from_the_cli_result() {
        assert_eq!(
            enrolled_runner_name(r#"{"ok":true,"reused":true,"runner_id":"r","name":"desktop-host-TEST"}"#).as_deref(),
            Some("desktop-host-TEST")
        );
        assert_eq!(enrolled_runner_name(r#"{"ok":true}"#), None);
        assert_eq!(enrolled_runner_name("not json"), None);
    }
}
