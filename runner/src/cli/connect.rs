//! ``pidash connect`` - deprecated one-time-token compatibility path.
//!
//! New runner setup should use ``pidash auth login`` followed by
//! ``pidash runner add``. This command remains executable only so
//! existing one-time enrollment or revive tokens can still be redeemed
//! during the transition.

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Args as ClapArgs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

use crate::cloud::http::{
    RunnerCredentials, SharedHttpTransport, TransportError, enroll_runner, write_runner_credentials,
};
use crate::config::file;
use crate::config::schema::{
    AgentKind, AgentSection, Config, Credentials, DaemonConfig, RunnerConfig, WorkspaceSection,
};
use crate::util::paths::Paths;

const REVOKE_TRANSPORT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Pi Dash cloud base URL (e.g. ``https://pidash.example.com``).
    #[arg(long)]
    pub url: String,

    /// Deprecated one-time enrollment token. Consumed on first use.
    #[arg(long)]
    pub token: String,

    /// Free-form host label. Defaults to the machine's hostname.
    #[arg(long)]
    pub host_label: Option<String>,

    /// Local working directory for this runner — usually the path to the
    /// project repo on disk. The daemon runs the agent CLI here.
    /// Defaults to ``data_dir/runners/<runner_id>/workspace``, which is
    /// fine for sandbox runs but not what most operators want.
    #[arg(long)]
    pub working_dir: Option<PathBuf>,

    /// Which agent CLI this runner drives (``codex`` or ``claude-code``).
    /// Defaults to ``AgentKind::default()`` when omitted.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    /// Default LLM model for this runner's agent. Omit to use the agent's
    /// own default. A model that doesn't apply to ``--agent`` is ignored
    /// with a warning.
    #[arg(long)]
    pub model: Option<String>,

    /// Codex reasoning-effort tier (``low`` / ``medium`` / ``high`` /
    /// ``xhigh``). Only applies when ``--agent codex``.
    #[arg(long)]
    pub reasoning_effort: Option<String>,

    /// Skip the post-enroll doctor + service install. Useful in CI.
    #[arg(long)]
    pub skip_service: bool,

    /// Skip ``loginctl enable-linger`` on Linux (avoids a sudo prompt
    /// in unattended installs).
    #[arg(long)]
    pub skip_linger: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    eprintln!(
        "Warning: `pidash connect` is deprecated and hidden from normal help. \
         For new runners, run `pidash auth login --url <URL>` and then \
         `pidash runner add --project <PROJECT>`. This command remains only \
         for existing one-time enrollment or revive tokens."
    );

    validate_cloud_url(&args.url)?;

    // First enrollment vs. add-another. ``existing_config`` carries the
    // current config when this machine has already enrolled at least one
    // runner; in that case we append the new ``[[runner]]`` block rather
    // than overwriting. The flag is captured up front because the Option
    // gets consumed by the merge below and the post-write steps still
    // need to know which path we took (e.g. to skip the service install
    // on subsequent enrollments — the unit is already there).
    let existing_config = if paths.config_path().exists() {
        Some(file::load_config(paths).context("loading existing config.toml")?)
    } else {
        None
    };
    let is_first_enrollment = existing_config.is_none();

    // Adding a second runner should target the SAME cloud as the first;
    // pointing one host at two distinct clouds isn't supported (the
    // supervisor multiplexes one HTTP transport across runners). Refuse
    // loudly so the operator notices before they end up with a
    // non-functional split-brain config.
    if let Some(cfg) = &existing_config
        && cfg.daemon.cloud_url != args.url
    {
        anyhow::bail!(
            "this machine is already enrolled with cloud {} — refusing to add a runner pointing at {}. \
             Run `pidash remove` first if you want to re-enroll against a different cloud.",
            cfg.daemon.cloud_url,
            args.url
        );
    }

    // Workspace collision check, mirroring Config::validate()'s rules
    // (see schema.rs §"Workspace collisions"). Two runners sharing — or
    // nesting under — a working directory will trample each other's git
    // state. `pidash runner add` runs this through validate() and rolls
    // back the cloud-side registration on conflict, but the HTTPS-flow
    // enrollment token is one-time and has no symmetric deregister, so
    // we have to refuse pre-cloud here. Only meaningful when the
    // operator passed --working-dir; the per-runner sandbox default is
    // unique by runner_id.
    if let (Some(cfg), Some(new_wd)) = (&existing_config, args.working_dir.as_ref()) {
        assert_no_workspace_collision(&cfg.runners, new_wd)?;
    }

    let host_label = args
        .host_label
        .clone()
        .unwrap_or_else(|| hostname().unwrap_or_else(|| "unknown-host".to_string()));

    let transport = SharedHttpTransport::new(args.url.clone())?;
    let resp = enroll_runner(&transport, &args.token, &host_label, None)
        .await
        .context("cloud enrollment failed")?;

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
    .context("writing runner credentials failed")?;

    // Re-enrollment after revive: the cloud reused the same Runner row
    // and minted a fresh refresh token. The local config already has a
    // matching `[[runner]]` block; rotating credentials.toml above is
    // the only thing required, and appending another block would create
    // a duplicate runner_id that the supervisor would reject. Skip the
    // append and the rest of the first-enrollment scaffolding.
    let already_configured = existing_config
        .as_ref()
        .map(|cfg| cfg.runners.iter().any(|r| r.runner_id == resp.runner_id))
        .unwrap_or(false);
    if already_configured {
        println!(
            "Re-enrolled runner {} ({}) — local config left as-is, refresh token rotated.",
            resp.runner_name, resp.runner_id
        );
        println!("Restarting service to pick up the new credentials…");
        let outcome = crate::service::reload::restart_and_verify(paths).await;
        if outcome.ok {
            println!("Service restarted ({}).", outcome.summary);
        } else {
            println!(
                "Service restart did not complete cleanly: {}",
                outcome.summary
            );
            if let Some(detail) = outcome.detail {
                println!("\n{detail}");
            }
            println!("\nThe new credentials were written. Try `pidash restart` manually.");
        }
        return Ok(());
    }

    // working_dir falls back to a per-runner sandbox under
    // data_dir/runners/<rid>/workspace when the operator didn't pass
    // ``--working-dir``. The sandbox path runs but is rarely what
    // anyone wants — most operators want the runner pointed at a real
    // project repo on disk.
    let working_dir = args
        .working_dir
        .clone()
        .unwrap_or_else(|| paths.runner_dir(resp.runner_id).join("workspace"));

    let agent_kind = args.agent.unwrap_or_default();
    let (codex, claude_code, cursor_agent, openclaw) = crate::cli::runner_ops::agent_sections_for(
        agent_kind,
        args.model.as_deref(),
        args.reasoning_effort.as_deref(),
    );
    let new_runner_block = RunnerConfig {
        name: resp.runner_name.clone(),
        runner_id: resp.runner_id,
        workspace_slug: Some(resp.workspace_slug.clone()),
        project_slug: Some(resp.project_identifier.clone()),
        pod_id: None,
        workspace: WorkspaceSection { working_dir },
        workdir: None,
        agent: AgentSection { kind: agent_kind },
        codex,
        claude_code,
        cursor_agent,
        openclaw,
        approval_policy: Default::default(),
    };

    // Either append to existing or build fresh. ``write_config`` is
    // truncating so we always pass the merged result, never just the
    // new block.
    let config = match existing_config {
        Some(mut cfg) => {
            cfg.runners.push(new_runner_block);
            cfg
        }
        None => Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: args.url.clone(),
                dev_machine_id: None,
                log_level: "info".to_string(),
                log_retention_days: 14,
                agent_observability_v1: false,
                auto_update: true,
            },
            runners: vec![new_runner_block],
            workdirs: vec![],
            cli: None,
        },
    };
    file::write_config(paths, &config)?;

    // Legacy machine-scoped credentials are no longer used by the
    // HTTP transport, but a minimal file keeps older CLI surfaces from
    // crashing while the rest of the migration lands. Only write on
    // first enrollment — subsequent runners inherit the existing file
    // (the legacy fields don't carry per-runner state).
    if is_first_enrollment {
        let creds = Credentials {
            connection_id: resp.runner_id,
            connection_secret: String::new(),
            connection_name: Some(resp.runner_name.clone()),
            api_token: None,
            issued_at: Utc::now(),
        };
        file::write_credentials(paths, &creds)?;
    }

    println!(
        "Enrolled runner {} ({}) (host_label={host_label}).",
        resp.runner_name, resp.runner_id
    );
    println!(
        "Workspace: {}; protocol v{}.",
        resp.workspace_slug, resp.protocol_version
    );

    if !is_first_enrollment {
        // Subsequent runners share the existing service unit. Auto-
        // restart the daemon so the new ``[[runner]]`` block goes live
        // without operator intervention. Use the verify helper so we
        // surface any failure loudly instead of silently leaving the
        // user with a stale daemon and a "wait, why isn't it online"
        // mystery later.
        println!("\nAdded runner to existing daemon. Restarting service to load the new config…");
        let outcome = crate::service::reload::restart_and_verify(paths).await;
        if outcome.ok {
            println!("Service restarted ({}).", outcome.summary);
        } else {
            println!(
                "Service restart did not complete cleanly: {}",
                outcome.summary
            );
            if let Some(detail) = outcome.detail {
                println!("\n{detail}");
            }
            println!(
                "\nThe runner config was written successfully. Try `pidash restart` manually."
            );
            // Don't return Err — the cloud-side enrollment + local
            // config write succeeded; the restart hiccup is recoverable
            // by the operator and doesn't undo the work above.
        }
        return Ok(());
    }

    if args.skip_service {
        println!("\nSkipping service install (--skip-service).");
        return Ok(());
    }

    let svc = crate::service::detect();
    svc.write_unit(paths).await?;
    svc.enable_and_start().await?;
    let boot_outcome = if args.skip_linger {
        crate::service::BootStartOutcome::Skipped
    } else {
        svc.ensure_boot_start().await
    };
    print_post_install_hints(&boot_outcome);

    Ok(())
}

