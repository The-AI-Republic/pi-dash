//! `pidash token …` — manage the machine token (a.k.a. "connection")
//! that authenticates this daemon's WebSocket.
//!
//! See `.ai_design/n_runners_in_same_machine/design.md` §5. The token is
//! created in the Pi Dash UI; this command pastes the resulting
//! `token_id` + `token_secret` + title into `credentials.toml` so the
//! daemon picks them up on next start.
//!
//! `pidash install` (systemd unit) is unrelated — that's for OS service
//! lifecycle. `pidash token install` is for credential lifecycle.
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde_json::json;
use uuid::Uuid;

use crate::config::schema::{
    AgentKind, AgentSection, ApprovalPolicySection, ClaudeCodeSection, CodexSection, RunnerConfig,
    TokenCredentials, WorkspaceSection,
};
use crate::util::paths::Paths;

#[derive(Debug, Args)]
pub struct TokenArgs {
    #[command(subcommand)]
    pub command: TokenCommand,
}

#[derive(Debug, Subcommand)]
pub enum TokenCommand {
    /// Install a token on this machine. Replaces any existing `[token]`
    /// block in credentials.toml; the daemon picks up the new values on
    /// next start.
    Install(InstallArgs),

    /// Print the configured token's id and title (no secret). Useful as
    /// a sanity check before/after `install`.
    Show,

    /// Register an additional runner under the locally-configured
    /// token. Calls the cloud's `register-under-token/` endpoint with
    /// the local token credentials and writes a new `[[runner]]` block
    /// to `config.toml`. The daemon picks up the new instance on next
    /// start (or via IPC reload, once that lands).
    AddRunner(AddRunnerArgs),

    /// Remove one runner from this machine. Deregisters it cloud-side
    /// (token-authenticated DELETE/POST), strips its `[[runner]]` block
    /// from `config.toml`, and deletes its local data directory under
    /// `data_dir/runners/<runner_id>/`.
    RemoveRunner(RemoveRunnerArgs),

    /// List the projects available under this token's workspace, with
    /// their identifiers, names, default-pod ids, and pod counts. Use
    /// the printed identifier as the value for `--project` on
    /// `pidash configure` or `pidash token add-runner`.
    ListProjects,
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Token id, as displayed in the Pi Dash UI.
    #[arg(long, env = "PIDASH_TOKEN_ID")]
    pub token_id: Uuid,

    /// Token secret. Shown once at creation time in the UI; this is
    /// the only chance to copy it. Subsequent `pidash token show`
    /// commands print only the id, never the secret.
    #[arg(long, env = "PIDASH_TOKEN_SECRET")]
    pub token_secret: String,

    /// Human-readable label for this connection. The Pi Dash UI shows
    /// it in the connections list; locally it's stamped on
    /// credentials.toml so `pidash token show` can echo it back.
    #[arg(long)]
    pub title: String,
}

#[derive(Debug, Args)]
pub struct AddRunnerArgs {
    /// Human-friendly runner name. Must be unique across this machine
    /// and across the token's owns-set cloud-side.
    #[arg(long)]
    pub name: String,

    /// Project identifier this runner serves. Required — every runner
    /// is bound to one project for its lifetime. Discover available
    /// projects with `pidash token list-projects`.
    #[arg(long)]
    pub project: String,

    /// Pod within the project. Optional; defaults to the project's
    /// default pod. Bare suffix (e.g. `--pod beefy`) is auto-prefixed
    /// with the project identifier server-side.
    #[arg(long)]
    pub pod: Option<String>,

    /// Working directory for the new runner. Required because the local
    /// daemon validates per-runner working_dir uniqueness — every
    /// runner on this machine must own a disjoint tree.
    #[arg(long)]
    pub working_dir: PathBuf,

    /// Agent CLI for the new runner. Defaults to codex.
    #[arg(long, value_enum, default_value_t = AgentKind::Codex)]
    pub agent: AgentKind,
}

#[derive(Debug, Args)]
pub struct RemoveRunnerArgs {
    /// Name of the runner to remove. Must match an entry in
    /// config.toml.
    #[arg(long)]
    pub name: String,
}

pub async fn run(args: TokenArgs, paths: &Paths) -> Result<()> {
    match args.command {
        TokenCommand::Install(install) => run_install(install, paths).await,
        TokenCommand::Show => run_show(paths).await,
        TokenCommand::AddRunner(args) => run_add_runner(args, paths).await,
        TokenCommand::RemoveRunner(args) => run_remove_runner(args, paths).await,
        TokenCommand::ListProjects => run_list_projects(paths).await,
    }
}

