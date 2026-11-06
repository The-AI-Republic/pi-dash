// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Moving from the private child daemon of older builds to the computer's
//! shared daemon (`.ai_design/built_engine_as_runner/design.md` §13, desktop
//! steps 1–5).
//!
//! Older builds ran one daemon per workspace under
//! `<app-data>/managed/pidash/<workspace>/`, each a *legacy desktop machine*
//! on the server. On the first launch of a new build the webview, which holds
//! the session, drives the steps; this module owns the files:
//!
//! - [`legacy_scan`] lists the trees still to migrate: the workspace, the
//!   legacy machine id (`[daemon].dev_machine_id`) and each engine runner
//!   block with its project and working dir (steps 2 and 3 need them);
//! - [`legacy_finish`] runs steps 4 and 5 once the merge and the
//!   `EngineRunnerAdd`s succeeded: it moves the signed-in account's legacy
//!   engine home into the daemon's engine home, then deletes the tree's
//!   `config.toml` and credentials and the legacy model token. Task folders
//!   are never touched — the migrated runners keep using them.
//!
//! A tree without a `config.toml` is not listed, so a finished tree is never
//! migrated twice, and a tree whose steps were blocked (no daemon, another
//! account, the service declined) is simply listed again on the next launch.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Runtime};
use uuid::Uuid;

use crate::app_paths::AppPaths;
use crate::daemon::DaemonPaths;

/// One engine runner block of a legacy tree.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LegacyEngineRunner {
    pub name: String,
    pub runner_id: Option<Uuid>,
    pub project: String,
    pub working_dir: PathBuf,
}

/// A legacy private-daemon tree still to migrate.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LegacyTree {
    pub workspace: String,
    /// The tree's directory, handed back to [`legacy_finish`].
    pub dir: PathBuf,
    pub cloud_url: Option<String>,
    /// The legacy desktop machine (the merge endpoint's source).
    pub dev_machine_id: Option<Uuid>,
    pub engine_runners: Vec<LegacyEngineRunner>,
}

/// Read one tree's `config.toml`. `None` when it is gone (already migrated)
/// or unreadable.
fn read_tree(dir: &Path, workspace_hint: Option<&str>) -> Option<LegacyTree> {
    let text = std::fs::read_to_string(dir.join("config.toml")).ok()?;
    let cfg: toml::Value = toml::from_str(&text).ok()?;
    let daemon = cfg.get("daemon");
    let str_at = |v: Option<&toml::Value>, key: &str| {
        v.and_then(|t| t.get(key))
            .and_then(|s| s.as_str())
            .map(str::to_string)
    };
    let blocks = cfg
        .get("runner")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let engine_runners: Vec<LegacyEngineRunner> = blocks
        .iter()
        .filter_map(|b| {
            Some(LegacyEngineRunner {
                name: str_at(Some(b), "name")?,
                runner_id: str_at(Some(b), "runner_id").and_then(|s| s.parse().ok()),
                project: str_at(Some(b), "project_slug")?,
                working_dir: PathBuf::from(str_at(b.get("workspace"), "working_dir")?),
            })
        })
        .collect();
    let workspace = workspace_hint
        .map(str::to_string)
        .or_else(|| str_at(cfg.get("cli"), "workspace_slug"))
        .or_else(|| {
            blocks
                .iter()
                .find_map(|b| str_at(Some(b), "workspace_slug"))
        })?;
    Some(LegacyTree {
        workspace,
        dir: dir.to_path_buf(),
        cloud_url: str_at(daemon, "cloud_url"),
        dev_machine_id: str_at(daemon, "dev_machine_id").and_then(|s| s.parse().ok()),
        engine_runners,
    })
}

