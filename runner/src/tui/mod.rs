pub mod app;
pub mod ipc_client;
pub mod views;
pub mod widgets;

use anyhow::Result;

use crate::util::paths::Paths;

pub async fn run(paths: Paths, no_onboarding: bool, initial_tab: app::Tab) -> Result<()> {
    // `no_onboarding` is kept as a CLI arg for backward compatibility but
    // is now a no-op: the Runners tab renders the inline registration
    // form when `config.toml` is missing, so there's no separate wizard
    // screen to skip. Route straight to the main app either way.
    let _ = no_onboarding;
    // On a fresh machine without config, send the user straight to the
    // Runners tab so the inline register form is the first thing they
    // see, regardless of which tab they asked for.
    let initial_tab = if paths.config_path().exists() {
        initial_tab
    } else {
        app::Tab::RunnerStatus
    };
    app::run(paths, initial_tab).await
}
