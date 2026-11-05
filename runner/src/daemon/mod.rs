mod activity_watchdog;
pub mod drain_journal;
pub mod engine_ops;
pub(crate) mod engine_reconcile;
pub mod machine_control;
pub mod observability;
pub mod reconnect_triggers;
mod run_event_mirror;
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
    // Resolve the built-in engine once and keep that path for the life of
    // this process (`crate::engine::locate::resolved`), so a `pidash update`
    // that installs a newer engine never swaps the binary under a run.
    crate::engine::locate::init_at_daemon_start(&config.engine);
    supervisor::Supervisor::new(config, creds, paths, opts)
        .run()
        .await
}
