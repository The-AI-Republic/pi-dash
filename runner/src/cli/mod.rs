use anyhow::Result;
use clap::{Parser, Subcommand};

mod configure;
pub mod doctor;
mod remove;
mod rotate;
mod service;
mod start;
mod status;
mod tui;

#[derive(Debug, Parser)]
#[command(name = "pi-dash-runner", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Override config directory (XDG config by default).
    #[arg(long, global = true, env = "APPLE_PI_RUNNER_CONFIG_DIR")]
    pub config_dir: Option<std::path::PathBuf>,

    /// Override data directory (XDG data by default).
    #[arg(long, global = true, env = "APPLE_PI_RUNNER_DATA_DIR")]
    pub data_dir: Option<std::path::PathBuf>,

    /// Log level filter (trace|debug|info|warn|error).
    #[arg(
        long,
        global = true,
        env = "APPLE_PI_RUNNER_LOG",
        default_value = "info"
    )]
    pub log: String,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Register with Pi Dash cloud using a one-time token.
    Configure(configure::Args),

    /// Run the daemon in the foreground.
    Start(start::Args),

    /// Print daemon status (queried via local IPC).
    Status(status::Args),

    /// Install, start, stop, or report the OS service.
    #[command(subcommand)]
    Service(service::ServiceCmd),

    /// Attach an interactive TUI to the running daemon.
    Tui(tui::Args),

    /// Run preflight checks (Codex installed, logged in; git configured; cloud reachable).
    Doctor(doctor::Args),

    /// Deregister with the cloud and delete local credentials.
    Remove(remove::Args),

    /// Rotate this runner's credential. The old one is invalidated.
    Rotate(rotate::Args),
}

pub async fn run(cli: Cli) -> Result<()> {
    crate::util::logging::init(&cli.log)?;
    let paths = crate::util::paths::Paths::resolve(cli.config_dir.clone(), cli.data_dir.clone())?;
    tracing::debug!(?paths, "resolved runner paths");

    match cli.command {
        Command::Configure(args) => configure::run(args, &paths).await,
        Command::Start(args) => start::run(args, &paths).await,
        Command::Status(args) => status::run(args, &paths).await,
        Command::Service(cmd) => service::run(cmd, &paths).await,
        Command::Tui(args) => tui::run(args, &paths).await,
        Command::Doctor(args) => doctor::run(args, &paths).await,
        Command::Remove(args) => remove::run(args, &paths).await,
        Command::Rotate(args) => rotate::run(args, &paths).await,
    }
}
