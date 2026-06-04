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
use std::io::IsTerminal;
use std::path::PathBuf;

use crate::cli::runner_ops;
use crate::cloud::http::{CreateRunnerRequest, SharedHttpTransport, create_runner};
use crate::cloud::runners::{delete_runner, probe_cloud_reachable};
use crate::config::file;
use crate::config::schema::{AgentKind, MAX_RUNNERS_PER_DAEMON, RunnerConfig};
use crate::util::confirm::maybe_confirm;
use crate::util::paths::Paths;
use crate::util::runner_name;

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
    /// Pi Dash cloud base URL. Used when this host is not logged in yet
    /// and `runner add` needs to start `pidash auth login` first.
    #[arg(long)]
    pub url: Option<String>,

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
///
/// Uses the shared dev-machine token written by `pidash auth login` to ask
/// the cloud to create a runner under this host. Older configs with a
/// user-scoped API token are upgraded when the cloud returns a machine token.
pub async fn add(args: AddArgs, paths: &Paths) -> Result<RunnerConfig> {
    let runner_name = args
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(name) = runner_name {
        runner_name::validate(name).with_context(|| {
            format!(
                "invalid --name {name:?}; use an identifier like `test-runner`, or omit --name to let Pi Dash assign one"
            )
        })?;
    }

    // Cap check is local + cheap; do it before auth or any network call.
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

    let api_token = ensure_cli_token(paths, args.url.as_deref(), args.workspace.as_deref()).await?;

    let cloud_url = if paths.config_path().exists() {
        file::load_config(paths)?.daemon.cloud_url
    } else {
        anyhow::bail!(
            "no [daemon].cloud_url configured — run `pidash auth login --url <URL>` first"
        )
    };
    if let Some(url) = args.url.as_deref() {
        let requested = url.trim_end_matches('/');
        if cloud_url != requested {
            anyhow::bail!(
                "this host is already configured for cloud {cloud_url} — refusing --url {requested}"
            );
        }
    }

    let host_label = hostname_or_unknown();
    let dev_machine_id =
        runner_ops::ensure_dev_machine_id(paths).context("ensuring local dev-machine identity")?;
    let transport =
        SharedHttpTransport::new(cloud_url.clone()).context("building HTTP transport for cloud")?;
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
        CreateRunnerRequest {
            api_token: &api_token,
            dev_machine_id: &dev_machine_id,
            workspace_slug: workspace_arg.as_deref(),
            project: &args.project,
            host_label: &host_label,
            name: runner_name,
            pod: args.pod.as_deref(),
        },
    )
    .await
    .with_context(|| format!("cloud rejected runner creation against {cloud_url}"))?;

    // Local persistence under the branch's rollback umbrella: if anything
    // between the cloud mint above and the on-disk write fails, we delete
    // the cloud-side row so the operator doesn't end up with an orphan.
    // The rollback uses the same X-Api-Key surface as `pidash runner
    // remove` (cascade so any daemon already polling tears down too); a
    // failed rollback is logged but does not mask the original error.
    let applied = match runner_ops::apply_enroll_response(
        paths,
        &resp,
        &cloud_url,
        args.working_dir.clone(),
        args.agent,
    )
    .await
    {
        Ok(applied) => applied,
        Err(persist_err) => {
            // Best-effort local cleanup before unwinding the cloud row.
            // `apply_enroll_response` may have written either nothing
            // (config-write failed) or just the `[[runner]]` block (the
            // credentials write failed after the config write); we strip
            // the block and the per-runner files regardless of which
            // step tripped so the operator isn't left with stray state.
            let runner_paths = paths.for_runner(resp.runner_id);
            let _ = file::mutate_config(paths, |cfg| {
                cfg.runners.retain(|r| r.runner_id != resp.runner_id);
                Ok(())
            });
            let _ = std::fs::remove_file(runner_paths.credentials_path());
            if let Err(rollback_err) =
                delete_runner(&cloud_url, &api_token, &resp.runner_id, true).await
            {
                tracing::warn!(
                    runner_id = %resp.runner_id,
                    "cloud rollback of orphaned runner failed: {rollback_err:#}; \
                     row may need manual cleanup via the web UI",
                );
            }
            return Err(persist_err.context("persisting new runner to config.toml"));
        }
    };

    println!(
        "Added runner {} ({}) under project {}.",
        applied.runner.name, applied.runner.runner_id, resp.project_identifier
    );
    let working_dir = &applied.runner.workspace.working_dir;
    if crate::workspace::git::is_git_repo(working_dir) {
        if let Err(err) = crate::cli::context::write_context_for_project(
            paths,
            working_dir,
            &resp.project_identifier,
        )
        .await
        {
            println!(
                "(Runner was added, but .pidash/context.md was not written: {})",
                err
            );
        }
    } else {
        println!("Workspace context will be written after the runner resolves its git workspace.");
    }
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
            println!(
                "(Service restart did not complete cleanly: {})",
                outcome.summary
            );
            if let Some(detail) = outcome.detail {
                println!("{detail}");
            }
        }
    }
    Ok(applied.runner)
}

