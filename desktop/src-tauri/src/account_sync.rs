// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! One sign-in for the app and the `pidash` CLI on this computer.
//!
//! The app's session lives in the webview's cookie jar; the CLI's (and so the
//! shared daemon's) lives in the default `config.toml`. The overlay keeps the
//! two in step — "most recent sign-in or sign-out wins" — and these commands
//! are the native half of that:
//!
//! * [`pidash_auth_status`] — `pidash auth status --json`: who the CLI is
//!   signed in as, when that last changed, and how many runs are in flight.
//! * [`pidash_auth_logout`] — `pidash auth logout --yes`, run after the app has
//!   warned about in-flight runs.
//! * [`pidash_desktop_handoff`] — `pidash __desktop handoff`, then navigate the
//!   main window to the one-time `redeem_url`, which signs the app in as the
//!   CLI's user. The URL never passes through the webview: this command checks
//!   it points at the Pi Dash server's `/api/` and navigates itself, so script
//!   in the page cannot use it to send the window anywhere else.
//! * [`desktop_auth_observe`] — the app's own last sign-in/sign-out time, kept
//!   on disk outside the webview's storage (which sign-out wipes) so it can be
//!   compared with the CLI's `changed_at`.
//!
//! Signing the CLI in as the app's user stays in `pidash_cli::pidash_cli_login`
//! (device-code grant approved by the app's session).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, Url};

use crate::shared_daemon::{self, CliError};

/// `pidash auth status --json` (contract §6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthStatus {
    pub signed_in: bool,
    #[serde(default)]
    pub cloud_url: Option<String>,
    #[serde(default)]
    pub user: Option<AuthUser>,
    #[serde(default)]
    pub workspace_slug: Option<String>,
    #[serde(default)]
    pub dev_machine_id: Option<String>,
    #[serde(default)]
    pub changed_at: Option<String>,
    #[serde(default)]
    pub daemon_running: bool,
    #[serde(default)]
    pub active_runs: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthUser {
    pub id: String,
    #[serde(default)]
    pub email: String,
}

/// Parse `auth status --json` output. Read whatever the exit status: the CLI
/// reports "signed out" as data, and only a real failure lacks the shape.
fn parse_auth_status(out: &shared_daemon::CliOutput) -> Result<AuthStatus, CliError> {
    serde_json::from_str::<AuthStatus>(out.stdout.trim()).map_err(|_| out.error("auth status"))
}

pub(crate) async fn auth_status<R: Runtime>(app: &AppHandle<R>) -> Result<AuthStatus, CliError> {
    let out = shared_daemon::run(
        app,
        vec!["auth".into(), "status".into(), "--json".into()],
        None,
    )
    .await?;
    parse_auth_status(&out)
}

/// Where a handoff may take the main window: the Pi Dash server's own `/api/`
/// (the redeem view), over http(s), on one of the origins this build talks
/// to. Anything else is refused rather than navigated.
fn handoff_target(raw: &str, servers: &[&Url]) -> Result<Url, CliError> {
    let refused = || CliError::new("handoff_failed", "pidash returned a sign-in link for another site");
    let url = Url::parse(raw).map_err(|_| refused())?;
    let allowed = matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && url.path().starts_with("/api/")
        && servers.iter().any(|server| server.origin() == url.origin());
    allowed.then_some(url).ok_or_else(refused)
}

// ---------------------------------------------------------------------------
// The app's own last auth change
// ---------------------------------------------------------------------------

/// The app's signed-in user and when that last changed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DesktopAuthMark {
    pub user_id: Option<String>,
    pub changed_at: Option<String>,
}

