//! `pidash uninstall` — stop the service and delete the unit file.
//!
//! PR 1 scope: equivalent to the old `pidash service uninstall`. The extended
//! `pidash remove` cleanup (which also calls uninstall) is wired in PR 2.

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {}

pub async fn run(_args: Args, paths: &Paths) -> Result<()> {
    let svc = crate::service::detect();
    svc.stop().await.ok();
    svc.uninstall(paths).await
}
