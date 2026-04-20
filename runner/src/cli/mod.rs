use anyhow::Result;
use clap::{Parser, Subcommand};

mod comment;
mod configure;
pub mod doctor;
mod issue;
mod remove;
pub mod resolve;
mod rotate;
mod service;
mod start;
mod state;
mod status;
mod tui;
mod workspace;

#[derive(Debug, Parser)]
#[command(name = "pidash", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Override config directory (XDG config by default).
    #[arg(long, global = true, env = "PIDASH_CONFIG_DIR")]
    pub config_dir: Option<std::path::PathBuf>,

    /// Override data directory (XDG data by default).
    #[arg(long, global = true, env = "PIDASH_DATA_DIR")]
    pub data_dir: Option<std::path::PathBuf>,

    /// Log level filter (trace|debug|info|warn|error).
    #[arg(
        long,
        global = true,
        env = "PIDASH_LOG",
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

    /// Read or update a Pi Dash work item (fetch, change state, edit fields).
    Issue(issue::IssueArgs),

    /// List, post, or edit work-item comments.
    Comment(comment::CommentArgs),

    /// Inspect workflow states on a project.
    State(state::StateArgs),

    /// Verify the CLI's Pi Dash credentials end-to-end.
    Workspace(workspace::WorkspaceArgs),
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
        Command::Issue(args) => run_crud(issue::run(args).await),
        Command::Comment(args) => run_crud(comment::run(args).await),
        Command::State(args) => run_crud(state::run(args).await),
        Command::Workspace(args) => run_crud(workspace::run(args).await),
    }
}

/// Translate a CRUD subcommand's numeric exit code into the `Result<()>`
/// contract the outer `run` uses.
///
/// The CLI subcommands already emit their own JSON error payload on stderr
/// before returning a non-zero code, so we `process::exit` here to preserve
/// that code end-to-end without letting `anyhow` re-print or collapse it.
fn run_crud(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        std::process::exit(code);
    }
}
