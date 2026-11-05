// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The app's own files, under Tauri's app-data dir.
//!
//! Since the app became a client of the computer's Pi Dash daemon
//! (`.ai_design/built_engine_as_runner/design.md` §4.1), it owns no daemon,
//! runner or engine. What it keeps here is its own state: local chat history
//! (`chat/`) and per-account settings (`accounts/<key>/settings.json`).
//!
//! The rest of the `managed/` tree is what builds before that change wrote for
//! the private child daemon they ran — one `pidash/<workspace>/` tree per
//! workspace, the engine's per-account home under `accounts/`, task folders
//! under `workdirs/` and the model token under `runtime/`. New builds only
//! read it, to move it to the shared daemon (`migration`), and never write a
//! config or credential into it.

use std::io::Write;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, Runtime};

#[derive(Debug, Clone)]
pub struct AppPaths {
    /// `<app-data>/managed`. The name is kept so existing chat history and
    /// settings stay where they are.
    pub root: PathBuf,
    /// `managed/accounts/<account-key>/`: settings, and the legacy engine home.
    pub accounts_dir: PathBuf,
    /// Root of the per-account local chat history tree (see `chat_history`).
    pub chat_dir: PathBuf,
    /// Legacy: `managed/pidash/<workspace>/`, one private daemon tree per
    /// workspace.
    pub legacy_trees_dir: PathBuf,
    /// Legacy: `managed/runtime/model.token`, the model credential the app
    /// used to write for the engine.
    pub legacy_model_token: PathBuf,
}

impl AppPaths {
    pub fn resolve<R: Runtime>(app: &AppHandle<R>) -> Result<Self, String> {
        let base = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("resolving app data dir: {e}"))?;
        Ok(Self::at(base.join("managed")))
    }

    /// The layout rooted at `root` (`<app-data>/managed`).
    pub fn at(root: PathBuf) -> Self {
        Self {
            accounts_dir: root.join("accounts"),
            chat_dir: root.join("chat"),
            legacy_trees_dir: root.join("pidash"),
            legacy_model_token: root.join("runtime").join("model.token"),
            root,
        }
    }

    /// The legacy engine home of one account:
    /// `managed/accounts/<account-key>/codex-home`, keyed like the chat store.
    pub(crate) fn legacy_engine_home(&self, account: &str) -> Result<PathBuf, String> {
        Ok(self
            .accounts_dir
            .join(crate::chat_history::account_key(account)?)
            .join("codex-home"))
    }

    /// Legacy: task folders of the private daemon's engine runners
    /// (`managed/workdirs/<workspace>/<project>`). Kept on disk; the migrated
    /// runners keep pointing at them.
    #[cfg(test)]
    pub(crate) fn legacy_workdirs(&self) -> PathBuf {
        self.root.join("workdirs")
    }

    /// The legacy device identity (`managed/device-id`), read by no new code.
    pub(crate) fn legacy_device_id(&self) -> PathBuf {
        self.root.join("device-id")
    }
}

/// Write via a temp file and rename, so a reader never sees a half-written
/// file. Owner-only on Unix.
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

    #[test]
    fn layout_keeps_the_app_state_where_it_was() {
        let paths = AppPaths::at(PathBuf::from("/data/managed"));
        assert_eq!(paths.chat_dir, PathBuf::from("/data/managed/chat"));
        assert_eq!(paths.accounts_dir, PathBuf::from("/data/managed/accounts"));
        assert_eq!(
            paths.legacy_trees_dir,
            PathBuf::from("/data/managed/pidash")
        );
        assert_eq!(
            paths.legacy_model_token,
            PathBuf::from("/data/managed/runtime/model.token")
        );
    }

    #[test]
    fn legacy_engine_home_is_per_account() {
        let paths = AppPaths::at(PathBuf::from("/data/managed"));
        let a = paths.legacy_engine_home("user-a").unwrap();
        let b = paths.legacy_engine_home("user-b").unwrap();
        assert_ne!(a, b);
        let key = crate::chat_history::account_key("user-a").unwrap();
        assert_eq!(a, paths.accounts_dir.join(key).join("codex-home"));
        assert!(paths.legacy_engine_home("").is_err());
    }

    #[test]
    fn atomic_write_sets_owner_only_permissions_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.json");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(!path.with_extension("tmp").exists());
    }
}
