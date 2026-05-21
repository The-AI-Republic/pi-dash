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

use crate::cli::runner_ops;
use crate::cloud::http::{SharedHttpTransport, create_runner};
use crate::config::file;
use crate::config::schema::{AgentKind, MAX_RUNNERS_PER_DAEMON, RunnerConfig};
use crate::util::paths::Paths;

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

    /// Pi Dash workspace slug. Optional in single-workspace setups;
    /// required if the caller belongs to multiple workspaces.
    #[arg(long)]
    pub workspace: Option<String>,

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

    /// Skip the cloud-side delete and only clean up local config +
    /// data dir. Use this when the runner is already revoked
    /// cloud-side, or when the cloud is unreachable.
    #[arg(long)]
    pub local_only: bool,
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
///
/// Uses the user-scoped CLI token written by `pidash auth login` to ask
/// the cloud to mint a runner under the caller's identity. Replaces
/// the legacy connection-secret-bearer flow.
pub async fn add(args: AddArgs, paths: &Paths) -> Result<RunnerConfig> {
    let api_token = runner_ops::load_cli_token(paths)
        .context("reading [cli].token from config.toml")?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no CLI token configured — run `pidash auth login` to authenticate this host first"
            )
        })?;

    // Cap check is local + cheap; do it before the network call.
    let existing_count = if paths.config_path().exists() {
        file::load_config(paths)?.runners.len()
    } else {
        0
    };
    if existing_count >= MAX_RUNNERS_PER_DAEMON {
        anyhow::bail!(
            "daemon already at the {MAX_RUNNERS_PER_DAEMON}-runner cap; remove one with `pidash runner remove <NAME>` first"
        );
    }

    let cloud_url = if paths.config_path().exists() {
        file::load_config(paths)?.daemon.cloud_url
    } else {
        anyhow::bail!(
            "no [daemon].cloud_url configured — run `pidash auth login --url <URL>` first"
        )
    };

    let host_label = hostname_or_unknown();
    let transport = SharedHttpTransport::new(cloud_url.clone())
        .context("building HTTP transport for cloud")?;
    // Workspace resolution order:
    //   1. Explicit `--workspace` from this call (highest precedence).
    //   2. `[cli].workspace_slug` persisted by `pidash auth login`
    //      (the v1 single-workspace-per-host binding).
    //   3. None — let the cloud infer from a single membership; it
    //      rejects with a clear error if the caller is multi-workspace.
    let workspace_arg = args
        .workspace
        .clone()
        .or(runner_ops::load_cli_workspace(paths)?);
    let resp = create_runner(
        &transport,
        &api_token,
        workspace_arg.as_deref(),
        &args.project,
        &host_label,
        args.name.as_deref(),
        args.pod.as_deref(),
    )
    .await
    .with_context(|| format!("cloud rejected runner creation against {cloud_url}"))?;

    let applied = runner_ops::apply_enroll_response(
        paths,
        &resp,
        &cloud_url,
        args.working_dir.clone(),
        args.agent,
    )
    .await?;

    println!(
        "Added runner {} ({}) under project {}.",
        applied.runner.name, applied.runner.runner_id, resp.project_identifier
    );
    if applied.is_first_runner {
        println!(
            "First runner on this host — installing service so the daemon starts automatically."
        );
        let svc = crate::service::detect();
        svc.write_unit(paths).await?;
        svc.enable_and_start().await?;
    } else {
        // Restart so the new [[runner]] block is loaded.
        let outcome = crate::service::reload::restart_and_verify(paths).await;
        if !outcome.ok {
            println!("(Service restart did not complete cleanly: {})", outcome.summary);
            if let Some(detail) = outcome.detail {
                println!("{detail}");
            }
        }
    }
    Ok(applied.runner)
}

fn hostname_or_unknown() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown-host".to_string())
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
    crate::cli::connect::revoke_additional_runner(paths, &args.name, args.local_only)
        .await
        .with_context(|| format!("removing runner {:?}", args.name))?;
    if args.local_only {
        println!(
            "Removed runner {} from local config (cloud row not touched).",
            args.name
        );
    } else {
        println!("Removed runner {}.", args.name);
    }
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
    remove(
        RemoveArgs {
            name,
            local_only: false,
        },
        paths,
    )
    .await
}
