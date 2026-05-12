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
    cfg.cli = Some(CliSection {
        token: Some(token.to_string()),
    });
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
    // Per-runner credentials.toml first — config.toml is the canonical
    // list but the supervisor refuses runner rows that have no
    // credentials file, so we want the credential on disk before the
    // config references it.
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

    let mut cfg = if paths.config_path().exists() {
        file::load_config(paths)?
    } else {
        // First runner on a host that never had a config — happens when
        // the user ran `pidash auth login` (which writes [cli] but no
        // [[runner]]). Bootstrap a minimal config seeded from the
        // response's cloud URL.
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
    let is_first_runner = cfg.runners.is_empty();
    cfg.runners.push(new_runner.clone());
    cfg.validate()
        .map_err(|e| anyhow::anyhow!("config invalid after add: {e}"))?;
    file::write_config(paths, &cfg)?;

    Ok(AppliedRunner {
        runner: new_runner,
        is_first_runner,
    })
}