fn print_post_install_hints(boot: &crate::service::BootStartOutcome) {
    use crate::service::BootStartOutcome::*;
    println!("\nService installed and running.");
    if cfg!(target_os = "linux") {
        match boot {
            AlreadyEnabled => {
                println!("Linger is enabled — the service will start automatically at boot.");
            }
            Enabled => {
                println!("Enabled linger — the service will now start automatically at boot.");
            }
            NonInteractive => {
                println!("No TTY available; skipped enabling linger.");
                println!("To start the service at boot, run:");
                println!("  sudo loginctl enable-linger $USER");
            }
            Skipped => {
                println!("Skipped `loginctl enable-linger` (--skip-linger).");
                println!("Without lingering, the service only starts at login.");
            }
            CheckFailed(err) => {
                println!("Couldn't check linger state ({err}).");
                println!("To start at boot, run: sudo loginctl enable-linger $USER");
            }
            EnableFailed(err) => {
                println!("Couldn't enable linger ({err}).");
                println!("To start at boot, run: sudo loginctl enable-linger $USER");
            }
            NotApplicable => {}
        }
    }
}

fn hostname() -> Option<String> {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Refuse to enroll if `new_wd` collides with any already-configured
/// runner's working_dir — exact match or nesting either direction.
/// Canonicalize when the path resolves on disk so `./foo` vs
/// `/abs/foo` and trailing-slash variants compare equal; fall back to
/// the raw `PathBuf` otherwise (e.g. the operator passed a path that
/// doesn't exist yet).
fn assert_no_workspace_collision(
    existing: &[crate::config::schema::RunnerConfig],
    new_wd: &std::path::Path,
) -> Result<()> {
    let new_canon = std::fs::canonicalize(new_wd).unwrap_or_else(|_| new_wd.to_path_buf());
    for r in existing {
        let existing_path = &r.workspace.working_dir;
        let existing_canon =
            std::fs::canonicalize(existing_path).unwrap_or_else(|_| existing_path.clone());
        if existing_canon == new_canon {
            anyhow::bail!(
                "runner {} ({}) already uses --working-dir {} — two runners cannot share \
                 a working directory (they would trample each other's git state). \
                 Pick a different --working-dir, or remove that runner first with \
                 `pidash runner remove {}`.",
                r.name,
                r.runner_id,
                existing_path.display(),
                r.name,
            );
        }
        if existing_canon.starts_with(&new_canon) || new_canon.starts_with(&existing_canon) {
            anyhow::bail!(
                "--working-dir {} is nested with runner {}'s working_dir {} — two runners cannot \
                 share or nest working directories. Pick a non-overlapping path, or remove that \
                 runner first with `pidash runner remove {}`.",
                new_wd.display(),
                r.name,
                existing_path.display(),
                r.name,
            );
        }
    }
    Ok(())
}

/// Refuse cleartext http:// to non-localhost hosts (token leak surface).
pub(crate) fn validate_cloud_url(url: &str) -> Result<()> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Ok(());
    }
    if let Some(rest) = lower.strip_prefix("http://") {
        let host = rest.split(['/', ':']).next().unwrap_or("");
        if host == "localhost" || host == "127.0.0.1" || host == "::1" {
            tracing::warn!("using cleartext http:// to {host} — only suitable for development");
            return Ok(());
        }
        anyhow::bail!(
            "refusing to enroll over cleartext http:// to non-localhost ({host}); use https://"
        );
    }
    anyhow::bail!("cloud URL must start with https:// (or http:// for localhost), got {url}")
}

