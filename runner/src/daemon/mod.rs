mod activity_watchdog;
pub mod drain_journal;
pub mod execution;
pub mod machine_control;
pub mod observability;
pub mod pidash_agent_ops;
pub(crate) mod pidash_agent_reconcile;
pub mod process_guard;
pub mod receipt;
pub mod reconnect_triggers;
mod run_event_mirror;
pub mod run_queue;
pub mod run_queue_store;
pub mod runner_instance;
pub mod runner_out;
pub mod state;
pub mod supervisor;
pub(crate) mod workspace_tokens;

use anyhow::Result;

use crate::config::schema::{Config, Credentials};
use crate::util::paths::Paths;

#[derive(Debug, Clone)]
pub struct Options {
    pub offline: bool,
}

pub async fn run(config: Config, creds: Credentials, paths: Paths, opts: Options) -> Result<()> {
    // Keep the first successfully resolved Pi Dash Agent path for the life of
    // this process (`crate::pidash_agent::locate::resolved`), so a `pidash update`
    // that installs a newer engine never swaps the binary under a run.
    if let Err(crate::pidash_agent::locate::LocateError::NotInstalled { .. }) =
        crate::pidash_agent::locate::init_at_daemon_start(&config.engine)
    {
        // An install updated in place from a `pidash` that predates the
        // engine has none yet. Fetch it in the background; never fatal.
        tokio::spawn(crate::pidash_agent::auto_install::install_missing(
            config.engine.clone(),
        ));
    }
    // Desktop/package-manager installs have no cargo-dist receipt. They
    // still need discovery when the engine is installed after service start.
    let discovery = tokio::spawn(crate::pidash_agent::locate::discover_missing());
    let result = supervisor::Supervisor::new(config, creds, paths, opts)
        .run()
        .await;
    discovery.abort();
    result
}
