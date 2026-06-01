//! `pidash remove` — the full teardown command.
//!
//! Inverse of `pidash install` + `pidash configure` in one call. Ordering is
//! deliberate: stop + uninstall the service first so the daemon isn't still
//! talking to the cloud (or holding the IPC socket) when we delete local
//! state and notify the cloud.
//!
//! 1. Stop the service (tolerant; no-op if not running).
//! 2. Uninstall the service unit (tolerant; no-op if not installed).
//! 3. Deregister with the cloud (skipped with `--local-only` or if no token).
//! 4. Delete local `config.toml` + legacy `credentials.toml`.
//!
//! Requires `--all` as an explicit confirmation: this command wipes EVERY
//! runner on the host plus the dev-machine token. To drop a single runner
//! instead, use `pidash runner remove <name>`.

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use std::time::Duration;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// REQUIRED confirmation that you want to wipe ALL runners on this
    /// host and the dev-machine token. Without this flag the
    /// command refuses to do anything. Use `pidash runner remove <name>`
    /// to drop a single runner instead.
    #[arg(long)]
    pub all: bool,

    /// Delete local state without contacting the cloud. Runner and dev-machine
    /// records remain in the cloud UI until the user removes them there.
    #[arg(long)]
    pub local_only: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    if !args.all {
        // Refuse and explain. List what would be removed so the user
        // sees the blast radius before re-running with --all.
        eprintln!("pidash remove: refusing to run without --all.");
        eprintln!();
        eprintln!("This command wipes EVERY runner on this host AND the dev-machine");
        eprintln!("token. It is not reversible without re-authenticating from scratch.");
        eprintln!();
        match crate::config::file::load_all(paths) {
            Ok((config, _)) => {
                eprintln!("Runners that would be removed:");
                for r in &config.runners {
                    eprintln!("  - {} ({})", r.name, r.runner_id);
                }
                if config.runners.is_empty() {
                    eprintln!("  (no runners configured)");
                }
            }
            Err(_) => {
                eprintln!("(no local configuration found — nothing to remove)");
            }
        }
        eprintln!();
        eprintln!("To drop a single runner instead:  pidash runner remove <NAME>");
        eprintln!("To proceed with the full teardown: pidash remove --all");
        bail!("--all is required");
    }

    let config = match crate::config::file::load_config(paths) {
        Ok(config) => Some(config),
        Err(e) => {
            eprintln!("no local configuration found to deregister: {e}");
            None
        }
    };

    let svc = crate::service::detect();
    if let Err(e) = svc.stop().await {
        tracing::warn!("service stop failed (ok if not running): {e:#}");
    }
    if let Err(e) = svc.uninstall(paths).await {
        tracing::warn!("service uninstall failed (ok if not installed): {e:#}");
    }

    // Cloud-side cleanup: best-effort delete each runner through the
    // shared dev-machine token, then revoke that token. Do not use the
    // legacy `pidash connect` self-revoke helper here: shared-token
    // installs intentionally have no per-runner credentials.toml.
    let mut cloud_cleanup_failed = false;
    let mut cloud_cleanup_skipped = false;
    if !args.local_only
        && let Some(config) = &config
    {
        match crate::cli::runner_ops::load_cli_token(paths)
            .context("reading [cli].token from config.toml")
        {
            Ok(Some(token)) => {
                for r in &config.runners {
                    match crate::cloud::runners::delete_runner(
                        &config.daemon.cloud_url,
                        &token,
                        &r.runner_id,
                        false,
                    )
                    .await
                    {
                        Ok(()) => {
                            tracing::info!(runner = %r.name, "cloud runner delete ok");
                        }
                        Err(e) => {
                            cloud_cleanup_failed = true;
                            eprintln!("cloud delete failed for runner {}: {e:#}", r.name);
                            tracing::warn!(
                                runner = %r.name,
                                "cloud runner delete failed: {e:#}"
                            );
                        }
                    }
                }
                if let Err(e) = revoke_cli_token(&config.daemon.cloud_url, &token).await {
                    cloud_cleanup_failed = true;
                    eprintln!("cloud token revoke failed: {e:#}");
                    tracing::warn!("cloud token revoke failed: {e:#}");
                }
            }
            Ok(None) => {
                cloud_cleanup_skipped = true;
                eprintln!("No CLI token configured; only local state will be removed.");
            }
            Err(e) => {
                cloud_cleanup_failed = true;
                eprintln!("Could not read CLI token; only local state will be removed: {e:#}");
            }
        }
    }

    crate::config::file::remove_all(paths)?;
    println!("local runner state removed.");
    if args.local_only {
        println!("Cloud cleanup was skipped because --local-only was passed.");
    } else if cloud_cleanup_skipped {
        println!("Cloud cleanup was skipped because no CLI token was configured.");
        println!("Check the cloud UI for remaining runner/dev-machine records.");
    } else if cloud_cleanup_failed {
        println!(
            "Some cloud cleanup failed; check the cloud UI for remaining runner/dev-machine records."
        );
    } else if config.is_some() {
        println!("Cloud runners deleted and dev-machine token revoked.");
    }
    Ok(())
}

async fn revoke_cli_token(cloud_url: &str, token: &str) -> Result<()> {
    let url = format!("{}/api/v1/auth/revoke/", cloud_url.trim_end_matches('/'));
    let resp = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?
        .post(&url)
        .header("X-Api-Key", token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = resp.status();
    if status.is_success() || status == reqwest::StatusCode::UNAUTHORIZED {
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    anyhow::bail!("revoke-token failed: HTTP {status}: {body}");
}
