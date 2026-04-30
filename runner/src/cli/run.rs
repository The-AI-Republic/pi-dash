//! Hidden `__run` subcommand: entry point that systemd / launchd exec.
//!
//! Not a user-facing verb. Users drive the daemon through service-lifecycle
//! verbs (`pidash install`, `start`, `stop`, `restart`, `status`). This handler
//! is what the generated unit files call via `ExecStart={exe} __run` (systemd)
//! and `<array><string>{exe}</string><string>__run</string></array>` (launchd).
//!
//! The body is the old `pidash start` foreground flow: load config + creds,
//! run the supervisor loop, block until shutdown.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Run without opening the cloud WS (local-only mode for debugging).
    #[arg(long)]
    pub offline: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let config_path = paths.config_path();
    let creds_path = paths.credentials_path();
    if !config_path.exists() {
        anyhow::bail!(
            "no config.toml at {config_path:?}. \
             Run `pidash connect --url <URL> --token <ONE_TIME_TOKEN>` first."
        );
    }
    if !creds_path.exists() {
        anyhow::bail!(
            "no credentials.toml at {creds_path:?}. \
             Run `pidash connect --url <URL> --token <ONE_TIME_TOKEN>` to enroll."
        );
    }

    let (config, creds) = crate::config::file::load_all(paths).context(
        "failed to load runner config; re-run `pidash connect` if the files are corrupt",
    )?;
    config
        .validate()
        .context("config.toml failed validation; refusing to start the daemon")?;
    let primary_name = config
        .primary_runner()
        .map(|r| r.name.as_str())
        .unwrap_or("(no runners)");
    tracing::info!(
        runner = %primary_name,
        connection_id = %creds.connection_id,
        runner_count = config.runners.len(),
        "starting daemon"
    );
    let opts = crate::daemon::Options {
        offline: args.offline,
    };
    crate::daemon::run(config, creds, paths.clone(), opts).await
}
