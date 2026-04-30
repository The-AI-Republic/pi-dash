//! ``pidash runner <add|list|remove>`` — manage runners under the active
//! connection.
//!
//! The CLI mints the runner UUID locally (via ``util::runner_id::mint``,
//! shared with the TUI), POSTs to the cloud, and then writes the new
//! ``[[runner]]`` block to ``config.toml``. Local config remains the
//! canonical runner list — the daemon picks up the change on its next
//! reload.

use anyhow::{Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use std::path::PathBuf;
use uuid::Uuid;

use crate::cloud::runners::{
    RegisterRunnerRequest, RunnerCrudError, delete_runner, register_runner,
};
use crate::config::file;
use crate::config::schema::{
    AgentKind, AgentSection, ApprovalPolicySection, ClaudeCodeSection, CodexSection,
    MAX_RUNNERS_PER_DAEMON, RunnerConfig, WorkspaceSection,
};
use crate::util::paths::Paths;
use crate::util::runner_id;
use crate::util::runner_name;

const MAX_AUTO_NAME_RETRIES: u32 = 5;

#[derive(Debug, ClapArgs)]
pub struct RunnerArgs {
    #[command(subcommand)]
    pub command: RunnerCommand,
}

#[derive(Debug, Subcommand)]
pub enum RunnerCommand {
    /// Register a new runner under the active connection.
    Add(AddArgs),
    /// List runners configured on this machine.
    List,
    /// Deregister a runner and remove its config block.
    Remove(RemoveArgs),
}

#[derive(Debug, Clone, ClapArgs)]
pub struct AddArgs {
    /// Human-friendly name. Auto-generated when omitted.
    #[arg(long)]
    pub name: Option<String>,

    /// Pi Dash project this runner serves.
    #[arg(long)]
    pub project: String,

    /// Pod within the project. Defaults to the project's default pod.
    #[arg(long)]
    pub pod: Option<String>,

    /// Working directory the runner clones into. Defaults to a path
    /// derived from the runner's data dir.
    #[arg(long)]
    pub working_dir: Option<PathBuf>,

    /// Which agent CLI this runner drives.
    #[arg(long, value_enum, default_value_t = AgentKind::Codex)]
    pub agent: AgentKind,
}

#[derive(Debug, ClapArgs)]
pub struct RemoveArgs {
    /// Name of the runner to remove. Must match a ``[[runner]]`` in
    /// ``config.toml``.
    pub name: String,
}

pub async fn run(args: RunnerArgs, paths: &Paths) -> Result<()> {
    match args.command {
        RunnerCommand::Add(a) => add(a, paths).await.map(|_| ()),
        RunnerCommand::List => list(paths),
        RunnerCommand::Remove(a) => remove(a, paths).await,
    }
}

/// Library entry point: exposed so the TUI's add-runner form can reuse the
/// same enrollment logic without going through clap.
pub async fn add(args: AddArgs, paths: &Paths) -> Result<RunnerConfig> {
    let creds = file::load_credentials(paths)
        .context("no credentials.toml — run `pidash connect` first")?;
    let mut cfg = file::load_config(paths)?;

    if cfg.runners.len() >= MAX_RUNNERS_PER_DAEMON {
        anyhow::bail!(
            "daemon already at the {MAX_RUNNERS_PER_DAEMON}-runner cap; remove one with `pidash runner remove <NAME>` first"
        );
    }

    let user_supplied = args.name.is_some();
    if let Some(n) = &args.name {
        runner_name::validate(n).with_context(|| format!("invalid --name value {n:?}"))?;
    }

    let runner_id = runner_id::mint();
    let working_dir = args
        .working_dir
        .clone()
        .unwrap_or_else(|| paths.runner_dir(runner_id).join("workspace"));

    let mut attempts = 0u32;
    let (resp, final_name) = loop {
        attempts += 1;
        let candidate = args
            .name
            .clone()
            .unwrap_or_else(runner_name::generate_default);
        let req = RegisterRunnerRequest {
            runner_id,
            name: candidate.clone(),
            project: args.project.clone(),
            pod: args.pod.clone().unwrap_or_default(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            version: crate::RUNNER_VERSION.to_string(),
            protocol_version: crate::PROTOCOL_VERSION,
        };
        match register_runner(&cfg.daemon.cloud_url, &creds.connection_id, &creds.connection_secret, &req).await {
            Ok(resp) => break (resp, candidate),
            Err(RunnerCrudError::NameTaken)
                if !user_supplied && attempts < MAX_AUTO_NAME_RETRIES =>
            {
                tracing::info!(
                    attempt = attempts,
                    "auto-generated runner name {candidate} taken; retrying"
                );
                continue;
            }
            Err(RunnerCrudError::NameTaken) if user_supplied => {
                anyhow::bail!(
                    "runner name {candidate:?} is already taken on this connection. \
                     Choose a different --name or omit it for an auto-generated one."
                );
            }
            Err(RunnerCrudError::NameTaken) => {
                anyhow::bail!(
                    "could not generate a unique runner name after {MAX_AUTO_NAME_RETRIES} attempts. \
                     Pass --name explicitly."
                );
            }
            Err(RunnerCrudError::Other(e)) => {
                return Err(e).context("cloud rejected runner registration");
            }
        }
    };

    let new_runner = RunnerConfig {
        name: final_name,
        runner_id: resp.runner_id,
        workspace_slug: None,
        project_slug: Some(resp.project_identifier.clone()),
        pod_id: Some(resp.pod_id),
        workspace: WorkspaceSection { working_dir },
        agent: AgentSection { kind: args.agent },
        codex: CodexSection::default(),
        claude_code: ClaudeCodeSection::default(),
        approval_policy: ApprovalPolicySection::default(),
    };
    cfg.runners.push(new_runner.clone());

    // Validate post-mutation so we catch conflicts (duplicate name,
    // duplicate working_dir) before persisting. Roll back the cloud-side
    // registration if validation fails — otherwise we'd leak an orphaned
    // runner row.
    if let Err(err) = cfg.validate() {
        let _ = delete_runner(
            &cfg.daemon.cloud_url,
            &creds.connection_id,
            &creds.connection_secret,
            &runner_id,
        )
        .await;
        anyhow::bail!("runner config invalid after add: {err}");
    }
    file::write_config(paths, &cfg)?;
    println!(
        "Added runner {} ({}) under project {}.",
        new_runner.name, new_runner.runner_id, resp.project_identifier
    );
    Ok(new_runner)
}

pub fn list(paths: &Paths) -> Result<()> {
    let cfg = file::load_config(paths)?;
    if cfg.runners.is_empty() {
        println!("No runners configured. Add one with `pidash runner add --name … --project …`.");
        return Ok(());
    }
    println!("{:<24} {:<36} {:<24}", "NAME", "RUNNER_ID", "PROJECT");
    for r in &cfg.runners {
        println!(
            "{:<24} {:<36} {:<24}",
            r.name,
            r.runner_id,
            r.project_slug.as_deref().unwrap_or("?"),
        );
    }
    Ok(())
}

pub async fn remove(args: RemoveArgs, paths: &Paths) -> Result<()> {
    let creds = file::load_credentials(paths)
        .context("no credentials.toml — run `pidash connect` first")?;
    let mut cfg = file::load_config(paths)?;
    let idx = cfg
        .runners
        .iter()
        .position(|r| r.name == args.name)
        .ok_or_else(|| anyhow::anyhow!("no runner named {:?} in config.toml", args.name))?;
    let runner_id = cfg.runners[idx].runner_id;
    delete_runner(
        &cfg.daemon.cloud_url,
        &creds.connection_id,
        &creds.connection_secret,
        &runner_id,
    )
    .await
    .context("cloud delete-runner failed")?;
    cfg.runners.remove(idx);
    file::write_config(paths, &cfg)?;
    let runner_data = paths.runner_dir(runner_id);
    if runner_data.exists() {
        let _ = std::fs::remove_dir_all(&runner_data);
    }
    println!("Removed runner {}.", args.name);
    Ok(())
}

/// Library shim used by the TUI to delete by `runner_id` (when the user
/// confirms in the runners-list view).
#[allow(dead_code)]
pub async fn remove_by_id(runner_id: &Uuid, paths: &Paths) -> Result<()> {
    let cfg = file::load_config(paths)?;
    let name = cfg
        .runners
        .iter()
        .find(|r| r.runner_id == *runner_id)
        .map(|r| r.name.clone())
        .ok_or_else(|| anyhow::anyhow!("no runner with id {runner_id} in config.toml"))?;
    remove(RemoveArgs { name }, paths).await
}