/// Friendly error surface for the TUI add-runner modal. Each variant
/// maps to a specific user-facing message; opaque cloud-side failures
/// fall through to `Server`. Constructed via `map_enroll_error`.
#[derive(thiserror::Error, Debug)]
pub enum EnrollAdditionalError {
    #[error(
        "this legacy token path requires an existing config.toml; for first setup run `pidash auth login` and then `pidash runner add`"
    )]
    NoExistingConfig,
    #[error("loading config.toml failed: {0}")]
    LoadConfigFailed(String),
    #[error("{0}")]
    WorkspaceCollision(String),
    #[error("enrollment token is invalid or expired — generate a new one in the cloud UI")]
    TokenInvalid,
    #[error("enrollment token has already been used — generate a new one in the cloud UI")]
    TokenAlreadyUsed,
    #[error("this runner has been revoked cloud-side")]
    RunnerRevoked,
    #[error("network error contacting cloud: {0}")]
    Network(String),
    #[error("cloud responded with HTTP {status}: {body}")]
    Server { status: u16, body: String },
    #[error("writing per-runner credentials failed: {0}")]
    WriteCredentials(String),
    #[error("writing config.toml failed: {0}")]
    WriteConfig(String),
}

fn map_enroll_error(err: TransportError) -> EnrollAdditionalError {
    match err {
        TransportError::Network(s) => EnrollAdditionalError::Network(s),
        TransportError::RunnerRevoked => EnrollAdditionalError::RunnerRevoked,
        TransportError::Server { status, body } => {
            if body.contains("invalid_or_expired_enrollment_token") {
                EnrollAdditionalError::TokenInvalid
            } else if body.contains("enrollment_token_already_used") {
                EnrollAdditionalError::TokenAlreadyUsed
            } else if body.contains("runner_revoked") {
                EnrollAdditionalError::RunnerRevoked
            } else {
                EnrollAdditionalError::Server { status, body }
            }
        }
        other => EnrollAdditionalError::Network(format!("{other:#}")),
    }
}