async fn run_install(args: InstallArgs, paths: &Paths) -> Result<()> {
    let title = args.title.trim();
    if title.is_empty() {
        anyhow::bail!("--title cannot be empty");
    }
    if title.len() > 128 {
        anyhow::bail!("--title cannot exceed 128 characters");
    }
    let token_secret = args.token_secret.trim().to_string();
    if token_secret.is_empty() {
        anyhow::bail!("--token-secret cannot be empty");
    }

    let config = crate::config::file::load_config(paths)
        .context("loading config.toml — run `pidash configure --url ... --token ...` first")?;
    let mut creds = crate::config::file::load_credentials(paths)
        .context("loading credentials.toml — run `pidash configure --url ... --token ...` first")?;

    // Cloud-side step: link the existing primary runner to the new
    // token BEFORE writing the [token] block locally. Without this,
    // the daemon would switch to token-auth on next start (because
    // [token] is present in credentials.toml) but the cloud's
    // `_resolve_token_runner` would reject the Hello — the runner row
    // still has `machine_token = NULL`. Sequencing this call first
    // means a network failure leaves credentials.toml unchanged, so
    // the daemon stays on legacy auth and keeps working.
    let url = format!(
        "{}/api/v1/runner/{}/link-to-token/",
        config.daemon.cloud_url.trim_end_matches('/'),
        creds.runner_id,
    );
    let body = json!({
        "token_id": args.token_id.to_string(),
        "token_secret": &token_secret,
    });
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let resp = http
        .post(&url)
        .header("Authorization", format!("Bearer {}", creds.runner_secret))
        .json(&body)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!(
            "link-to-token failed (HTTP {status}): {text}\n\
             Token NOT installed locally. Verify the token id/secret \
             and that this runner is in the same workspace as the token."
        );
    }

    creds.token = Some(TokenCredentials {
        token_id: args.token_id,
        token_secret,
        title: title.to_string(),
    });
    crate::config::file::write_credentials(paths, &creds)?;
    println!(
        "Installed token {} (\"{}\") and linked runner {}.",
        args.token_id, title, creds.runner_id,
    );
    println!("Restart the daemon to use the new token.");
    Ok(())
}

