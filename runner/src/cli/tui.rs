use anyhow::Result;
use clap::Args as ClapArgs;

use crate::tui::app::Tab;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Deprecated: onboarding is now inline on the Config tab and there is
    /// no separate splash screen to skip. Retained as a no-op so existing
    /// scripts and systemd units don't break.
    #[arg(long, hide = true)]
    pub no_onboarding: bool,

    /// Open directly into the given tab. Accepts the canonical name
    /// (`runner`, `config`, `runs`, `approvals`) or a 1-based index (`1`–`4`).
    /// Defaults to `runner`.
    #[arg(long, value_name = "TAB")]
    pub tab: Option<String>,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let initial_tab = match args.tab.as_deref() {
        None => Tab::RunnerStatus,
        Some(raw) => Tab::parse_cli(raw).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown --tab value {raw:?}; expected one of: \
                 runner (1), config (2), runs (3), approvals (4)"
            )
        })?,
    };
    crate::tui::run(paths.clone(), args.no_onboarding, initial_tab).await
}
