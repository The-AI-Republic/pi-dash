//! `pidash auth login` — RFC 8628 device-code flow client.
//!
//! Walks the user through the standard device-code dance:
//!
//! 1. `POST /api/v1/auth/device/start/` — get `user_code` + `device_code`.
//! 2. Show `user_code` + verification URI; try to open the URL in a
//!    browser as a convenience.
//! 3. Poll `POST /api/v1/auth/device/token/` at the cloud-specified
//!    interval until the user approves in the browser, hits a terminal
//!    error, or the grant expires.
//! 4. Write the returned `APIToken` to `[cli].token` in `config.toml`.
//!
//! After a successful login, if the host has no `[[runner]]` block and
//! we're on an interactive TTY, offer to register one inline — this is
//! the common dev-laptop case.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use serde::Deserialize;
use std::io::{IsTerminal, Write};
use std::time::Duration;

use crate::cli::runner_ops;
use crate::config::file;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Pi Dash cloud base URL (e.g. `https://pidash.example.com`).
    /// Optional if this host already has a `config.toml`; we reuse the
    /// existing `[daemon].cloud_url` in that case.
    #[arg(long)]
    pub url: Option<String>,

    /// Don't try to open the verification URL in a browser.
    #[arg(long)]
    pub no_browser: bool,

    /// Don't prompt to register a runner after login succeeds, even on
    /// a TTY. Useful when scripting an auth-only setup.
    #[arg(long)]
    pub no_runner_prompt: bool,

    /// Workspace slug to bind this CLI install to after login.
    #[arg(long, hide = true)]
    pub workspace: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StartResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct TokenSuccess {
    access_token: String,
    #[serde(default)]
    user_email: Option<String>,
    // The server also returns `workspace_slug` here (the workspace the
    // approve step auto-picked), but the CLI deliberately ignores it
    // and re-resolves via `GET /api/v1/auth/workspaces/` so multi-
    // workspace users can pick rather than silently inheriting the
    // server's auto-pick.
}

#[derive(Debug, Deserialize)]
struct MachineTokenResponse {
    machine_token: String,
    workspace_slug: String,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let no_runner_prompt = args.no_runner_prompt;
    let outcome = login_and_bind_workspace(&args, paths).await?;

    if no_runner_prompt {
        return Ok(());
    }
    if !std::io::stdout().is_terminal() {
        return Ok(());
    }
    maybe_offer_runner_add(
        paths,
        &outcome.cloud_url,
        &outcome.access_token,
        &outcome.workspace_slug,
    )
    .await?;
    Ok(())
}

pub async fn run_auth_only(args: Args, paths: &Paths) -> Result<()> {
    login_and_bind_workspace(&args, paths).await.map(|_| ())
}

struct LoginOutcome {
    cloud_url: String,
    access_token: String,
    workspace_slug: String,
}

async fn login_and_bind_workspace(args: &Args, paths: &Paths) -> Result<LoginOutcome> {
    let cloud_url = resolve_cloud_url(args, paths)?;
    crate::cli::connect::validate_cloud_url(&cloud_url)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("building HTTP client")?;

    let start = start_device_code(&client, &cloud_url).await?;

    print_user_code_block(&start);

    if !args.no_browser {
        let with_code = format!("{}?code={}", start.verification_uri, start.user_code);
        let _ = crate::util::browser::open(&with_code);
    }

    let token = poll_for_token(&client, &cloud_url, &start).await?;

    // Persist the short-lived bridge APIToken just long enough to seed the
    // local config and workspace binding. It is replaced below by a shared
    // dev-machine MachineToken, which is the credential used by both the CLI
    // and all runners hosted by this install.
    runner_ops::write_cli_token(paths, &cloud_url, &token.access_token)
        .context("writing temporary [cli].token to config.toml")?;

    println!();
    if let Some(email) = token.user_email.as_deref() {
        println!("✓ Logged in as {email}.");
    } else {
        println!("✓ Logged in.");
    }

    // v1: a CLI install is bound to one workspace. Resolve which one
    // (auto-pick if there's only one membership; prompt otherwise),
    // persist the choice, and use it for any subsequent runner-add.
    let workspace_slug = resolve_workspace_binding(
        paths,
        &cloud_url,
        &token.access_token,
        args.workspace.as_deref(),
    )
    .await?;
    let dev_machine_id =
        runner_ops::ensure_dev_machine_id(paths).context("ensuring local dev-machine identity")?;
    let host_label = crate::util::hostname::default_hostname();
    let machine_token = exchange_for_machine_token(
        &client,
        &cloud_url,
        &token.access_token,
        &workspace_slug,
        &dev_machine_id,
        &host_label,
    )
    .await?;
    runner_ops::write_cli_token(paths, &cloud_url, &machine_token.machine_token)
        .context("writing dev-machine token to config.toml")?;
    if machine_token.workspace_slug != workspace_slug {
        runner_ops::write_cli_workspace(paths, &machine_token.workspace_slug)
            .context("writing returned workspace binding to config.toml")?;
    }
    println!("  Workspace: {}", machine_token.workspace_slug);

    Ok(LoginOutcome {
        cloud_url,
        access_token: machine_token.machine_token,
        workspace_slug: machine_token.workspace_slug,
    })
}

