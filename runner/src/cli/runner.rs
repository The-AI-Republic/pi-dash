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
    RegisterRunnerRequest, RunnerCrudError, delete_runner, probe_cloud_reachable, register_runner,
};
use crate::config::file;
use crate::config::schema::{
    AgentKind, AgentSection, ApprovalPolicySection, ClaudeCodeSection, CodexSection,
    MAX_RUNNERS_PER_DAEMON, RunnerConfig, WorkspaceSection,
};
use crate::util::confirm::maybe_confirm;
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

    /// Skip the cloud-side delete and only clean up local config +
    /// data dir. Use this when the runner is already revoked
    /// cloud-side, or when the cloud is unreachable.
    #[arg(long)]
    pub local_only: bool,

    /// Skip the interactive y/N confirm. Required for non-interactive
    /// (CI / scripted) callers.
    #[arg(short = 'y', long = "yes")]
    pub yes: bool,
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
        match register_runner(
            &cfg.daemon.cloud_url,
            &creds.connection_id,
            &creds.connection_secret,
            &req,
        )
        .await
        {
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
    // runner row. The rollback uses the X-Api-Key delete surface; if
    // the credentials don't carry an api_token (older enrollments) we
    // log and move on — the operator can clean up via the web UI.
    if let Err(err) = cfg.validate() {
        if let Some(token) = creds.api_token.as_deref() {
            let _ = delete_runner(&cfg.daemon.cloud_url, token, &runner_id, true).await;
        } else {
            tracing::warn!(
                "rollback skipped: credentials lack api_token; \
                 runner row {runner_id} may be orphaned cloud-side"
            );
        }
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

/// Outcome of the `pidash runner remove` connectivity probe; drives
/// the prompt copy and whether we hit the cloud at all.
#[derive(Debug, Clone, Copy)]
enum RemoveMode {
    /// `--local-only` was passed: we never touch the cloud regardless
    /// of reachability. Spec B3.
    LocalOnly,
    /// Cloud unreachable (or no api_token in credentials): can't issue
    /// a cloud delete, fall back to local-only with the B2 prompt.
    OfflineFallback,
    /// Cloud reachable, api_token present: cascade delete (cloud +
    /// local). Spec B1.
    Cascade,
}

pub async fn remove(args: RemoveArgs, paths: &Paths) -> Result<()> {
    let cfg = file::load_config(paths)?;
    let idx = cfg
        .runners
        .iter()
        .position(|r| r.name == args.name)
        .ok_or_else(|| anyhow::anyhow!("no runner named {:?} in config.toml", args.name))?;
    let runner_id = cfg.runners[idx].runner_id;
    let cloud_url = cfg.daemon.cloud_url.clone();

    // Decide the mode before prompting so the prompt copy matches the
    // action we'll actually take.
    let (mode, api_token): (RemoveMode, Option<String>) = if args.local_only {
        (RemoveMode::LocalOnly, None)
    } else {
        let creds = file::load_credentials(paths)
            .context("no credentials.toml — run `pidash connect` first, or pass --local-only")?;
        match creds.api_token {
            Some(token) if probe_cloud_reachable(&cloud_url).await => {
                (RemoveMode::Cascade, Some(token))
            }
            // Either cloud is unreachable, or credentials lack the
            // X-Api-Key the v1 delete endpoint wants. Both paths
            // collapse to local-only with the B2 prompt copy.
            _ => (RemoveMode::OfflineFallback, None),
        }
    };

    let prompt = match mode {
        RemoveMode::Cascade => format!(
            "Delete runner '{}' from cloud and remove the local instance?",
            args.name
        ),
        RemoveMode::OfflineFallback => format!(
            "Cannot reach the cloud right now. \
             Only the local runner instance for '{}' can be deleted, continue?",
            args.name
        ),
        RemoveMode::LocalOnly => format!(
            "Remove the local instance of '{}'? \
             The cloud row will not be touched.",
            args.name
        ),
    };
    if !maybe_confirm(&prompt, false, args.yes) {
        println!("Aborted; runner '{}' was not removed.", args.name);
        return Ok(());
    }

    // Cloud delete first (cascade only). The daemon will receive the
    // remove_runner frame and tear itself down — but we still do our
    // own local cleanup below as belt-and-suspenders, since the
    // daemon may not be running on this host right now.
    if let RemoveMode::Cascade = mode
        && let Some(token) = api_token.as_deref()
        && let Err(e) = delete_runner(&cloud_url, token, &runner_id, true).await
    {
        eprintln!("cloud delete-runner failed: {e:#}");
        eprintln!();
        eprintln!("To remove this runner from the cloud, use the web UI's");
        eprintln!("\"Delete\" button on the runners page.");
        eprintln!("To clean up local state only, re-run with --local-only:");
        eprintln!("  pidash runner remove {} --local-only --yes", args.name);
        anyhow::bail!("cloud delete-runner failed");
    }

    // Local cleanup: strip the [[runner]] block under the host-wide
    // config lock, then drop the per-runner data dir. The daemon's
    // cascade handler does the same thing on its own; doing it here
    // too is a no-op when both paths run, and the only path when the
    // daemon is offline.
    file::mutate_config(paths, |c| {
        c.runners.retain(|r| r.runner_id != runner_id);
        Ok(())
    })?;
    let runner_data = paths.runner_dir(runner_id);
    if runner_data.exists() {
        let _ = std::fs::remove_dir_all(&runner_data);
    }

    match mode {
        RemoveMode::Cascade => {
            println!("Removed runner {} (cloud + local).", args.name);
        }
        RemoveMode::OfflineFallback => {
            println!(
                "Removed runner {} locally. The cloud row remains; \
                 delete it from the web UI when you're back online.",
                args.name
            );
        }
        RemoveMode::LocalOnly => {
            println!(
                "Removed runner {} from local config (cloud row not touched).",
                args.name
            );
        }
    }
    Ok(())
}

/// Library shim used by the TUI to delete by `runner_id` (when the user
/// confirms in the runners-list view). The TUI runs its own confirm
/// dialog before calling, so we pass `yes=true` here.
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
            yes: true,
        },
        paths,
    )
    .await
}
