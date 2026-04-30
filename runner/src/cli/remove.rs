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

use crate::cloud::runners::delete_runner;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Delete local state without contacting the cloud. The connection
    /// row remains in the cloud UI until the user revokes it there.
    #[arg(long)]
    pub local_only: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let svc = crate::service::detect();
    if let Err(e) = svc.stop().await {
        tracing::warn!("service stop failed (ok if not running): {e:#}");
    }
    if let Err(e) = svc.uninstall(paths).await {
        tracing::warn!("service uninstall failed (ok if not installed): {e:#}");
    }

    // Cloud-side cleanup: best-effort delete each runner under this
    // connection. Connection itself is left for the user to revoke from
    // the cloud UI — the daemon doesn't have authority to revoke its own
    // connection row in the new design (the bearer it holds would
    // self-defeat at exactly the wrong moment).
    match crate::config::file::load_all(paths) {
        Ok((config, creds)) => {
            if !args.local_only {
                for r in &config.runners {
                    match delete_runner(
                        &config.daemon.cloud_url,
                        &creds.connection_id,
                        &creds.connection_secret,
                        &r.runner_id,
                    )
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
