// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash __managed` — provisioning verbs for the Pi Dash desktop app.
//!
//! The desktop app bundles this binary and needs to do what `pidash auth
//! login` + `pidash runner add` do, without a terminal. Rather than
//! re-implementing config writing and enrollment inside the Tauri host — a
//! second copy of `runner_ops` in a second language, drifting from this one —
//! the app shells out to these hidden subcommands and passes the paths it
//! controls.
//!
//! Everything here is ordinary runner machinery: the same `[cli].token` write
//! `auth login` performs, the same `create_runner` call `runner add` makes, the
//! same `[[runner]]` block. The only additions are the managed Codex fields,
//! which point the bundled engine at Pi Dash's own config and credential
//! instead of the user's.
//!
//! Hidden because these are not a supported way to configure a runner by hand:
//! they assume the caller owns the config directory and will keep it in sync.

use anyhow::{Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use std::io::Read;
use std::path::PathBuf;

use crate::cli::runner_ops::{self, ApplyEnrollOptions, RunnerWorkdirPlan};
use crate::cloud::http::{CreateRunnerRequest, SharedHttpTransport, create_runner};
use crate::config::file;
use crate::config::schema::AgentKind;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct ManagedArgs {
    #[command(subcommand)]
    pub command: ManagedCommand,
}

#[derive(Debug, Subcommand)]
pub enum ManagedCommand {
    /// Write the cloud URL, machine token and workspace binding.
    Bootstrap(BootstrapArgs),
    /// Register a runner for one project and write its `[[runner]]` block.
    Enroll(EnrollArgs),
    /// Drop a project's managed runner from local config.
    Remove(RemoveArgs),
}

#[derive(Debug, ClapArgs)]
pub struct BootstrapArgs {
    #[arg(long)]
    pub cloud_url: String,
    #[arg(long)]
    pub workspace: String,
    /// Identity returned by desktop-enroll, already bound to the machine token.
    #[arg(long)]
    pub dev_machine_id: uuid::Uuid,
    /// Read the machine token from stdin rather than argv, so it never appears
    /// in the process table or a shell history.
    #[arg(long, default_value_t = true)]
    pub machine_token_stdin: bool,
}

#[derive(Debug, ClapArgs)]
pub struct EnrollArgs {
    #[arg(long)]
    pub workspace: String,
    #[arg(long)]
    pub project: String,
    /// Absolute path of the bundled agent engine binary.
    #[arg(long)]
    pub engine: PathBuf,
    /// Private `CODEX_HOME` the desktop wrote for this install.
    #[arg(long)]
    pub codex_home: PathBuf,
    /// Working copy root for this project.
    #[arg(long)]
    pub working_dir: PathBuf,
    /// Directory prepended to the agent's `PATH` (holds the bundled CLI).
    #[arg(long)]
    pub path_prepend: PathBuf,
    /// File the short-lived model credential is read from.
    #[arg(long)]
    pub model_token_file: PathBuf,
    /// Runner name; defaults to `desktop-<hostname>`.
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub host_label: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct RemoveArgs {
    #[arg(long)]
    pub workspace: String,
    #[arg(long)]
    pub project: String,
}

pub async fn run(args: ManagedArgs, paths: &Paths) -> Result<()> {
    match args.command {
        ManagedCommand::Bootstrap(a) => bootstrap(a, paths).await,
        ManagedCommand::Enroll(a) => enroll(a, paths).await,
        ManagedCommand::Remove(a) => remove(a, paths),
    }
}

async fn bootstrap(args: BootstrapArgs, paths: &Paths) -> Result<()> {
    crate::cli::connect::validate_cloud_url(&args.cloud_url)?;
    let token = read_token_from_stdin()?;
    if token.is_empty() {
        anyhow::bail!("no machine token on stdin");
    }
    // Exactly what `pidash auth login` writes after the device-code exchange:
    // `[cli].token` *is* the machine token, so everything downstream (the
    // agent's own `pidash` calls included) authenticates the same way it does
    // for a hand-installed runner.
    runner_ops::write_cli_token(paths, &args.cloud_url, &token)
        .context("writing machine token to config.toml")?;
    runner_ops::write_cli_workspace(paths, &args.workspace)
        .context("writing workspace binding to config.toml")?;
    let mut cfg = file::load_config(paths)?;
    cfg.daemon.dev_machine_id = Some(args.dev_machine_id);
    file::write_config(paths, &cfg).context("writing enrolled dev-machine identity")?;
    println!("{{\"ok\":true,\"config\":{:?}}}", paths.config_path());
    Ok(())
}

async fn enroll(args: EnrollArgs, paths: &Paths) -> Result<()> {
    let cfg = file::load_config(paths)
        .context("loading config.toml — run `__managed bootstrap` first")?;
    let cloud_url = cfg.daemon.cloud_url.clone();
    let api_token = cfg
        .cli
        .as_ref()
        .and_then(|c| c.token.clone())
        .context("no [cli].token — run `__managed bootstrap` first")?;

    // Reuse an existing runner for this project rather than minting a second:
    // the desktop calls enroll every time it opens a project, and the cloud
    // caps bundled runners per project anyway.
    if let Some(existing) = cfg
        .runners
        .iter()
        .find(|r| r.project_slug.as_deref() == Some(args.project.as_str()))
    {
        println!(
            "{{\"ok\":true,\"reused\":true,\"runner_id\":\"{}\",\"name\":{:?}}}",
            existing.runner_id, existing.name
        );
        return Ok(());
    }

    let host_label = args.host_label.clone().unwrap_or_else(hostname_or_desktop);
    let name = args
        .name
        .clone()
        .unwrap_or_else(|| format!("desktop-{}-{}", sanitize_name(&host_label), args.project));

    let dev_machine_id =
        runner_ops::ensure_dev_machine_id(paths).context("ensuring local dev-machine identity")?;
    let transport =
        SharedHttpTransport::new(cloud_url.clone()).context("building HTTP transport for cloud")?;
    let resp = create_runner(
        &transport,
        CreateRunnerRequest {
            api_token: &api_token,
            dev_machine_id: &dev_machine_id,
            workspace_slug: Some(&args.workspace),
            project: &args.project,
            host_label: &host_label,
            name: Some(&name),
            pod: None,
        },
    )
    .await
    .context("registering the managed runner with Pi Dash")?;

    let applied = runner_ops::apply_enroll_response(
        paths,
        &resp,
        &cloud_url,
        ApplyEnrollOptions {
            working_dir: Some(args.working_dir.clone()),
            agent_kind: AgentKind::Codex,
            model: None,
            reasoning_effort: None,
            // Direct working-dir mode: one project, one clone, no pool. The
            // desktop can be promoted to a pooled workdir later without a
            // re-clone, since the canonical clone already exists by then.
            workdir_plan: RunnerWorkdirPlan::Legacy,
        },
    )
    .await
    .context("writing the managed runner to config.toml")?;

    // Stamp the managed Codex fields onto the block we just wrote. They are
    // deliberately not part of ApplyEnrollOptions: no other caller has a
    // bundled engine, and threading them through would put desktop-only
    // concepts into the shared enrollment path.
    set_managed_codex_fields(paths, &args)?;

    println!(
        "{{\"ok\":true,\"reused\":false,\"runner_id\":\"{}\",\"name\":{:?},\"first_runner\":{}}}",
        resp.runner_id, applied.runner.name, applied.is_first_runner
    );
    Ok(())
}

fn remove(args: RemoveArgs, paths: &Paths) -> Result<()> {
    let mut cfg = file::load_config(paths).context("loading config.toml")?;
    let before = cfg.runners.len();
    cfg.runners
        .retain(|r| r.project_slug.as_deref() != Some(args.project.as_str()));
    if cfg.runners.len() == before {
        println!("{{\"ok\":true,\"removed\":0}}");
        return Ok(());
    }
    file::write_config(paths, &cfg).context("rewriting config.toml")?;
    println!("{{\"ok\":true,\"removed\":{}}}", before - cfg.runners.len());
    Ok(())
}

/// Apply the managed Codex paths to the runner block for `args.project`.
fn set_managed_codex_fields(paths: &Paths, args: &EnrollArgs) -> Result<()> {
    let mut cfg = file::load_config(paths).context("re-loading config.toml")?;
    let Some(runner) = cfg
        .runners
        .iter_mut()
        .find(|r| r.project_slug.as_deref() == Some(args.project.as_str()))
    else {
        anyhow::bail!("enrolled runner for {:?} not found in config", args.project);
    };
    runner.codex.binary = args.engine.to_string_lossy().into_owned();
    runner.codex.codex_home = Some(args.codex_home.clone());
    runner.codex.path_prepend = Some(args.path_prepend.clone());
    runner.codex.model_token_file = Some(args.model_token_file.clone());
    cfg.validate()
        .map_err(|e| anyhow::anyhow!("managed config failed validation: {e}"))?;
    file::write_config(paths, &cfg).context("writing managed Codex fields")
}

/// Best-effort host name for the runner label; the cloud only uses it for
/// display, so a fallback is fine.
fn hostname_or_desktop() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "desktop".to_string())
}

fn read_token_from_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading machine token from stdin")?;
    Ok(buf.trim().to_string())
}

/// Reduce a hostname to the characters a runner name allows.
fn sanitize_name(host: &str) -> String {
    let cleaned: String = host
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "host".to_string()
    } else {
        trimmed.chars().take(96).collect()
    }
}