fn resolve_cloud_url(args: &Args, paths: &Paths) -> Result<String> {
    if let Some(u) = &args.url {
        return Ok(u.trim_end_matches('/').to_string());
    }
    if paths.config_path().exists() {
        let cfg = file::load_config(paths)?;
        if !cfg.daemon.cloud_url.is_empty() {
            return Ok(cfg.daemon.cloud_url.trim_end_matches('/').to_string());
        }
    }
    if std::io::stdin().is_terminal() {
        return prompt_for_cloud_url();
    }
    anyhow::bail!("no cloud URL configured — pass --url https://your-pi-dash-instance.example.com");
}

fn prompt_for_cloud_url() -> Result<String> {
    use std::io::BufRead;
    println!();
    println!("Enter your Pi Dash cloud URL.");
    println!("(For AI Republic-hosted Pi Dash this is https://pidash.airepublic.com;");
    println!(" for a self-hosted instance use your own URL.)");
    println!();
    print!("Cloud URL: ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .context("reading cloud URL from stdin")?;
    let url = line.trim();
    if url.is_empty() {
        anyhow::bail!("no cloud URL entered — re-run `pidash auth login --url <URL>`");
    }
    Ok(url.trim_end_matches('/').to_string())
}

async fn start_device_code(client: &reqwest::Client, cloud_url: &str) -> Result<StartResponse> {
    let url = format!("{cloud_url}/api/v1/auth/device/start/");
    let resp = client
        .post(&url)
        .json(&serde_json::json!({}))
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("device-code start failed: HTTP {status}: {body}");
    }
    resp.json::<StartResponse>()
        .await
        .context("parsing device-code start response")
}

fn print_user_code_block(start: &StartResponse) {
    let minutes = start.expires_in / 60;
    println!();
    println!("First, copy your one-time code:");
    println!();
    println!("    {}", start.user_code);
    println!();
    println!("Then open this URL in your browser and approve the login:");
    println!();
    println!("    {}", start.verification_uri);
    println!();
    println!("(Code expires in {minutes} minutes.)");
    println!();
    print!("Waiting for browser approval...");
    let _ = std::io::stdout().flush();
}

async fn poll_for_token(
    client: &reqwest::Client,
    cloud_url: &str,
    start: &StartResponse,
) -> Result<TokenSuccess> {
    let url = format!("{cloud_url}/api/v1/auth/device/token/");
    let mut interval_secs = start.interval.max(1);
    // Hard ceiling at the cloud's stated expiry, plus a small grace.
    let deadline = std::time::Instant::now() + Duration::from_secs(start.expires_in + 5);

    loop {
        if std::time::Instant::now() >= deadline {
            println!();
            anyhow::bail!("device code expired before approval — run `pidash auth login` again");
        }

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        print!(".");
        let _ = std::io::stdout().flush();

        let resp = client
            .post(&url)
            .json(&serde_json::json!({ "device_code": start.device_code }))
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();

        if status.is_success() {
            return serde_json::from_str::<TokenSuccess>(&body_text)
                .with_context(|| format!("parsing device-code token response: {body_text}"));
        }

        // Parse the RFC 8628 error code. Anything we don't recognise is
        // surfaced as-is so the operator can decide what to do.
        let err: TokenError = serde_json::from_str(&body_text).unwrap_or(TokenError {
            error: format!("http_{}", status.as_u16()),
        });
        match err.error.as_str() {
            "authorization_pending" => {}
            "slow_down" => {
                interval_secs = (interval_secs + 5).min(30);
            }
            "expired_token" => {
                println!();
                anyhow::bail!("device code expired — run `pidash auth login` again");
            }
            "access_denied" => {
                println!();
                anyhow::bail!("login was denied in the browser");
            }
            other => {
                println!();
                anyhow::bail!("device-code poll returned {other} (HTTP {status}): {body_text}");
            }
        }
    }
}

