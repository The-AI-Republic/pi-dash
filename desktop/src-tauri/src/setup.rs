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
//!    (`pidash_cli::install_pidash_cli`; the network installer on Windows);
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
use std::process::Stdio;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Runtime};

use crate::daemon::{DaemonPaths, HelloProbe};
use crate::pidash_cli;

/// How long `pidash start` may take before the daemon answers `Hello`.
const START_WAIT: Duration = Duration::from_secs(30);
/// Cap on the login shell printing its `PATH` (a slow rc file must not hang
/// the set-up).
const SHELL_PATH_TIMEOUT: Duration = Duration::from_secs(5);

/// The `PATH` a login shell of `shell` sets up: `$SHELL -lc 'printf %s "$PATH"'`.
/// `None` when the shell cannot be run or prints nothing in time.
pub fn login_shell_path_with(shell: &Path) -> Option<String> {
    let mut child = std::process::Command::new(shell)
        .args(["-lc", "printf %s \"$PATH\""])
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
    // rc files may print banners; the PATH is the last line.
    let text = String::from_utf8_lossy(&out.stdout);
    let path = text.lines().last()?.trim().to_string();
    (!path.is_empty()).then_some(path)
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

#[cfg(unix)]
fn login_shell_path() -> Option<String> {
    let shell = std::env::var_os("SHELL").map(PathBuf::from)?;
    login_shell_path_with(&shell)
}

#[cfg(not(unix))]
fn login_shell_path() -> Option<String> {
    // Windows services inherit the user's environment from the registry; the
    // app's own PATH is already the user's.
    None
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
    let path = blocking(move || {
        Ok(service_path(
            login_shell_path(),
            &app_path,
            cli_dir.as_deref(),
        ))
    })
    .await?;
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

/// "Update Pi Dash on this computer" (design §4.4): install the CLI the app
/// carries over the installed one, atomically, then restart the service with
/// the CLI's reload helper (`pidash restart`). Only on the user's click, and
/// refused for an install the app cannot write.
#[tauri::command]
pub async fn daemon_update_cli<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let bundled = pidash_cli::bundled_cli(&app);
    blocking(move || {
        let cli = installed_cli()?;
        let (updatable, hint) = crate::daemon::cli_update_offer(Some(&cli));
        if !updatable {
            return Err(hint.unwrap_or_else(|| "This app cannot update Pi Dash here.".into()));
        }
        let bundled = bundled.ok_or(
            "This build of the app carries no Pi Dash CLI. Run `pidash update` in a terminal.",
        )?;
        let dir = cli.parent().ok_or("the installed CLI has no directory")?;
        log(&app, format!("Updating {}", cli.display()));
        pidash_cli::copy_bundled_cli(&bundled, dir)?;
        run_cli(&app, &cli, &["restart".into()], None, None)?;
        let has_engine =
            pidash_cli::spawn_piped(&cli, &["engine".into(), "path".into()], None, None)
                .and_then(|c| c.wait_with_output().map_err(|e| e.to_string()))
                .is_ok_and(|out| out.status.success());
        if !has_engine {
            run_cli(&app, &cli, &["engine".into(), "install".into()], None, None)?;
        }
        Ok(())
    })
    .await
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
            "[ \"$1\" = \"-lc\" ] || exit 3\necho 'Welcome banner'\nprintf %s /opt/homebrew/bin:/usr/bin",
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
