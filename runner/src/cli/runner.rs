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
    // Initial unlocked load: used for read-only access (cap check,
    // cloud_url for the register call). The actual mutation happens
    // under `mutate_config` below, which re-reads inside the host-wide
    // flock so a concurrent `pidash runner remove` or daemon-side
    // strip can't lose this write.
    let cfg_initial = file::load_config(paths)?;

    if cfg_initial.runners.len() >= MAX_RUNNERS_PER_DAEMON {
        anyhow::bail!(
            "daemon already at the {MAX_RUNNERS_PER_DAEMON}-runner cap; remove one with `pidash runner remove <NAME>` first"
        );
    }
    let cloud_url = cfg_initial.daemon.cloud_url.clone();

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
            &cloud_url,
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
    // Persist under the host-wide config lock so a concurrent
    // `pidash runner remove` or the daemon's RemoveRunner handler
    // can't last-writer-wins our new block. `mutate_config` runs
    // `Config::validate` after the closure (cap, project slug,
    // duplicate name/runner_id) — we still re-check cap explicitly
    // inside the closure because the initial unlocked read could
    // have been racy.
    let push_result = file::mutate_config(paths, |cfg| {
        if cfg.runners.len() >= MAX_RUNNERS_PER_DAEMON {
            anyhow::bail!(
                "daemon already at the {MAX_RUNNERS_PER_DAEMON}-runner cap; remove one with `pidash runner remove <NAME>` first"
            );
        }
        cfg.runners.push(new_runner.clone());
        Ok(())
    });

    // Roll back the cloud-side registration on any persistence
    // failure — invalid post-mutation state, lock contention, IO
    // error, anything. Otherwise we'd leak an orphaned runner row.
    // The rollback uses the X-Api-Key delete surface; if the
    // credentials don't carry an api_token (older enrollments) we
    // log and move on — the operator can clean up via the web UI.
    if let Err(err) = push_result {
        if let Some(token) = creds.api_token.as_deref() {
            let _ = delete_runner(&cloud_url, token, &runner_id, true).await;
        } else {
            tracing::warn!(
                "rollback skipped: credentials lack api_token; \
                 runner row {runner_id} may be orphaned cloud-side"
            );
        }
        return Err(err.context("persisting new runner to config.toml"));
    }
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
    /// Cloud unreachable or credentials lack an `api_token`: can't
    /// issue a cloud delete, fall back to local-only. Spec B2. The
    /// specific cause is carried in [`FallbackReason`] so we can pick
    /// honest prompt + success-message copy.
    OfflineFallback,
    /// Cloud reachable, api_token present: cascade delete (cloud +
    /// local). Spec B1.
    Cascade,
}

/// Why we fell back to local-only when the user did not pass
/// `--local-only`. Lets us avoid the "cannot reach the cloud" lie
/// when the real cause is missing credentials.
#[derive(Debug, Clone, Copy)]
enum FallbackReason {
    /// Probe to `/api/v1/runner/health/` returned non-success or timed
    /// out within the configured window.
    CloudUnreachable,
    /// `credentials.toml` has no `api_token`; the v1 delete endpoint
    /// requires one.
    MissingApiToken,
    /// Mode is not `OfflineFallback`; field is meaningless. Used in
    /// place of `Option` to keep the destructuring tuple flat.
    Unused,
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
    // action we'll actually take. The fallback reason (no api_token vs
    // cloud unreachable) is captured separately so the prompt text
    // doesn't claim "cannot reach the cloud" when the real cause is a
    // missing credential.
    let (mode, api_token, fallback_reason): (RemoveMode, Option<String>, FallbackReason) =
        if args.local_only {
            (RemoveMode::LocalOnly, None, FallbackReason::Unused)
        } else {
            let creds = file::load_credentials(paths).context(
                "no credentials.toml — run `pidash connect` first, or pass --local-only",
            )?;
            match creds.api_token {
                None => (
                    RemoveMode::OfflineFallback,
                    None,
                    FallbackReason::MissingApiToken,
                ),
                Some(token) if probe_cloud_reachable(&cloud_url).await => {
                    (RemoveMode::Cascade, Some(token), FallbackReason::Unused)
                }
                Some(_) => (
                    RemoveMode::OfflineFallback,
                    None,
                    FallbackReason::CloudUnreachable,
                ),
            }
        };

