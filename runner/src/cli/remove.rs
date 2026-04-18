use anyhow::Result;
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Removal token issued by the cloud UI (optional; if missing we only clean local state).
    #[arg(long)]
    pub token: Option<String>,

    /// Delete local state without contacting the cloud.
    #[arg(long)]
    pub local_only: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let (config, creds) = match crate::config::file::load_all(paths) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("no local configuration found: {e}");
            return Ok(());
        }
    };
    if !args.local_only {
        match crate::cloud::register::deregister(
            &config.runner.cloud_url,
            &creds.runner_id,
            &creds.runner_secret,
            args.token.as_deref(),
        )
        .await
        {
            Ok(()) => tracing::info!("cloud deregistration acknowledged"),
            Err(e) => tracing::warn!("cloud deregistration failed: {e:#}"),
        }
    }
    crate::config::file::remove_all(paths)?;
    println!("local runner state removed.");
    Ok(())
}
