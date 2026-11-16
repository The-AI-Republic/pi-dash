// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Setting up, updating and signing in the computer's Pi Dash daemon from the
//! app (`.ai_design/built_engine_as_runner/design.md` §4.4, §4.6, §4.7).
//!
//! Every step goes through the installed `pidash` CLI rather than a second
//! implementation, so the result is identical to a terminal install:
//!
//! 1. install the CLI the app carries into the official installer's location
//!    (`pidash_cli::install_pidash_cli`);
//! 2. sign it in with a device grant the app approved
//!    (`pidash_cli::pidash_cli_login`);
//! 3. `pidash install` with the user's **login-shell** `PATH` — the service
//!    unit records the caller's `PATH`, and own-agent runners resolve their
//!    agents through it, while a GUI app's own `PATH` lacks the user's shell
//!    set-up — then `pidash start`;
//! 4. `pidash engine install` when the engine is not there yet.
//!
//! Each step runs only because the user accepted the consent screen (the
//! service is required, PO-12). Nothing here runs on its own.

use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Stdio;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Runtime};

use crate::daemon::{DaemonPaths, HelloProbe};
use crate::pidash_cli;

/// How long `pidash start` may take before the daemon answers `Hello`.
const START_WAIT: Duration = Duration::from_secs(30);
#[cfg(unix)]
/// Cap on the login shell printing its `PATH` (a slow rc file must not hang
/// the set-up).
const SHELL_PATH_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(unix)]
/// Brackets around the `PATH` the login shell prints, so rc-file banners
/// (with or without a trailing newline) are never taken for it.
const PATH_BEGIN: &str = "__PIDASH_PATH_BEGIN__";
#[cfg(unix)]
const PATH_END: &str = "__PIDASH_PATH_END__";

