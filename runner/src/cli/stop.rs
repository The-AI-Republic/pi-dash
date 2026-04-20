//! `pidash stop` — stop the installed service.

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {}

pub async fn run(_args: Args, _paths: &Paths) -> Result<()> {
    crate::service::detect().stop().await
}
