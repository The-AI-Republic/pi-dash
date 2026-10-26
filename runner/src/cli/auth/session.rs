// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Local sign-in state: who this host is signed in as, and whose runners
//! are configured.
//!
//! `[[runner]]` blocks belong to the cloud user that registered them. When
//! the host signs out, or signs in as somebody else, the current user's
//! runners are *parked* in `<config_dir>/parked/<user_id>.toml` instead of
//! being deleted or left to fail against the wrong account; signing in as
//! that user again restores them. Parked files are written 0600, like
//! `config.toml`.
//!
//! The running daemon notices the rewritten `config.toml` (token and runner
//! set changed) and reloads itself, which stops the parked runners and
//! starts the restored ones.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::config::file;
use crate::config::schema::{Config, DaemonConfig, RunnerConfig};
use crate::util::paths::Paths;

/// On-disk shape of `<config_dir>/parked/<user_id>.toml`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ParkedRunners {
    #[serde(default = "parked_version")]
    version: u32,
    /// Display only — so a human looking at the folder knows whose it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_email: Option<String>,
    #[serde(default, rename = "runner")]
    runners: Vec<RunnerConfig>,
}

fn parked_version() -> u32 {
    1
}

/// Path of a user's parked-runner file. `None` for an id that is not safe
/// to use as a file name (the server sends UUIDs; anything else is refused
/// rather than risking a path outside `parked/`).
pub fn parked_path(paths: &Paths, user_id: &str) -> Option<PathBuf> {
    let safe = !user_id.is_empty()
        && user_id.len() <= 128
        && user_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    safe.then(|| paths.parked_dir().join(format!("{user_id}.toml")))
}

fn read_parked(path: &std::path::Path) -> Result<ParkedRunners> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("parsing {path:?}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ParkedRunners::default()),
        Err(e) => Err(e).with_context(|| format!("reading {path:?}")),
    }
}

fn write_parked(path: &std::path::Path, parked: &ParkedRunners) -> Result<()> {
    let body = toml::to_string_pretty(parked).context("serializing parked runners")?;
    file::write_private(path, body.as_bytes())
}

fn same_runner(a: &RunnerConfig, b: &RunnerConfig) -> bool {
    a.runner_id == b.runner_id || a.name == b.name
}

/// Move every `[[runner]]` block out of `cfg` into `user_id`'s parked file,
/// merging with whatever is already parked there. Returns how many runners
/// were parked. Must be called under the config lock (inside
/// [`file::mutate_config`]) so the move is atomic with the config write.
fn park_runners(
    paths: &Paths,
    cfg: &mut Config,
    user_id: &str,
    user_email: Option<&str>,
) -> Result<usize> {
    if cfg.runners.is_empty() {
        return Ok(0);
    }
    let path = parked_path(paths, user_id)
        .ok_or_else(|| anyhow::anyhow!("refusing to park runners under user id {user_id:?}"))?;
    let mut parked = read_parked(&path)?;
    parked.version = parked_version();
    if user_email.is_some() {
        parked.user_email = user_email.map(str::to_string);
    }
    let moved = std::mem::take(&mut cfg.runners);
    let count = moved.len();
    for runner in moved {
        // The live block wins over a stale parked copy of the same runner.
        parked.runners.retain(|p| !same_runner(p, &runner));
        parked.runners.push(runner);
    }
    write_parked(&path, &parked)?;
    Ok(count)
}

/// Result of pulling a user's parked runners back into `cfg`.
#[derive(Debug, Default)]
struct Restore {
    restored: usize,
    /// The parked file to delete once the config write has landed (all of
    /// it was restored or was already present).
    delete: Option<PathBuf>,
}

/// Merge `user_id`'s parked runners into `cfg`, skipping any already
/// configured (same id or name) and any that would make the config invalid
/// (e.g. a working directory now used by another runner). Runners that could
/// not be restored stay parked.
fn restore_parked(paths: &Paths, cfg: &mut Config, user_id: &str) -> Result<Restore> {
    let Some(path) = parked_path(paths, user_id) else {
        return Ok(Restore::default());
    };
    if !path.exists() {
        return Ok(Restore::default());
    }
    let mut parked = read_parked(&path)?;
    let mut leftover = Vec::new();
    let mut restored = 0;
    for runner in std::mem::take(&mut parked.runners) {
        if cfg.runners.iter().any(|r| same_runner(r, &runner)) {
            continue;
        }
        cfg.runners.push(runner);
        if cfg.validate().is_ok() {
            restored += 1;
        } else {
            let runner = cfg.runners.pop().expect("just pushed");
            tracing::warn!(
                runner = %runner.name,
                "parked runner conflicts with the current config; leaving it parked"
            );
            leftover.push(runner);
        }
    }
    if leftover.is_empty() {
        Ok(Restore {
            restored,
            delete: Some(path),
        })
    } else {
        parked.runners = leftover;
        write_parked(&path, &parked)?;
        Ok(Restore {
            restored,
            delete: None,
        })
    }
}

