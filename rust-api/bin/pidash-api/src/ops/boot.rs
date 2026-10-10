//! Boot-group ops commands: `wait_for_db`, `wait_for_migrations`,
//! `clear_cache` (PIDASHCONV-806).
//!
//! Thin clap/env wrappers: argument shapes live here, the flows live in
//! `pidash-services` (`ops::boot`), and stdout stays byte-identical.

use super::{OpsFailure, WAIT_FOR_DB_SLEEP, WAIT_FOR_MIGRATIONS_SLEEP};
use pidash_services::ops::boot;

/// `clear_cache.py:14-16`: `--key`, `nargs="?"`, no const — so a bare
/// `--key` behaves like an absent one (argparse yields `None` either
/// way, and the empty string is falsy in the `options["key"]` check).
#[derive(Debug, Clone, clap::Args)]
pub struct ClearCacheArgs {
    /// Key to clear cache
    #[arg(long, num_args(0..=1), default_missing_value(""))]
    pub key: Option<String>,
}

fn print_line(line: &str) {
    println!("{line}");
}

fn database_url() -> Result<String, OpsFailure> {
    // Without `DATABASE_URL` Django cannot even import its settings
    // (traceback, exit 1); failing fast with one stderr line matches the
    // exit code without the traceback.
    std::env::var("DATABASE_URL").map_err(|_| OpsFailure("DATABASE_URL is not set".to_string()))
}

pub async fn run_wait_for_db() -> Result<(), OpsFailure> {
    let url = database_url()?;
    let mut out = print_line;
    boot::run_wait_for_db(&url, &mut out, WAIT_FOR_DB_SLEEP, None)
        .await
        .map_err(|error| OpsFailure(error.to_string()))?;
    Ok(())
}

pub async fn run_wait_for_migrations() -> Result<(), OpsFailure> {
    let url = database_url()?;
    let mut out = print_line;
    boot::run_wait_for_migrations(&url, &mut out, WAIT_FOR_MIGRATIONS_SLEEP, None)
        .await
        .map_err(|error| OpsFailure(error.to_string()))?;
    Ok(())
}

pub async fn run_clear_cache(key: Option<&str>) -> Result<(), OpsFailure> {
    // `REDIS_URL` is env-infra (`config/registry.py`); unset or empty
    // reaches the same failure line Django's `LOCATION=None` produces.
    let url = std::env::var("REDIS_URL").ok();
    print_line(&boot::clear_cache(url.as_deref(), key).await);
    Ok(())
}