async fn ensure_cli_token(
    paths: &Paths,
    cloud_url: Option<&str>,
    workspace: Option<&str>,
) -> Result<String> {
    if let Some(token) =
        runner_ops::load_cli_token(paths).context("reading [cli].token from config.toml")?
    {
        return Ok(token);
    }

    println!("No Pi Dash auth token found; starting `pidash auth login` first.");
    crate::cli::auth::login::run_auth_only(
        crate::cli::auth::login::Args {
            url: cloud_url.map(|u| u.trim_end_matches('/').to_string()),
            no_browser: !std::io::stderr().is_terminal() && !std::io::stdin().is_terminal(),
            no_runner_prompt: true,
            workspace: workspace.map(str::to_string),
        },
        paths,
    )
    .await
    .context("auth login before runner add failed")?;

    runner_ops::load_cli_token(paths)
        .context("reading [cli].token from config.toml after auth login")?
        .ok_or_else(|| {
            anyhow::anyhow!("auth login completed but no CLI token was written to config.toml")
        })
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

/// Outcome of the `pidash runner remove` connectivity probe; drives
/// the prompt copy and whether we hit the cloud at all.
#[derive(Debug, Clone, Copy)]
enum RemoveMode {
    /// `--local-only` was passed: we never touch the cloud regardless
    /// of reachability. Spec B3.
    LocalOnly,
    /// Cloud unreachable or no `[cli].token` configured: can't issue a
    /// cloud delete, fall back to local-only. Spec B2. The specific
    /// cause is carried in [`FallbackReason`] so we can pick honest
    /// prompt + success-message copy.
    OfflineFallback,
    /// Cloud reachable, CLI token present: cascade delete (cloud +
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
    /// `[cli].token` in `config.toml` is absent; the v1 delete endpoint
    /// requires it. Operator needs to run `pidash auth login` first.
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
    // action we'll actually take. The fallback reason (no CLI token vs
    // cloud unreachable) is captured separately so the prompt text
    // doesn't claim "cannot reach the cloud" when the real cause is a
    // missing credential.
    //
    // The CLI token comes from `[cli].token` in `config.toml` — the
    // single source of truth populated by `pidash auth login`. The
    // legacy workspace-level `credentials.toml::api_token` field is
    // never populated by any current path (see
    // `cli/connect.rs::write_credentials` which hard-codes
    // `api_token: None`), so reading from it would always classify
    // every host as `MissingApiToken` and silently kill cascade
    // delete. Aligns this with `pidash runner add` which also reads
    // via `runner_ops::load_cli_token`.
    let (mode, api_token, fallback_reason): (RemoveMode, Option<String>, FallbackReason) =
        if args.local_only {
            (RemoveMode::LocalOnly, None, FallbackReason::Unused)
        } else {
            let cli_token = runner_ops::load_cli_token(paths)
                .context("reading [cli].token from config.toml")?;
            match cli_token {
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
                "No CLI token configured (run `pidash auth login` to authenticate \
                 this host with the cloud); cloud-side deregistration is not \
                 possible from this CLI. Only the local runner instance for '{}' \
                 can be deleted — delete the cloud row from the web UI. Continue?",
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
                     delete it from the web UI, or run `pidash auth login` \
                     and re-run this command to enable cloud-side delete.",
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
