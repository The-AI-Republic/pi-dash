//! Migration of pre-PDASHOSS01-134 pooled configs to the one-dir-per-runner
//! shape.
//!
//! PDASHOSS01-134 deleted the worktree pool: the `[[workdir]]` tables and the
//! per-runner `workdir = "..."` reference no longer exist in the schema. Serde
//! silently drops unknown keys, so an existing `config.toml` that still carries
//! them parses cleanly into a [`Config`] — but the pool that gave those keys
//! meaning is gone. Left alone, that is exactly the failure PDASHOSS01-135
//! exists to prevent: the daemon would boot and run an agent in whatever
//! `workspace.working_dir` happened to hold, which for a `runner add --workdir`
//! install is an empty placeholder directory, not the operator's canonical
//! clone.
//!
//! This module re-reads the raw TOML (the only place the removed keys still
//! survive), then reconciles the parsed [`Config`] against it:
//!
//! - **Case 1 — one runner referencing one `[[workdir]]`.** Point the runner
//!   directly at `workdir.path` (the canonical clone its runs already shared
//!   via leased worktrees) and report the change. Behaviour is equivalent.
//! - **Case 2 — N runners sharing one `[[workdir]]`.** Only one runner can keep
//!   the shared path. Per the operator decision on PDASHOSS01-135, the **first
//!   runner in config order wins** and keeps the canonical clone; every other
//!   sharer is given its own new working dir (the same
//!   `data_dir/workspaces/<proj>_<runner>_<id>` a fresh `runner add` would pick,
//!   via [`crate::cli::runner_ops::default_working_dir_in`]). That directory
//!   starts empty; the runner needs a checkout of the project to work in, so the
//!   cloud's per-run `repo_url` populates it with a `git clone` on first run
//!   (`workspace::resolve`) — nothing is cloned at upgrade time. Every
//!   reassignment is reported, so no agent moves silently.
//! - **Case 3 — legacy runners with no `[[workdir]]`.** Nothing here; the early
//!   return leaves them to 134's ordinary validation.
//!
//! Leftover on-disk pool directories (`data_dir/worktrees/<name>/` or the
//! `worktrees_dir` override) and per-runner chat worktrees
//! (`data_dir/runners/<id>/chat-worktree/`) may hold uncommitted agent work, so
//! they are never touched — only reported.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::schema::Config;

/// One migrated/reported line, surfaced to the operator via `tracing::warn!`
/// from [`crate::config::file::load_config`]. Not an error — the config loaded
/// and the daemon can start; the operator just needs to know what moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyMigration {
    pub report: Vec<String>,
}

/// The migration could not be applied automatically: the exact edit the
/// operator must make is in `message`. `Display` is that message verbatim, so
/// it prints cleanly through the `anyhow` chain the daemon surfaces at startup.
#[derive(Debug, Clone)]
pub struct LegacyMigrationError {
    pub message: String,
}

impl std::fmt::Display for LegacyMigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LegacyMigrationError {}

/// A `[[workdir]]` pool table as it appeared in a pre-134 config.
struct LegacyWorkdir {
    path: PathBuf,
    /// `worktrees_dir` override; `None` => `data_dir/worktrees/<name>`.
    worktrees_dir: Option<PathBuf>,
}

