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

use crate::cli::runner_ops::{self, ApplyEnrollOptions};
use crate::cloud::http::{
    CreateRunnerRequest, PROVISIONING_DESKTOP_BUNDLED, SharedHttpTransport, create_runner,
};
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
    /// Print the current model credential for the managed engine's auth helper.
    ModelToken(ModelTokenArgs),
    /// Refresh host-owned paths for all projects before starting the daemon.
    Rebind(EnginePathsArgs),
    /// Move a legacy desktop-managed config's runners into this (default)
    /// config — only those of the workspace this config is signed in to.
    /// Prints `{"adopted": [...], "skipped": [...], "fully_adopted": bool}`.
    Adopt(AdoptArgs),
}

#[derive(Debug, ClapArgs)]
pub struct AdoptArgs {
    /// The legacy per-workspace config directory (holding `config.toml`)
    /// the desktop app used to run its own daemon from.
    #[arg(long)]
    pub from_config_dir: PathBuf,
}

#[derive(Debug, ClapArgs)]
pub struct ModelTokenArgs {
    #[arg(long)]
    pub file: PathBuf,
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
    #[command(flatten)]
    pub engine_paths: EnginePathsArgs,
    /// Working copy root for this project.
    #[arg(long)]
    pub working_dir: PathBuf,
    /// Runner name; defaults to `desktop-<hostname>`.
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub host_label: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct EnginePathsArgs {
    /// Absolute path of the bundled agent engine binary.
    #[arg(long)]
    pub engine: PathBuf,
    /// Private `CODEX_HOME` the desktop wrote for this install.
    #[arg(long)]
    pub codex_home: PathBuf,
    /// Directory prepended to the agent's `PATH` (holds the bundled CLI).
    #[arg(long)]
    pub path_prepend: PathBuf,
    /// File the short-lived model credential is read from.
    #[arg(long)]
    pub model_token_file: PathBuf,
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
        ManagedCommand::Rebind(a) => set_engine_paths(paths, &a, None),
        ManagedCommand::Adopt(a) => adopt(a, paths).await,
        ManagedCommand::ModelToken(a) => {
            let token = std::fs::read_to_string(&a.file)
                .context("managed model credential unavailable; reconnect Pi Dash Desktop")?;
            let token = token.trim();
            if token.is_empty() || token.chars().any(|c| c.is_whitespace() || c.is_control()) {
                anyhow::bail!("managed model credential is empty or malformed");
            }
            println!("{token}");
            Ok(())
        }
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
    runner_ops::write_cli_token(paths, &args.cloud_url, &token, false)
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

    // Reuse an existing built-in-engine runner for this project rather than
    // minting a second: the desktop calls enroll every time it opens a
    // project, and the cloud caps bundled runners per project anyway. The
    // config is shared with the user's own `pidash` runners, so only a
    // managed runner qualifies — a user's runner on the same project is never
    // reused or rewritten.
    if let Some(existing) = cfg
        .runners
        .iter()
        .find(|r| r.project_slug.as_deref() == Some(args.project.as_str()) && is_managed_runner(r))
    {
        set_managed_codex_fields(paths, &args, existing.runner_id)?;
        activate_if_running(paths, &existing.name).await?;
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
            provisioning: Some(PROVISIONING_DESKTOP_BUNDLED),
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
        },
    )
    .await
    .context("writing the managed runner to config.toml")?;

    // Stamp the managed Codex fields onto the block we just wrote. They are
    // deliberately not part of ApplyEnrollOptions: no other caller has a
    // bundled engine, and threading them through would put desktop-only
    // concepts into the shared enrollment path.
    set_managed_codex_fields(paths, &args, applied.runner.runner_id)?;
    activate_if_running(paths, &applied.runner.name).await?;

    println!(
        "{{\"ok\":true,\"reused\":false,\"runner_id\":\"{}\",\"name\":{:?},\"first_runner\":{}}}",
        resp.runner_id, applied.runner.name, applied.is_first_runner
    );
    Ok(())
}

fn remove(args: RemoveArgs, paths: &Paths) -> Result<()> {
    let mut cfg = file::load_config(paths).context("loading config.toml")?;
    let before = cfg.runners.len();
    // Only the desktop's own (managed) runners; the user's runners on the
    // same project stay.
    cfg.runners.retain(|r| {
        r.project_slug.as_deref() != Some(args.project.as_str()) || !is_managed_runner(r)
    });
    if cfg.runners.len() == before {
        println!("{{\"ok\":true,\"removed\":0}}");
        return Ok(());
    }
    file::write_config(paths, &cfg).context("rewriting config.toml")?;
    println!("{{\"ok\":true,\"removed\":{}}}", before - cfg.runners.len());
    Ok(())
}

