// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Provisioning and supervision of the built-in agent engine.
//!
//! The desktop app ships three things: this window, the `pidash` daemon, and
//! the agent engine. This module owns the second and third — it writes their
//! configuration, starts the daemon as a child process, keeps the model
//! credential fresh on disk, and tears all of it down on sign-out.
//!
//! **What this module deliberately does not do.** It never talks to the Pi
//! Dash API itself. The session lives in the webview's cookie jar, so the
//! overlay JS makes every authenticated call and hands the results here
//! through `invoke`. Replaying cookies from Rust would mean re-implementing
//! CSRF handling and would put a second copy of the session in play.
//!
//! It also never re-implements enrollment. Writing `config.toml`, registering
//! a runner and applying the response are `pidash`'s own code paths, reached
//! through the hidden `pidash __managed` subcommands. A second implementation
//! in a second language would drift from the first the week after it shipped.
//!
//! See `pi-dash/.ai_design/managed_runner/design.md` §9.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

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

/// Daemons this app started, keyed by workspace.
///
/// Empty whenever no daemon is running — which is the normal state before
/// sign-in and after sign-out, not an error.
#[derive(Default)]
pub struct DaemonState(pub Mutex<BTreeMap<String, Child>>);

/// Absolute paths of the managed tree, derived once from Tauri's app-data dir.
///
/// Everything the bundled runner touches lives under `managed/`, passed to it
/// as `PIDASH_CONFIG_DIR` / `PIDASH_DATA_DIR`. A user-installed `pidash` uses
/// the XDG defaults and never sees any of this, which is what keeps the two
/// installs from colliding in either direction.
#[derive(Debug, Clone, Serialize)]
pub struct ManagedPaths {
    pub root: PathBuf,
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub codex_home: PathBuf,
    pub runtime_dir: PathBuf,
    pub model_token_file: PathBuf,
    pub workdirs: PathBuf,
    pub engine: PathBuf,
    pub runner: PathBuf,
    pub bin_dir: PathBuf,
}

impl ManagedPaths {
    fn for_workspace<R: Runtime>(app: &AppHandle<R>, workspace: &str) -> Result<Self, String> {
        if !valid_component(workspace) {
            return Err("Invalid workspace slug".into());
        }
        let mut paths = Self::resolve(app)?;
        paths.config_dir = paths.config_dir.join(workspace);
        paths.data_dir = paths.config_dir.join("data");
        Ok(paths)
    }

