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

pub async fn run(config: Config, creds: Credentials, paths: Paths, opts: Options) -> Result<()> {
    supervisor::Supervisor::new(config, creds, paths, opts)
        .run()
        .await
}