/// Adopt a legacy desktop-managed config. JSON on stdout either way; a
/// failure is `{"error": "adopt_failed", "message": ...}` with exit 1.
/// Success prints the [`AdoptReport`]:
///
/// ```json
/// {"adopted": [{"name": "...", "workspace_id": "..."}],
///  "skipped": [{"name": "...", "workspace_id": "...", "reason": "workspace_mismatch"}],
///  "fully_adopted": false}
/// ```
///
/// `workspace_id` is the runner's workspace as the config records it (its
/// slug), or null when the legacy config doesn't say. The legacy config is
/// never modified; the caller may delete it only when `fully_adopted`.
async fn adopt(args: AdoptArgs, paths: &Paths) -> Result<()> {
    match adopt_runners(&args.from_config_dir, paths) {
        Ok(report) => {
            // Bring the adopted runners up in a daemon that is already
            // running; with no daemon (or one that must reload first) the
            // next start picks them up from config.toml.
            for runner in &report.adopted {
                if let Err(e) = activate_if_running(paths, &runner.name).await {
                    tracing::warn!(runner = %runner.name, "adopted runner not activated: {e:#}");
                }
            }
            println!("{}", serde_json::to_string(&report)?);
            Ok(())
        }
        Err(e) => {
            println!(
                "{}",
                serde_json::json!({ "error": "adopt_failed", "message": format!("{e:#}") })
            );
            std::process::exit(1);
        }
    }
}

