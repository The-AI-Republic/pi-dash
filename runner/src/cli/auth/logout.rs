//! `pidash auth logout` — revoke the dev-machine tokens server-side and
//! clear them locally. Leaves `[[runner]]` blocks untouched; the runners of
//! a signed-out workspace show as `workspace_signed_out` until
//! `pidash auth login --workspace <slug>` signs it in again.

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

    /// Sign out only this workspace; the other workspaces on this computer
    /// stay signed in. Without it, every workspace is signed out.
    #[arg(long)]
    pub workspace: Option<String>,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let cfg = if paths.config_path().exists() {
        Some(file::load_config(paths)?)
    } else {
        None
    };
    let cli = cfg.as_ref().and_then(|c| c.cli.clone()).unwrap_or_default();
    let workspace = args
        .workspace
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let tokens: Vec<String> = match workspace {
        Some(slug) => cli
            .token_for_workspace(Some(slug))
            .filter(|_| cli.workspace_slugs().iter().any(|s| s == slug))
            .map(|t| vec![t.to_string()])
            .unwrap_or_default(),
        None => cli.workspace_tokens().into_iter().map(|(_, t)| t).collect(),
    };
    if tokens.is_empty() {
        match workspace {
            Some(slug) => println!("Workspace {slug:?} is not signed in — nothing to revoke."),
            None => println!("Not logged in — nothing to revoke."),
        }
        return Ok(());
    }

    if !args.local_only {
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
        for token in &tokens {
            let resp = client
                .post(&url)
                .header("X-Api-Key", token)
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
    }

    match workspace {
        Some(slug) => {
            runner_ops::clear_workspace_token(paths, slug)
                .with_context(|| format!("clearing workspace {slug:?}'s token"))?;
            println!("Signed out workspace {slug}.");
        }
        None => {
            runner_ops::clear_cli_token(paths).context("clearing tokens from config.toml")?;
            println!("Logged out.");
        }
    }
    crate::service::reload::notify_credentials_changed(paths).await;

    let cfg = file::load_config(paths).ok();
    if let Some(c) = &cfg {
        let affected = c
            .runners
            .iter()
            .filter(|r| match workspace {
                Some(slug) => r.workspace_slug.as_deref() == Some(slug),
                None => true,
            })
            .count();
        if affected > 0 {
            println!();
            println!(
                "{affected} runner(s) of the signed-out workspace(s) are still configured on this host."
            );
            println!(
                "Run `pidash auth login{}` before reconnecting them, or `pidash runner remove <name>` to remove one.",
                workspace
                    .map(|s| format!(" --workspace {s}"))
                    .unwrap_or_default()
            );
        }
    }
    Ok(())
}