async fn run_add_runner(args: AddRunnerArgs, paths: &Paths) -> Result<()> {
    let name = args.name.trim();
    if name.is_empty() {
        anyhow::bail!("--name cannot be empty");
    }

    let mut config = crate::config::file::load_config(paths)
        .context("loading config.toml — run `pidash configure --url ... --token ...` first")?;
    let creds = crate::config::file::load_credentials(paths).context("loading credentials.toml")?;
    let token = creds.token.as_ref().ok_or_else(|| {
        anyhow::anyhow!("no [token] block in credentials.toml — run `pidash token install` first")
    })?;

    if config.runners.iter().any(|r| r.name == name) {
        anyhow::bail!(
            "a runner named {name:?} already exists on this machine; \
             pick a different --name"
        );
    }
    if config
        .runners
        .iter()
        .any(|r| r.workspace.working_dir == args.working_dir)
    {
        anyhow::bail!(
            "another runner already uses {:?} as its working_dir; \
             each runner must have its own tree",
            args.working_dir,
        );
    }

    let url = format!(
        "{}/api/v1/runner/register-under-token/",
        config.daemon.cloud_url.trim_end_matches('/'),
    );
    let project_slug = args.project.trim();
    if project_slug.is_empty() {
        anyhow::bail!("--project cannot be empty");
    }
    let mut body = serde_json::json!({
        "name": name,
        "project": project_slug,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "version": crate::RUNNER_VERSION,
        "protocol_version": crate::PROTOCOL_VERSION,
    });
    if let Some(pod) = args.pod.as_deref() {
        let pod = pod.trim();
        if !pod.is_empty() {
            body["pod"] = serde_json::Value::String(pod.to_string());
        }
    }
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let resp = http
        .post(&url)
        .header("X-Token-Id", token.token_id.to_string())
        .header("Authorization", format!("Bearer {}", token.token_secret))
        .json(&body)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("register-under-token failed: HTTP {status}: {text}");
    }
    let resp_json: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("parsing register-under-token response: {text}"))?;
    let runner_id_str = resp_json
        .get("runner_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("response missing runner_id: {text}"))?;
    let runner_id: Uuid = runner_id_str
        .parse()
        .with_context(|| format!("invalid runner_id from cloud: {runner_id_str}"))?;
    // pod_id is informational; older servers may omit it. Forward-compat
    // by parsing if present and storing None otherwise.
    let pod_id: Option<Uuid> = resp_json
        .get("pod_id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok());

    config.runners.push(RunnerConfig {
        name: name.to_string(),
        runner_id,
        workspace_slug: None,
        project_slug: Some(project_slug.to_string()),
        pod_id,
        workspace: WorkspaceSection {
            working_dir: args.working_dir,
        },
        agent: AgentSection { kind: args.agent },
        codex: CodexSection::default(),
        claude_code: ClaudeCodeSection::default(),
        approval_policy: ApprovalPolicySection::default(),
    });
    config
        .validate()
        .context("config validation rejected the new runner; check the error message")?;
    crate::config::file::write_config(paths, &config)?;

    println!(
        "Registered runner {name:?} (id {runner_id}) under token {}.",
        token.token_id,
    );
    println!("Restart the daemon to bring the new runner online.");
    Ok(())
}

async fn run_remove_runner(args: RemoveRunnerArgs, paths: &Paths) -> Result<()> {
    let name = args.name.trim();
    if name.is_empty() {
        anyhow::bail!("--name cannot be empty");
    }
    let mut config = crate::config::file::load_config(paths).context("loading config.toml")?;
    let creds = crate::config::file::load_credentials(paths).context("loading credentials.toml")?;
    let token = creds.token.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "no [token] block in credentials.toml — \
             per-runner removal requires token auth"
        )
    })?;

    let pos = config
        .runners
        .iter()
        .position(|r| r.name == name)
        .ok_or_else(|| anyhow::anyhow!("no runner named {name:?} in config.toml"))?;
    let runner_id = config.runners[pos].runner_id;

    let url = format!(
        "{}/api/v1/runner/{}/deregister/",
        config.daemon.cloud_url.trim_end_matches('/'),
        runner_id,
    );
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let resp = http
        .post(&url)
        .header("X-Token-Id", token.token_id.to_string())
        .header("Authorization", format!("Bearer {}", token.token_secret))
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("deregister failed: HTTP {status}: {text}");
    }

    // Remove from config.toml.
    config.runners.remove(pos);
    crate::config::file::write_config(paths, &config)?;

    // Delete the runner's local data directory (history, logs, identity).
    // Per design.md §11.4 / decisions.md Q11, removed runners' data is
    // discarded — keeping orphan history is just disk waste.
    let runner_dir = paths.runner_dir(runner_id);
    if runner_dir.exists()
        && let Err(e) = std::fs::remove_dir_all(&runner_dir)
    {
        tracing::warn!(
            "failed to delete {:?}: {e:#} (file removal is best-effort)",
            runner_dir,
        );
    }

    println!("Removed runner {name:?} (id {runner_id}). Data directory deleted.",);
    println!("Restart the daemon for the change to take effect.");
    Ok(())
}

async fn run_show(paths: &Paths) -> Result<()> {
    let creds = crate::config::file::load_credentials(paths)?;
    match creds.token {
        Some(token) => {
            println!("token_id: {}", token.token_id);
            println!("title:    {}", token.title);
        }
        None => {
            println!("no token configured. Run `pidash token install` to install one.");
        }
    }
    Ok(())
}

async fn run_list_projects(paths: &Paths) -> Result<()> {
    let config = crate::config::file::load_config(paths).context("loading config.toml")?;
    let creds = crate::config::file::load_credentials(paths).context("loading credentials.toml")?;
    let token = creds.token.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "no [token] block in credentials.toml — run `pidash token install` first"
        )
    })?;

    let url = format!(
        "{}/api/runners/projects/",
        config.daemon.cloud_url.trim_end_matches('/'),
    );
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let resp = http
        .get(&url)
        .header("X-Token-Id", token.token_id.to_string())
        .header("Authorization", format!("Bearer {}", token.token_secret))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("list-projects failed: HTTP {status}: {text}");
    }
    let projects: Vec<serde_json::Value> = serde_json::from_str(&text)
        .with_context(|| format!("parsing list-projects response: {text}"))?;
    if projects.is_empty() {
        println!(
            "no projects in this workspace yet. Create one in the Pi Dash UI \
             and rerun this command."
        );
        return Ok(());
    }
    println!("{:<24} {:<32} {:<10} DEFAULT_POD_ID", "IDENTIFIER", "NAME", "PODS");
    for p in projects {
        let identifier = p.get("identifier").and_then(|v| v.as_str()).unwrap_or("?");
        let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let pod_count = p.get("pod_count").and_then(|v| v.as_u64()).unwrap_or(0);
        let default_pod_id = p
            .get("default_pod_id")
            .and_then(|v| v.as_str())
            .unwrap_or("(none)");
        println!("{identifier:<24} {name:<32} {pod_count:<10} {default_pod_id}");
    }
    Ok(())
}
