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

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Removal token issued by the cloud UI (optional; if missing we only clean local state).
    #[arg(long)]
    pub token: Option<String>,

    /// Delete local state without contacting the cloud.
    #[arg(long)]
    pub local_only: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    // Step 1 + 2: tear down the service before touching local state, so the
    // daemon isn't still running against to-be-deleted creds. Both calls are
    // tolerant: they return Ok even when the service was never installed /
    // wasn't running, because that's the expected path for a never-installed
    // or already-cleaned machine.
    let svc = crate::service::detect();
    // Warn-level so a partial teardown is visible at default log config —
    // a silent uninstall failure can leave the unit file behind pointing at
    // about-to-be-deleted creds, recreating the crash-loop on next restart.
    if let Err(e) = svc.stop().await {
        tracing::warn!("service stop failed (ok if not running): {e:#}");
    }
    if let Err(e) = svc.uninstall(paths).await {
        tracing::warn!("service uninstall failed (ok if not installed): {e:#}");
    }

    // Step 3: deregister with the cloud while we still have creds.
    match crate::config::file::load_all(paths) {
        Ok((config, creds)) => {
            if !args.local_only {
                match crate::cloud::register::deregister(
                    &config.daemon.cloud_url,
                    &creds.runner_id,
                    &creds.runner_secret,
                    args.token.as_deref(),
                )
                .await
                {
                    Ok(()) => tracing::info!("cloud deregistration acknowledged"),
                    Err(e) => tracing::warn!("cloud deregistration failed: {e:#}"),
                }
            }
        }
        Err(e) => {
            eprintln!("no local configuration found to deregister: {e}");
        }
    }

    // Step 4: delete local config + creds. Idempotent when files are absent.
    crate::config::file::remove_all(paths)?;
    println!("local runner state removed.");
    Ok(())
}