    pub fn resolve<R: Runtime>(app: &AppHandle<R>) -> Result<Self, String> {
        let base = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("resolving app data dir: {e}"))?;
        let root = base.join("managed");
        let bin_dir = app
            .path()
            .resource_dir()
            .map_err(|e| format!("resolving resource dir: {e}"))?
            .join("bin");
        Ok(Self {
            config_dir: root.join("pidash"),
            data_dir: root.join("pidash/data"),
            codex_home: root.join("codex-home"),
            runtime_dir: root.join("runtime"),
            model_token_file: root.join("runtime/model.token"),
            workdirs: root.join("workdirs"),
            engine: bin_dir.join(ENGINE_BIN),
            runner: bin_dir.join(RUNNER_BIN),
            bin_dir,
            root,
        })
    }

    /// Create every directory the daemon and engine expect to exist.
    ///
    /// `CODEX_HOME` in particular must exist as a directory before the engine
    /// starts — it refuses to run against a missing one rather than creating
    /// it, so a first launch would otherwise fail with a confusing error.
    fn ensure(&self) -> Result<(), String> {
        for dir in [
            &self.root,
            &self.config_dir,
            &self.data_dir,
            &self.codex_home,
            &self.runtime_dir,
            &self.workdirs,
        ] {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        }
        Ok(())
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

/// Write the machine token into the managed config.
///
/// Takes the token as an argument from the overlay (which fetched it from the
/// session-authenticated enroll endpoint) and pipes it to `pidash` on **stdin**
/// rather than argv, so it never appears in the process table.
#[tauri::command]
pub async fn managed_bootstrap<R: Runtime>(
    app: AppHandle<R>,
    cloud_url: String,
    workspace: String,
    machine_token: String,
    dev_machine_id: String,
) -> Result<String, String> {
    let paths = ManagedPaths::for_workspace(&app, &workspace)?;
    paths.ensure()?;

    let mut child = base_command(&paths, &paths.runner)
        .args([
            "__managed",
            "bootstrap",
            "--cloud-url",
            &cloud_url,
            "--workspace",
            &workspace,
            "--dev-machine-id",
            &dev_machine_id,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawning pidash __managed bootstrap: {e}"))?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| "pidash stdin unavailable".to_string())?
        .write_all(machine_token.as_bytes())
        .map_err(|e| format!("writing machine token: {e}"))?;
    // Drop stdin so the child sees EOF; without this it blocks reading forever.
    drop(child.stdin.take());

    finish(child, "bootstrap")
}

/// Register a runner for one project and write its `[[runner]]` block.
///
/// Idempotent by design: the overlay calls this every time a project is
/// opened, and `__managed enroll` reuses an existing block rather than minting
/// a second runner.
#[tauri::command]
pub async fn managed_enroll<R: Runtime>(
    app: AppHandle<R>,
    workspace: String,
    project: String,
    host_label: String,
) -> Result<String, String> {
    if !valid_component(&project) {
        return Err("Invalid project identifier".into());
    }
    let paths = ManagedPaths::for_workspace(&app, &workspace)?;
    paths.ensure()?;
    let working_dir = paths.workdirs.join(&workspace).join(&project);
    std::fs::create_dir_all(&working_dir)
        .map_err(|e| format!("creating {}: {e}", working_dir.display()))?;

    let child = base_command(&paths, &paths.runner)
        .args([
            "__managed",
            "enroll",
            "--workspace",
            &workspace,
            "--project",
            &project,
            "--host-label",
            &host_label,
            "--engine",
            &paths.engine.to_string_lossy(),
            "--codex-home",
            &paths.codex_home.to_string_lossy(),
            "--working-dir",
            &working_dir.to_string_lossy(),
            "--path-prepend",
            &paths.bin_dir.to_string_lossy(),
            "--model-token-file",
            &paths.model_token_file.to_string_lossy(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawning pidash __managed enroll: {e}"))?;
    finish(child, "enroll")
}

/// Write the managed `CODEX_HOME/config.toml` from the cloud's profile.
///
/// Rewritten whenever the profile changes (a settings save, a model switch)
/// and read by the engine at its next spawn, so a user changing their model in
/// Pi Dash settings does not have to restart anything.
#[tauri::command]
pub async fn managed_write_engine_config<R: Runtime>(
    app: AppHandle<R>,
    profile: AgentProfile,
) -> Result<(), String> {
    let paths = ManagedPaths::resolve(&app)?;
    paths.ensure()?;
    if !profile.available {
        return Err(format!(
            "profile unavailable ({}); refusing to write an engine config that cannot run",
            profile.reason_code
        ));
    }
    let config = render_engine_config(&profile, &paths.runner, &paths.model_token_file);
    atomic_write(&paths.codex_home.join("config.toml"), config.as_bytes())
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

/// Start the bundled daemon as a child of this app.
///
/// Deliberately not a system service: the managed runner is online exactly
/// while the app is open, which is the contract the cloud's availability check
/// and the "Runs on this computer" affordance are built on. A service would
/// keep accepting work after the user closed the window.
#[tauri::command]
pub async fn managed_start_daemon<R: Runtime>(
    app: AppHandle<R>,
    workspace: String,
) -> Result<(), String> {
    let paths = ManagedPaths::for_workspace(&app, &workspace)?;
    paths.ensure()?;

    let state = app.state::<DaemonState>();
    let mut guard = state.0.lock().map_err(|_| "daemon state poisoned")?;
    if let Some(child) = guard.get_mut(&workspace) {
        match child.try_wait() {
            // Already running — starting a second daemon on the same config
            // would have two processes claiming the same runner rows.
            Ok(None) => return Ok(()),
            _ => {
                guard.remove(&workspace);
            }
        }
    }

    // Refresh every configured project's paths, even when sessionStorage
    // restores a workspace without reopening each project. AppImage mounts
    // are ephemeral; persisted paths from the previous launch are invalid.
    let rebind = base_command(&paths, &paths.runner)
        .args(["__managed", "rebind", "--engine"])
        .arg(&paths.engine)
        .arg("--codex-home")
        .arg(&paths.codex_home)
        .arg("--path-prepend")
        .arg(&paths.bin_dir)
        .arg("--model-token-file")
        .arg(&paths.model_token_file)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawning managed path refresh: {e}"))?;
    finish(rebind, "rebind")?;

    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.data_dir.join("desktop-daemon.log"))
        .map_err(|e| format!("opening daemon log: {e}"))?;
    let child = base_command(&paths, &paths.runner)
        .arg("__run")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|e| format!("spawning pidash __run: {e}"))?;
    guard.insert(workspace, child);
    Ok(())
}

/// Stop the daemon, waiting briefly for an in-flight run to finish.
///
/// The cloud's heartbeat reaper will clean up whatever we kill mid-run, but a
/// killed run is a failed run in the user's history — worth a few seconds of
/// patience on a normal window close.
#[tauri::command]
pub async fn managed_stop_daemon<R: Runtime>(
    app: AppHandle<R>,
    grace_seconds: Option<u64>,
) -> Result<(), String> {
    stop_daemon(&app, grace_seconds.unwrap_or(30));
    Ok(())
}

/// Sign-out teardown: stop the daemon and destroy every local credential.
///
/// After this the bundled binaries are inert — the engine has no model
/// credential and the daemon has no machine token — which is what makes
/// "signed out" mean something for a binary the user could still execute.
#[tauri::command]
pub async fn managed_sign_out<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    stop_daemon(&app, 5);
    let paths = ManagedPaths::resolve(&app)?;
    let _ = std::fs::remove_file(&paths.model_token_file);
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
    Ok(())
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
        "config_present": paths.config_dir.join("config.toml").exists(),
        "token_present": paths.model_token_file.exists(),
        "codex_home": paths.codex_home.to_string_lossy(),
    }))
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// A `pidash` invocation pointed at the managed tree.
///
/// Every call goes through here so no code path can accidentally run against
/// the user's own `pidash` configuration.
fn base_command(paths: &ManagedPaths, program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.env("PIDASH_CONFIG_DIR", &paths.config_dir);
    cmd.env("PIDASH_DATA_DIR", &paths.data_dir);
    #[cfg(windows)]
    {
        // Keep the console window from flashing on every invocation.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

fn finish(child: Child, what: &str) -> Result<String, String> {
    let out = child
        .wait_with_output()
        .map_err(|e| format!("waiting for pidash {what}: {e}"))?;
    if !out.status.success() {
        // stderr carries pidash's own diagnostics, which are more useful to a
        // user than anything we could synthesise here.
        return Err(format!(
            "pidash {what} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn stop_daemon<R: Runtime>(app: &AppHandle<R>, grace_seconds: u64) {
    let Some(state) = app.try_state::<DaemonState>() else {
        return;
    };
    let Ok(mut guard) = state.0.lock() else {
        return;
    };
    let children = std::mem::take(&mut *guard);
    for (_, mut child) in children {
        // Poll rather than sleeping the full grace period: a daemon with nothing
        // in flight exits immediately and the user should not wait for a timer.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(grace_seconds);
        #[cfg(unix)]
        unsafe {
            // SIGTERM asks the daemon to finish its current run and shut down
            // cleanly; SIGKILL below is the fallback if it does not.
            libc::kill(child.id() as i32, libc::SIGTERM);
        }
        while std::time::Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(200)),
                Err(_) => break,
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    }
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
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
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

/// Best-effort teardown for app exit.
pub fn shutdown<R: Runtime>(app: &AppHandle<R>) {
    stop_daemon(app, 5);
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let output = Command::new("python3")
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
}