/// Everything a successful `auth login` persists.
#[derive(Debug, Clone)]
pub struct SignIn {
    pub cloud_url: String,
    /// Rebind `[daemon].cloud_url` when it differs (`auth login --force`).
    pub force: bool,
    pub machine_token: String,
    pub workspace_slug: String,
    pub dev_machine_id: Uuid,
    pub user_id: Option<String>,
    pub user_email: Option<String>,
    pub handoff_key: Option<String>,
}

#[derive(Debug, Default)]
pub struct SignInOutcome {
    /// `Some(old_user_id)` when this sign-in replaced a different user.
    pub switched_from: Option<String>,
    pub parked: usize,
    pub restored: usize,
}

/// The stored identity a sign-in is compared against.
#[derive(Debug, Default, Clone)]
pub struct CurrentIdentity {
    pub cloud_url: Option<String>,
    pub token: Option<String>,
    pub user_id: Option<String>,
    pub user_email: Option<String>,
    pub runner_count: usize,
}

pub fn current_identity(paths: &Paths) -> Result<CurrentIdentity> {
    let Some(cfg) = file::load_config_opt(paths)? else {
        return Ok(CurrentIdentity::default());
    };
    let cli = cfg.cli.unwrap_or_default();
    Ok(CurrentIdentity {
        cloud_url: (!cfg.daemon.cloud_url.is_empty()).then_some(cfg.daemon.cloud_url),
        token: cli.token.filter(|t| !t.is_empty()),
        user_id: cli.user_id,
        user_email: cli.user_email,
        runner_count: cfg.runners.len(),
    })
}

/// Is signing in as `new_user_id` an account switch? Only when both the
/// stored and the new user are known and differ: a config written before
/// user ids were recorded (or a signed-out one) is not a switch.
pub fn is_account_switch(stored_user_id: Option<&str>, new_user_id: Option<&str>) -> bool {
    matches!((stored_user_id, new_user_id), (Some(a), Some(b)) if a != b)
}

/// Persist a sign-in. On an account switch the previous user's runners are
/// parked first; then the new user's parked runners (if any) are restored.
pub fn apply_sign_in(paths: &Paths, sign_in: &SignIn) -> Result<SignInOutcome> {
    let mut outcome = SignInOutcome::default();
    let mut delete_after = None;
    let init_url = sign_in.cloud_url.clone();
    file::mutate_config_or_init(
        paths,
        move || Some(bootstrap_config(&init_url)),
        |cfg| {
            if !cfg.daemon.cloud_url.is_empty() && cfg.daemon.cloud_url != sign_in.cloud_url {
                if !sign_in.force {
                    anyhow::bail!(
                        "this host is already enrolled with cloud {} — refusing to overwrite [cli] for a different cloud {} (pass --force to re-enroll against the new cloud)",
                        cfg.daemon.cloud_url,
                        sign_in.cloud_url
                    );
                }
                cfg.daemon.cloud_url = sign_in.cloud_url.clone();
            }
            if cfg.daemon.cloud_url.is_empty() {
                cfg.daemon.cloud_url = sign_in.cloud_url.clone();
            }
            let mut cli = cfg.cli.take().unwrap_or_default();
            if is_account_switch(cli.user_id.as_deref(), sign_in.user_id.as_deref()) {
                let old = cli.user_id.clone().expect("switch implies a stored user");
                outcome.parked = park_runners(paths, cfg, &old, cli.user_email.as_deref())?;
                outcome.switched_from = Some(old);
            }
            if let Some(user_id) = sign_in.user_id.as_deref() {
                let restore = restore_parked(paths, cfg, user_id)?;
                outcome.restored = restore.restored;
                delete_after = restore.delete;
            }
            cfg.daemon.dev_machine_id = Some(sign_in.dev_machine_id);
            cli.token = Some(sign_in.machine_token.clone());
            cli.workspace_slug = Some(sign_in.workspace_slug.clone());
            // A legacy exchange response without `user` keeps whatever was
            // recorded; a new one replaces it.
            if sign_in.user_id.is_some() {
                cli.user_id = sign_in.user_id.clone();
                cli.user_email = sign_in.user_email.clone();
            }
            cli.handoff_key = sign_in.handoff_key.clone();
            cli.signed_in_at = Some(Utc::now());
            cfg.cli = Some(cli);
            Ok(())
        },
    )?;
    if let Some(path) = delete_after
        && let Err(e) = std::fs::remove_file(&path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("restored runners but could not delete {path:?}: {e}");
    }
    Ok(outcome)
}