#[cfg(unix)]
/// The `PATH` a login shell of `shell` sets up: `$SHELL -lc 'printf … "$PATH"'`,
/// read between [`PATH_BEGIN`] and [`PATH_END`]. `None` when the shell cannot
/// be run, takes no `-lc` (tcsh/csh, nushell), or prints nothing in time.
pub fn login_shell_path_with(shell: &Path) -> Option<String> {
    let script = format!("printf '{PATH_BEGIN}%s{PATH_END}' \"$PATH\"");
    let mut child = std::process::Command::new(shell)
        .args(["-lc", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < SHELL_PATH_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(25))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let out = child.wait_with_output().ok()?;
    parse_marked_path(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(unix)]
/// The `PATH` between the last [`PATH_BEGIN`] and the [`PATH_END`] after it.
fn parse_marked_path(text: &str) -> Option<String> {
    let start = text.rfind(PATH_BEGIN)? + PATH_BEGIN.len();
    let len = text[start..].find(PATH_END)?;
    let path = text[start..start + len].trim().to_string();
    (!path.is_empty()).then_some(path)
}

/// Shells to ask for the login `PATH`, in order: the user's `$SHELL`, then
/// the platform's default login shell, so an unset `$SHELL` or one that
/// takes no `-lc` (tcsh/csh, nushell) still gets the user's profile.
#[cfg(unix)]
fn login_shell_candidates(shell_env: Option<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = shell_env.into_iter().collect();
    let defaults: &[&str] = if cfg!(target_os = "macos") {
        &["/bin/zsh", "/bin/bash"]
    } else {
        &["/bin/bash", "/bin/sh"]
    };
    for d in defaults {
        let d = PathBuf::from(d);
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// The `PATH` to install the service with: the login shell's, with the CLI's
/// own directory in front when it is missing, else the app's own `PATH`.
pub fn service_path(login_shell: Option<String>, app_path: &str, cli_dir: Option<&Path>) -> String {
    let mut path = login_shell.unwrap_or_else(|| app_path.to_string());
    if let Some(dir) = cli_dir {
        let dir = dir.to_string_lossy();
        let sep = if cfg!(windows) { ';' } else { ':' };
        if !path.split(sep).any(|entry| entry == dir) {
            path = if path.is_empty() {
                dir.into_owned()
            } else {
                format!("{dir}{sep}{path}")
            };
        }
    }
    path
}

/// The login-shell `PATH`, plus a warning for the set-up log when it came
/// from a fallback shell or could not be read at all (the service then gets
/// the app's own, shorter `PATH`, and own-agent runners may not find their
/// agents).
#[cfg(unix)]
fn login_shell_path() -> (Option<String>, Option<String>) {
    let shell_env = std::env::var_os("SHELL")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    for (i, shell) in login_shell_candidates(shell_env.clone()).iter().enumerate() {
        if let Some(path) = login_shell_path_with(shell) {
            let warning = (i > 0 || shell_env.is_none()).then(|| {
                format!(
                    "Could not read PATH from your login shell ({}); used {} instead.",
                    shell_env
                        .as_deref()
                        .map(|s| s.display().to_string())
                        .unwrap_or_else(|| "$SHELL is not set".into()),
                    shell.display()
                )
            });
            return (Some(path), warning);
        }
    }
    (
        None,
        Some(
            "Could not read PATH from your login shell; the service uses the app's PATH. \
             If a runner cannot find its agent, run `pidash install` from a terminal."
                .into(),
        ),
    )
}

#[cfg(not(unix))]
fn login_shell_path() -> (Option<String>, Option<String>) {
    // Windows services inherit the user's environment from the registry; the
    // app's own PATH is already the user's.
    (None, None)
}

/// The steps of the service set-up, as `pidash` argv lists (design §4.7
/// step 3). Separate so the order and arguments are tested.
pub fn service_steps() -> Vec<Vec<String>> {
    vec![vec!["install".into()], vec!["start".into()]]
}

fn installed_cli() -> Result<PathBuf, String> {
    pidash_cli::detect_pidash_cli_sync()
        .path
        .ok_or_else(|| "The pidash CLI is not installed yet.".to_string())
}

fn log<R: Runtime>(app: &AppHandle<R>, line: impl Into<String>) {
    let _ = app.emit(
        "pidash-install-log",
        serde_json::json!({ "line": line.into() }),
    );
}

/// Run `pidash <args>` with its output streamed to the set-up log.
fn run_cli<R: Runtime>(
    app: &AppHandle<R>,
    cli: &Path,
    args: &[String],
    stdin: Option<&str>,
    path: Option<&str>,
) -> Result<(), String> {
    log(app, format!("$ pidash {}", args.join(" ")));
    pidash_cli::run_streaming(app, cli, args, stdin, path)
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("task failed: {e}"))?
}

/// Wait until the standard daemon answers `Hello`.
async fn wait_for_hello(socket: &Path, deadline: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if matches!(
            crate::daemon::probe_hello(socket).await,
            HelloProbe::Answered(_)
        ) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

/// Step 3: install the user service with the login-shell `PATH`, start it,
/// and wait for the daemon to answer. Run only after the user accepted the
/// consent screen; the CLI must be installed and signed in already.
#[tauri::command]
pub async fn daemon_install_service<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let cli = blocking(installed_cli).await?;
    let app_path = std::env::var("PATH").unwrap_or_default();
    let cli_dir = cli.parent().map(Path::to_path_buf);
    let (path, warning) = blocking(move || {
        let (login, warning) = login_shell_path();
        Ok((service_path(login, &app_path, cli_dir.as_deref()), warning))
    })
    .await?;
    if let Some(warning) = warning {
        eprintln!("set-up: {warning}");
        log(&app, format!("Warning: {warning}"));
    }
    let run_app = app.clone();
    let run_cli_path = cli.clone();
    blocking(move || {
        for step in service_steps() {
            run_cli(&run_app, &run_cli_path, &step, None, Some(&path))?;
        }
        Ok(())
    })
    .await?;
    let socket = DaemonPaths::standard()?.socket();
    if !wait_for_hello(&socket, START_WAIT).await {
        return Err(
            "Pi Dash was installed as a service but did not start. Run `pidash status` in a terminal to see why."
                .into(),
        );
    }
    log(&app, "Pi Dash is running on this computer.");
    Ok(())
}

/// Step 4: `pidash engine install` unless `pidash engine path` finds one.
#[tauri::command]
pub async fn daemon_ensure_engine<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    blocking(move || {
        let cli = installed_cli()?;
        let present = pidash_cli::spawn_piped(&cli, &["engine".into(), "path".into()], None, None)
            .and_then(|c| c.wait_with_output().map_err(|e| e.to_string()))
            .is_ok_and(|out| out.status.success());
        if present {
            return Ok(());
        }
        run_cli(&app, &cli, &["engine".into(), "install".into()], None, None)
    })
    .await
}

/// Whether the daemon's runners are idle (no run in flight, no approval
/// pending). `pidash restart` stops any agent run in progress, so an
/// auto-update defers the restart until this is true and retries at the
/// next launch or idle point. A missing/unreachable daemon counts as idle
/// (there is no run to interrupt).
async fn daemon_is_idle() -> bool {
    match crate::daemon::daemon_status().await {
        Ok(snapshot) => crate::daemon::is_idle(&snapshot),
        Err(_) => true,
    }
}

/// "Update Pi Dash on this computer" (design §4.4): install the CLI the app
/// carries over the installed one, atomically, then restart the service with
/// the CLI's reload helper (`pidash restart`), deferring the restart until
/// runners are idle. Only on the user's click, and refused for an install the
/// app cannot write.
///
/// Version-aware (PIDESKAPP-137): never downgrades, never overwrites an
/// unparseable version, and shows the manual-update hint for system installs.
/// The same logic backs the silent launch-time check
/// [`daemon_maybe_update_cli`].
#[tauri::command]
pub async fn daemon_update_cli<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let bundled_path = pidash_cli::bundled_cli(&app);
    let bundled_ver = pidash_cli::bundled_version_parsed(&app);
    let bundled_ver_str = pidash_cli::bundled_version(&app);
    // Decide before the copy (needs the installed version without an AppHandle)
    let status = crate::pidash_cli::detect_pidash_cli_sync();
    let installed_ver = status.version.as_deref().and_then(crate::pidash_cli::parse_version_string);
    let decision = crate::pidash_cli::decide_update(status.installed, installed_ver.clone(), bundled_ver.clone());
    match decision {
        crate::pidash_cli::UpdateDecision::Install => {
            // Nothing installed: fall through to the copy below (handled by
            // `installed_cli` erroring, but we install via the bundled copy into
            // the standard install dir). To keep this command's contract, try
            // the standard install path; if there is no bundled copy, error.
            let bundled = bundled_path.ok_or(
                "This build of the app carries no Pi Dash CLI. Run `pidash update` in a terminal.",
            )?;
            let Some(home) = crate::pidash_cli::home_for_install() else {
                return Err(
                    "Unable to locate the user's home directory to install Pi Dash.".into(),
                );
            };
            let dest_dir = crate::pidash_cli::cli_install_dir_for_home(&home);
            let app_clone = app.clone();
            blocking(move || {
                log(&app_clone, format!("Installing {}", dest_dir.display()));
                pidash_cli::copy_bundled_cli(&bundled, &dest_dir)?;
                Ok(())
            })
            .await?;
            return Ok(());
        }
        crate::pidash_cli::UpdateDecision::Keep => {
            return Err(format!(
                "Pi Dash {} is already {}. Nothing to install.",
                status.version.as_deref().unwrap_or(""),
                bundled_ver_str.as_deref().unwrap_or("bundled"),
            ));
        }
        crate::pidash_cli::UpdateDecision::Unknown => {
            return Err(
                "Unable to determine the installed Pi Dash version; it will not be overwritten. Update it manually (for example `pidash update`)."
                    .into(),
            );
        }
        crate::pidash_cli::UpdateDecision::Update => {}
    }
    let bundled_path_for_copy = bundled_path;
    let bundled_ver_for_log = bundled_ver_str;
    // From here: installed < bundled and the decision is to update.
    let app_clone = app.clone();
    let status_path = status.path.clone();
    blocking(move || {
        let cli = status_path.ok_or("The pidash CLI is not installed yet.")?;
        let (updatable, hint) = crate::daemon::cli_update_offer(Some(&cli));
        if !updatable {
            return Err(hint.unwrap_or_else(|| "This app cannot update Pi Dash here.".into()));
        }
        let bundled = bundled_path_for_copy.ok_or(
            "This build of the app carries no Pi Dash CLI. Run `pidash update` in a terminal.",
        )?;
        let dir = cli.parent().ok_or("the installed CLI has no directory")?;
        log(
            &app_clone,
            format!(
                "Updating {} (bundled {})",
                cli.display(),
                bundled_ver_for_log.as_deref().unwrap_or("")
            ),
        );
        pidash_cli::copy_bundled_cli(&bundled, dir)?;
        Ok(())
    })
    .await?;
    // Restart after the copy, deferring until idle so a running agent run is
    // not killed (PIDESKAPP-137 default: wait until idle, retry at next launch).
    if !daemon_is_idle().await {
        log(
            &app,
            "Pi Dash on this computer will restart when its runners are idle. You can keep working; the restart happens at the next idle point or next launch.",
        );
        return Ok(());
    }
    let cli = blocking(installed_cli).await?;
    let app_clone = app.clone();
    blocking(move || {
        run_cli(&app_clone, &cli, &["restart".into()], None, None)?;
        let has_engine =
            pidash_cli::spawn_piped(&cli, &["engine".into(), "path".into()], None, None)
                .and_then(|c| c.wait_with_output().map_err(|e| e.to_string()))
                .is_ok_and(|out| out.status.success());
        if !has_engine {
            run_cli(&app_clone, &cli, &["engine".into(), "install".into()], None, None)?;
        }
        Ok(())
    })
    .await
}

/// Silent launch-time version check (PIDESKAPP-137): if the installed CLI is
/// older than the bundled one and this app may write to it, copy the bundled
/// CLI over atomically and restart when idle. Call this in the background
/// after `daemon_discover`; it never downgrades and never overwrites an
/// unparseable install. Returns `Ok(())` when nothing needed doing.
#[tauri::command]
pub async fn daemon_maybe_update_cli<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
    let bundled_path = pidash_cli::bundled_cli(&app);
    let bundled_ver = pidash_cli::bundled_version_parsed(&app);
    if bundled_path.is_none() || bundled_ver.is_none() {
        return Ok(false);
    }
    let status = crate::pidash_cli::detect_pidash_cli_sync();
    if !status.installed {
        return Ok(false); // Install stays consent-gated via `install_pidash_cli`.
    }
    let installed_ver = status.version.as_deref().and_then(crate::pidash_cli::parse_version_string);
    if crate::pidash_cli::decide_update(true, installed_ver, bundled_ver) != crate::pidash_cli::UpdateDecision::Update {
        return Ok(false);
    }
    let cli = status.path.clone().ok_or("install vanished during update check")?;
    let (updatable, hint) = crate::daemon::cli_update_offer(Some(&cli));
    if !updatable {
        // System installs: do not touch; surface the hint via CliInfo.
        let _ = hint;
        return Ok(false);
    }
    let app_clone = app.clone();
    let bundled = bundled_path.unwrap();
    let dir = cli.parent().ok_or("the installed CLI has no directory")?.to_path_buf();
    let installed_display = cli.clone();
    blocking(move || {
        log(&app_clone, format!("Auto-updating {}", installed_display.display()));
        pidash_cli::copy_bundled_cli(&bundled, &dir).map(|_| ())
    })
    .await?;
    if !daemon_is_idle().await {
        log(
            &app,
            "Auto-update copied; Pi Dash will restart when its runners are idle.",
        );
        return Ok(true);
    }
    let cli_again = blocking(installed_cli).await?;
    let app_clone = app.clone();
    blocking(move || {
        run_cli(&app_clone, &cli_again, &["restart".into()], None, None)?;
        Ok(())
    })
    .await?;
    Ok(true)
}

