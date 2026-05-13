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
    #[serde(default)]
    workspace_slug: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let cloud_url = resolve_cloud_url(&args, paths)?;
    crate::cli::connect::validate_cloud_url(&cloud_url)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("building HTTP client")?;

    let start = start_device_code(&client, &cloud_url).await?;

    print_user_code_block(&start);

    if !args.no_browser {
        let with_code = format!("{}?code={}", start.verification_uri, start.user_code);
        let _ = try_open_browser(&with_code);
    }

    let token = poll_for_token(&client, &cloud_url, &start).await?;

    runner_ops::write_cli_token(paths, &cloud_url, &token.access_token)
        .context("writing [cli].token to config.toml")?;

    println!();
    if let Some(email) = token.user_email.as_deref() {
        println!("✓ Logged in as {email}.");
    } else {
        println!("✓ Logged in.");
    }
    if let Some(ws) = token.workspace_slug.as_deref() {
        println!("  Workspace: {ws}");
    }

    if args.no_runner_prompt {
        return Ok(());
    }
    if !std::io::stdout().is_terminal() {
        return Ok(());
    }
    maybe_offer_runner_add(paths, &cloud_url, &token.access_token).await?;
    Ok(())
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
    anyhow::bail!(
        "no cloud URL configured — pass --url https://your-pi-dash-instance.example.com"
    );
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

fn try_open_browser(url: &str) -> Result<()> {
    let (program, arg_prefix): (&str, Option<&str>) = if cfg!(target_os = "macos") {
        ("open", None)
    } else if cfg!(target_os = "windows") {
        ("cmd", Some("/C start"))
    } else {
        ("xdg-open", None)
    };
    let mut cmd = std::process::Command::new(program);
    if let Some(prefix) = arg_prefix {
        for arg in prefix.split_whitespace() {
            cmd.arg(arg);
        }
    }
    cmd.arg(url);
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.spawn().context("opening browser")?;
    Ok(())
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

async fn maybe_offer_runner_add(paths: &Paths, cloud_url: &str, api_token: &str) -> Result<()> {
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
        println!("Use `pidash runner add` to register another, or `pidash runner list` to see them.");
        return Ok(());
    }

    // Fetch the user's projects to decide what to suggest.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let projects =
        match fetch_projects(&client, cloud_url, api_token).await {
            Ok(p) => p,
            Err(e) => {
                // Non-fatal: we logged in fine, the picker just can't run.
                println!();
                println!("(Couldn't list projects: {e}. You can run `pidash runner add --project <SLUG>` later.)");
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
            name: None,
            project: project.identifier.clone(),
            workspace: None,
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
        print!("Register this host for project '{}'? [Y/n] ", projects[0].identifier);
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

