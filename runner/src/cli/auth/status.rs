//! `pidash auth status` — show login state.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use serde::Deserialize;
use std::time::Duration;

use crate::cli::runner_ops;
use crate::config::file;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {}

#[derive(Debug, Deserialize)]
struct UserMe {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    first_name: Option<String>,
    #[serde(default)]
    last_name: Option<String>,
}

pub async fn run(_args: Args, paths: &Paths) -> Result<()> {
    let token = runner_ops::load_cli_token(paths)?.ok_or_else(|| {
        anyhow::anyhow!("not logged in — run `pidash auth login` to authenticate this host")
    })?;

    let cfg = file::load_config(paths).ok();
    let cloud_url = cfg
        .as_ref()
        .map(|c| c.daemon.cloud_url.clone())
        .unwrap_or_default();
    if cloud_url.is_empty() {
        anyhow::bail!("config.toml is missing [daemon].cloud_url — re-run `pidash auth login`");
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let url = format!("{cloud_url}/api/v1/users/me/");
    let resp = client
        .get(&url)
        .header("X-Api-Key", &token)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        anyhow::bail!(
            "logged in to {cloud_url} but the cloud rejected the token (HTTP {status}) — run `pidash auth login` to re-authenticate"
        );
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("/api/v1/users/me/ returned HTTP {status}: {body}");
    }
    let me: UserMe = resp.json().await.context("parsing /users/me/")?;

    println!("Logged in to {cloud_url}");
    if let Some(email) = me.email.as_deref() {
        let name = format!(
            "{} {}",
            me.first_name.as_deref().unwrap_or(""),
            me.last_name.as_deref().unwrap_or("")
        )
        .trim()
        .to_string();
        if name.is_empty() {
            println!("  ✓ Account: {email}");
        } else {
            println!("  ✓ Account: {email} ({name})");
        }
    } else {
        println!("  ✓ Account: <unknown>");
    }

    if let Some(c) = &cfg {
        let workspaces = c
            .cli
            .as_ref()
            .map(|cli| cli.workspace_slugs())
            .unwrap_or_default();
        if !workspaces.is_empty() {
            println!(
                "  ✓ Workspaces signed in: {} (default: {})",
                workspaces.join(", "),
                workspaces[0]
            );
        }
        print_credentials(paths, c).await;
        let runner_count = c.runners.len();
        if runner_count == 0 {
            println!("  - No runners registered on this host.");
            println!("    Run `pidash runner add` to register one.");
        } else {
            println!("  ✓ Runners on this host: {runner_count}");
            for r in &c.runners {
                let project = r.project_slug.as_deref().unwrap_or("?");
                println!("      - {} (project {project})", r.name);
            }
        }
    }
    Ok(())
}

/// Both credentials, their expiry and the engine sign-in state (design
/// §4.3 "`pidash auth status` shows both credentials and their expiry").
async fn print_credentials(paths: &Paths, cfg: &crate::config::schema::Config) {
    let cli = cfg.cli.clone().unwrap_or_default();
    let workspaces = cli
        .workspace_slugs()
        .len()
        .max(usize::from(cli.token.is_some()));
    println!(
        "  ✓ Daemon credential: {workspaces} workspace token(s); each expires 60 days after its last use"
    );
    let now = chrono::Utc::now();
    let state = cli.engine_sign_in_state.unwrap_or_default();
    match cli
        .engine_credential
        .as_ref()
        .filter(|c| !c.token.is_empty())
    {
        Some(c) => match c.expires_at {
            Some(e) if e <= now => println!(
                "  ✗ Engine credential: expired on {} — run `pidash login --engine`",
                e.format("%Y-%m-%d")
            ),
            Some(e) => {
                let days = (e - now).num_days();
                let mark = if days < crate::engine::manager::EXPIRING_WITHIN_DAYS {
                    "!"
                } else {
                    "✓"
                };
                println!(
                    "  {mark} Engine credential: expires {} (in {days} day(s))",
                    e.format("%Y-%m-%d %H:%M UTC")
                );
            }
            None => println!("  ✓ Engine credential: held (expiry unknown)"),
        },
        None => println!("  - Engine credential: none — run `pidash login --engine`"),
    }
    println!("    Engine sign-in state: {}", state.as_str());
    // What the running daemon makes of it, when one answers.
    if let Ok(mut client) = crate::ipc::client::Client::connect(paths.ipc_socket_path()).await
        && let Ok(Ok(crate::ipc::protocol::Response::EngineStatus(st))) = tokio::time::timeout(
            Duration::from_secs(5),
            client.call(crate::ipc::protocol::Request::EngineStatus),
        )
        .await
    {
        println!(
            "    Engine on this computer: {}{}",
            serde_json::to_value(st.state)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default(),
            st.reason_code
                .map(|r| format!(" ({r})"))
                .unwrap_or_default()
        );
    }
    println!(
        "    Stored in: {}",
        match crate::config::secrets::location(cfg) {
            "keychain" => "the OS keychain".to_string(),
            _ => format!("{} (0600)", paths.config_path().display()),
        }
    );
}
