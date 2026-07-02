use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

mod ai;
pub mod auth;
mod comment;
pub mod config_cmd;
pub mod connect;
pub mod context;
pub mod doctor;
mod install;
mod issue;
mod project;
mod remove;
pub mod resolve;
mod restart;
mod run;
// `runner` is `pub` so the TUI can call its library functions
// (`add`, `remove`) directly without going through clap.
pub mod runner;
pub mod runner_ops;
mod start;
mod state;
mod status;
mod stop;
mod tui;
mod uninstall;
pub mod update;
mod workdir;
mod workpad;
mod workspace;

/// Re-exported for integration tests that want to exercise the daemon-entry
/// error paths without routing through clap.
pub use run::Args as RunArgs;

#[doc(hidden)]
pub async fn run_for_tests(args: RunArgs, paths: &crate::util::paths::Paths) -> anyhow::Result<()> {
    run::run(args, paths).await
}

#[derive(Debug, Parser)]
#[command(name = "pidash", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

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
    /// Authenticate this host as a user (`auth login` / `status` /
    /// `logout`). Mints a CLI token used by `pidash` commands and by
    /// `pidash runner add` to register runners.
    Auth(auth::AuthArgs),

    /// Deprecated compatibility path for one-time enrollment tokens.
    #[command(hide = true)]
    Connect(connect::Args),

    /// Manage local CLI configuration.
    Config(config_cmd::ConfigArgs),

    /// Manage runners under the active connection (add / list / remove).
    Runner(runner::RunnerArgs),

    /// Manage shared work directories (worktree pools): `workdir add` /
    /// `list` / `remove`. Lets multiple runners share one repo checkout.
    Workdir(workdir::WorkdirArgs),

    /// Read Pi Dash projects in the active workspace.
    Project(project::ProjectArgs),

    /// Initialize or inspect local Pi Dash workspace context.
    Context(context::ContextArgs),

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

    /// Swap the on-disk `pidash` binary for the latest GitHub release.
    /// The running daemon keeps its loaded copy; pass `--restart` to
    /// also restart so the new code takes effect immediately.
    Update(update::Args),

    /// Deregister with the cloud and delete local credentials.
    Remove(remove::Args),

    /// Send a command to the connected Pi Dash cloud AI assistant and print
    /// its reply. BYOK: requires an LLM provider + key configured in Pi Dash.
    Ai(ai::AiArgs),

    /// Read or update a Pi Dash work item (fetch, change state, edit fields).
    Issue(issue::IssueArgs),

    /// List, post, or edit work-item comments.
    Comment(comment::CommentArgs),

    /// Inspect workflow states on a project.
    State(state::StateArgs),

    /// Read or overwrite the coding agent's per-issue workpad.
    Workpad(workpad::WorkpadArgs),

    /// Verify the CLI's Pi Dash credentials end-to-end.
    Workspace(workspace::WorkspaceArgs),

    /// Internal: run the daemon in the foreground. Invoked by systemd/launchd
    /// via the generated unit file. Not a user-facing verb.
    #[command(name = "__run", hide = true)]
    Run(run::Args),
}

pub async fn run(cli: Cli) -> Result<()> {
    crate::util::logging::init(&cli.log)?;
    let paths = crate::util::paths::Paths::resolve(cli.config_dir.clone(), cli.data_dir.clone())?;
    tracing::debug!(?paths, "resolved runner paths");

    let Some(command) = cli.command else {
        return run_default(&paths).await;
    };

    match command {
        Command::Auth(args) => auth::run(args, &paths).await,
        Command::Connect(args) => connect::run(args, &paths).await,
        Command::Config(args) => config_cmd::run(args, &paths).await,
        Command::Runner(args) => runner::run(args, &paths).await,
        Command::Workdir(args) => workdir::run(args, &paths).await,
        Command::Project(args) => run_crud(project::run(args, &paths).await),
        Command::Context(args) => run_crud(context::run(args, &paths).await),
        Command::Install(args) => install::run(args, &paths).await,
        Command::Uninstall(args) => uninstall::run(args, &paths).await,
        Command::Start(args) => start::run(args, &paths).await,
        Command::Stop(args) => stop::run(args, &paths).await,
        Command::Restart(args) => restart::run(args, &paths).await,
        Command::Status(args) => status::run(args, &paths).await,
        Command::Tui(args) => tui::run(args, &paths).await,
        Command::Doctor(args) => doctor::run(args, &paths).await,
        Command::Update(args) => update::run(args, &paths).await,
        Command::Remove(args) => remove::run(args, &paths).await,
        Command::Ai(args) => ai::run(args, &paths).await,
        Command::Issue(args) => run_crud(issue::run(args, &paths).await),
        Command::Comment(args) => run_crud(comment::run(args, &paths).await),
        Command::State(args) => run_crud(state::run(args, &paths).await),
        Command::Workpad(args) => run_crud(workpad::run(args, &paths).await),
        Command::Workspace(args) => run_crud(workspace::run(args, &paths).await),
        Command::Run(args) => run::run(args, &paths).await,
    }
}

/// No-subcommand entrypoint. Acts as a first-run launcher: if this
/// host has no config yet, drop straight into `pidash auth login` so
/// the user lands in the device-code flow immediately after the
/// install one-liner. If config already exists, fall through to
/// clap's help so the bare command behaves like every other CLI.
async fn run_default(paths: &crate::util::paths::Paths) -> Result<()> {
    if !paths.config_path().exists() {
        let args = auth::AuthArgs {
            command: auth::AuthCommand::Login(auth::login::Args {
                url: None,
                no_browser: false,
                no_runner_prompt: false,
                workspace: None,
            }),
        };
        return auth::run(args, paths).await;
    }
    Cli::command().print_help()?;
    println!();
    Ok(())
}

fn run_crud(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        std::process::exit(code);
    }
}
