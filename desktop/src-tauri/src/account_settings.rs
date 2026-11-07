// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Per-account settings that must outlive the webview's own storage.
//!
//! Sign-out wipes the webview's whole profile (`desktop_clear_web_data`), and
//! `localStorage` goes with the cookies. The local-chat approval mode lives
//! there, so without a copy on this side a user who chose "Ask" came back from
//! a sign-out running under the full-access default, with nothing telling them
//! so. The overlay hands the modes over just before the wipe and asks for them
//! again once the same account is signed back in.
//!
//! Stored as `managed/accounts/<account-key>/settings.json`, keyed like the
//! chat store, so one account's choice never applies to the next person who
//! signs in on the machine. It is deliberately *not* under `managed/chat/`:
//! that tree is what "delete chat history" removes, and a setting is not
//! history.
//!
//! The same file holds the app's one-time notices (design §13): the legacy
//! task folders a migration left on disk and the projects whose engines became
//! pod members on upgrade (PO-11), until the user dismisses them; and the keys
//! of every notice dismissed — and every "Replace it" prompt declined (§4.10)
//! — so none is shown twice.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use tauri::{AppHandle, Runtime};

use crate::app_paths::AppPaths;

/// The modes the engine understands (`runner/src/approval/mode.rs`).
const APPROVAL_MODES: [&str; 3] = ["ask", "workspace", "full_access"];

/// Upper bound on remembered runners — the desktop bundle hosts one today.
const MAX_RUNNERS: usize = 32;

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct AccountSettings {
    /// Approval mode per chat runner id, as the chat page keys it.
    #[serde(default)]
    chat_approval_modes: BTreeMap<String, String>,
    /// One-time notices not yet dismissed.
    #[serde(default)]
    notices: Notices,
    /// Keys of dismissed notices and declined prompts, oldest first.
    #[serde(default)]
    dismissed: Vec<String>,
}

/// A legacy task folder the migration left on disk: the daemon already ran
/// the engine for that project, so the server kept the daemon's runner
/// (design §13, "Collision") and nothing uses this folder any more.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LegacyFolderNotice {
    pub workspace: String,
    pub project: String,
    pub path: String,
}

/// A project whose engine runner on this computer became a pod member on
/// upgrade (PO-11), with the runner the "pod work" switch acts on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PodMemberNotice {
    pub runner_id: String,
    pub workspace: String,
    pub project: String,
}

/// The one-time notices still to show.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Notices {
    #[serde(default)]
    pub legacy_folders: Vec<LegacyFolderNotice>,
    #[serde(default)]
    pub pod_member_projects: Vec<PodMemberNotice>,
}

/// What the webview reads: the pending notices and the dismissed keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct NoticesView {
    pub notices: Notices,
    pub dismissed: Vec<String>,
}

/// Bounds on what the (user-writable) settings file may hold.
const MAX_NOTICES: usize = 64;
const MAX_DISMISSED: usize = 512;
const MAX_FIELD: usize = 4096;

impl LegacyFolderNotice {
    fn key(&self) -> String {
        format!("folder:{}", self.path)
    }
}

impl PodMemberNotice {
    fn key(&self) -> String {
        format!("pod:{}", self.runner_id)
    }
}

/// A notice field or key: non-blank, bounded, no NUL.
fn valid_field(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= MAX_FIELD && !s.contains('\0')
}

/// Remember `modes` (runner id → approval mode) for `account`.
///
/// Merged over what is already stored, so a runner the webview did not mention
/// keeps its mode.
#[tauri::command]
pub async fn chat_approval_modes_save<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    modes: BTreeMap<String, String>,
) -> Result<(), String> {
    save_approval_modes(&AppPaths::resolve(&app)?, &account, modes)
}

/// The approval modes remembered for `account`; empty when there are none.
#[tauri::command]
pub async fn chat_approval_modes_load<R: Runtime>(
    app: AppHandle<R>,
    account: String,
) -> Result<BTreeMap<String, String>, String> {
    load_approval_modes(&AppPaths::resolve(&app)?, &account)
}