/// Fold one observation of the app's signed-in user into the stored mark.
///
/// A different user (or signed in ↔ out) is a change, stamped `now`. The
/// first sign-in ever observed is stamped too — a build upgrading into this
/// one has no record, and the session it finds is the one the user has now.
fn observe(previous: Option<DesktopAuthMark>, user_id: Option<String>, now: String) -> (DesktopAuthMark, bool) {
    match previous {
        Some(mark) if mark.user_id == user_id => (mark, false),
        None if user_id.is_none() => (DesktopAuthMark::default(), false),
        _ => (
            DesktopAuthMark {
                user_id,
                changed_at: Some(now),
            },
            true,
        ),
    }
}

fn mark_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("desktop-auth.json"))
        .map_err(|e| format!("resolving app data dir: {e}"))
}

fn read_mark(path: &Path) -> Option<DesktopAuthMark> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Seconds since the epoch as RFC 3339 UTC (`2026-10-26T09:30:00Z`), the
/// format the CLI writes `changed_at` in. Hand-rolled to keep a date crate
/// out of the app for one timestamp.
pub(crate) fn rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn now_rfc3339() -> String {
    rfc3339(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Who the CLI is signed in as. Rejects with a [`CliError`]; `cli_outdated`
/// means the installed CLI predates `auth status --json`.
#[tauri::command]
pub async fn pidash_auth_status<R: Runtime>(app: AppHandle<R>) -> Result<AuthStatus, CliError> {
    auth_status(&app).await
}

/// Sign the CLI (and so the shared daemon) out. The caller has already warned
/// about in-flight runs, so the CLI is told not to ask again.
#[tauri::command]
pub async fn pidash_auth_logout<R: Runtime>(app: AppHandle<R>) -> Result<(), CliError> {
    let out = shared_daemon::run(
        &app,
        vec!["auth".into(), "logout".into(), "--yes".into()],
        None,
    )
    .await?;
    if out.success {
        return Ok(());
    }
    if out.rejected_command_line() {
        // An older CLI has no `--yes` — and no prompt for it to skip.
        let old = shared_daemon::run(&app, vec!["auth".into(), "logout".into()], None).await?;
        return if old.success { Ok(()) } else { Err(old.error("auth logout")) };
    }
    Err(out.error("auth logout"))
}

/// Sign the app in as the CLI's user: mint a one-time code with the CLI's
/// handoff key, then load the redeem URL in the main window.
#[tauri::command]
pub async fn pidash_desktop_handoff<R: Runtime>(app: AppHandle<R>) -> Result<(), CliError> {
    let out = shared_daemon::run(&app, vec!["__desktop".into(), "handoff".into()], None).await?;
    if !out.success {
        return Err(out.error("__desktop handoff"));
    }
    #[derive(Deserialize)]
    struct Handoff {
        redeem_url: String,
    }
    let handoff: Handoff = serde_json::from_str(out.stdout.trim())
        .map_err(|_| CliError::new("handoff_failed", "pidash __desktop handoff printed no sign-in link"))?;
    let target = {
        let config = app.state::<crate::AppConfig>();
        let api = &app.state::<crate::desktop_http::DesktopHttp>().api_url;
        handoff_target(&handoff.redeem_url, &[&config.target_url, api])?
    };
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| CliError::new("handoff_failed", "no main window"))?;
    window
        .navigate(target)
        .map_err(|e| CliError::new("handoff_failed", format!("opening the sign-in link: {e}")))
}

/// Record the app's currently signed-in user (`None` when signed out) and
/// return when that last changed.
#[tauri::command]
pub async fn desktop_auth_observe<R: Runtime>(
    app: AppHandle<R>,
    user_id: Option<String>,
) -> Result<DesktopAuthMark, String> {
    let path = mark_path(&app)?;
    let user_id = user_id.filter(|id| !id.trim().is_empty());
    let (mark, changed) = observe(read_mark(&path), user_id, now_rfc3339());
    if changed {
        let bytes = serde_json::to_vec(&mark).map_err(|e| e.to_string())?;
        crate::managed_runner::atomic_write(&path, &bytes)?;
    }
    Ok(mark)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: i32, stdout: &str, stderr: &str) -> shared_daemon::CliOutput {
        shared_daemon::CliOutput {
            success: code == 0,
            code: Some(code),
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn auth_status_parses_the_contract_shape() {
        let json = r#"{"signed_in": true, "cloud_url": "https://pidash.test",
            "user": {"id": "u-1", "email": "a@b.test"}, "workspace_slug": "acme",
            "dev_machine_id": "m-1", "changed_at": "2026-10-26T09:00:00Z",
            "daemon_running": true, "active_runs": 2}"#;
        let status = parse_auth_status(&out(0, json, "")).unwrap();
        assert!(status.signed_in && status.daemon_running);
        assert_eq!(status.user.unwrap().id, "u-1");
        assert_eq!(status.active_runs, 2);
        let signed_out = r#"{"signed_in": false, "cloud_url": null, "user": null, "workspace_slug": null,
            "dev_machine_id": null, "changed_at": null, "daemon_running": false, "active_runs": 0}"#;
        assert!(!parse_auth_status(&out(0, signed_out, "")).unwrap().signed_in);
    }

    #[test]
    fn auth_status_from_an_old_cli_is_outdated() {
        let e = parse_auth_status(&out(2, "", "error: unexpected argument '--json' found")).unwrap_err();
        assert!(e.is_outdated());
        // The old human-readable output is not mistaken for a status.
        let e = parse_auth_status(&out(0, "Logged in to https://pidash.test", "")).unwrap_err();
        assert_eq!(e.code, "cli_failed");
    }

    #[test]
    fn handoff_only_navigates_to_the_servers_api() {
        let web = Url::parse("https://pidash.example.com").unwrap();
        let api = Url::parse("https://api.pidash.example.com").unwrap();
        let servers = [&web, &api];
        for ok in [
            "https://pidash.example.com/api/v1/auth/desktop-handoff/redeem/?code=abc",
            "https://api.pidash.example.com/api/auth/desktop-handoff/redeem/?code=abc",
        ] {
            assert_eq!(handoff_target(ok, &servers).unwrap().as_str(), ok);
        }
        for refused in [
            "https://evil.example.com/api/v1/auth/desktop-handoff/redeem/?code=abc",
            "http://pidash.example.com/api/v1/auth/desktop-handoff/redeem/?code=abc",
            "https://pidash.example.com/acme/projects/",
            "https://user:pw@pidash.example.com/api/v1/auth/desktop-handoff/redeem/",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "not a url",
        ] {
            assert_eq!(handoff_target(refused, &servers).unwrap_err().code, "handoff_failed", "{refused}");
        }
    }

    #[test]
    fn auth_mark_changes_only_when_the_user_does() {
        let at = |s: &str| Some(s.to_string());
        let u = |s: &str| Some(s.to_string());
        // Nothing recorded, signed out: nothing to record.
        assert_eq!(observe(None, None, "t0".into()), (DesktopAuthMark::default(), false));
        // First sign-in seen (including an upgrade from a build without marks).
        let (a, changed) = observe(None, u("a"), "t1".into());
        assert!(changed);
        assert_eq!(a, DesktopAuthMark { user_id: u("a"), changed_at: at("t1") });
        // Same user again: kept, with its original time.
        assert_eq!(observe(Some(a.clone()), u("a"), "t2".into()), (a.clone(), false));
        // Switch and sign-out are both changes.
        let (b, _) = observe(Some(a), u("b"), "t3".into());
        assert_eq!(b.changed_at, at("t3"));
        let (out, changed) = observe(Some(b), None, "t4".into());
        assert!(changed);
        assert_eq!(out, DesktopAuthMark { user_id: None, changed_at: at("t4") });
    }

    #[test]
    fn rfc3339_matches_known_instants() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_793_007_000), "2026-10-26T09:30:00Z");
        assert_eq!(rfc3339(4_102_444_799), "2099-12-31T23:59:59Z");
    }
}
