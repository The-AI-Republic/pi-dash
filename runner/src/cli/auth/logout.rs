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

    /// Sign out only the built-in engine: revoke this computer's engine
    /// credential. The daemon and its runners stay signed in; engine
    /// runners pause until `pidash login --engine`. The desktop app does not
    /// renew an engine signed out this way.
    #[arg(long, conflicts_with = "workspace")]
    pub engine: bool,
}

/// Revoke this machine's engine credential (unless `local_only`) and
/// forget it, recording `signed_out`. Returns whether one was held.
pub(crate) async fn sign_out_engine(paths: &Paths, local_only: bool) -> Result<bool> {
    let Some(cfg) = file::load_config_opt(paths)? else {
        return Ok(false);
    };
    let cli = cfg.cli.clone().unwrap_or_default();
    let held = cli
        .engine_credential
        .as_ref()
        .is_some_and(|c| !c.token.is_empty());
    if !local_only {
        let machine_token = cli
            .workspace_tokens()
            .into_iter()
            .map(|(_, t)| t)
            .find(|t| t.starts_with("mt_"));
        match machine_token {
            Some(mt) if !cfg.daemon.cloud_url.is_empty() => {
                let api = crate::engine::api::EngineApi::new(&cfg.daemon.cloud_url)?;
                if let Err(e) = api.revoke_credential(&mt).await {
                    // Still forget it locally, so the user is not stuck.
                    tracing::warn!("revoking the engine credential failed: {e}");
                    eprintln!("warning: the server did not confirm the engine sign-out: {e}");
                }
            }
            _ => {
                if held {
                    eprintln!(
                        "warning: no daemon credential to revoke the engine credential with; forgetting it locally"
                    );
                }
            }
        }
    }
    runner_ops::store_engine_credential(
        paths,
        None,
        crate::config::schema::EngineSignInState::SignedOut,
    )
    .context("forgetting the engine credential")?;
    Ok(held)
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    if args.engine {
        let held = sign_out_engine(paths, args.local_only).await?;
        crate::service::reload::notify_credentials_changed(paths).await;
        if held {
            println!(
                "Signed out the built-in engine. Run `pidash login --engine` to sign it in again."
            );
        } else {
            println!("The built-in engine was not signed in; recorded it as signed out.");
        }
        return Ok(());
    }
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

    if workspace.is_none() {
        // `pidash auth logout` signs the engine out too (design §4.3). Its
        // credential is revoked first, while a machine token still works.
        sign_out_engine(paths, args.local_only).await?;
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
            println!("Logged out (the built-in engine too).");
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
