//! OSS-owned serve and worker startup (PIDASHCONV-1541, OVL-03).
//!
//! [`ServeBuilder`](serve::ServeBuilder) and [`WorkerBuilder`](worker::WorkerBuilder)
//! own the full boot sequence the OSS `main.rs` used to spell out step by
//! step — settings resolution, pools, the SeaORM handle, the queue schema,
//! Redis, state/registry composition, and graceful shutdown. The OSS
//! binary's `serve` and `worker` are thin callers of the same code path an
//! extension binary (the cloud overlay) composes with its own settings,
//! seams, routes and jobs, so a new OSS startup step can no longer drift
//! between the two mains.
//!
//! The builders move the `main.rs` sequence verbatim; the only new
//! behaviour is the worker builder's duplicate guard: an extra
//! registration over an already-owned task name is a build error, never
//! the silent replace that once dropped the OSS stale-turn sweep.
//!
//! The helpers below are the `main.rs` boot pieces both builders share,
//! moved unchanged.

pub mod serve;
pub mod worker;

pub use serve::{ServeBuilder, ServeParts};
pub use worker::{BuiltWorker, WorkerBuilder};

use pidash_db::config::{NoOverlay, Profile, Settings};

/// Resolve [`Settings`] the way Django resolves its settings module:
/// `DJANGO_SETTINGS_MODULE`'s last segment picks the [`Profile`]
/// (`test` / `local` / `production`); anything else, including unset,
/// keeps [`Profile::Common`]. `serve` and `worker` boot the same profile
/// as the Django server/worker they run beside, so profile-driven
/// settings — `EMAIL_BACKEND` first among them (PIDASHCONV-1126) — agree
/// instead of Rust always answering with the common (SMTP) resolution.
fn settings_from_env() -> Result<Settings, Box<dyn std::error::Error>> {
    Ok(Settings::from_env_with(
        profile_from_settings_module(),
        &NoOverlay,
    )?)
}

/// Pure mapping behind [`settings_from_env`], split out for unit tests.
fn profile_from_settings_module() -> Profile {
    let module = std::env::var("DJANGO_SETTINGS_MODULE").unwrap_or_default();
    match module.rsplit('.').next().unwrap_or("") {
        "test" => Profile::Test,
        "local" => Profile::Local,
        "production" => Profile::Production,
        _ => Profile::Common,
    }
}

/// Connect the pools `serve` attaches to its state. A missing or
/// unreachable database is a boot error, never a per-request 500: without
/// pools every DB-backed handler answers the generic 500 (PIDASHCONV-126),
/// so serving pool-less is strictly worse than refusing to start (same
/// fail-fast as `worker`).
///
/// `DATABASE_URL` must be a TCP `postgres://` URL; socket-dir URLs
/// (`?host=/tmp`) are rejected by the URL parser at connect time.
async fn connect_serve_pools() -> Result<pidash_db::Pools, Box<dyn std::error::Error>> {
    let db = pidash_db::DbConfig::from_env()?;
    let pools = pidash_db::Pools::connect(&db, None).await?;
    tracing::info!(db = %db.redacted_url(), "connected postgres");
    Ok(pools)
}

/// Seed-data directory for `workspace_seed` (`settings.SEED_DIR`,
/// `apps/api/pi_dash/settings/common.py:743`): `SEED_DIR` env, else the
/// `seeds` checkout dir. Missing files only empty the seed.
fn seed_data_dir() -> std::path::PathBuf {
    std::env::var("SEED_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("seeds"))
}

/// Graceful-shutdown trigger for `serve` and `worker`: SIGINT or SIGTERM.
///
/// `ctrl_c()` alone listens for SIGINT only, but containers stop with
/// SIGTERM — without a SIGTERM arm the drain paths never run on redeploy
/// (PIDASHCONV-1326): as PID 1 the kernel ignores the bare SIGTERM until
/// SIGKILL, otherwise the default action kills the process at once. Celery
/// (warm shutdown) and gunicorn (graceful timeout) both drain on SIGTERM;
/// this future matches them.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler installs");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIDASHCONV-1126: `serve`/`worker` boot the profile matching
    /// `DJANGO_SETTINGS_MODULE`'s last segment, Common when unset or
    /// unknown — the same module Django itself boots from. This is the
    /// only test in the crate touching that variable, and nothing reads
    /// it concurrently, so no lock is needed.
    #[test]
    fn settings_module_selects_profile() {
        let saved = std::env::var("DJANGO_SETTINGS_MODULE").ok();
        for (module, expected) in [
            ("pi_dash.settings.test", Profile::Test),
            ("pi_dash.settings.local", Profile::Local),
            ("pi_dash.settings.production", Profile::Production),
            ("pi_dash.settings.common", Profile::Common),
            ("something.else", Profile::Common),
            ("", Profile::Common),
        ] {
            std::env::set_var("DJANGO_SETTINGS_MODULE", module);
            assert_eq!(profile_from_settings_module(), expected, "module={module}");
        }
        std::env::remove_var("DJANGO_SETTINGS_MODULE");
        assert_eq!(profile_from_settings_module(), Profile::Common);
        if let Some(value) = saved {
            std::env::set_var("DJANGO_SETTINGS_MODULE", value);
        }
    }
}