#[derive(Debug, Default)]
pub struct SignOutOutcome {
    pub parked: usize,
}

/// Persist a sign-out: park the signed-in user's runners, clear the
/// credentials and stamp `signed_out_at`. Runners of a config that never
/// recorded its user stay where they are (there is no key to park them
/// under). No-op without a config.
pub fn apply_sign_out(paths: &Paths) -> Result<SignOutOutcome> {
    if !paths.config_path().exists() {
        return Ok(SignOutOutcome::default());
    }
    let mut outcome = SignOutOutcome::default();
    file::mutate_config(paths, |cfg| {
        let mut cli = cfg.cli.take().unwrap_or_default();
        if let Some(user_id) = cli.user_id.clone() {
            outcome.parked = park_runners(paths, cfg, &user_id, cli.user_email.as_deref())?;
        }
        cli.token = None;
        cli.handoff_key = None;
        cli.user_id = None;
        cli.user_email = None;
        cli.signed_out_at = Some(Utc::now());
        cfg.cli = Some(cli);
        Ok(())
    })?;
    Ok(outcome)
}

fn bootstrap_config(cloud_url: &str) -> Config {
    Config {
        version: 2,
        daemon: DaemonConfig {
            cloud_url: cloud_url.to_string(),
            ..DaemonConfig::default()
        },
        runners: vec![],
        cli: None,
    }
}

/// What the local daemon is doing right now, as far as sign-in cares.
#[derive(Debug, Default, Clone, Copy)]
pub struct DaemonActivity {
    pub running: bool,
    /// Runners with a run in flight.
    pub active_runs: usize,
}

