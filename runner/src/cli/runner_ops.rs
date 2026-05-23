//! Shared local-side plumbing for both `pidash connect` (legacy
//! enrollment-token flow) and `pidash runner add` (CLI-token flow).
//!
//! After a successful cloud-side mint (`EnrollResponse`), both paths
//! need to do the same local work: write `[[runner]]` to `config.toml`,
//! drop per-runner `credentials.toml`, and install the OS service unit
//! on first add. That shared work lives here so the two paths can't
//! drift.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::cloud::http::{EnrollResponse, RunnerCredentials, write_runner_credentials};
use crate::config::file;
use crate::config::schema::{
    AgentKind, AgentSection, ApprovalPolicySection, ClaudeCodeSection, CliSection, CodexSection,
    Config, DaemonConfig, RunnerConfig, WorkspaceSection,
};
use crate::util::paths::Paths;

/// Read the user's CLI token from `[cli].token` in `config.toml`.
/// Returns `Ok(None)` when no config exists yet or the section is
/// absent; the caller is expected to surface a friendly "run `pidash
/// auth login` first" message in that case.
pub fn load_cli_token(paths: &Paths) -> Result<Option<String>> {
    if !paths.config_path().exists() {
        return Ok(None);
    }
    let cfg = file::load_config(paths)?;
    Ok(cfg.cli.and_then(|c| c.token))
}

/// Write `[cli].token` into `config.toml`, creating the file if it
/// doesn't exist yet. Used by `pidash auth login` after the device-code
/// exchange returns an `APIToken`.
///
/// First-run bootstrap: when no config exists, we seed a minimal
/// `[daemon]` block with the cloud URL the login was performed against.
/// No `[[runner]]` blocks are added — the runner is registered
/// separately via `pidash runner add`.
///
/// Preserves any pre-existing `[cli].workspace_slug` so a re-login
/// (e.g. token refresh) doesn't wipe the host's workspace binding.
pub fn write_cli_token(paths: &Paths, cloud_url: &str, token: &str) -> Result<()> {
    let mut cfg = if paths.config_path().exists() {
        file::load_config(paths)?
    } else {
        Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: cloud_url.to_string(),
                log_level: "info".to_string(),
                log_retention_days: 14,
                agent_observability_v1: false,
                auto_update: true,
            },
            runners: vec![],
            cli: None,
        }
    };
    // Pre-existing config? Don't quietly rebind it to a different cloud.
    if !cfg.daemon.cloud_url.is_empty() && cfg.daemon.cloud_url != cloud_url {
        anyhow::bail!(
            "this host is already enrolled with cloud {} — refusing to overwrite [cli] for a different cloud {}",
            cfg.daemon.cloud_url,
            cloud_url
        );
    }
    if cfg.daemon.cloud_url.is_empty() {
        cfg.daemon.cloud_url = cloud_url.to_string();
    }
    let preserved_workspace = cfg.cli.as_ref().and_then(|c| c.workspace_slug.clone());
    let preserved_default_project = cfg.cli.as_ref().and_then(|c| c.default_project.clone());
    cfg.cli = Some(CliSection {
        token: Some(token.to_string()),
        workspace_slug: preserved_workspace,
        default_project: preserved_default_project,
    });
    file::write_config(paths, &cfg)?;
    Ok(())
}

/// Read `[cli].workspace_slug` from `config.toml`.
///
/// Returns `Ok(None)` if no config exists, the `[cli]` section is
/// absent, or `workspace_slug` was never set. `pidash runner add` uses
/// this as the default workspace when the caller omits `--workspace`.
pub fn load_cli_workspace(paths: &Paths) -> Result<Option<String>> {
    if !paths.config_path().exists() {
        return Ok(None);
    }
    let cfg = file::load_config(paths)?;
    Ok(cfg.cli.and_then(|c| c.workspace_slug))
}