/// Migrate a parsed [`Config`] whose source `config.toml` may still carry the
/// removed `[[workdir]]` / `runner.workdir` keys.
///
/// `raw` is the same TOML text parsed as a generic [`toml::Value`] — the parsed
/// `Config` cannot be used for detection because serde already discarded the
/// removed keys. `data_dir` locates leftover pool directories to report and is
/// the root under which displaced sharers (case 2) get their new working dirs.
///
/// Returns `Ok(None)` when the config has no legacy pool keys (the common path
/// for anything written at or after 134 — no work, no allocation of a report).
/// Returns `Ok(Some(_))` when case 1 or case 2 was applied in place (the report
/// names every runner whose directory moved). Returns `Err` only when the config
/// is structurally broken — a `workdir` reference to a name no `[[workdir]]`
/// defines.
pub fn migrate_legacy_pool(
    cfg: &mut Config,
    raw: &toml::Value,
    data_dir: &Path,
) -> Result<Option<LegacyMigration>, LegacyMigrationError> {
    let workdirs = collect_workdirs(raw);
    let refs = collect_runner_refs(raw);

    // Fast path: a config written at/after 134 has neither key. Do nothing so
    // ordinary loads pay only two cheap `Value::get` lookups.
    if workdirs.is_empty() && refs.is_empty() {
        return Ok(None);
    }

    // Group the runners that reference each pool so we can tell case 1 (one
    // runner) from case 2 (several) — order-stable for deterministic messages.
    let mut by_workdir: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (runner, workdir) in &refs {
        by_workdir.entry(workdir.as_str()).or_default().push(runner.as_str());
    }

    // A `workdir = "x"` that names no `[[workdir]] name = "x"` is a broken
    // config the pool loader would also have rejected. Name it rather than
    // silently dropping the reference and running in the placeholder dir.
    let mut dangling: Vec<String> = Vec::new();
    for (workdir, runners) in &by_workdir {
        if !workdirs.contains_key(*workdir) {
            dangling.push(format!(
                "runner(s) {} reference workdir {:?}, which no [[workdir]] defines",
                quote_join(runners),
                workdir
            ));
        }
    }
    if !dangling.is_empty() {
        return Err(LegacyMigrationError {
            message: format!(
                "configuration error: config.toml has dangling workdir references \
                 left from the removed worktree pool ({}). The pool was deleted \
                 (PDASHOSS01-134); set each affected runner's [runner.workspace] \
                 working_dir to the directory it should run in and delete the \
                 `workdir = ...` line.",
                dangling.join("; ")
            ),
        });
    }

    // Repoint each referencing runner, walking `refs` in config order. The
    // first runner to reference a given pool wins it (case 1 is just the N=1 of
    // this) and keeps the canonical clone; a second-or-later sharer (case 2)
    // can't have the same path — the operator chose "first runner wins", so it
    // is displaced to its own fresh dir. Every move is reported, so no agent
    // silently changes directory.
    let mut report: Vec<String> = Vec::new();
    let mut claimed: BTreeSet<&str> = BTreeSet::new();
    for (runner_name, workdir_name) in &refs {
        let info = workdirs.get(workdir_name.as_str()).expect(
            "dangling references were rejected above, so every ref resolves here",
        );
        // Snapshot what we need from the pool table before the mutable borrow
        // of `cfg.runners` below.
        let canonical = info.path.clone();
        // `insert` returns true the first time we see this pool — that runner
        // wins and keeps the canonical clone; later sharers are displaced.
        let is_winner = claimed.insert(workdir_name.as_str());

        let Some(runner) = cfg.runners.iter_mut().find(|r| &r.name == runner_name) else {
            // The raw `[[runner]]` had a name the parsed Config doesn't — the
            // file is internally inconsistent. Report it but don't fail the
            // load over a runner that won't be dispatched anyway.
            report.push(format!(
                "runner {runner_name:?} referenced pool {workdir_name:?} but is \
                 missing from the parsed config; skipped."
            ));
            continue;
        };
        let previous = runner.workspace.working_dir.clone();

        if is_winner {
            // Keeps the shared canonical clone: it runs where its worktrees
            // used to be sourced from. Behaviour is equivalent for this runner.
            runner.workspace.working_dir = canonical.clone();
            if previous == canonical {
                report.push(format!(
                    "runner {runner_name:?}: keeps working_dir {:?} — formerly the \
                     canonical clone of removed pool {workdir_name:?}. Runs now \
                     execute here directly instead of in a leased worktree.",
                    canonical.display()
                ));
            } else {
                report.push(format!(
                    "runner {runner_name:?}: now runs directly in {:?} (the canonical \
                     clone of removed pool {workdir_name:?}); its previous \
                     working_dir {:?} was a pool placeholder and is no longer used.",
                    canonical.display(),
                    previous.display()
                ));
            }
        } else {
            // A second-or-later sharer of the same pool. It cannot keep the
            // canonical clone (one dir per runner now), so give it the same
            // fresh dir a `runner add` with no --working-dir would pick. It
            // starts empty; the cloud clones the repo into it on first run.
            let project_slug = runner.project_slug.as_deref().unwrap_or("project");
            let assigned = crate::cli::runner_ops::default_working_dir_in(
                data_dir,
                project_slug,
                runner_name,
                runner.runner_id,
            );
            runner.workspace.working_dir = assigned.clone();
            report.push(format!(
                "runner {runner_name:?}: shared removed pool {workdir_name:?} with \
                 another runner, which keeps the canonical clone {:?}. This runner \
                 now has its own working_dir {:?} (created empty; the repo is cloned \
                 into it on first run). Its previous working_dir {:?} is no longer \
                 used.",
                canonical.display(),
                assigned.display(),
                previous.display()
            ));
        }
    }

    report_leftover_dirs(cfg, &workdirs, &by_workdir, data_dir, &mut report);

    report.push(
        "The [[workdir]] tables and `workdir = ...` runner keys are now ignored; \
         you can delete them from config.toml (they are rewritten out on the next \
         `pidash runner add`/`remove`)."
            .to_string(),
    );

    Ok(Some(LegacyMigration { report }))
}

