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

use std::collections::BTreeMap;
use std::path::PathBuf;

use tauri::{AppHandle, Runtime};

use crate::managed_runner::ManagedPaths;

/// The modes the engine understands (`runner/src/approval/mode.rs`).
const APPROVAL_MODES: [&str; 3] = ["ask", "workspace", "full_access"];

/// Upper bound on remembered runners — the desktop bundle hosts one today.
const MAX_RUNNERS: usize = 32;

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct AccountSettings {
    /// Approval mode per chat runner id, as the chat page keys it.
    #[serde(default)]
    chat_approval_modes: BTreeMap<String, String>,
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
    save_approval_modes(&ManagedPaths::resolve(&app)?, &account, modes)
}

/// The approval modes remembered for `account`; empty when there are none.
#[tauri::command]
pub async fn chat_approval_modes_load<R: Runtime>(
    app: AppHandle<R>,
    account: String,
) -> Result<BTreeMap<String, String>, String> {
    load_approval_modes(&ManagedPaths::resolve(&app)?, &account)
}

fn save_approval_modes(
    paths: &ManagedPaths,
    account: &str,
    modes: BTreeMap<String, String>,
) -> Result<(), String> {
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
    let path = settings_path(paths, account)?;
    let bytes =
        serde_json::to_vec_pretty(&settings).map_err(|e| format!("account settings: {e}"))?;
    crate::managed_runner::atomic_write(&path, &bytes)?;
    if let Some(dir) = path.parent() {
        crate::chat_history::restrict_dir(dir);
    }
    Ok(())
}

fn load_approval_modes(
    paths: &ManagedPaths,
    account: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut modes = read(paths, account)?.chat_approval_modes;
    // The file is user-writable; hand back only what the webview may apply.
    modes.retain(|runner, mode| valid_runner(runner) && APPROVAL_MODES.contains(&mode.as_str()));
    Ok(modes)
}

/// `managed/accounts/<account-key>/settings.json`.
fn settings_path(paths: &ManagedPaths, account: &str) -> Result<PathBuf, String> {
    Ok(paths
        .root
        .join("accounts")
        .join(crate::chat_history::account_key(account)?)
        .join("settings.json"))
}

/// Missing or unreadable settings are the same as none: a damaged file must not
/// stop sign-out, and the next save rewrites it.
fn read(paths: &ManagedPaths, account: &str) -> Result<AccountSettings, String> {
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

    fn paths(root: &Path) -> ManagedPaths {
        ManagedPaths::for_test_root(root.join("managed"))
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
}
