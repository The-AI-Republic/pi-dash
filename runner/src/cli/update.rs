//! `pidash update` — swap the on-disk `pidash` binary for the latest
//! release published to GitHub Releases.
//!
//! By design, this command **only swaps the file**. The currently-running
//! daemon keeps its loaded copy and is not disturbed. The new code only
//! takes effect on the next natural restart (`pidash restart`, host reboot,
//! systemd respawn after a crash). Pass `--restart` to also trigger a
//! service restart at the end.
//!
//! `--check` reports whether an update is available without performing one.
//!
//! The daemon's auto-update path calls [`check_or_swap`] so the manual
//! CLI and the daemon-driven swap share one code path.
//!
//! ## Receipt requirement
//!
//! This command only works for binaries installed via `pidash-installer.sh`
//! (which cargo-dist writes an install receipt for). Source builds and
//! `cargo install`'d binaries lack a receipt and will get a clear error
//! suggesting they reinstall via the installer if they want self-update.

use anyhow::{Context, Result};
use axoupdater::AxoUpdater;
use clap::Args as ClapArgs;

use crate::RUNNER_VERSION;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Only report whether an update is available; do not download or
    /// swap the binary.
    #[arg(long)]
    pub check: bool,

    /// After a successful swap, also restart the daemon so the new
    /// binary takes effect immediately. Default is to swap only and
    /// print a "run `pidash restart` to apply" hint.
    #[arg(long)]
    pub restart: bool,
}
pub async fn run(args: Args, _paths: &Paths) -> Result<()> {
    match check_or_swap(args.check).await? {
        SwapOutcome::AlreadyLatest => {
            println!("pidash is already on the latest version (v{RUNNER_VERSION}).");
        }
        SwapOutcome::UpdateAvailable { new_version } => {
            // --check path: print and exit without touching disk.
            println!(
                "update available: v{new_version} (running v{RUNNER_VERSION}). run `pidash update` to install."
            );
        }
        SwapOutcome::Swapped { new_version, old_version } => {
            let old = old_version.unwrap_or_else(|| RUNNER_VERSION.to_string());
            println!("installed v{new_version} (was v{old}).");
            if args.restart {
                println!("restarting daemon to apply...");
                let svc = crate::service::detect();
                svc.stop().await.ok();
                svc.start()
                    .await
                    .context("daemon restart after update")?;
                println!("daemon restarted on the new binary.");
            } else {
                println!("run `pidash restart` to apply (the running daemon is still on v{old}).");
            }
        }
    }
    Ok(())
}

/// What the update flow did. `check=true` short-circuits to either
/// `AlreadyLatest` or `UpdateAvailable` without touching the binary;
/// `check=false` either confirms latest or returns `Swapped`.
#[derive(Debug)]
pub enum SwapOutcome {
    AlreadyLatest,
    UpdateAvailable {
        new_version: String,
    },
    Swapped {
        new_version: String,
        old_version: Option<String>,
    },
}

/// Shared swap entry point used by both the manual CLI and the daemon's
/// auto-update orchestration. Loads the cargo-dist install receipt,
/// queries the GitHub Releases for `pidash`, and (when `check` is false)
/// runs the platform installer in-place over the existing binary.
///
/// Returns an error if no install receipt exists — i.e. the user did not
/// install via `pidash-installer.sh`. That's a hard failure for the CLI
/// but the daemon's auto-update path should treat it as "skip silently"
/// since auto-swap doesn't apply to source builds.
pub async fn check_or_swap(check: bool) -> Result<SwapOutcome> {
    let mut updater = AxoUpdater::new_for("pidash");
    updater
        .load_receipt()
        .context(
            "no install receipt found — `pidash update` only works for binaries installed via `pidash-installer.sh`. Reinstall with the installer or update manually.",
        )?;
    if !updater
        .check_receipt_is_for_this_executable()
        .context("validating cargo-dist install receipt")?
    {
        anyhow::bail!(
            "install receipt does not match this executable — refusing to self-update a binary we don't own"
        );
    }

    let needed = updater
        .is_update_needed()
        .await
        .context("checking GitHub releases for a newer pidash")?;
    if !needed {
        return Ok(SwapOutcome::AlreadyLatest);
    }

    if check {
        let v = updater
            .query_new_version()
            .await
            .context("querying latest pidash version")?
            .map(|v| v.to_string())
            .unwrap_or_else(|| "(unknown)".to_string());
        return Ok(SwapOutcome::UpdateAvailable { new_version: v });
    }

    let result = updater
        .run()
        .await
        .context("running cargo-dist installer to swap binary")?;
    match result {
        Some(r) => Ok(SwapOutcome::Swapped {
            new_version: r.new_version.to_string(),
            old_version: r.old_version.map(|v| v.to_string()),
        }),
        // `run` returned `Ok(None)` means it raced and another process
        // brought us up to date. Treat as already-latest.
        None => Ok(SwapOutcome::AlreadyLatest),
    }
}