/// Hand an engine-only device grant the app approved to the daemon's CLI
/// (`pidash login --engine --device-code-stdin`, design §4.3). The code goes
/// on stdin, never argv.
#[tauri::command]
pub async fn pidash_cli_engine_login<R: Runtime>(
    app: AppHandle<R>,
    device_code: String,
) -> Result<(), String> {
    blocking(move || {
        let cli = installed_cli()?;
        let (args, stdin) = engine_login_invocation(device_code);
        run_cli(&app, &cli, &args, stdin.as_deref(), None)
    })
    .await
}

pub fn engine_login_invocation(device_code: String) -> (Vec<String>, Option<String>) {
    (
        vec![
            "login".into(),
            "--engine".into(),
            "--device-code-stdin".into(),
        ],
        Some(device_code),
    )
}

/// `pidash auth logout` (`engine = true`: `--engine`, the engine only). The
/// app's own sign-out never runs this unless the user ticked "Also sign out
/// Pi Dash on this computer" (design §4.6).
#[tauri::command]
pub async fn pidash_cli_logout<R: Runtime>(
    app: AppHandle<R>,
    engine: Option<bool>,
) -> Result<(), String> {
    blocking(move || {
        let cli = installed_cli()?;
        run_cli(
            &app,
            &cli,
            &logout_invocation(engine.unwrap_or(false)),
            None,
            None,
        )
    })
    .await
}