/// Ask the daemon (if one answers on this host's IPC socket) how many runs
/// are in flight. Never fails: no daemon means nothing is running.
pub async fn daemon_activity(paths: &Paths) -> DaemonActivity {
    let socket = paths.ipc_socket_path();
    let probe = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        crate::ipc::probe::status_at(&socket),
    )
    .await;
    match probe {
        Ok(Ok(snapshot)) => DaemonActivity {
            running: true,
            active_runs: snapshot
                .runners
                .iter()
                .filter(|r| r.current_run.is_some())
                .count(),
        },
        _ => DaemonActivity::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{CliSection, WorkspaceSection};
    use tempfile::tempdir;

    fn paths_for(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
        }
    }

    fn runner(name: &str, root: &std::path::Path) -> RunnerConfig {
        RunnerConfig {
            name: name.into(),
            runner_id: Uuid::new_v4(),
            workspace_slug: Some("acme".into()),
            project_slug: Some("PROJ".into()),
            pod_id: None,
            workspace: WorkspaceSection {
                working_dir: root.join(format!("wd-{name}")),
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

    fn signed_in_config(paths: &Paths, user: &str, runners: Vec<RunnerConfig>) {
        let cfg = Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://cloud.example".into(),
                dev_machine_id: Some(Uuid::new_v4()),
                ..DaemonConfig::default()
            },
            runners,
            cli: Some(CliSection {
                token: Some("mt_old".into()),
                workspace_slug: Some("acme".into()),
                user_id: Some(user.into()),
                user_email: Some(format!("{user}@example.com")),
                handoff_key: Some("dhk_old".into()),
                ..Default::default()
            }),
        };
        file::write_config(paths, &cfg).unwrap();
    }

    fn sign_in_as(user: &str) -> SignIn {
        SignIn {
            cloud_url: "https://cloud.example".into(),
            force: false,
            machine_token: format!("mt_{user}"),
            workspace_slug: "acme".into(),
            dev_machine_id: Uuid::new_v4(),
            user_id: Some(user.into()),
            user_email: Some(format!("{user}@example.com")),
            handoff_key: Some(format!("dhk_{user}")),
        }
    }

    #[test]
    fn same_user_relogin_keeps_runners() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let r = runner("alpha", tmp.path());
        signed_in_config(&paths, "u1", vec![r.clone()]);

        let out = apply_sign_in(&paths, &sign_in_as("u1")).unwrap();
        assert!(out.switched_from.is_none());
        assert_eq!(out.parked, 0);

        let cfg = file::load_config(&paths).unwrap();
        assert_eq!(cfg.runners.len(), 1);
        assert_eq!(cfg.runners[0].runner_id, r.runner_id);
        let cli = cfg.cli.unwrap();
        assert_eq!(cli.token.as_deref(), Some("mt_u1"));
        assert_eq!(cli.handoff_key.as_deref(), Some("dhk_u1"));
        assert!(cli.signed_in_at.is_some());
        assert!(!paths.parked_dir().join("u1.toml").exists());
    }

    #[test]
    fn account_switch_parks_old_runners_and_restores_them_on_switch_back() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let r = runner("alpha", tmp.path());
        signed_in_config(&paths, "u1", vec![r.clone()]);

        let out = apply_sign_in(&paths, &sign_in_as("u2")).unwrap();
        assert_eq!(out.switched_from.as_deref(), Some("u1"));
        assert_eq!(out.parked, 1);
        let cfg = file::load_config(&paths).unwrap();
        assert!(cfg.runners.is_empty());
        assert_eq!(cfg.cli.as_ref().unwrap().user_id.as_deref(), Some("u2"));
        let parked_file = paths.parked_dir().join("u1.toml");
        assert!(parked_file.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&parked_file)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }

        let out = apply_sign_in(&paths, &sign_in_as("u1")).unwrap();
        assert_eq!(out.switched_from.as_deref(), Some("u2"));
        assert_eq!(out.restored, 1);
        let cfg = file::load_config(&paths).unwrap();
        assert_eq!(cfg.runners.len(), 1);
        assert_eq!(cfg.runners[0].runner_id, r.runner_id);
        assert!(
            !parked_file.exists(),
            "fully restored parked file is removed"
        );
    }

    #[test]
    fn sign_out_parks_and_clears_credentials_then_sign_in_restores() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let r = runner("alpha", tmp.path());
        signed_in_config(&paths, "u1", vec![r.clone()]);

        let out = apply_sign_out(&paths).unwrap();
        assert_eq!(out.parked, 1);
        let cfg = file::load_config(&paths).unwrap();
        assert!(cfg.runners.is_empty());
        let cli = cfg.cli.unwrap();
        assert!(cli.token.is_none());
        assert!(cli.handoff_key.is_none());
        assert!(cli.user_id.is_none());
        assert!(cli.user_email.is_none());
        assert!(cli.signed_out_at.is_some());
        assert_eq!(cli.changed_at(), cli.signed_out_at);
        // The workspace binding survives a sign-out.
        assert_eq!(cli.workspace_slug.as_deref(), Some("acme"));

        let out = apply_sign_in(&paths, &sign_in_as("u1")).unwrap();
        assert!(out.switched_from.is_none());
        assert_eq!(out.restored, 1);
        let cfg = file::load_config(&paths).unwrap();
        assert_eq!(cfg.runners[0].runner_id, r.runner_id);
        let cli = cfg.cli.unwrap();
        assert_eq!(cli.changed_at(), cli.signed_in_at);
    }

    #[test]
    fn restore_skips_runners_already_configured() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let r = runner("alpha", tmp.path());
        signed_in_config(&paths, "u1", vec![r.clone()]);
        apply_sign_out(&paths).unwrap();
        // The same runner was re-added by hand while signed out.
        file::mutate_config(&paths, |cfg| {
            cfg.runners.push(r.clone());
            Ok(())
        })
        .unwrap();
        let out = apply_sign_in(&paths, &sign_in_as("u1")).unwrap();
        assert_eq!(out.restored, 0);
        assert_eq!(file::load_config(&paths).unwrap().runners.len(), 1);
    }

    #[test]
    fn sign_in_on_fresh_host_bootstraps_config() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let sign_in = sign_in_as("u1");
        apply_sign_in(&paths, &sign_in).unwrap();
        let cfg = file::load_config(&paths).unwrap();
        assert_eq!(cfg.daemon.cloud_url, "https://cloud.example");
        assert_eq!(cfg.daemon.dev_machine_id, Some(sign_in.dev_machine_id));
        assert_eq!(cfg.cli.unwrap().user_id.as_deref(), Some("u1"));
    }

    #[test]
    fn sign_in_refuses_other_cloud_without_force() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        signed_in_config(&paths, "u1", vec![]);
        let mut sign_in = sign_in_as("u1");
        sign_in.cloud_url = "https://other.example".into();
        assert!(apply_sign_in(&paths, &sign_in).is_err());
        sign_in.force = true;
        apply_sign_in(&paths, &sign_in).unwrap();
        assert_eq!(
            file::load_config(&paths).unwrap().daemon.cloud_url,
            "https://other.example"
        );
    }

    #[test]
    fn legacy_config_without_user_id_is_not_a_switch() {
        assert!(!is_account_switch(None, Some("u1")));
        assert!(!is_account_switch(Some("u1"), None));
        assert!(!is_account_switch(Some("u1"), Some("u1")));
        assert!(is_account_switch(Some("u1"), Some("u2")));
    }

    #[test]
    fn parked_path_rejects_unsafe_ids() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        assert!(parked_path(&paths, "../evil").is_none());
        assert!(parked_path(&paths, "").is_none());
        assert!(parked_path(&paths, "a/b").is_none());
        assert!(parked_path(&paths, "1e6a7a56-0000-0000-0000-000000000001").is_some());
    }
}