/// Write `[cli].workspace_slug` into `config.toml`. Called by
/// `pidash auth login` after the workspace picker resolves which
/// workspace this host is bound to. Requires a pre-existing config
/// (the token write always happens first).
///
/// Rebinding to a different workspace is allowed — the login flow
/// itself decides whether to keep the existing binding (slug still in
/// the user's memberships) or pick a new one. Callers that need a
/// safety check should compare before calling.
pub fn write_cli_workspace(paths: &Paths, workspace_slug: &str) -> Result<()> {
    if !paths.config_path().exists() {
        anyhow::bail!(
            "no config.toml — `pidash auth login` must write the CLI token before the workspace binding"
        );
    }
    let mut cfg = file::load_config(paths)?;
    let mut cli = cfg.cli.unwrap_or_default();
    cli.workspace_slug = Some(workspace_slug.to_string());
    cfg.cli = Some(cli);
    file::write_config(paths, &cfg)?;
    Ok(())
}

pub fn load_cli_default_project(paths: &Paths) -> Result<Option<String>> {
    if !paths.config_path().exists() {
        return Ok(None);
    }
    let cfg = file::load_config(paths)?;
    Ok(cfg.cli.and_then(|c| c.default_project))
}

pub fn write_cli_default_project(paths: &Paths, project: &str) -> Result<()> {
    if !paths.config_path().exists() {
        anyhow::bail!("no config.toml — run `pidash auth login` before setting a default project");
    }
    let mut cfg = file::load_config(paths)?;
    let mut cli = cfg.cli.unwrap_or_default();
    cli.default_project = Some(project.to_string());
    cfg.cli = Some(cli);
    file::write_config(paths, &cfg)?;
    Ok(())
}

/// Clear `[cli].token` from `config.toml`. Used by `pidash auth logout`.
/// Leaves `[[runner]]` blocks untouched so the daemon keeps running
/// under its own identity. No-op if no config exists.
pub fn clear_cli_token(paths: &Paths) -> Result<()> {
    if !paths.config_path().exists() {
        return Ok(());
    }
    let mut cfg = file::load_config(paths)?;
    if let Some(ref mut cli) = cfg.cli {
        cli.token = None;
    }
    file::write_config(paths, &cfg)?;
    Ok(())
}

/// Result of applying a mint locally: the new `RunnerConfig` block and
/// whether this was the host's first runner (so callers can decide
/// whether to install the OS service unit / restart it).
#[derive(Debug)]
pub struct AppliedRunner {
    pub runner: RunnerConfig,
    pub is_first_runner: bool,
}

