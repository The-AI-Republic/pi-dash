//! `pidash restart` — restart the installed service and verify daemon health.

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {}

pub async fn run(_args: Args, paths: &Paths) -> Result<()> {
    println!("restarting daemon...");
    let outcome = crate::service::reload::restart_and_verify(paths).await;
    if outcome.ok {
        println!("daemon restarted ({}).", outcome.summary);
        return Ok(());
    }
    anyhow::bail!(
        "daemon restart did not complete cleanly: {}\n{}",
        outcome.summary,
        outcome.detail.unwrap_or_default()
    )
}