/// Append never-delete reports for on-disk pool + chat worktree directories.
fn report_leftover_dirs(
    cfg: &Config,
    workdirs: &BTreeMap<String, LegacyWorkdir>,
    by_workdir: &BTreeMap<&str, Vec<&str>>,
    data_dir: &Path,
    report: &mut Vec<String>,
) {
    for (name, info) in workdirs {
        let pool_dir = info
            .worktrees_dir
            .clone()
            .unwrap_or_else(|| data_dir.join("worktrees").join(name));
        if pool_dir.exists() {
            report.push(format!(
                "leftover pool worktrees at {:?} were left in place — they may hold \
                 uncommitted agent work. Pi Dash no longer uses them; move or delete \
                 them yourself once you've saved anything you need.",
                pool_dir.display()
            ));
        }
        if !by_workdir.contains_key(name.as_str()) {
            report.push(format!(
                "pool {name:?} (canonical clone {:?}) was defined but referenced by \
                 no runner; it is now ignored.",
                info.path.display()
            ));
        }
    }

    for r in &cfg.runners {
        let chat = data_dir
            .join("runners")
            .join(r.runner_id.to_string())
            .join("chat-worktree");
        if chat.exists() {
            report.push(format!(
                "leftover chat worktree for runner {:?} at {:?} was left in place — it \
                 may hold uncommitted work. Pi Dash no longer uses it; move or delete \
                 it yourself once you've saved anything you need.",
                r.name,
                chat.display()
            ));
        }
    }
}

/// Collect `[[workdir]]` tables from raw TOML into name -> info.
fn collect_workdirs(raw: &toml::Value) -> BTreeMap<String, LegacyWorkdir> {
    let mut out = BTreeMap::new();
    let Some(array) = raw.get("workdir").and_then(toml::Value::as_array) else {
        return out;
    };
    for entry in array {
        let Some(name) = entry.get("name").and_then(toml::Value::as_str) else {
            continue;
        };
        let Some(path) = entry.get("path").and_then(toml::Value::as_str) else {
            continue;
        };
        let worktrees_dir = entry
            .get("worktrees_dir")
            .and_then(toml::Value::as_str)
            .map(PathBuf::from);
        out.insert(
            name.to_string(),
            LegacyWorkdir {
                path: PathBuf::from(path),
                worktrees_dir,
            },
        );
    }
    out
}

/// Collect `(runner_name, workdir_name)` for every `[[runner]]` that carries a
/// `workdir = "..."` reference, preserving config order.
fn collect_runner_refs(raw: &toml::Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(array) = raw.get("runner").and_then(toml::Value::as_array) else {
        return out;
    };
    for entry in array {
        let Some(workdir) = entry.get("workdir").and_then(toml::Value::as_str) else {
            continue;
        };
        // A runner with no name is already rejected by the strict `Config`
        // parse; fall back to a placeholder only so detection doesn't panic.
        let name = entry
            .get("name")
            .and_then(toml::Value::as_str)
            .unwrap_or("<unnamed>");
        out.push((name.to_string(), workdir.to_string()));
    }
    out
}

