//! `pidash auth logout` — revoke the shared dev-machine token server-side
//! (which also revokes this host's desktop handoff keys) and clear it
//! locally. The signed-in user's `[[runner]]` blocks are parked under
//! `<config_dir>/parked/<user_id>.toml` and come back when that user signs
//! in again; a running daemon notices the config change and stops them.
//! Runners of a config that never recorded its user (and whose user can't
//! be looked up) are parked under an `unknown-<timestamp>` key instead.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::time::Duration;

use crate::cli::auth::{session, status};
use crate::cli::runner_ops;
use crate::config::file;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Skip the server-side revoke and only clear the token locally.
    /// Useful when the cloud is unreachable.
    #[arg(long)]
    pub local_only: bool,

    /// Don't ask for confirmation when runs are in progress (they are
    /// stopped).
    #[arg(long)]
    pub yes: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let token = match runner_ops::load_cli_token(paths)? {
        Some(t) => t,
        None => {
            println!("Not logged in — nothing to revoke.");
            return Ok(());
        }
    };

    if !args.yes {
        let activity = session::daemon_activity(paths).await;
        if activity.active_runs > 0 {
            println!(
                "{} run(s) in progress will be stopped.",
                activity.active_runs
            );
            if !crate::util::confirm::ask("Log out anyway?", false) {
                anyhow::bail!("logout cancelled");
            }
        }
    }

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
        // Learn whose runners these are while the token still works, so
        // they are parked under that user rather than an `unknown-…` key.
        if session::current_identity(paths)?.user_unknown() {
            status::backfill_user(paths).await;
        }
        let status = revoke_token(&cloud_url, &token).await?;
        // 200 or 401 are both acceptable: the cloud either revoked it
        // for us (200) or the token was already invalid (401). Anything
        // else, we surface but still clear locally so the user isn't
        // stuck with a half-state.
        if let Err(body) = status {
            tracing::warn!("revoke endpoint returned {body}");
        }
    }

    let outcome =
        session::apply_sign_out(paths).context("clearing [cli].token from config.toml")?;
    println!("Logged out.");
    if outcome.parked > 0 {
        println!(
            "{} runner(s) set aside; they come back when you sign in again with `pidash auth login`.",
            outcome.parked
        );
    }
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

/// `POST /api/v1/auth/revoke/` with `token`. The outer `Result` is a
/// transport failure; the inner one is `Err("HTTP <status>: <body>")` for
/// any answer other than 2xx or 401 (already invalid).
pub async fn revoke_token(cloud_url: &str, token: &str) -> Result<std::result::Result<(), String>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let url = format!("{}/api/v1/auth/revoke/", cloud_url.trim_end_matches('/'));
    let resp = client
        .post(&url)
        .header("X-Api-Key", token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = resp.status();
    if status.is_success() || status.as_u16() == 401 {
        return Ok(Ok(()));
    }
    let body = resp.text().await.unwrap_or_default();
    Ok(Err(format!("HTTP {status}: {body}")))
}