/// Why a legacy runner was not adopted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptSkipReason {
    /// Already in the default config (same id or name) — nothing is lost.
    AlreadyPresent,
    /// The default config has no machine token: there is no credential the
    /// runner could use.
    NotSignedIn,
    /// Registered in a workspace other than the one the default config's
    /// machine token is for; it would fail to authenticate.
    WorkspaceMismatch,
    /// Neither side records a workspace to compare.
    WorkspaceUnknown,
    /// Registered against a different cloud.
    CloudMismatch,
    /// Would make the default config invalid (e.g. a working directory
    /// already used by another runner).
    InvalidConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AdoptedRunner {
    pub name: String,
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SkippedRunner {
    pub name: String,
    pub workspace_id: Option<String>,
    pub reason: AdoptSkipReason,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AdoptReport {
    pub adopted: Vec<AdoptedRunner>,
    pub skipped: Vec<SkippedRunner>,
    /// Nothing is left behind: every legacy runner was adopted now or is
    /// already in the default config, so the legacy dir is safe to delete.
    pub fully_adopted: bool,
}

impl AdoptReport {
    fn finish(mut self) -> Self {
        self.fully_adopted = self
            .skipped
            .iter()
            .all(|s| s.reason == AdoptSkipReason::AlreadyPresent);
        self
    }
}

/// Merge the `[[runner]]` blocks (managed Codex fields included) of
/// `<from_dir>/config.toml` into the default config — only runners of the
/// workspace the default config is signed in to, because its one
/// `[cli].token` is a machine token for that workspace and every runner
/// authenticates with it. Runners of other workspaces, of another cloud,
/// already present (same id or name), or that would make the config invalid
/// are skipped. A default config that isn't signed in adopts nothing. Never
/// copies the legacy token or `dev_machine_id`, and never modifies the
/// legacy config. Idempotent — a second run reports what the first adopted
/// as `already_present`.
pub fn adopt_runners(from_dir: &std::path::Path, paths: &Paths) -> Result<AdoptReport> {
    let same_dir = match (from_dir.canonicalize(), paths.config_dir.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if same_dir {
        anyhow::bail!("{from_dir:?} is this host's own config directory");
    }
    let legacy_path = from_dir.join("config.toml");
    let text = std::fs::read_to_string(&legacy_path)
        .with_context(|| format!("reading {legacy_path:?}"))?;
    let legacy: crate::config::schema::Config =
        toml::from_str(&text).with_context(|| format!("parsing {legacy_path:?}"))?;
    let mut report = AdoptReport::default();
    if legacy.runners.is_empty() {
        return Ok(report.finish());
    }
    let legacy_cloud = legacy.daemon.cloud_url.trim_end_matches('/').to_string();
    let legacy_workspace = legacy.cli.as_ref().and_then(|c| c.workspace_slug.clone());
    let workspace_of = |r: &crate::config::schema::RunnerConfig| {
        r.workspace_slug
            .clone()
            .or_else(|| legacy_workspace.clone())
    };
    let skip_all = |report: &mut AdoptReport, reason| {
        for runner in &legacy.runners {
            report.skipped.push(SkippedRunner {
                name: runner.name.clone(),
                workspace_id: workspace_of(runner),
                reason,
            });
        }
    };

    // No default config means not signed in: adopt nothing, write nothing.
    if file::load_config_opt(paths)?.is_none() {
        skip_all(&mut report, AdoptSkipReason::NotSignedIn);
        return Ok(report.finish());
    }
    file::mutate_config(paths, |cfg| {
        let signed_in_workspace = cfg
            .cli
            .as_ref()
            .filter(|c| c.token.as_deref().is_some_and(|t| !t.is_empty()))
            .map(|c| c.workspace_slug.clone());
        let cloud_matches = cfg.daemon.cloud_url.trim_end_matches('/') == legacy_cloud;
        for runner in &legacy.runners {
            let workspace_id = workspace_of(runner);
            let present = cfg
                .runners
                .iter()
                .any(|r| r.runner_id == runner.runner_id || r.name == runner.name);
            let reason = if present {
                Some(AdoptSkipReason::AlreadyPresent)
            } else {
                match &signed_in_workspace {
                    None => Some(AdoptSkipReason::NotSignedIn),
                    Some(_) if !cloud_matches => Some(AdoptSkipReason::CloudMismatch),
                    Some(Some(ws)) if workspace_id.as_deref() == Some(ws.as_str()) => None,
                    Some(Some(_)) if workspace_id.is_some() => {
                        Some(AdoptSkipReason::WorkspaceMismatch)
                    }
                    Some(_) => Some(AdoptSkipReason::WorkspaceUnknown),
                }
            };
            let reason = reason.or_else(|| {
                let mut adopted = runner.clone();
                // Keep the binding explicit now that the runner lives in a
                // config whose own workspace may change at the next sign-in.
                adopted.workspace_slug = workspace_id.clone();
                cfg.runners.push(adopted);
                match cfg.validate() {
                    Ok(()) => None,
                    Err(e) => {
                        cfg.runners.pop();
                        tracing::warn!(runner = %runner.name, "not adopting runner: {e}");
                        Some(AdoptSkipReason::InvalidConfig)
                    }
                }
            });
            match reason {
                None => report.adopted.push(AdoptedRunner {
                    name: runner.name.clone(),
                    workspace_id,
                }),
                Some(reason) => report.skipped.push(SkippedRunner {
                    name: runner.name.clone(),
                    workspace_id,
                    reason,
                }),
            }
        }
        Ok(())
    })?;
    Ok(report.finish())
}

/// A new project joins the already-running daemon instead of waiting for an
/// app restart. The first enrollment has no daemon yet; the host starts it.
async fn activate_if_running(paths: &Paths, name: &str) -> Result<()> {
    use crate::ipc::{
        client::Client,
        protocol::{Request, Response},
    };
    let Ok(mut client) = Client::connect(paths.ipc_socket_path()).await else {
        return Ok(());
    };
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.call(Request::RunnerActivateLocal {
            runner: name.to_owned(),
        }),
    )
    .await
    .context("timed out activating managed runner")??;
    match response {
        Response::Ack => Ok(()),
        Response::Error(error) => anyhow::bail!("activating managed runner: {}", error.message),
        _ => anyhow::bail!("unexpected managed runner activation response"),
    }
}

/// Refresh all host-owned engine paths before the daemon starts. AppImage
/// resource paths change on every launch, including for projects not reopened.
/// Working directories and user-installed runners are deliberately preserved.
fn set_managed_codex_fields(paths: &Paths, args: &EnrollArgs, runner_id: uuid::Uuid) -> Result<()> {
    set_engine_paths(paths, &args.engine_paths, Some(runner_id))
}

/// A runner the desktop app provisioned for its bundled engine: a Codex
/// runner carrying the managed Codex fields (`codex_home` /
/// `model_token_file`, which `Config::validate` keeps all-or-nothing). The
/// user's own runners never have them.
pub fn is_managed_runner(runner: &crate::config::schema::RunnerConfig) -> bool {
    runner.agent.kind == AgentKind::Codex
        && (runner.codex.codex_home.is_some() || runner.codex.model_token_file.is_some())
}

/// Point managed runners at the current engine paths: every existing managed
/// runner, plus `target` (a runner just created by enroll, which has no
/// managed fields yet). User runners are never touched.
fn set_engine_paths(
    paths: &Paths,
    args: &EnginePathsArgs,
    target: Option<uuid::Uuid>,
) -> Result<()> {
    file::mutate_config(paths, |cfg| {
        if let Some(id) = target
            && !cfg.runners.iter().any(|r| r.runner_id == id)
        {
            anyhow::bail!("enrolled runner {id} not found in config");
        }
        for runner in &mut cfg.runners {
            if Some(runner.runner_id) == target || is_managed_runner(runner) {
                runner.codex.binary = args.engine.to_string_lossy().into_owned();
                runner.codex.codex_home = Some(args.codex_home.clone());
                runner.codex.path_prepend = Some(args.path_prepend.clone());
                runner.codex.model_token_file = Some(args.model_token_file.clone());
            }
        }
        cfg.validate()
            .map_err(|e| anyhow::anyhow!("managed config failed validation: {e}"))?;
        Ok(())
    })
    .map(|_| ())
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
