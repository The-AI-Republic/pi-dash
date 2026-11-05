//! Install this release's engine at daemon start when it is missing.
//!
//! `pidash update` installs the new release's engine before it swaps
//! `pidash`, but that step lives in the binary doing the update. A `pidash`
//! released before the engine shipped has no such step, so updating it in
//! place leaves the new binary without an engine. The daemon closes that gap:
//! when the engine is not installed and this `pidash` was installed by the
//! cargo-dist installer (it has an install receipt for this executable), it
//! downloads and installs the engine in the background. A failure is logged
//! and retried at the next daemon start; runners driving the user's own agent
//! are never affected.
//!
//! Installs without a receipt (MSI, which places the engine itself; source
//! builds; extracted archives) are left alone.

use crate::RUNNER_VERSION;
use crate::config::schema::EngineSection;

/// Whether this executable is the one a cargo-dist install receipt is for.
fn is_updater_managed() -> bool {
    let mut updater = axoupdater::AxoUpdater::new_for("pidash");
    if updater.load_receipt().is_err() {
        return false;
    }
    updater
        .check_receipt_is_for_this_executable()
        .unwrap_or(false)
}

/// Install the engine of this release if the install is updater-managed,
/// then resolve it so this daemon can use it without a restart.
pub async fn install_missing(config: EngineSection) {
    let managed = tokio::task::spawn_blocking(is_updater_managed)
        .await
        .unwrap_or(false);
    if !managed {
        tracing::info!(
            "built-in engine not installed and this pidash has no install receipt; run `pidash engine install`"
        );
        return;
    }
    let root = match super::install::install_root_for_current_exe() {
        Ok(root) => root,
        Err(err) => {
            tracing::warn!(error = %format!("{err:#}"), "cannot locate the engine directory");
            return;
        }
    };
    tracing::info!(root = %root.display(), "installing the missing built-in engine");
    match super::install::install_release(RUNNER_VERSION, &root).await {
        Ok(outcome) => {
            tracing::info!(
                installed = ?outcome.installed,
                already_present = ?outcome.already_present,
                "built-in engine installed"
            );
            let _ = super::locate::init_at_daemon_start(&config);
        }
        Err(err) => {
            tracing::warn!(
                error = %format!("{err:#}"),
                "installing the built-in engine failed; it is retried at the next daemon start, or run `pidash engine install`"
            );
        }
    }
}