/// Pick which workspace this CLI install is bound to and persist it
/// to `[cli].workspace_slug`. Always reaches out to the cloud (even if
/// a stored binding exists) so a stale slug from a removed membership
/// gets corrected.
///
/// Rules:
/// - 0 memberships → bail; the user must be invited first.
/// - 1 membership → silently use it (no prompt).
/// - explicit slug → validate membership and use it without prompting.
/// - ≥2 memberships without explicit slug → if a stored slug is still valid,
///   keep it; else prompt the user to pick.
async fn resolve_workspace_binding(
    paths: &Paths,
    cloud_url: &str,
    api_token: &str,
    explicit_workspace: Option<&str>,
) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("building HTTP client for workspace list")?;
    let workspaces = fetch_workspaces(&client, cloud_url, api_token).await?;

    if workspaces.is_empty() {
        anyhow::bail!(
            "your account isn't a member of any workspace — ask an admin to invite you, then re-run `pidash auth login`"
        );
    }

    let explicit_workspace = explicit_workspace.map(str::trim).filter(|s| !s.is_empty());
    let stored = runner_ops::load_cli_workspace(paths)?;

    let chosen_slug = if let Some(slug) = explicit_workspace {
        if workspaces.iter().any(|w| w.slug == slug) {
            slug.to_string()
        } else {
            anyhow::bail!(
                "workspace {slug:?} is not available for this account — check --workspace or ask an admin to invite you"
            );
        }
    } else if workspaces.len() == 1 {
        workspaces[0].slug.clone()
    } else if let Some(slug) = stored.as_deref()
        && workspaces.iter().any(|w| w.slug == slug)
    {
        // Stable across re-logins: keep the existing binding.
        slug.to_string()
    } else {
        if stored.is_some() {
            println!();
            println!("(Previous workspace binding is no longer valid — pick a new one.)");
        }
        let picked = pick_workspace(&workspaces)?;
        picked.slug.clone()
    };

    runner_ops::write_cli_workspace(paths, &chosen_slug)
        .context("writing [cli].workspace_slug to config.toml")?;
    Ok(chosen_slug)
}

#[derive(Debug, Deserialize)]
struct WorkspaceRow {
    slug: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceListResponse {
    workspaces: Vec<WorkspaceRow>,
}

async fn fetch_workspaces(
    client: &reqwest::Client,
    cloud_url: &str,
    api_token: &str,
) -> Result<Vec<WorkspaceRow>> {
    let url = format!("{cloud_url}/api/v1/auth/workspaces/");
    let resp = client
        .get(&url)
        .header("X-Api-Key", api_token)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("workspace list failed: HTTP {status}: {body}");
    }
    let parsed: WorkspaceListResponse = resp.json().await.context("parsing workspace list")?;
    Ok(parsed.workspaces)
}

async fn exchange_for_machine_token(
    client: &reqwest::Client,
    cloud_url: &str,
    api_token: &str,
    workspace_slug: &str,
    dev_machine_id: &uuid::Uuid,
    host_label: &str,
) -> Result<MachineTokenResponse> {
    let url = format!("{cloud_url}/api/v1/auth/machine-token/");
    let resp = client
        .post(&url)
        .header("X-Api-Key", api_token)
        .json(&serde_json::json!({
            "workspace_slug": workspace_slug,
            "dev_machine_id": dev_machine_id,
            "host_label": host_label,
        }))
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("machine-token exchange failed: HTTP {status}: {body}");
    }
    resp.json::<MachineTokenResponse>()
        .await
        .context("parsing machine-token exchange response")
}