/// Every legacy tree still holding a config: the per-workspace trees, and the
/// flat tree the earliest builds wrote at `managed/pidash/config.toml`.
pub fn scan(paths: &AppPaths) -> Vec<LegacyTree> {
    let mut trees = Vec::new();
    if let Some(flat) = read_tree(&paths.legacy_trees_dir, None) {
        trees.push(flat);
    }
    let Ok(entries) = std::fs::read_dir(&paths.legacy_trees_dir) else {
        return trees;
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
        if name.as_deref() == Some("data") {
            continue; // the flat tree's data dir
        }
        if let Some(tree) = read_tree(&dir, name.as_deref()) {
            trees.push(tree);
        }
    }
    trees
}

/// What [`finish`] did.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct FinishReport {
    /// Where the account's engine home now is, when one was moved.
    pub engine_home_moved_to: Option<PathBuf>,
    /// Files deleted.
    pub removed: Vec<PathBuf>,
}

/// Steps 4 and 5 for one tree. Idempotent: run twice, the second run finds
/// nothing to move or delete.
pub fn finish(
    paths: &AppPaths,
    daemon: &DaemonPaths,
    tree_dir: &Path,
    account: &str,
    daemon_account: Option<&str>,
) -> Result<FinishReport, String> {
    let tree_dir = confine(paths, tree_dir)?;
    let mut report = FinishReport::default();

    // Step 4: the signed-in account's engine home. Other accounts' homes stay.
    let source = paths.legacy_engine_home(account)?;
    if source.is_dir() {
        // The daemon keys its engine home by its own recorded account and
        // uses `default` when it recorded none; follow it, so the history
        // lands where the engine reads it.
        let target = daemon.engine_home(daemon_account);
        move_merge(&source, &target)?;
        restrict_tree(&target);
        report.engine_home_moved_to = Some(target);
    }

    // Step 5: the tree's config and credentials, and the model token.
    let mut doomed = vec![
        tree_dir.join("config.toml"),
        tree_dir.join("credentials.toml"),
        paths.legacy_model_token.clone(),
    ];
    if let Ok(runners) = std::fs::read_dir(tree_dir.join("data").join("runners")) {
        for entry in runners.flatten() {
            doomed.push(entry.path().join("credentials.toml"));
        }
    }
    for file in doomed {
        match std::fs::remove_file(&file) {
            Ok(()) => report.removed.push(file),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("removing {}: {e}", file.display())),
        }
    }
    // The device identity only identified legacy machines; once no tree is
    // left it has nothing to identify.
    if scan(paths).is_empty() {
        let id = paths.legacy_device_id();
        if std::fs::remove_file(&id).is_ok() {
            report.removed.push(id);
        }
    }
    Ok(report)
}

/// `tree_dir` comes from the webview: accept only a tree under the app's own
/// legacy root, so this command can never delete files anywhere else.
fn confine(paths: &AppPaths, tree_dir: &Path) -> Result<PathBuf, String> {
    let root = paths
        .legacy_trees_dir
        .canonicalize()
        .map_err(|e| format!("legacy tree root: {e}"))?;
    let dir = tree_dir
        .canonicalize()
        .map_err(|e| format!("legacy tree {}: {e}", tree_dir.display()))?;
    if dir == root || dir.parent() == Some(root.as_path()) {
        Ok(dir)
    } else {
        Err(format!(
            "{} is not a legacy Pi Dash tree",
            tree_dir.display()
        ))
    }
}

/// Move `source` into `target`. When `target` does not exist this is a
/// rename (a copy and delete across filesystems). When it does, entries it
/// lacks are moved in and entries it has are left alone on both sides —
/// nothing the daemon already wrote is overwritten. The app's old
/// `config.toml` is never carried over: the daemon writes the engine's own.
pub fn move_merge(source: &Path, target: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(source.join("config.toml"));
    if !target.exists() {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        if std::fs::rename(source, target).is_ok() {
            return Ok(());
        }
    }
    copy_merge(source, target)?;
    // Whatever is left in `source` collided with the daemon's own files.
    remove_empty_dirs(source);
    Ok(())
}