/// Library entry point for the TUI add-runner modal: enroll an
/// additional runner against this machine's existing config and
/// persist the new `[[runner]]` block. Mirrors the
/// non-first-enrollment branch of `run()` above; caller is responsible
/// for restarting the daemon afterward (the unit is already installed).
pub async fn enroll_additional_runner(
    paths: &Paths,
    token: &str,
    host_label: &str,
    working_dir: Option<PathBuf>,
) -> std::result::Result<RunnerConfig, EnrollAdditionalError> {
    if !paths.config_path().exists() {
        return Err(EnrollAdditionalError::NoExistingConfig);
    }
    let mut cfg = file::load_config(paths)
        .map_err(|e| EnrollAdditionalError::LoadConfigFailed(format!("{e:#}")))?;

    if let Some(new_wd) = working_dir.as_ref() {
        assert_no_workspace_collision(&cfg.runners, new_wd)
            .map_err(|e| EnrollAdditionalError::WorkspaceCollision(format!("{e:#}")))?;
    }

    let transport = SharedHttpTransport::new(cfg.daemon.cloud_url.clone())
        .map_err(|e| EnrollAdditionalError::Network(format!("transport: {e:#}")))?;
    let resp = enroll_runner(&transport, token, host_label, None)
        .await
        .map_err(map_enroll_error)?;

    let runner_paths = paths.for_runner(resp.runner_id);
    runner_paths
        .ensure()
        .map_err(|e| EnrollAdditionalError::WriteCredentials(format!("create dirs: {e:#}")))?;
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
    .map_err(|e| EnrollAdditionalError::WriteCredentials(format!("{e:#}")))?;

    // Re-enrollment of an existing runner row (cloud-side ``revive``):
    // credentials.toml has already been rotated above. Return the
    // existing config block instead of pushing a duplicate.
    if let Some(existing) = cfg.runners.iter().find(|r| r.runner_id == resp.runner_id) {
        return Ok(existing.clone());
    }

    let working_dir =
        working_dir.unwrap_or_else(|| paths.runner_dir(resp.runner_id).join("workspace"));
    let new_runner = RunnerConfig {
        name: resp.runner_name.clone(),
        runner_id: resp.runner_id,
        workspace_slug: Some(resp.workspace_slug.clone()),
        project_slug: Some(resp.project_identifier.clone()),
        pod_id: None,
        workspace: WorkspaceSection { working_dir },
        workdir: None,
        agent: Default::default(),
        codex: Default::default(),
        claude_code: Default::default(),
        cursor_agent: Default::default(),
        openclaw: Default::default(),
        approval_policy: Default::default(),
    };
    cfg.runners.push(new_runner.clone());
    file::write_config(paths, &cfg)
        .map_err(|e| EnrollAdditionalError::WriteConfig(format!("{e:#}")))?;
    Ok(new_runner)
}