fn pick_workspace(workspaces: &[WorkspaceRow]) -> Result<&WorkspaceRow> {
    use std::io::BufRead;
    println!();
    println!("Your account belongs to multiple workspaces.");
    println!("Pick the one this host should be bound to:");
    println!();
    for (i, w) in workspaces.iter().enumerate() {
        println!("  {}) {:<20} {}", i + 1, w.slug, w.name);
    }
    print!("Pick a workspace [1-{}]: ", workspaces.len());
    std::io::stdout().flush().ok();
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).ok();
    let ans = line.trim();
    let idx: usize = ans.parse().context("expected a number")?;
    if idx == 0 || idx > workspaces.len() {
        anyhow::bail!("selection {idx} out of range");
    }
    Ok(&workspaces[idx - 1])
}

async fn maybe_offer_runner_add(
    paths: &Paths,
    cloud_url: &str,
    api_token: &str,
    workspace_slug: &str,
) -> Result<()> {
    // Skip the prompt entirely if a runner already exists — the user
    // ran `auth login` to refresh a token, not to onboard.
    let existing_runners = if paths.config_path().exists() {
        file::load_config(paths)?.runners.len()
    } else {
        0
    };
    if existing_runners > 0 {
        println!();
        println!("This host already has {existing_runners} runner(s) registered.");
        println!(
            "Use `pidash runner add` to register another, or `pidash runner list` to see them."
        );
        return Ok(());
    }

    // Fetch the user's projects to decide what to suggest.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let projects = match fetch_projects(&client, cloud_url, api_token).await {
        Ok(p) => p,
        Err(e) => {
            // Non-fatal: we logged in fine, the picker just can't run.
            println!();
            println!(
                "(Couldn't list projects: {e}. You can run `pidash runner add --project <SLUG>` later.)"
            );
            return Ok(());
        }
    };

    if projects.is_empty() {
        println!();
        println!("You don't have any projects yet.");
        println!("Create one in the cloud UI, then run `pidash runner add` to register this host.");
        return Ok(());
    }

    println!();
    println!("Register this host as a runner now?");
    let pick = pick_project(&projects)?;
    let Some(project) = pick else {
        println!("Skipped. Run `pidash runner add` later to register this host.");
        return Ok(());
    };

    println!();
    println!("Registering runner under project {}...", project.identifier);
    crate::cli::runner::add(
        crate::cli::runner::AddArgs {
            url: None,
            name: None,
            project: project.identifier.clone(),
            workspace: Some(workspace_slug.to_string()),
            pod: None,
            working_dir: None,
            agent: crate::config::schema::AgentKind::default(),
        },
        paths,
    )
    .await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ProjectRow {
    identifier: String,
    name: String,
}

async fn fetch_projects(
    client: &reqwest::Client,
    cloud_url: &str,
    api_token: &str,
) -> Result<Vec<ProjectRow>> {
    let url = format!("{cloud_url}/api/v1/runner/projects/");
    let resp = client
        .get(&url)
        .header("X-Api-Key", api_token)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("HTTP {status}: {body}");
    }
    let rows: Vec<ProjectRow> = resp.json().await.context("parsing project list")?;
    Ok(rows)
}

fn pick_project(projects: &[ProjectRow]) -> Result<Option<&ProjectRow>> {
    use std::io::BufRead;
    if projects.len() == 1 {
        print!(
            "Register this host for project '{}'? [Y/n] ",
            projects[0].identifier
        );
        std::io::stdout().flush().ok();
        let stdin = std::io::stdin();
        let mut line = String::new();
        stdin.lock().read_line(&mut line).ok();
        let ans = line.trim().to_lowercase();
        if ans.is_empty() || ans == "y" || ans == "yes" {
            return Ok(Some(&projects[0]));
        }
        return Ok(None);
    }
    println!();
    for (i, p) in projects.iter().enumerate() {
        println!("  {}) {:<12} {}", i + 1, p.identifier, p.name);
    }
    println!("  q) skip");
    print!("Pick a project [1-{}]: ", projects.len());
    std::io::stdout().flush().ok();
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).ok();
    let ans = line.trim().to_lowercase();
    if ans.is_empty() || ans == "q" || ans == "skip" {
        return Ok(None);
    }
    let idx: usize = ans.parse().context("expected a number or 'q'")?;
    if idx == 0 || idx > projects.len() {
        anyhow::bail!("selection {idx} out of range");
    }
    Ok(Some(&projects[idx - 1]))
}