fn copy_merge(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target).map_err(|e| format!("creating {}: {e}", target.display()))?;
    let entries =
        std::fs::read_dir(source).map_err(|e| format!("reading {}: {e}", source.display()))?;
    for entry in entries.flatten() {
        let from = entry.path();
        let to = target.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            copy_merge(&from, &to)?;
        } else if !to.exists() && std::fs::rename(&from, &to).is_err() {
            std::fs::copy(&from, &to).map_err(|e| format!("copying {}: {e}", from.display()))?;
            let _ = std::fs::remove_file(&from);
        }
    }
    Ok(())
}

fn remove_empty_dirs(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                remove_empty_dirs(&entry.path());
            }
        }
    }
    let _ = std::fs::remove_dir(dir);
}

fn restrict_tree(dir: &Path) {
    crate::chat_history::restrict_dir(dir);
    if let Some(account_root) = dir.parent() {
        crate::chat_history::restrict_dir(account_root);
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn legacy_scan<R: Runtime>(app: AppHandle<R>) -> Result<Vec<LegacyTree>, String> {
    Ok(scan(&AppPaths::resolve(&app)?))
}

/// Steps 4 and 5 for the tree at `dir`, once its machine was merged and its
/// engine runners re-added to the daemon.
#[tauri::command]
pub async fn legacy_finish<R: Runtime>(
    app: AppHandle<R>,
    dir: String,
    account: String,
    daemon_account: Option<String>,
) -> Result<FinishReport, String> {
    let paths = AppPaths::resolve(&app)?;
    let daemon = DaemonPaths::standard()?;
    tauri::async_runtime::spawn_blocking(move || {
        finish(
            &paths,
            &daemon,
            Path::new(&dir),
            &account,
            daemon_account.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("migration task failed: {e}"))?
}

/// `folder` comes from the webview: accept only a folder under the app's
/// legacy task folders (`managed/workdirs/`).
fn confine_legacy_folder(paths: &AppPaths, folder: &Path) -> Result<PathBuf, String> {
    let root = paths
        .legacy_workdirs()
        .canonicalize()
        .map_err(|e| format!("legacy task folders: {e}"))?;
    let dir = folder
        .canonicalize()
        .map_err(|e| format!("legacy task folder {}: {e}", folder.display()))?;
    if dir != root && dir.starts_with(&root) {
        Ok(dir)
    } else {
        Err(format!("{} is not a legacy task folder", folder.display()))
    }
}

/// Step 3 for a runner the daemon's reconcile re-added with a fresh folder
/// after an interrupted migration: give it its legacy task folder back
/// (design §13 step 3: a migrated runner keeps its folder).
#[tauri::command]
pub async fn legacy_restore_folder<R: Runtime>(
    app: AppHandle<R>,
    runner: String,
    working_dir: String,
) -> Result<(), String> {
    let paths = AppPaths::resolve(&app)?;
    match restore_target(&paths, Path::new(&working_dir))? {
        Some(folder) => crate::daemon::set_runner_working_dir(runner, folder).await,
        None => Ok(()),
    }
}

/// The folder to give back, or `None` when the legacy folder no longer
/// exists: the runner keeps the fresh folder the reconcile gave it, rather
/// than the migration failing on every launch.
fn restore_target(paths: &AppPaths, folder: &Path) -> Result<Option<PathBuf>, String> {
    if !folder.exists() {
        return Ok(None);
    }
    confine_legacy_folder(paths, folder).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_legacy_task_folder_can_be_restored() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(tmp.path().to_path_buf());
        let task = paths.legacy_workdirs().join("acme/WEB");
        std::fs::create_dir_all(&task).unwrap();
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        assert!(confine_legacy_folder(&paths, &task).is_ok());
        assert!(confine_legacy_folder(&paths, &paths.legacy_workdirs()).is_err());
        assert!(confine_legacy_folder(&paths, &elsewhere).is_err());
        assert!(confine_legacy_folder(&paths, &task.join("../../../elsewhere")).is_err());
        // A legacy folder deleted since: nothing to restore, not an error.
        let gone = paths.legacy_workdirs().join("acme/GONE");
        assert_eq!(restore_target(&paths, &gone), Ok(None));
        assert_eq!(
            restore_target(&paths, &task),
            Ok(Some(task.canonicalize().unwrap()))
        );
    }

    const MACHINE: &str = "11111111-2222-3333-4444-555555555555";

    fn legacy_config(workspace: &str, project: &str, workdir: &Path) -> String {
        format!(
            r#"[daemon]
cloud_url = "https://pidash.example.com"
dev_machine_id = "{MACHINE}"

[cli]
token = "mt_legacy"
workspace_slug = "{workspace}"

[[runner]]
name = "desktop-desktop-x-{project}"
runner_id = "00000000-0000-0000-0000-0000000000aa"
workspace_slug = "{workspace}"
project_slug = "{project}"

[runner.workspace]
working_dir = '{workdir}'

[runner.codex]
binary = "/old/bundle/pidash-agent-engine"
"#,
            workdir = workdir.display()
        )
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
        daemon: DaemonPaths,
        task: PathBuf,
        tree: PathBuf,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(tmp.path().join("app/managed"));
        let daemon = DaemonPaths {
            config_dir: tmp.path().join("xdg/config/pidash"),
            data_dir: tmp.path().join("xdg/data/pidash"),
            runtime_dir: tmp.path().join("xdg/run/pidash"),
        };
        let task = paths.legacy_workdirs().join("acme/WEB");
        std::fs::create_dir_all(&task).unwrap();
        std::fs::write(task.join("notes.md"), "work in progress").unwrap();
        let tree = paths.legacy_trees_dir.join("acme");
        std::fs::create_dir_all(tree.join("data/runners/aa")).unwrap();
        std::fs::write(
            tree.join("config.toml"),
            legacy_config("acme", "WEB", &task),
        )
        .unwrap();
        std::fs::write(tree.join("credentials.toml"), "secret").unwrap();
        std::fs::write(tree.join("data/runners/aa/credentials.toml"), "refresh").unwrap();
        std::fs::write(tree.join("data/runners/aa/identity.toml"), "id").unwrap();
        crate::app_paths::atomic_write(&paths.legacy_model_token, b"tok").unwrap();
        std::fs::write(paths.legacy_device_id(), "dev-id").unwrap();
        let home = paths.legacy_engine_home("user-1").unwrap();
        std::fs::create_dir_all(home.join("sessions/2026/10")).unwrap();
        std::fs::write(home.join("sessions/2026/10/rollout.jsonl"), "transcript").unwrap();
        std::fs::write(home.join("history.jsonl"), "history").unwrap();
        std::fs::write(home.join("config.toml"), "app-written").unwrap();
        Fixture {
            _tmp: tmp,
            paths,
            daemon,
            task,
            tree,
        }
    }

    #[test]
    fn scan_lists_each_tree_with_its_machine_and_engine_blocks() {
        let f = fixture();
        let trees = scan(&f.paths);
        assert_eq!(trees.len(), 1);
        let tree = &trees[0];
        assert_eq!(tree.workspace, "acme");
        assert_eq!(tree.dev_machine_id, Some(MACHINE.parse().unwrap()));
        assert_eq!(
            tree.cloud_url.as_deref(),
            Some("https://pidash.example.com")
        );
        assert_eq!(tree.engine_runners.len(), 1);
        assert_eq!(tree.engine_runners[0].project, "WEB");
        assert_eq!(tree.engine_runners[0].working_dir, f.task);
        // Nothing secret crosses into the webview.
        let json = serde_json::to_string(&trees).unwrap();
        assert!(!json.contains("mt_legacy"), "{json}");
    }

    #[test]
    fn scan_of_a_computer_without_legacy_trees_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(scan(&AppPaths::at(tmp.path().join("managed"))).is_empty());
    }

    #[test]
    fn finish_moves_the_engine_home_deletes_credentials_and_keeps_task_folders() {
        let f = fixture();
        let report = finish(&f.paths, &f.daemon, &f.tree, "user-1", Some("user-1")).unwrap();
        let target = f.daemon.data_dir.join("engine/user-1/codex-home");
        assert_eq!(
            report.engine_home_moved_to.as_deref(),
            Some(target.as_path())
        );
        assert_eq!(
            std::fs::read_to_string(target.join("sessions/2026/10/rollout.jsonl")).unwrap(),
            "transcript"
        );
        assert!(
            !target.join("config.toml").exists(),
            "the daemon writes the engine config"
        );
        assert!(!f.paths.legacy_engine_home("user-1").unwrap().exists());
        for gone in [
            f.tree.join("config.toml"),
            f.tree.join("credentials.toml"),
            f.tree.join("data/runners/aa/credentials.toml"),
            f.paths.legacy_model_token.clone(),
            f.paths.legacy_device_id(),
        ] {
            assert!(!gone.exists(), "{} survived", gone.display());
        }
        assert!(f.tree.join("data/runners/aa/identity.toml").exists());
        assert_eq!(
            std::fs::read_to_string(f.task.join("notes.md")).unwrap(),
            "work in progress"
        );
        // A finished tree is not listed again.
        assert!(scan(&f.paths).is_empty());
    }

    /// A daemon that recorded no account reads `engine/default/`: the
    /// history goes there, not under the app's user id.
    #[test]
    fn with_no_daemon_account_the_engine_home_goes_to_default() {
        let f = fixture();
        let report = finish(&f.paths, &f.daemon, &f.tree, "user-1", None).unwrap();
        assert_eq!(
            report.engine_home_moved_to.as_deref(),
            Some(f.daemon.engine_home(None).as_path())
        );
        assert!(
            f.daemon
                .engine_home(None)
                .ends_with("engine/default/codex-home")
        );
    }

    #[test]
    fn finish_is_idempotent() {
        let f = fixture();
        finish(&f.paths, &f.daemon, &f.tree, "user-1", Some("user-1")).unwrap();
        let again = finish(&f.paths, &f.daemon, &f.tree, "user-1", Some("user-1")).unwrap();
        assert_eq!(again, FinishReport::default());
        assert!(f.task.join("notes.md").exists());
        assert!(
            f.daemon
                .data_dir
                .join("engine/user-1/codex-home/history.jsonl")
                .exists()
        );
    }

    #[test]
    fn other_accounts_engine_homes_stay() {
        let f = fixture();
        let other = f.paths.legacy_engine_home("user-2").unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("history.jsonl"), "theirs").unwrap();
        finish(&f.paths, &f.daemon, &f.tree, "user-1", None).unwrap();
        assert_eq!(
            std::fs::read_to_string(other.join("history.jsonl")).unwrap(),
            "theirs"
        );
        assert!(!f.daemon.data_dir.join("engine/user-2").exists());
    }

    #[test]
    fn an_existing_daemon_engine_home_is_merged_without_overwriting() {
        let f = fixture();
        let target = f.daemon.engine_home(Some("user-1"));
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("history.jsonl"), "daemon's own").unwrap();
        std::fs::write(target.join("config.toml"), "daemon config").unwrap();
        finish(&f.paths, &f.daemon, &f.tree, "user-1", Some("user-1")).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("history.jsonl")).unwrap(),
            "daemon's own"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("config.toml")).unwrap(),
            "daemon config"
        );
        assert!(target.join("sessions/2026/10/rollout.jsonl").exists());
    }

    #[test]
    fn finish_refuses_a_directory_outside_the_legacy_root() {
        let f = fixture();
        let outside = f.paths.root.join("elsewhere");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("config.toml"), "keep").unwrap();
        assert!(finish(&f.paths, &f.daemon, &outside, "user-1", None).is_err());
        assert!(outside.join("config.toml").exists());
    }
}