/// Serialises every read-modify-write of an account's `settings.json`:
/// Tauri runs async commands concurrently, and two unlocked updates (a
/// dismissal and a new notice, say) would lose one of them.
static SETTINGS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn settings_lock() -> std::sync::MutexGuard<'static, ()> {
    SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn save_approval_modes(
    paths: &AppPaths,
    account: &str,
    modes: BTreeMap<String, String>,
) -> Result<(), String> {
    let _guard = settings_lock();
    let mut settings = read(paths, account)?;
    for (runner, mode) in modes {
        if !valid_runner(&runner) {
            return Err(format!("invalid runner id {runner:?}"));
        }
        if !APPROVAL_MODES.contains(&mode.as_str()) {
            return Err(format!("unknown approval mode {mode:?}"));
        }
        settings.chat_approval_modes.insert(runner, mode);
    }
    if settings.chat_approval_modes.len() > MAX_RUNNERS {
        return Err("too many runners".into());
    }
    write(paths, account, &settings)
}

/// The pending notices and dismissed keys for `account`.
#[tauri::command]
pub async fn desktop_notices_load<R: Runtime>(
    app: AppHandle<R>,
    account: String,
) -> Result<NoticesView, String> {
    load_notices(&AppPaths::resolve(&app)?, &account)
}

/// Add `notices` to the pending ones — except any already pending or already
/// dismissed — and return the result.
#[tauri::command]
pub async fn desktop_notices_record<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    notices: Notices,
) -> Result<NoticesView, String> {
    record_notices(&AppPaths::resolve(&app)?, &account, notices)
}

/// Dismiss the notices (or decline the prompts) with these keys, for good.
#[tauri::command]
pub async fn desktop_notices_dismiss<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    keys: Vec<String>,
) -> Result<NoticesView, String> {
    dismiss_notices(&AppPaths::resolve(&app)?, &account, keys)
}

fn view(settings: AccountSettings) -> NoticesView {
    NoticesView {
        notices: settings.notices,
        dismissed: settings.dismissed,
    }
}

/// Drop anything malformed, dismissed or duplicated: the file is
/// user-writable, and the webview must only ever see well-formed notices.
fn sanitize(settings: &mut AccountSettings) {
    let mut keys = BTreeSet::new();
    settings
        .dismissed
        .retain(|k| valid_field(k) && keys.insert(k.clone()));
    let excess = settings.dismissed.len().saturating_sub(MAX_DISMISSED);
    settings.dismissed.drain(..excess);
    let dismissed: BTreeSet<&String> = settings.dismissed.iter().collect();
    let mut seen = BTreeSet::new();
    settings.notices.legacy_folders.retain(|n| {
        valid_field(&n.workspace)
            && valid_field(&n.project)
            && valid_field(&n.path)
            && !dismissed.contains(&n.key())
            && seen.insert(n.key())
    });
    settings.notices.pod_member_projects.retain(|n| {
        valid_runner(&n.runner_id)
            && valid_field(&n.workspace)
            && valid_field(&n.project)
            && !dismissed.contains(&n.key())
            && seen.insert(n.key())
    });
    settings.notices.legacy_folders.truncate(MAX_NOTICES);
    settings.notices.pod_member_projects.truncate(MAX_NOTICES);
}

fn load_notices(paths: &AppPaths, account: &str) -> Result<NoticesView, String> {
    let mut settings = read(paths, account)?;
    sanitize(&mut settings);
    Ok(view(settings))
}

fn record_notices(
    paths: &AppPaths,
    account: &str,
    notices: Notices,
) -> Result<NoticesView, String> {
    let _guard = settings_lock();
    let mut settings = read(paths, account)?;
    settings
        .notices
        .legacy_folders
        .extend(notices.legacy_folders);
    settings
        .notices
        .pod_member_projects
        .extend(notices.pod_member_projects);
    sanitize(&mut settings);
    write(paths, account, &settings)?;
    Ok(view(settings))
}

fn dismiss_notices(
    paths: &AppPaths,
    account: &str,
    keys: Vec<String>,
) -> Result<NoticesView, String> {
    if let Some(bad) = keys.iter().find(|k| !valid_field(k)) {
        return Err(format!("invalid notice key {bad:?}"));
    }
    let _guard = settings_lock();
    let mut settings = read(paths, account)?;
    settings.dismissed.extend(keys);
    sanitize(&mut settings);
    write(paths, account, &settings)?;
    Ok(view(settings))
}

