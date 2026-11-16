//! `pidash restart` — restart the installed service and verify daemon health.

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {}

pub async fn run(_args: Args, paths: &Paths) -> Result<()> {
    println!("restarting daemon...");
    let outcome =
        crate::service::reload::restart_and_verify_with_progress(paths, |msg| eprintln!("{msg}"))
            .await;
    if outcome.ok {
        match &outcome.warnings {
            // Degraded success: the daemon restarted and reached the cloud,
            // but some runners didn't open their session in time. Exit 0 —
            // non-zero is reserved for genuine daemon failures so scripts
            // that only care about the daemon keep working (PDASHOSS01-222).
            Some(warnings) => {
                println!("{}:", outcome.summary);
                println!("{warnings}");
            }
            None => println!("daemon restarted ({}).", outcome.summary),
        }
        return Ok(());
    }
    anyhow::bail!(
        "daemon restart did not complete cleanly: {}\n{}",
        outcome.summary,
        outcome.detail.unwrap_or_default()
    )
}
