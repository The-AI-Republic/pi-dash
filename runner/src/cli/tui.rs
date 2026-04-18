use anyhow::Result;
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Skip splash / onboarding even if no config exists.
    #[arg(long)]
    pub no_onboarding: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    crate::tui::run(paths.clone(), args.no_onboarding).await
}