/// Friendly error surface for the TUI / CLI runner-remove flow.
#[derive(thiserror::Error, Debug)]
pub enum RemoveAdditionalError {
    #[error("no existing config — nothing to remove")]
    NoExistingConfig,
    #[error("loading config.toml failed: {0}")]
    LoadConfigFailed(String),
    #[error("no runner named {0:?} in config.toml")]
    UnknownRunner(String),
    #[error("loading per-runner credentials failed: {0}")]
    LoadCredentials(String),
    #[error("network error contacting cloud: {0}")]
    Network(String),
    #[error("cloud responded with HTTP {status}: {body}")]
    Server { status: u16, body: String },
    #[error("writing config.toml failed: {0}")]
    WriteConfig(String),
}

fn map_revoke_error(err: TransportError) -> RemoveAdditionalError {
    match err {
        TransportError::Network(s) => RemoveAdditionalError::Network(s),
        TransportError::Server { status, body } => RemoveAdditionalError::Server { status, body },
        other => RemoveAdditionalError::Network(format!("{other:#}")),
    }
}

/// Library entry point for the TUI / CLI runner-remove flow. Self-revokes
/// the runner cloud-side via the new machine-token endpoint, drops the
/// `[[runner]]` block from the local config, and best-effort wipes the
/// per-runner data dir. Caller is responsible for restarting the daemon.
///
/// `local_only=true` skips the cloud call (use when the cloud is
/// unreachable or the runner row is already gone).
pub async fn revoke_additional_runner(
    paths: &Paths,
    runner_name: &str,
    local_only: bool,
) -> std::result::Result<(), RemoveAdditionalError> {
    if !paths.config_path().exists() {
        return Err(RemoveAdditionalError::NoExistingConfig);
    }
    let mut cfg = file::load_config(paths)
        .map_err(|e| RemoveAdditionalError::LoadConfigFailed(format!("{e:#}")))?;
    let idx = cfg
        .runners
        .iter()
        .position(|r| r.name == runner_name)
        .ok_or_else(|| RemoveAdditionalError::UnknownRunner(runner_name.to_string()))?;
    let runner_id = cfg.runners[idx].runner_id;

    if !local_only {
        let runner_paths = paths.for_runner(runner_id);
        let creds = crate::cloud::http::load_runner_credentials_from(
            runner_paths.credentials_path(),
            runner_name,
        )
        .await
        .map_err(|e| RemoveAdditionalError::LoadCredentials(format!("{e:#}")))?;
        let transport = SharedHttpTransport::new_with_timeout(
            cfg.daemon.cloud_url.clone(),
            REVOKE_TRANSPORT_TIMEOUT,
        )
        .map_err(|e| RemoveAdditionalError::Network(format!("transport: {e:#}")))?;
        let client = crate::cloud::http::RunnerCloudClient::new(runner_id, creds, transport);
        crate::cloud::http::revoke_runner_self(&client)
            .await
            .map_err(map_revoke_error)?;
    }

    cfg.runners.remove(idx);
    file::write_config(paths, &cfg)
        .map_err(|e| RemoveAdditionalError::WriteConfig(format!("{e:#}")))?;
    let runner_data = paths.runner_dir(runner_id);
    if runner_data.exists() {
        // Best effort: a stranded directory is annoying but not fatal;
        // the runner is already gone cloud-side.
        let _ = std::fs::remove_dir_all(&runner_data);
    }
    Ok(())
}

