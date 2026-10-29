mod activity_watchdog;
pub mod drain_journal;
pub mod machine_control;
pub mod observability;
mod run_event_mirror;
pub mod runner_instance;
pub mod runner_out;
pub mod state;
pub mod supervisor;

use anyhow::Result;

use crate::config::schema::{Config, Credentials};
use crate::util::paths::Paths;

#[derive(Debug, Clone)]
pub struct Options {
    pub offline: bool,
}

/// Run the daemon until shutdown. When `config.toml` changes the daemon's
/// identity under it (sign-in, sign-out, account switch) the supervisor
/// tears itself down and is rebuilt here from the new config — same
/// process, so whatever supervises it (service manager or the desktop app)
/// sees no restart.
///
/// Only the startup config can fail the daemon: the supervisor hands back a
/// reload config only after it parsed and validated, and an invalid edit is
/// logged and ignored while the daemon keeps running on the last good one.
/// Exiting on it would make launchd / systemd restart the daemon into the
/// same broken file over and over.
pub async fn run(config: Config, creds: Credentials, paths: Paths, opts: Options) -> Result<()> {
    let mut config = config;
    loop {
        let exit = supervisor::Supervisor::new(config, creds.clone(), paths.clone(), opts.clone())
            .run()
            .await?;
        match exit {
            supervisor::SupervisorExit::Shutdown => return Ok(()),
            supervisor::SupervisorExit::Reload(next) => {
                config = *next;
                tracing::info!(
                    runner_count = config.runners.len(),
                    "daemon reloaded from config.toml"
                );
            }
        }
    }
}