    let prompt = match mode {
        RemoveMode::Cascade => format!(
            "Delete runner '{}' from cloud and remove the local instance?",
            args.name
        ),
        RemoveMode::OfflineFallback => match fallback_reason {
            FallbackReason::CloudUnreachable => format!(
                "Cannot reach the cloud right now. \
                 Only the local runner instance for '{}' can be deleted, continue?",
                args.name
            ),
            FallbackReason::MissingApiToken => format!(
                "These credentials lack an api_token; cloud-side deregistration \
                 is not possible from this CLI. Only the local runner instance for \
                 '{}' can be deleted — delete the cloud row from the web UI. Continue?",
                args.name
            ),
            FallbackReason::Unused => unreachable!("OfflineFallback always has a reason"),
        },
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

    // For OfflineFallback / LocalOnly modes, ask the local daemon to
    // tear down the runner via IPC before we touch config.toml or the
    // data dir. Without this the daemon keeps polling the cloud against
    // its in-memory copy of the config and re-creates the data dir
    // until the operator manually restarts the service. For Cascade
    // mode, the cloud already enqueued a remove_runner frame and the
    // daemon will react to that; the direct mutation below is the
    // belt-and-suspenders "what if the daemon isn't running on this
    // host" path.
    if !matches!(mode, RemoveMode::Cascade) {
        let _ = try_ipc_remove_local(paths, &args.name).await;
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
        RemoveMode::OfflineFallback => match fallback_reason {
            FallbackReason::CloudUnreachable => {
                println!(
                    "Removed runner {} locally. The cloud row remains; \
                     delete it from the web UI when you're back online.",
                    args.name
                );
            }
            FallbackReason::MissingApiToken => {
                println!(
                    "Removed runner {} locally. The cloud row remains; \
                     delete it from the web UI (these credentials lack an \
                     api_token).",
                    args.name
                );
            }
            FallbackReason::Unused => unreachable!("OfflineFallback always has a reason"),
        },
        RemoveMode::LocalOnly => {
            println!(
                "Removed runner {} from local config (cloud row not touched).",
                args.name
            );
        }
    }
    Ok(())
}

/// Best-effort IPC ping to the running daemon: ask it to tear down
/// the named runner without touching the cloud. The daemon's handler
/// runs the same teardown the cloud's `remove_runner` wire frame
/// triggers (cancel run, strip config.toml, delete data dir), but
/// bounded to this one runner; the shared `pidash.service` systemd
/// unit and other runners are left alone.
///
/// Failure is non-fatal: if the daemon isn't running, the socket
/// doesn't exist, or it's an older `pidash` that doesn't know the
/// `RunnerRemoveLocal` verb, we silently swallow the error and let
/// the caller fall through to the direct config-mutation path. The
/// IPC call is purely an optimisation to avoid the "daemon keeps
/// polling for a runner that's been removed from config" window —
/// not a correctness requirement.
async fn try_ipc_remove_local(paths: &Paths, runner_name: &str) -> Result<()> {
    use crate::ipc::client::Client;
    use crate::ipc::protocol::{Request, Response};

    let socket = paths.ipc_socket_path();
    let mut client = match Client::connect(&socket).await {
        Ok(c) => c,
        Err(e) => {
            // Daemon isn't running on this host — that's the common
            // case for a fresh install or a service that's been
            // stopped. Not noise-worthy.
            tracing::debug!(
                "ipc connect to {socket:?} failed: {e:#}; skipping daemon-side teardown"
            );
            return Ok(());
        }
    };
    let req = Request::RunnerRemoveLocal {
        runner: runner_name.to_string(),
    };
    match client.call(req).await {
        Ok(Response::Ack) => {
            tracing::info!(runner = %runner_name, "daemon acked local-remove via IPC");
            Ok(())
        }
        Ok(Response::Error(err)) => {
            // Older daemon that doesn't know the verb, or the runner
            // name resolution failed. The fallback config-mutation
            // path will still clean up.
            tracing::debug!(
                runner = %runner_name,
                "ipc remove_local rejected by daemon: {} (code {}); \
                 falling back to direct config mutation",
                err.message,
                err.code,
            );
            Ok(())
        }
        Ok(other) => {
            tracing::debug!(
                runner = %runner_name,
                "unexpected ipc response from daemon: {other:?}"
            );
            Ok(())
        }
        Err(e) => {
            tracing::debug!(
                runner = %runner_name,
                "ipc remove_local call failed: {e:#}; falling back to direct config mutation"
            );
            Ok(())
        }
    }
}