/// Apply an `EnrollResponse` (from either the legacy enroll endpoint or
/// the new CLI-initiated create endpoint) to local disk:
///
/// 1. Write the per-runner refresh credential.
/// 2. Append a new `[[runner]]` block to `config.toml`.
///
/// The caller is responsible for:
/// - Validating the cloud URL up front.
/// - Restarting the service afterwards (subsequent runners) or installing
///   it (first runner) — see `is_first_runner` on the return.
pub async fn apply_enroll_response(
    paths: &Paths,
    resp: &EnrollResponse,
    cloud_url: &str,
    working_dir: Option<PathBuf>,
    agent_kind: AgentKind,
) -> Result<AppliedRunner> {
    let working_dir =
        working_dir.unwrap_or_else(|| paths.runner_dir(resp.runner_id).join("workspace"));

    let new_runner = RunnerConfig {
        name: resp.runner_name.clone(),
        runner_id: resp.runner_id,
        workspace_slug: Some(resp.workspace_slug.clone()),
        project_slug: Some(resp.project_identifier.clone()),
        pod_id: None,
        workspace: WorkspaceSection { working_dir },
        agent: AgentSection { kind: agent_kind },
        codex: CodexSection::default(),
        claude_code: ClaudeCodeSection::default(),
        approval_policy: ApprovalPolicySection::default(),
    };

    // Bootstrap closure for the no-config-yet case (first runner on a
    // fresh host that ran `pidash auth login` but not `pidash connect`).
    // Seeds a minimal `[daemon]` block from the response's cloud URL.
    let bootstrap_cloud_url = cloud_url.to_string();
    let init_cfg = move || {
        Some(Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: bootstrap_cloud_url.clone(),
                log_level: "info".to_string(),
                log_retention_days: 14,
                agent_observability_v1: false,
                auto_update: true,
            },
            runners: vec![],
            cli: None,
        })
    };

    // Persist the `[[runner]]` block under the host-wide `.config.lock`
    // so a concurrent `pidash runner add` / daemon `RemoveRunner` strip
    // can't last-writer-wins this write. The closure also performs the
    // cloud-URL mismatch check inside the lock — doing it before
    // `mutate_config_or_init` would race with a concurrent `auth login
    // --url <different>` that may be rewriting `[cli]` at the same time.
    let new_runner_for_closure = new_runner.clone();
    let mut is_first_runner = false;
    file::mutate_config_or_init(paths, init_cfg, |cfg| {
        if !cfg.daemon.cloud_url.is_empty() && cfg.daemon.cloud_url != cloud_url {
            anyhow::bail!(
                "this host is enrolled with cloud {} — refusing to add a runner pointing at {}",
                cfg.daemon.cloud_url,
                cloud_url
            );
        }
        if cfg.daemon.cloud_url.is_empty() {
            cfg.daemon.cloud_url = cloud_url.to_string();
        }
        is_first_runner = cfg.runners.is_empty();
        cfg.runners.push(new_runner_for_closure);
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("persisting [[runner]] block under config lock: {e}"))?;

    // Credentials write happens AFTER config so a config-write failure
    // (cloud-URL mismatch, validation error, IO) doesn't leave an orphan
    // per-runner credentials file pointing at a runner the supervisor
    // never sees. The flipside — config block exists but credentials
    // don't — is recoverable: the supervisor logs the missing-file
    // error and `pidash runner remove <name>` can clean up by name.
    // (The caller's rollback umbrella in cli/runner.rs also strips the
    // partial `[[runner]]` block on credentials-write failure, see the
    // `Err(persist_err)` arm there.)
    let runner_paths = paths.for_runner(resp.runner_id);
    runner_paths.ensure()?;
    write_runner_credentials(
        runner_paths.credentials_path(),
        RunnerCredentials {
            runner_id: resp.runner_id,
            name: resp.runner_name.clone(),
            refresh_token: resp.refresh_token.clone(),
            refresh_token_generation: resp.refresh_token_generation,
        },
    )
    .await
    .context("writing per-runner credentials failed")?;

    Ok(AppliedRunner {
        runner: new_runner,
        is_first_runner,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::http::EnrollResponse;
    use tempfile::tempdir;
    use uuid::Uuid;

    fn paths_for(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
        }
    }

    fn sample_response(runner_name: &str) -> EnrollResponse {
        EnrollResponse {
            runner_id: Uuid::new_v4(),
            runner_name: runner_name.into(),
            refresh_token: "refresh".into(),
            access_token: "access".into(),
            access_token_expires_at: "2099-01-01T00:00:00+00:00".into(),
            refresh_token_generation: 1,
            workspace_slug: "acme".into(),
            pod_slug: "default".into(),
            project_identifier: "WEB".into(),
            long_poll_interval_secs: 25,
            protocol_version: 4,
            machine_token: None,
            machine_token_minted: false,
        }
    }

    #[tokio::test]
    async fn first_runner_bootstraps_config_when_none_exists() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let resp = sample_response("r1");
        let applied = apply_enroll_response(
            &paths,
            &resp,
            "https://example.com",
            Some(tmp.path().join("wd")),
            AgentKind::Codex,
        )
        .await
        .unwrap();
        assert!(applied.is_first_runner);
        let cfg = file::load_config(&paths).unwrap();
        assert_eq!(cfg.daemon.cloud_url, "https://example.com");
        assert_eq!(cfg.runners.len(), 1);
        assert_eq!(cfg.runners[0].name, "r1");
    }

    #[tokio::test]
    async fn second_runner_appends_and_flags_not_first() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let r1 = sample_response("r1");
        apply_enroll_response(
            &paths,
            &r1,
            "https://example.com",
            Some(tmp.path().join("wd1")),
            AgentKind::Codex,
        )
        .await
        .unwrap();

        let r2 = sample_response("r2");
        let applied = apply_enroll_response(
            &paths,
            &r2,
            "https://example.com",
            Some(tmp.path().join("wd2")),
            AgentKind::Codex,
        )
        .await
        .unwrap();
        assert!(!applied.is_first_runner);
        let cfg = file::load_config(&paths).unwrap();
        assert_eq!(cfg.runners.len(), 2);
    }

    #[tokio::test]
    async fn cloud_url_mismatch_does_not_leave_orphan_credentials() {
        // The whole point of the URL-check-before-write reorder. If the
        // user is enrolled with cloud A and a misuse points us at cloud
        // B, we MUST refuse before writing per-runner credentials —
        // otherwise an orphan file is left for the operator to clean up.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let r1 = sample_response("r1");
        apply_enroll_response(
            &paths,
            &r1,
            "https://cloud-a.example.com",
            Some(tmp.path().join("wd1")),
            AgentKind::Codex,
        )
        .await
        .unwrap();

        let r2 = sample_response("r2");
        let err = apply_enroll_response(
            &paths,
            &r2,
            "https://cloud-b.example.com",
            Some(tmp.path().join("wd2")),
            AgentKind::Codex,
        )
        .await
        .unwrap_err();
        assert!(format!("{err}").contains("refusing to add a runner pointing at"));
        // No credentials file should exist for r2 — the bail happened
        // before the write.
        let r2_creds = paths.for_runner(r2.runner_id).credentials_path();
        assert!(
            !r2_creds.exists(),
            "orphan credentials file at {r2_creds:?}"
        );
    }

    #[test]
    fn write_then_load_cli_token_roundtrips() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://example.com", "pi_dash_api_xxx").unwrap();
        let token = load_cli_token(&paths).unwrap();
        assert_eq!(token.as_deref(), Some("pi_dash_api_xxx"));
    }

    #[test]
    fn write_then_load_cli_workspace_roundtrips() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://example.com", "pi_dash_api_xxx").unwrap();
        assert_eq!(load_cli_workspace(&paths).unwrap(), None);
        write_cli_workspace(&paths, "acme").unwrap();
        assert_eq!(load_cli_workspace(&paths).unwrap().as_deref(), Some("acme"));
    }

    #[test]
    fn write_cli_token_preserves_existing_workspace_slug() {
        // Re-login (e.g. token refresh) must not clobber the host's
        // existing workspace binding — otherwise users hit the picker
        // every time their token rolls.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://example.com", "tok-1").unwrap();
        write_cli_workspace(&paths, "acme").unwrap();
        write_cli_token(&paths, "https://example.com", "tok-2").unwrap();
        let cfg = file::load_config(&paths).unwrap();
        let cli = cfg.cli.expect("cli section");
        assert_eq!(cli.token.as_deref(), Some("tok-2"));
        assert_eq!(cli.workspace_slug.as_deref(), Some("acme"));
    }

    #[test]
    fn write_cli_workspace_rebinds_when_called_with_new_slug() {
        // Workspace rebinding is intentionally allowed (membership
        // changes, user pick a different one on re-login). The login
        // flow gates this; the helper itself does not refuse.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://example.com", "tok").unwrap();
        write_cli_workspace(&paths, "acme").unwrap();
        write_cli_workspace(&paths, "zenith").unwrap();
        assert_eq!(
            load_cli_workspace(&paths).unwrap().as_deref(),
            Some("zenith")
        );
    }

    #[test]
    fn clear_cli_token_keeps_runner_blocks() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        // Seed config with a runner block + token via the normal flow.
        write_cli_token(&paths, "https://example.com", "pi_dash_api_xxx").unwrap();
        // Append a runner block manually by writing config.
        let mut cfg = file::load_config(&paths).unwrap();
        cfg.runners.push(RunnerConfig {
            name: "r1".into(),
            runner_id: Uuid::new_v4(),
            workspace_slug: Some("acme".into()),
            project_slug: Some("WEB".into()),
            pod_id: None,
            workspace: WorkspaceSection {
                working_dir: tmp.path().join("wd"),
            },
            agent: AgentSection::default(),
            codex: CodexSection::default(),
            claude_code: ClaudeCodeSection::default(),
            approval_policy: ApprovalPolicySection::default(),
        });
        file::write_config(&paths, &cfg).unwrap();

        clear_cli_token(&paths).unwrap();
        let after = file::load_config(&paths).unwrap();
        assert!(after.cli.as_ref().and_then(|c| c.token.as_ref()).is_none());
        assert_eq!(after.runners.len(), 1, "runner block must be preserved");
    }
}