fn write(paths: &AppPaths, account: &str, settings: &AccountSettings) -> Result<(), String> {
    let path = settings_path(paths, account)?;
    let bytes =
        serde_json::to_vec_pretty(settings).map_err(|e| format!("account settings: {e}"))?;
    crate::app_paths::atomic_write(&path, &bytes)?;
    if let Some(dir) = path.parent() {
        crate::chat_history::restrict_dir(dir);
    }
    Ok(())
}

fn load_approval_modes(
    paths: &AppPaths,
    account: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut modes = read(paths, account)?.chat_approval_modes;
    // The file is user-writable; hand back only what the webview may apply.
    modes.retain(|runner, mode| valid_runner(runner) && APPROVAL_MODES.contains(&mode.as_str()));
    Ok(modes)
}

/// `managed/accounts/<account-key>/settings.json`.
fn settings_path(paths: &AppPaths, account: &str) -> Result<PathBuf, String> {
    Ok(paths
        .root
        .join("accounts")
        .join(crate::chat_history::account_key(account)?)
        .join("settings.json"))
}

/// Missing or unreadable settings are the same as none: a damaged file must not
/// stop sign-out, and the next save rewrites it.
fn read(paths: &AppPaths, account: &str) -> Result<AccountSettings, String> {
    let path = settings_path(paths, account)?;
    Ok(std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default())
}

