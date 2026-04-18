use anyhow::Result;
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Run without opening the cloud WS (local-only mode for debugging).
    #[arg(long)]
    pub offline: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let (config, creds) = crate::config::file::load_all(paths)?;
    tracing::info!(
        runner = %config.runner.name,
        runner_id = %creds.runner_id,
        "starting daemon"
    );
    let opts = crate::daemon::Options {
        offline: args.offline,
    };
    crate::daemon::run(config, creds, paths.clone(), opts).await
}
