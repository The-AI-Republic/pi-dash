mod activity_watchdog;
pub mod drain_journal;
pub mod machine_control;
pub mod observability;
mod run_event_mirror;
pub mod runner_instance;
pub mod runner_out;
pub mod state;
pub mod supervisor;

use anyhow::{Context, Result};

use crate::config::schema::{Config, Credentials};
use crate::util::paths::Paths;

#[derive(Debug, Clone)]
pub struct Options {
    pub offline: bool,
}

/// Run the daemon until shutdown. When `config.toml` changes the daemon's
/// identity under it (sign-in, sign-out, account switch) the supervisor
/// tears itself down and is rebuilt here from the re-read config — same
/// process, so whatever supervises it (service manager or the desktop app)
/// sees no restart.
pub async fn run(config: Config, creds: Credentials, paths: Paths, opts: Options) -> Result<()> {
    let mut config = config;
    loop {
        let exit = supervisor::Supervisor::new(config, creds.clone(), paths.clone(), opts.clone())
            .run()
            .await?;
        match exit {
            supervisor::SupervisorExit::Shutdown => return Ok(()),
            supervisor::SupervisorExit::Reload => {
                config = crate::config::file::load_config(&paths)
                    .context("re-reading config.toml for daemon reload")?;
                config
                    .validate()
                    .context("config.toml failed validation on reload")?;
                tracing::info!(
                    runner_count = config.runners.len(),
                    "daemon reloaded from config.toml"
                );
            }
        }
    }
}