pub fn logout_invocation(engine: bool) -> Vec<String> {
    let mut args = vec!["auth".to_string(), "logout".to_string()];
    if engine {
        args.push("--engine".into());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// A stand-in login shell: it must be asked for a login shell (`-l`) and
    /// its PATH taken from the last line, after any rc-file banner.
    #[cfg(unix)]
    #[test]
    fn login_shell_path_comes_from_a_login_shell() {
        let tmp = tempfile::tempdir().unwrap();
        let shell = script(
            tmp.path(),
            "fake-shell",
            "[ \"$1\" = \"-lc\" ] || exit 3\nprintf 'Welcome banner, no newline'\nPATH=/opt/homebrew/bin:/usr/bin\neval \"$2\"",
        );
        // ETXTBSY can hit a freshly written script while another test forks.
        let mut path = None;
        for _ in 0..20 {
            path = login_shell_path_with(&shell);
            if path.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(path.as_deref(), Some("/opt/homebrew/bin:/usr/bin"));
        let silent = script(tmp.path(), "silent-shell", "exit 0");
        assert_eq!(login_shell_path_with(&silent), None);
        assert_eq!(login_shell_path_with(&tmp.path().join("missing")), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_path_is_read_between_the_markers() {
        assert_eq!(
            parse_marked_path("banner__PIDASH_PATH_BEGIN__/a:/b__PIDASH_PATH_END__trailer")
                .as_deref(),
            Some("/a:/b")
        );
        assert_eq!(parse_marked_path("just a banner\n/usr/bin"), None);
        assert_eq!(
            parse_marked_path("__PIDASH_PATH_BEGIN____PIDASH_PATH_END__"),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unset_shell_falls_back_to_the_platform_login_shell() {
        let c = login_shell_candidates(None);
        assert!(!c.is_empty());
        let c = login_shell_candidates(Some(PathBuf::from("/bin/tcsh")));
        assert_eq!(c[0], PathBuf::from("/bin/tcsh"));
        assert!(c.len() >= 2);
    }

    #[cfg(unix)]
    #[test]
    fn service_path_prefers_the_login_shell_and_adds_the_cli_dir() {
        let cli_dir = Path::new("/home/dev/.local/bin");
        assert_eq!(
            service_path(
                Some("/opt/homebrew/bin:/usr/bin".into()),
                "/usr/bin",
                Some(cli_dir)
            ),
            "/home/dev/.local/bin:/opt/homebrew/bin:/usr/bin"
        );
        // Already there: unchanged.
        assert_eq!(
            service_path(
                Some("/usr/bin:/home/dev/.local/bin".into()),
                "/x",
                Some(cli_dir)
            ),
            "/usr/bin:/home/dev/.local/bin"
        );
        // No login shell: the app's PATH.
        assert_eq!(service_path(None, "/usr/bin", None), "/usr/bin");
    }

    #[test]
    fn service_is_installed_then_started() {
        assert_eq!(
            service_steps(),
            vec![vec!["install".to_string()], vec!["start".to_string()]]
        );
    }

    #[test]
    fn engine_login_puts_the_code_on_stdin_only() {
        let (args, stdin) = engine_login_invocation("dc-SECRET".into());
        assert_eq!(args, ["login", "--engine", "--device-code-stdin"]);
        assert!(!args.iter().any(|a| a.contains("dc-SECRET")));
        assert_eq!(stdin.as_deref(), Some("dc-SECRET"));
    }

    #[test]
    fn logout_signs_out_the_engine_only_when_asked() {
        assert_eq!(logout_invocation(false), ["auth", "logout"]);
        assert_eq!(logout_invocation(true), ["auth", "logout", "--engine"]);
    }

    /// A fake `pidash` records the argv and PATH each set-up step runs with:
    /// the service is installed with the login-shell PATH, and nothing the
    /// app inherited re-roots the CLI.
    #[cfg(unix)]
    #[test]
    fn set_up_steps_run_with_the_given_path_and_no_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("calls");
        let fake = script(
            tmp.path(),
            "pidash",
            &format!(
                "echo \"$* PATH=$PATH CFG=${{PIDASH_CONFIG_DIR:-unset}} DATA=${{PIDASH_DATA_DIR:-unset}}\" >> '{}'",
                log.display()
            ),
        );
        // SAFETY: test-only; no other test reads these variables.
        unsafe {
            std::env::set_var("PIDASH_CONFIG_DIR", "/should/not/leak");
        }
        for step in service_steps() {
            let mut child = loop {
                match pidash_cli::spawn_piped(&fake, &step, None, Some("/login/shell/bin:/usr/bin"))
                {
                    Err(e) if e.contains("Text file busy") => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    other => break other.unwrap(),
                }
            };
            assert!(child.wait().unwrap().success());
        }
        unsafe {
            std::env::remove_var("PIDASH_CONFIG_DIR");
        }
        let calls = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = calls.lines().collect();
        assert_eq!(lines.len(), 2, "{calls}");
        assert!(
            lines[0].starts_with("install PATH=/login/shell/bin:/usr/bin"),
            "{calls}"
        );
        assert!(lines[1].starts_with("start "), "{calls}");
        assert!(calls.contains("CFG=unset DATA=unset"), "{calls}");
    }
}
