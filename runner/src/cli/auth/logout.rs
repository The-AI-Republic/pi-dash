//! `pidash auth logout` — revoke the shared dev-machine token server-side
//! and clear it locally. Leaves `[[runner]]` blocks untouched, but a daemon
//! restart needs `pidash auth login` before runners can reconnect.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::time::Duration;

use crate::cli::runner_ops;
use crate::config::file;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Skip the server-side revoke and only clear the token locally.
    /// Useful when the cloud is unreachable.
    #[arg(long)]
    pub local_only: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let token = match runner_ops::load_cli_token(paths)? {
        Some(t) => t,
        None => {
            println!("Not logged in — nothing to revoke.");
            return Ok(());
        }
    };

    if !args.local_only {
        let cfg = file::load_config(paths).ok();
        let cloud_url = cfg
            .as_ref()
            .map(|c| c.daemon.cloud_url.clone())
            .unwrap_or_default();
        if cloud_url.is_empty() {
            anyhow::bail!(
                "config.toml is missing [daemon].cloud_url — pass --local-only to clear the local token without contacting the cloud"
            );
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()?;
        let url = format!("{cloud_url}/api/v1/auth/revoke/");
        let resp = client
            .post(&url)
            .header("X-Api-Key", &token)
            .json(&serde_json::json!({}))
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        // 200 or 401 are both acceptable: the cloud either revoked it
        // for us (200) or the token was already invalid (401). Anything
        // else, we surface but still clear locally so the user isn't
        // stuck with a half-state.
        let status = resp.status();
        if !status.is_success() && status.as_u16() != 401 {
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!("revoke endpoint returned HTTP {status}: {body}");
        }
    }

    runner_ops::clear_cli_token(paths).context("clearing [cli].token from config.toml")?;
    println!("Logged out.");
    let cfg = file::load_config(paths).ok();
    if let Some(c) = &cfg
        && !c.runners.is_empty()
    {
        println!();
        println!(
            "{} runner(s) are still configured on this host.",
            c.runners.len()
        );
        println!(
            "Run `pidash auth login` before reconnecting them, or `pidash runner remove <name>` to remove one."
        );
    }
    Ok(())
}
