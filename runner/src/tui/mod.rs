pub mod app;
pub mod ipc_client;
pub mod onboarding;
pub mod views;

use anyhow::Result;

use crate::util::paths::Paths;

pub async fn run(paths: Paths, no_onboarding: bool) -> Result<()> {
    let needs_onboarding = !no_onboarding && !paths.config_path().exists();
    if needs_onboarding {
        onboarding::run(paths.clone()).await?;
        // If the user aborted without registering, return; otherwise fall
        // through to the dashboard.
        if !paths.config_path().exists() {
            return Ok(());
        }
    }
    app::run(paths).await
}
