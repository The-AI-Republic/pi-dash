//! `pidash remove` — the full teardown command.
//!
//! Inverse of `pidash install` + `pidash configure` in one call. Ordering is
//! deliberate: stop + uninstall the service first so the daemon isn't still
//! talking to the cloud (or holding the IPC socket) when we delete local
//! state and notify the cloud.
//!
//! 1. Stop the service (tolerant; no-op if not running).
//! 2. Uninstall the service unit (tolerant; no-op if not installed).
//! 3. Deregister with the cloud (skipped with `--local-only` or if no creds).
//! 4. Delete local `config.toml` + `credentials.toml`.
//!
//! Requires `--all` as an explicit confirmation: this command wipes EVERY
//! runner on the host plus connection credentials. To drop a single runner
//! instead, use `pidash runner remove <name>`.

use anyhow::{Result, bail};
use clap::Args as ClapArgs;

use crate::cloud::runners::delete_runner;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// REQUIRED confirmation that you want to wipe ALL runners on this
    /// host and the connection credentials. Without this flag the
    /// command refuses to do anything. Use `pidash runner remove <name>`
    /// to drop a single runner instead.
    #[arg(long)]
    pub all: bool,

    /// Delete local state without contacting the cloud. The connection
    /// row remains in the cloud UI until the user revokes it there.
    #[arg(long)]
    pub local_only: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    if !args.all {
        // Refuse and explain. List what would be removed so the user
        // sees the blast radius before re-running with --all.
        eprintln!("pidash remove: refusing to run without --all.");
        eprintln!();
        eprintln!("This command wipes EVERY runner on this host AND the connection");
        eprintln!("credentials. It is not reversible without re-enrolling from scratch.");
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

    let svc = crate::service::detect();
    if let Err(e) = svc.stop().await {
        tracing::warn!("service stop failed (ok if not running): {e:#}");
    }
    if let Err(e) = svc.uninstall(paths).await {
        tracing::warn!("service uninstall failed (ok if not installed): {e:#}");
    }

    // Cloud-side cleanup: best-effort delete each runner. The systemd
    // service has already been stopped + uninstalled above, so the
    // daemon won't be alive to receive the `remove_runner` cascade
    // frame anyway — pass `purge_local=false` so the cloud emits a
    // plain `revoke` and we don't waste a cascade payload that no one
    // will read. Local files are wiped wholesale by `remove_all` below.
    match crate::config::file::load_all(paths) {
        Ok((config, creds)) => {
            if !args.local_only {
                let Some(api_token) = creds.api_token.as_deref() else {
                    eprintln!(
                        "credentials lack an api_token; skipping cloud-side \
                         deregistration. Delete each runner from the web UI."
                    );
                    crate::config::file::remove_all(paths)?;
                    println!("local runner state removed.");
                    // Non-zero exit so CI scripts notice that cloud-side
                    // state was *not* deregistered. Caller can pass
                    // `--local-only` to opt out of this check.
                    bail!(
                        "cloud-side deregistration skipped (no api_token); \
                         delete each runner from the web UI"
                    );
                };
                for r in &config.runners {
                    match delete_runner(&config.daemon.cloud_url, api_token, &r.runner_id, false)
                        .await
                    {
                        Ok(()) => {
                            tracing::info!(runner = %r.name, "cloud delete-runner ok");
                        }
                        Err(e) => {
                            tracing::warn!(
                                runner = %r.name,
                                "cloud delete-runner failed: {e:#}"
                            );
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("no local configuration found to deregister: {e}");
        }
    }

    crate::config::file::remove_all(paths)?;
    println!("local runner state removed.");
    println!("Note: revoke this connection from the cloud UI to end it server-side.");
    Ok(())
}
