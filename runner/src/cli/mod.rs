use anyhow::Result;
use clap::{Parser, Subcommand};

mod comment;
pub mod configure;
pub mod doctor;
mod install;
mod issue;
mod remove;
pub mod resolve;
mod restart;
mod rotate;
mod run;
mod start;
mod state;
mod status;
mod stop;
mod token;
mod tui;
mod uninstall;
mod workspace;

/// Re-exported for integration tests that want to exercise the daemon-entry
/// error paths without routing through clap.
pub use run::Args as RunArgs;

/// Test-only shim: invoke the hidden `__run` handler directly. Exposed so
/// `tests/pidash_run_errors.rs` can assert the error-message contract when
/// `config.toml` or `credentials.toml` is missing.
#[doc(hidden)]
pub async fn run_for_tests(args: RunArgs, paths: &crate::util::paths::Paths) -> anyhow::Result<()> {
    run::run(args, paths).await
}

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
    #[arg(long, global = true, env = "PIDASH_LOG", default_value = "info")]
    pub log: String,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Register with Pi Dash cloud, edit config fields, or open the Config
    /// tab of `pidash tui`. Aliased as `config` / `c`.
    #[command(alias = "config", alias = "c")]
    Configure(configure::Args),

    /// Install the OS service (systemd user unit / launchd agent).
    Install(install::Args),

    /// Uninstall the OS service unit.
    Uninstall(uninstall::Args),

    /// Start the installed service.
    Start(start::Args),

    /// Stop the installed service.
    Stop(stop::Args),

    /// Restart the installed service.
    Restart(restart::Args),

    /// Print service + daemon status.
    Status(status::Args),

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

    /// Manage the machine token (a.k.a. connection) that authenticates
    /// the daemon's WebSocket. Subcommands: `install`, `show`.
    Token(token::TokenArgs),

    /// Internal: run the daemon in the foreground. Invoked by systemd/launchd
    /// via the generated unit file. Not a user-facing verb.
    #[command(name = "__run", hide = true)]
    Run(run::Args),
}

pub async fn run(cli: Cli) -> Result<()> {
    crate::util::logging::init(&cli.log)?;
    let paths = crate::util::paths::Paths::resolve(cli.config_dir.clone(), cli.data_dir.clone())?;
    tracing::debug!(?paths, "resolved runner paths");

    match cli.command {
        Command::Configure(args) => configure::run(args, &paths).await,
        Command::Install(args) => install::run(args, &paths).await,
        Command::Uninstall(args) => uninstall::run(args, &paths).await,
        Command::Start(args) => start::run(args, &paths).await,
        Command::Stop(args) => stop::run(args, &paths).await,
        Command::Restart(args) => restart::run(args, &paths).await,
        Command::Status(args) => status::run(args, &paths).await,
        Command::Tui(args) => tui::run(args, &paths).await,
        Command::Doctor(args) => doctor::run(args, &paths).await,
        Command::Remove(args) => remove::run(args, &paths).await,
        Command::Rotate(args) => rotate::run(args, &paths).await,
        Command::Issue(args) => run_crud(issue::run(args).await),
        Command::Comment(args) => run_crud(comment::run(args).await),
        Command::State(args) => run_crud(state::run(args).await),
        Command::Workspace(args) => run_crud(workspace::run(args).await),
        Command::Token(args) => token::run(args, &paths).await,
        Command::Run(args) => run::run(args, &paths).await,
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