/// Hook left for symmetry with future ``pidash connect --read-stdin`` flows.
#[allow(dead_code)]
fn read_line() -> Option<String> {
    if !std::io::stdin().is_terminal() {
        return None;
    }
    print!("> ");
    std::io::stdout().flush().ok()?;
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).ok()?;
    Some(line.trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{
        AgentSection, ApprovalPolicySection, ClaudeCodeSection, CursorAgentSection, CodexSection, RunnerConfig,
        WorkspaceSection,
    };
    use std::path::PathBuf;

    #[test]
    fn validate_cloud_url_rejects_non_localhost_http() {
        let err = validate_cloud_url("http://evil.example.com")
            .unwrap_err()
            .to_string();
        assert!(err.contains("cleartext"));
    }

    #[test]
    fn validate_cloud_url_allows_https_and_localhost_http() {
        assert!(validate_cloud_url("http://localhost").is_ok());
        assert!(validate_cloud_url("http://localhost:3000").is_ok());
        assert!(validate_cloud_url("https://pi-dash.example").is_ok());
    }

    fn runner_with_wd(name: &str, wd: PathBuf) -> RunnerConfig {
        RunnerConfig {
            name: name.to_string(),
            runner_id: uuid::Uuid::new_v4(),
            workspace_slug: Some("ws".into()),
            project_slug: Some("p".into()),
            pod_id: None,
            workspace: WorkspaceSection { working_dir: wd },
            workdir: None,
            agent: AgentSection::default(),
            codex: CodexSection::default(),
            claude_code: ClaudeCodeSection::default(),
            cursor_agent: CursorAgentSection::default(),
            openclaw: crate::config::schema::OpenClawSection::default(),
            approval_policy: ApprovalPolicySection::default(),
        }
    }

    #[test]
    fn workspace_collision_rejects_exact_dup() {
        let existing = vec![runner_with_wd("a", PathBuf::from("/tmp/repo"))];
        let err = assert_no_workspace_collision(&existing, std::path::Path::new("/tmp/repo"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot share"));
        assert!(err.contains("a"));
    }

    #[test]
    fn workspace_collision_rejects_new_nested_under_existing() {
        let existing = vec![runner_with_wd("a", PathBuf::from("/tmp/repo"))];
        let err =
            assert_no_workspace_collision(&existing, std::path::Path::new("/tmp/repo/subproject"))
                .unwrap_err()
                .to_string();
        assert!(err.contains("nested"));
    }

    #[test]
    fn workspace_collision_rejects_existing_nested_under_new() {
        let existing = vec![runner_with_wd("a", PathBuf::from("/tmp/repo/subproject"))];
        let err = assert_no_workspace_collision(&existing, std::path::Path::new("/tmp/repo"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("nested"));
    }

    #[test]
    fn workspace_collision_allows_disjoint_paths() {
        let existing = vec![
            runner_with_wd("a", PathBuf::from("/tmp/repo-a")),
            runner_with_wd("b", PathBuf::from("/tmp/repo-b")),
        ];
        assert!(
            assert_no_workspace_collision(&existing, std::path::Path::new("/tmp/repo-c")).is_ok()
        );
    }

    #[test]
    fn workspace_collision_allows_first_enrollment() {
        // No existing runners → nothing can collide.
        assert!(assert_no_workspace_collision(&[], std::path::Path::new("/tmp/repo")).is_ok());
    }
}