fn valid_runner(runner: &str) -> bool {
    !runner.is_empty()
        && runner.len() <= 64
        && runner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn paths(root: &Path) -> AppPaths {
        AppPaths::at(root.join("managed"))
    }

    fn modes(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(runner, mode)| (runner.to_string(), mode.to_string()))
            .collect()
    }

    #[test]
    fn a_saved_mode_is_read_back_for_the_same_account_only() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        save_approval_modes(&paths, "user-a", modes(&[("pidash-builtin", "ask")])).unwrap();

        assert_eq!(
            load_approval_modes(&paths, "user-a").unwrap(),
            modes(&[("pidash-builtin", "ask")])
        );
        // The next person on the machine starts with no remembered mode.
        assert!(load_approval_modes(&paths, "user-b").unwrap().is_empty());
    }

    #[test]
    fn settings_live_under_the_account_root_and_survive_a_history_wipe() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        save_approval_modes(&paths, "user-a", modes(&[("pidash-builtin", "ask")])).unwrap();

        let key = crate::chat_history::account_key("user-a").unwrap();
        let file = paths.root.join("accounts").join(&key).join("settings.json");
        assert!(file.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&file), 0o600);
            assert_eq!(mode(file.parent().unwrap()), 0o700);
        }

        // "Delete chat history" removes the chat tree, not the user's settings.
        crate::chat_history::clear_account_history(&paths, "user-a").unwrap();
        assert_eq!(
            load_approval_modes(&paths, "user-a").unwrap(),
            modes(&[("pidash-builtin", "ask")])
        );
    }

    #[test]
    fn a_later_save_replaces_the_mode_and_keeps_other_runners() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        save_approval_modes(
            &paths,
            "user-a",
            modes(&[("pidash-builtin", "ask"), ("other", "workspace")]),
        )
        .unwrap();
        save_approval_modes(
            &paths,
            "user-a",
            modes(&[("pidash-builtin", "full_access")]),
        )
        .unwrap();

        assert_eq!(
            load_approval_modes(&paths, "user-a").unwrap(),
            modes(&[("other", "workspace"), ("pidash-builtin", "full_access")])
        );
    }

    #[test]
    fn unknown_modes_and_unsafe_runner_ids_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        assert!(
            save_approval_modes(&paths, "user-a", modes(&[("pidash-builtin", "yolo")])).is_err()
        );
        assert!(save_approval_modes(&paths, "user-a", modes(&[("../x", "ask")])).is_err());
        assert!(save_approval_modes(&paths, "  ", modes(&[("pidash-builtin", "ask")])).is_err());
        assert!(load_approval_modes(&paths, "user-a").unwrap().is_empty());
    }

    #[test]
    fn a_hand_edited_file_cannot_smuggle_in_an_unknown_mode() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let path = settings_path(&paths, "user-a").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            br#"{"chat_approval_modes":{"pidash-builtin":"ask","other":"root"}}"#,
        )
        .unwrap();
        assert_eq!(
            load_approval_modes(&paths, "user-a").unwrap(),
            modes(&[("pidash-builtin", "ask")])
        );

        // A file that is not JSON at all reads as "nothing remembered".
        std::fs::write(&path, b"not json").unwrap();
        assert!(load_approval_modes(&paths, "user-a").unwrap().is_empty());
    }

    fn folder(path: &str) -> LegacyFolderNotice {
        LegacyFolderNotice {
            workspace: "acme".into(),
            project: "WEB".into(),
            path: path.into(),
        }
    }

    fn pod(runner: &str) -> PodMemberNotice {
        PodMemberNotice {
            runner_id: runner.into(),
            workspace: "acme".into(),
            project: "WEB".into(),
        }
    }

    #[test]
    fn notices_are_kept_until_dismissed_and_never_come_back() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let notices = Notices {
            legacy_folders: vec![folder("/old/acme/WEB")],
            pod_member_projects: vec![pod("r-1")],
        };
        record_notices(&paths, "user-a", notices.clone()).unwrap();
        // A retried migration records them again: no duplicates.
        let again = record_notices(&paths, "user-a", notices.clone()).unwrap();
        assert_eq!(again.notices, notices);
        // Still there on the next launch (a fresh read).
        assert_eq!(load_notices(&paths, "user-a").unwrap().notices, notices);
        // Another account sees none of it.
        assert_eq!(
            load_notices(&paths, "user-b").unwrap(),
            NoticesView::default()
        );

        let after = dismiss_notices(
            &paths,
            "user-a",
            vec!["folder:/old/acme/WEB".into(), "pod:r-1".into()],
        )
        .unwrap();
        assert_eq!(after.notices, Notices::default());
        // Shown once: recording the same notices again shows nothing.
        let replay = record_notices(&paths, "user-a", notices).unwrap();
        assert_eq!(replay.notices, Notices::default());
        assert_eq!(replay.dismissed, vec!["folder:/old/acme/WEB", "pod:r-1"]);
    }

    /// Concurrent updates (Tauri runs commands in parallel) lose nothing.
    #[test]
    fn concurrent_records_and_dismissals_all_land() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        std::thread::scope(|scope| {
            for i in 0..16 {
                let paths = &paths;
                scope.spawn(move || {
                    if i % 2 == 0 {
                        record_notices(
                            paths,
                            "user-a",
                            Notices {
                                legacy_folders: vec![],
                                pod_member_projects: vec![pod(&format!("r-{i}"))],
                            },
                        )
                        .unwrap();
                    } else {
                        dismiss_notices(paths, "user-a", vec![format!("pod:x-{i}")]).unwrap();
                    }
                });
            }
        });
        let view = load_notices(&paths, "user-a").unwrap();
        assert_eq!(view.notices.pod_member_projects.len(), 8);
        assert_eq!(view.dismissed.len(), 8);
    }

    #[test]
    fn notices_and_approval_modes_share_the_file_without_clobbering() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        record_notices(
            &paths,
            "user-a",
            Notices {
                legacy_folders: vec![folder("/old")],
                pod_member_projects: vec![],
            },
        )
        .unwrap();
        save_approval_modes(&paths, "user-a", modes(&[("pidash-builtin", "ask")])).unwrap();
        dismiss_notices(&paths, "user-a", vec!["replace:m-1".into()]).unwrap();
        dismiss_notices(&paths, "user-a", vec!["replace:m-1".into()]).unwrap();
        let view = load_notices(&paths, "user-a").unwrap();
        assert_eq!(view.notices.legacy_folders, vec![folder("/old")]);
        assert_eq!(view.dismissed, vec!["replace:m-1"]);
        assert_eq!(
            load_approval_modes(&paths, "user-a").unwrap(),
            modes(&[("pidash-builtin", "ask")])
        );
    }

    #[test]
    fn malformed_notices_and_keys_are_dropped_or_refused() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let view = record_notices(
            &paths,
            "user-a",
            Notices {
                legacy_folders: vec![folder(""), folder("/ok")],
                pod_member_projects: vec![pod("../escape"), pod("r-2")],
            },
        )
        .unwrap();
        assert_eq!(view.notices.legacy_folders, vec![folder("/ok")]);
        assert_eq!(view.notices.pod_member_projects, vec![pod("r-2")]);
        assert!(dismiss_notices(&paths, "user-a", vec!["  ".into()]).is_err());

        let path = settings_path(&paths, "user-a").unwrap();
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(
            load_notices(&paths, "user-a").unwrap(),
            NoticesView::default()
        );
    }
}
