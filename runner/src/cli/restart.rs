//! `pidash restart` — stop then start the installed service.

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {}

pub async fn run(_args: Args, _paths: &Paths) -> Result<()> {
    let svc = crate::service::detect();
    // Tolerant: stop is a no-op if the service isn't currently running.
    svc.stop().await.ok();
    svc.start().await
}