/// `["a", "b"]` -> `"a", "b"` for operator-facing messages.
fn quote_join(items: &[&str]) -> String {
    items
        .iter()
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{
        Config, DaemonConfig, RunnerConfig, WorkspaceSection,
    };
    use std::path::PathBuf;
    use uuid::Uuid;

    fn runner(name: &str, working_dir: &str) -> RunnerConfig {
        RunnerConfig {
            name: name.into(),
            runner_id: Uuid::new_v4(),
            workspace_slug: Some("acme".into()),
            project_slug: Some("TEST".into()),
            pod_id: None,
            workspace: WorkspaceSection {
                working_dir: PathBuf::from(working_dir),
            },
            agent: Default::default(),
            codex: Default::default(),
            claude_code: Default::default(),
            cursor_agent: Default::default(),
            openclaw: Default::default(),
            grok: Default::default(),
            muse_code: Default::default(),
            approval_policy: Default::default(),
        }
    }

    fn config_with(runners: Vec<RunnerConfig>) -> Config {
        Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://x".into(),
                dev_machine_id: None,
                log_level: "info".into(),
                log_retention_days: 14,
                agent_observability_v1: false,
                auto_update: true,
            },
            runners,
            cli: None,
        }
    }

    fn raw(toml_text: &str) -> toml::Value {
        toml::from_str(toml_text).expect("valid toml fixture")
    }

    #[test]
    fn no_legacy_keys_is_a_noop() {
        // A config written at/after 134 has neither `[[workdir]]` nor a
        // per-runner `workdir` key: migration must not touch it.
        let mut cfg = config_with(vec![runner("main", "/work/main")]);
        let value = raw(r#"
            version = 2
            [[runner]]
            name = "main"
            [runner.workspace]
            working_dir = "/work/main"
        "#);
        let out = migrate_legacy_pool(&mut cfg, &value, Path::new("/data")).unwrap();
        assert!(out.is_none(), "expected no migration, got {out:?}");
        assert_eq!(cfg.runners[0].workspace.working_dir, PathBuf::from("/work/main"));
    }

    #[test]
    fn case1_repoints_single_runner_to_canonical_clone() {
        // The common case: one runner bound to one pool. Its working_dir was a
        // placeholder; after migration it runs in the pool's canonical clone.
        let mut cfg = config_with(vec![runner("codex", "/data/runners/x/workspace")]);
        let value = raw(r#"
            version = 2
            [[workdir]]
            name = "repo"
            path = "/home/me/repo"
            pool_size = 2
            [[runner]]
            name = "codex"
            workdir = "repo"
            [runner.workspace]
            working_dir = "/data/runners/x/workspace"
        "#);
        let migration = migrate_legacy_pool(&mut cfg, &value, Path::new("/data"))
            .unwrap()
            .expect("case 1 should migrate");
        assert_eq!(
            cfg.runners[0].workspace.working_dir,
            PathBuf::from("/home/me/repo")
        );
        assert!(
            migration.report.iter().any(|l| l.contains("now runs directly in")
                && l.contains("/home/me/repo")),
            "report should announce the move: {:?}",
            migration.report
        );
    }

    #[test]
    fn case1_keeps_working_dir_when_already_the_clone() {
        // auto-pool shape: the runner's working_dir already equals the pool
        // path. Migration is a no-move but still reports the behaviour change
        // (worktree -> direct) so the operator is told (AC2).
        let mut cfg = config_with(vec![runner("codex", "/home/me/repo")]);
        let value = raw(r#"
            version = 2
            [[workdir]]
            name = "repo"
            path = "/home/me/repo"
            [[runner]]
            name = "codex"
            workdir = "repo"
            [runner.workspace]
            working_dir = "/home/me/repo"
        "#);
        let migration = migrate_legacy_pool(&mut cfg, &value, Path::new("/data"))
            .unwrap()
            .expect("case 1 should migrate");
        assert_eq!(
            cfg.runners[0].workspace.working_dir,
            PathBuf::from("/home/me/repo")
        );
        assert!(
            migration.report.iter().any(|l| l.contains("keeps working_dir")),
            "report: {:?}",
            migration.report
        );
    }

    #[test]
    fn case2_shared_pool_first_runner_wins_others_get_new_dirs() {
        // N runners sharing one pool: the first in config order keeps the
        // canonical clone; every other sharer is displaced to its own fresh
        // dir under data_dir/workspaces, and every move is reported.
        let mut cfg = config_with(vec![
            runner("codex", "/home/me/repo"),
            runner("claude", "/home/me/repo"),
        ]);
        let value = raw(r#"
            version = 2
            [[workdir]]
            name = "repo"
            path = "/home/me/repo"
            [[runner]]
            name = "codex"
            workdir = "repo"
            [runner.workspace]
            working_dir = "/home/me/repo"
            [[runner]]
            name = "claude"
            workdir = "repo"
            [runner.workspace]
            working_dir = "/home/me/repo"
        "#);
        let migration = migrate_legacy_pool(&mut cfg, &value, Path::new("/data"))
            .unwrap()
            .expect("case 2 migrates in place, first runner wins");

        // Winner (first in config order) keeps the canonical clone.
        assert_eq!(
            cfg.runners[0].workspace.working_dir,
            PathBuf::from("/home/me/repo"),
            "first runner should keep the canonical clone",
        );
        // Loser gets a distinct, non-shared dir under data_dir/workspaces.
        let loser = &cfg.runners[1].workspace.working_dir;
        assert_ne!(
            loser,
            &PathBuf::from("/home/me/repo"),
            "second sharer must not keep the shared path",
        );
        assert!(
            loser.starts_with("/data/workspaces"),
            "loser should be reassigned under the data dir: {loser:?}",
        );
        assert_ne!(
            cfg.runners[0].workspace.working_dir, cfg.runners[1].workspace.working_dir,
            "the two runners must no longer resolve to the same dir",
        );
        // The move is announced for both runners, not silent.
        assert!(
            migration.report.iter().any(|l| l.contains("\"claude\"")
                && l.contains("has its own working_dir")),
            "report should announce the displaced runner: {:?}",
            migration.report
        );
    }

    #[test]
    fn dangling_workdir_reference_fails() {
        // A `workdir = "gone"` with no matching `[[workdir]]` is a broken
        // config; name it rather than silently running in the placeholder.
        let mut cfg = config_with(vec![runner("codex", "/placeholder")]);
        let value = raw(r#"
            version = 2
            [[runner]]
            name = "codex"
            workdir = "gone"
            [runner.workspace]
            working_dir = "/placeholder"
        "#);
        let err = migrate_legacy_pool(&mut cfg, &value, Path::new("/data")).unwrap_err();
        assert!(err.to_string().contains("dangling"), "{err}");
        assert!(err.to_string().contains("\"gone\""), "{err}");
    }

    #[test]
    fn orphan_workdir_without_runner_ref_is_reported_not_failed() {
        // A `[[workdir]]` no runner references is harmless — report it as
        // ignored, don't fail. The runner has no `workdir` key (legacy dir).
        let mut cfg = config_with(vec![runner("codex", "/work/codex")]);
        let value = raw(r#"
            version = 2
            [[workdir]]
            name = "unused"
            path = "/home/me/other"
            [[runner]]
            name = "codex"
            [runner.workspace]
            working_dir = "/work/codex"
        "#);
        let migration = migrate_legacy_pool(&mut cfg, &value, Path::new("/data"))
            .unwrap()
            .expect("orphan workdir still yields a report");
        assert_eq!(cfg.runners[0].workspace.working_dir, PathBuf::from("/work/codex"));
        assert!(
            migration.report.iter().any(|l| l.contains("no runner")),
            "report: {:?}",
            migration.report
        );
    }

    #[test]
    fn leftover_pool_and_chat_dirs_are_reported_never_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path();
        // Pool worktrees at the default location.
        let pool_dir = data_dir.join("worktrees").join("repo");
        std::fs::create_dir_all(pool_dir.join("wt-1")).unwrap();
        std::fs::write(pool_dir.join("wt-1").join("uncommitted.txt"), b"work").unwrap();

        let mut cfg = config_with(vec![runner("codex", "/data/runners/x/workspace")]);
        // Chat worktree under the runner's data dir.
        let chat = data_dir
            .join("runners")
            .join(cfg.runners[0].runner_id.to_string())
            .join("chat-worktree");
        std::fs::create_dir_all(&chat).unwrap();

        let value = raw(r#"
            version = 2
            [[workdir]]
            name = "repo"
            path = "/home/me/repo"
            [[runner]]
            name = "codex"
            workdir = "repo"
            [runner.workspace]
            working_dir = "/data/runners/x/workspace"
        "#);
        let migration = migrate_legacy_pool(&mut cfg, &value, data_dir)
            .unwrap()
            .expect("case 1");

        assert!(
            migration.report.iter().any(|l| l.contains("leftover pool worktrees")
                && l.contains("worktrees")),
            "report: {:?}",
            migration.report
        );
        assert!(
            migration.report.iter().any(|l| l.contains("leftover chat worktree")),
            "report: {:?}",
            migration.report
        );
        // The migration must never delete on-disk work.
        assert!(pool_dir.join("wt-1").join("uncommitted.txt").exists());
        assert!(chat.exists());
    }

    #[test]
    fn worktrees_dir_override_is_reported_at_its_real_location() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path();
        let override_dir = data_dir.join("elsewhere").join("wts");
        std::fs::create_dir_all(&override_dir).unwrap();

        let mut cfg = config_with(vec![runner("codex", "/data/runners/x/workspace")]);
        let value = raw(&format!(
            r#"
            version = 2
            [[workdir]]
            name = "repo"
            path = "/home/me/repo"
            worktrees_dir = "{}"
            [[runner]]
            name = "codex"
            workdir = "repo"
            [runner.workspace]
            working_dir = "/data/runners/x/workspace"
        "#,
            override_dir.display()
        ));
        let migration = migrate_legacy_pool(&mut cfg, &value, data_dir)
            .unwrap()
            .expect("case 1");
        assert!(
            migration
                .report
                .iter()
                .any(|l| l.contains(&override_dir.display().to_string())),
            "report should name the override location: {:?}",
            migration.report
        );
    }
}
