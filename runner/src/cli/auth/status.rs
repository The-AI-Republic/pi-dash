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
    let token = match runner_ops::load_cli_token(paths)? {
        Some(t) => t,
        None => {
            println!("Not logged in.");
            println!("Run `pidash auth login` to authenticate this host.");
            std::process::exit(1);
        }
    };

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
        println!("Logged in to {cloud_url}");
        println!("  ✗ Token rejected by cloud ({status}). Run `pidash auth login` to re-authenticate.");
        std::process::exit(1);
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
