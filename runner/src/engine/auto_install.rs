//! Install this release's engine at daemon start when it is missing.
//!
//! `pidash update` installs the new release's engine before it swaps
//! `pidash`, but that step lives in the binary doing the update. A `pidash`
//! released before the engine shipped has no such step, so updating it in
//! place leaves the new binary without an engine. The daemon closes that gap:
//! when the engine is not installed and this `pidash` was installed by the
//! cargo-dist installer (it has an install receipt for this executable), it
//! downloads and installs the engine in the background. A failure is logged
//! and retried with backoff for as long as the daemon runs (a daemon can run
//! for weeks, so a one-shot attempt at start would leave a flaky network or a
//! racing `pidash engine install` to strand it); runners driving the user's
//! own agent are never affected.
//!
//! Installs without a receipt (MSI, which places the engine itself; source
//! builds; extracted archives) are left alone.

use std::future::Future;
use std::time::Duration;

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

/// First retry delay after a failed install; doubles up to [`MAX_RETRY_DELAY`].
const FIRST_RETRY_DELAY: Duration = Duration::from_secs(60);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(60 * 60);

/// Delay before retry number `attempt` (0-based): 1m, 2m, 4m, … capped at 1h.
fn retry_delay(attempt: u32) -> Duration {
    FIRST_RETRY_DELAY
        .saturating_mul(2u32.saturating_pow(attempt.min(16)))
        .min(MAX_RETRY_DELAY)
}

/// Install the engine of this release if the install is updater-managed,
/// then resolve it so this daemon can use it without a restart. Retries with
/// backoff until the engine resolves.
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
    install_until_resolved(
        || {
            let root = root.clone();
            async move {
                let outcome = super::install::install_release(RUNNER_VERSION, &root).await?;
                tracing::info!(
                    installed = ?outcome.installed,
                    already_present = ?outcome.already_present,
                    "built-in engine installed"
                );
                Ok(())
            }
        },
        || super::locate::init_at_daemon_start(&config).is_ok(),
        tokio::time::sleep,
    )
    .await;
}

/// Run `install` until `resolve` succeeds. `resolve` is checked before every
/// attempt too, so an engine another process installed meanwhile (`pidash
/// engine install`, `pidash update`) is picked up without downloading again.
async fn install_until_resolved<I, IF, R, S, SF>(
    mut install: I,
    mut resolve: R,
    mut sleep: S,
) -> u32
where
    I: FnMut() -> IF,
    IF: Future<Output = anyhow::Result<()>>,
    R: FnMut() -> bool,
    S: FnMut(Duration) -> SF,
    SF: Future<Output = ()>,
{
    let mut attempt = 0u32;
    loop {
        if attempt > 0 && resolve() {
            return attempt;
        }
        match install().await {
            Ok(()) if resolve() => return attempt + 1,
            Ok(()) => {
                tracing::warn!("the built-in engine was installed but does not resolve; retrying")
            }
            Err(err) => tracing::warn!(
                error = %format!("{err:#}"),
                "installing the built-in engine failed; retrying, or run `pidash engine install`"
            ),
        }
        let delay = retry_delay(attempt);
        tracing::info!(
            retry_in_secs = delay.as_secs(),
            "built-in engine install retry scheduled"
        );
        sleep(delay).await;
        attempt = attempt.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn retry_delay_backs_off_to_an_hour() {
        assert_eq!(retry_delay(0), Duration::from_secs(60));
        assert_eq!(retry_delay(1), Duration::from_secs(120));
        assert_eq!(retry_delay(5), Duration::from_secs(1920));
        assert_eq!(retry_delay(6), MAX_RETRY_DELAY);
        assert_eq!(retry_delay(u32::MAX), MAX_RETRY_DELAY);
    }

    #[tokio::test]
    async fn a_failed_install_is_retried_until_it_resolves() {
        let installs = Cell::new(0u32);
        let installed = Cell::new(false);
        let slept = std::cell::RefCell::new(Vec::new());
        let attempts = install_until_resolved(
            || {
                installs.set(installs.get() + 1);
                let ok = installs.get() >= 3;
                installed.set(ok);
                async move {
                    if ok {
                        Ok(())
                    } else {
                        anyhow::bail!("network down")
                    }
                }
            },
            || installed.get(),
            |d| {
                slept.borrow_mut().push(d);
                async {}
            },
        )
        .await;
        assert_eq!(installs.get(), 3);
        assert_eq!(attempts, 3);
        assert_eq!(*slept.borrow(), vec![retry_delay(0), retry_delay(1)]);
    }

    #[tokio::test]
    async fn an_engine_installed_by_another_process_stops_the_retries() {
        let installs = Cell::new(0u32);
        let resolves = Cell::new(0u32);
        install_until_resolved(
            || {
                installs.set(installs.get() + 1);
                async { anyhow::bail!("lost the rename race") }
            },
            || {
                resolves.set(resolves.get() + 1);
                // The failed attempt's pre-retry check finds the other
                // process's install.
                resolves.get() >= 1
            },
            |_| async {},
        )
        .await;
        assert_eq!(installs.get(), 1, "no second download once it resolves");
    }
}
