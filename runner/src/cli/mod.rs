use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

mod agent_install;
mod ai;
pub mod auth;
mod comment;
pub mod config_cmd;
pub mod connect;
pub mod context;
pub mod docs;
pub mod doctor;
mod install;
pub mod issue;
pub mod managed;
pub mod pidash_agent_cmd;
// `pub` so the CLI contract tests can drive the label subcommands directly.
pub mod label;
// `pub` so the CLI contract tests can drive the page subcommands directly.
pub mod page;
mod project;
mod reconnect;
mod remove;
pub mod resolve;
mod restart;
mod run;
pub mod run_cmd;
// `runner` is `pub` so the TUI can call its library functions
// (`add`, `remove`) directly without going through clap.
pub mod runner;
pub mod runner_ops;
mod scheduler;
mod start;
pub mod state;
mod status;
mod stop;
mod tui;
mod uninstall;
pub mod update;
mod workpad;
mod workspace;

/// Re-exported for integration tests that want to exercise the daemon-entry
/// error paths without routing through clap.
pub use run::Args as RunArgs;

/// Whether a top-level `pidash` subcommand may run inside an agent run
/// (NEWPIDASH-320 §6.5). Agent commands are the run's own tools (issue,
/// comment, workpad, …); everything that manages the runner, its service
/// or the operator's login is refused in-run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClass {
    Agent,
    Operator,
}

use CommandClass::{Agent, Operator};

/// Every top-level subcommand and its class, keyed by the clap command
/// name. The single source of truth for the in-run refusal and the
/// filtered `--help`. `command_name`'s exhaustive match fails to compile
/// on a new variant, and `every_subcommand_is_classified` fails on a
/// name missing here.
const COMMAND_CLASSES: &[(&str, CommandClass)] = &[
    ("login", Operator),
    ("auth", Operator),
    ("connect", Operator),
    ("config", Operator),
    ("runner", Operator),
    ("project", Agent),
    ("context", Agent),
    ("install", Operator),
    ("uninstall", Operator),
    ("start", Operator),
    ("stop", Operator),
    ("restart", Operator),
    ("reconnect", Operator),
    ("status", Operator),
    ("tui", Operator),
    ("doctor", Operator),
    ("update", Operator),
    ("engine", Operator),
    ("remove", Operator),
    ("ai", Operator),
    ("issue", Agent),
    ("label", Agent),
    ("comment", Agent),
    ("page", Agent),
    ("state", Agent),
    ("scheduler", Agent),
    ("workpad", Agent),
    ("workspace", Operator),
    ("run", Agent),
    ("docs", Agent),
    ("__run", Operator),
    ("__managed", Operator),
    ("__engine", Operator),
];

/// The clap command name for a parsed subcommand. Exhaustive on purpose:
/// adding a variant without a name here is a compile error, which forces
/// the author to also pick its [`CommandClass`] in [`COMMAND_CLASSES`].
fn command_name(cmd: &Command) -> &'static str {
    match cmd {
        Command::Login(_) => "login",
        Command::Auth(_) => "auth",
        Command::Connect(_) => "connect",
        Command::Config(_) => "config",
        Command::Runner(_) => "runner",
        Command::Project(_) => "project",
        Command::Context(_) => "context",
        Command::Install(_) => "install",
        Command::Uninstall(_) => "uninstall",
        Command::Start(_) => "start",
        Command::Stop(_) => "stop",
        Command::Restart(_) => "restart",
        Command::Reconnect(_) => "reconnect",
        Command::Status(_) => "status",
        Command::Tui(_) => "tui",
        Command::Doctor(_) => "doctor",
        Command::Update(_) => "update",
        Command::Engine(_) => "engine",
        Command::Remove(_) => "remove",
        Command::Ai(_) => "ai",
        Command::Issue(_) => "issue",
        Command::Docs(_) => "docs",
        Command::Label(_) => "label",
        Command::Comment(_) => "comment",
        Command::Page(_) => "page",
        Command::State(_) => "state",
        Command::Scheduler(_) => "scheduler",
        Command::Workpad(_) => "workpad",
        Command::Workspace(_) => "workspace",
        Command::RunCmd(_) => "run",
        Command::Run(_) => "__run",
        Command::Managed(_) => "__managed",
        Command::EngineHelper(_) => "__engine",
    }
}

/// The class of a parsed subcommand, or `None` when its name is missing
/// from [`COMMAND_CLASSES`] (a bug the classification test catches).
pub fn classify_command(cmd: &Command) -> Option<CommandClass> {
    classify_name(command_name(cmd))
}

/// The class for a clap command name, or `None` when unclassified.
/// Table lookup, so tests can pin every pinned name without building
/// arg structs.
pub fn classify_name(name: &str) -> Option<CommandClass> {
    COMMAND_CLASSES
        .iter()
        .find(|(entry, _)| *entry == name)
        .map(|(_, class)| *class)
}

/// Error JSON printed when an operator command is refused inside a run.
pub const OPERATOR_REFUSED_ERROR: &str =
    r#"{"error": "operator command not available inside an agent run"}"#;

/// Whether this process runs inside an agent run: the daemon exports
/// `PIDASH_RUN_ID` on every agent process (see `util::shell::RunEnv`).
/// Chat sessions never set it.
pub fn in_agent_run() -> bool {
    std::env::var_os(crate::api_client::RUN_ID_ENV).is_some_and(|v| !v.is_empty())
}

/// Operator bypass for genuine debugging inside a shell that inherited
/// the run variable (NEWPIDASH-320 §6.5).
pub fn operator_bypassed() -> bool {
    std::env::var_os("PIDASH_ALLOW_OPERATOR").is_some_and(|v| v == "1")
}

/// The refusal for an operator subcommand named on the command line,
/// checked before clap parsing (NEWPIDASH-320 §6.5). Clap validates
/// required flags first, so a refusal inside `run()` would come back as
/// a usage error instead of the refusal JSON for commands like
/// `pidash runner add` (whose `--project` is required). Help and version
/// requests are exempt: reading help is never an operation.
pub fn early_operator_refusal(argv: &[String]) -> Option<&'static str> {
    let run_id = std::env::var(crate::api_client::RUN_ID_ENV).ok();
    early_refusal_for(argv, run_id.as_deref(), operator_bypassed())
}

/// Pure core of [`early_operator_refusal`] for unit tests.
fn early_refusal_for(argv: &[String], run_id: Option<&str>, bypass: bool) -> Option<&'static str> {
    if run_id.is_none_or(|id| id.is_empty()) || bypass {
        return None;
    }
    if argv.iter().any(|arg| {
        arg == "-h" || arg == "--help" || arg == "-V" || arg == "--version" || arg == "help"
    }) {
        return None;
    }
    // Skip the binary name plus global flags and their values; the first
    // remaining positional is the subcommand.
    let mut args = argv.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config-dir" || arg == "--data-dir" || arg == "--log" {
            args.next();
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        if classify_name(arg) == Some(Operator) {
            return Some(OPERATOR_REFUSED_ERROR);
        }
        return None;
    }
    None
}

/// The refusal for `cmd` inside an agent run, if it is refused: operator
/// commands exit non-zero with [`OPERATOR_REFUSED_ERROR`] unless the
/// bypass is set. Pure over its inputs so the rule is unit-testable.
pub fn refusal_for(
    class: Option<CommandClass>,
    run_id: Option<&str>,
    bypass: bool,
) -> Option<&'static str> {
    if run_id.is_some_and(|id| !id.is_empty()) && !bypass && class == Some(Operator) {
        Some(OPERATOR_REFUSED_ERROR)
    } else {
        None
    }
}

/// Whether this invocation should render the filtered top-level help
/// (NEWPIDASH-320 §6.5): inside an agent run, `pidash --help`, `pidash -h`
/// and the equivalent `pidash help` list only agent commands. Pure over
/// its inputs so the rule is unit-testable; per-command help (`pidash
/// <cmd> --help`, `pidash help <cmd>`) keeps clap's default rendering.
pub fn wants_filtered_help_for(argv: &[String], in_run: bool, bypassed: bool) -> bool {
    if !in_run || bypassed || argv.len() != 2 {
        return false;
    }
    matches!(argv[1].as_str(), "--help" | "-h" | "help")
}

/// Env-reading wrapper for [`wants_filtered_help_for`].
pub fn wants_filtered_help(argv: &[String]) -> bool {
    wants_filtered_help_for(argv, in_agent_run(), operator_bypassed())
}

/// Top-level help with operator commands hidden, for agent runs
/// (NEWPIDASH-320 §6.5). `pidash --help` inside a run lists only agent
/// commands, plus a note saying the rest are refused.
pub fn agent_help() -> String {
    let mut cmd = Cli::command();
    let hidden: Vec<&str> = COMMAND_CLASSES
        .iter()
        .filter(|(_, class)| *class == Operator)
        .map(|(name, _)| *name)
        .collect();
    // `Command::hide` takes ownership, so swap each hidden subcommand out,
    // hide it, and put it back.
    for sub in cmd.get_subcommands_mut() {
        if hidden.contains(&sub.get_name()) {
            let owned = std::mem::replace(sub, clap::Command::new(""));
            *sub = owned.hide(true);
        }
    }
    cmd.after_help(
        "Inside an agent run (PIDASH_RUN_ID is set) only the agent commands above are available; \
         operator commands are refused. Set PIDASH_ALLOW_OPERATOR=1 to bypass for debugging.",
    )
    .render_help()
    .to_string()
}

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
    /// Log this host in to Pi Dash (same as `pidash auth login`; see
    /// `pidash auth` for status and logout).
    Login(auth::login::Args),

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

    /// Retry the cloud connection now instead of waiting out the backoff
    /// (all runners, or just `<runner>`).
    Reconnect(reconnect::Args),

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

    /// The Pi Dash Agent engine shipped with this release (`engine install`
    /// / `path` / `status` / `pause` / `resume`). Installed next to pidash,
    /// never on PATH.
    Engine(pidash_agent_cmd::PidashAgentArgs),

    /// Deregister with the cloud and delete local credentials.
    Remove(remove::Args),

    /// Send a command to the connected Pi Dash cloud AI assistant and print
    /// its reply. BYOK: requires an LLM provider + key configured in Pi Dash.
    Ai(ai::AiArgs),

    /// Read or update a Pi Dash work item (fetch, change state, edit fields).
    Issue(issue::IssueArgs),

    /// Show the on-demand CLI reference (`docs <topic>` for the full text
    /// behind the prompt's core commands).
    Docs(docs::DocsArgs),

    /// Manage a project's labels (`label list` / `create` / `update` /
    /// `delete`). Attaching a label to a work item lives on `pidash issue`.
    Label(label::LabelArgs),

    /// List, post, or edit work-item comments.
    Comment(comment::CommentArgs),

    /// Read project pages — the project-scoped wiki (`page list` / `page get`).
    Page(page::PageArgs),

    /// Inspect workflow states on a project.
    State(state::StateArgs),

    /// Project schedulers: `scheduler list` / `scheduler get <id>` /
    /// `scheduler runs <id>`; for project admins `scheduler install` /
    /// `update` / `run-now` / `uninstall`; for workspace admins
    /// `scheduler create` / `edit` / `delete`.
    Scheduler(scheduler::SchedulerArgs),

    /// Read or overwrite the coding agent's per-issue workpad.
    Workpad(workpad::WorkpadArgs),

    /// Verify the CLI's Pi Dash credentials end-to-end.
    Workspace(workspace::WorkspaceArgs),

    /// Report this agent run's outcome to Pi Dash (`run yield`). Used by
    /// the agent from inside a run; reads the run id from `PIDASH_RUN_ID`.
    #[command(name = "run")]
    RunCmd(run_cmd::RunCmdArgs),

    /// Internal: run the daemon in the foreground. Invoked by systemd/launchd
    /// via the generated unit file. Not a user-facing verb.
    #[command(name = "__run", hide = true)]
    Run(run::Args),

    /// Internal: provisioning verbs driven by the Pi Dash desktop app, which
    /// owns the config directory these operate on. Not a user-facing verb —
    /// configure a runner with `pidash runner add` instead.
    #[command(name = "__managed", hide = true)]
    Managed(managed::ManagedArgs),

    /// Internal: verbs the Pi Dash Agent runs (its model-token auth
    /// helper). Not a user-facing verb.
    #[command(name = "__engine", hide = true)]
    EngineHelper(pidash_agent_cmd::PidashAgentHelperArgs),
}

pub async fn run(cli: Cli) -> Result<()> {
    crate::util::logging::init(&cli.log)?;
    let paths = crate::util::paths::Paths::resolve(cli.config_dir.clone(), cli.data_dir.clone())?;
    tracing::debug!(?paths, "resolved runner paths");

    let Some(command) = cli.command else {
        return run_default(&paths).await;
    };

    // Operator commands are refused inside an agent run (NEWPIDASH-320
    // §6.5): they manage the runner or the operator's login, never the
    // run. `PIDASH_ALLOW_OPERATOR=1` bypasses for debugging.
    let run_id = std::env::var(crate::api_client::RUN_ID_ENV).ok();
    if let Some(error) = refusal_for(
        classify_command(&command),
        run_id.as_deref(),
        operator_bypassed(),
    ) {
        eprintln!("{error}");
        std::process::exit(2);
    }

    match command {
        Command::Login(args) => auth::login::run(args, &paths).await,
        Command::Auth(args) => auth::run(args, &paths).await,
        Command::Connect(args) => connect::run(args, &paths).await,
        Command::Config(args) => config_cmd::run(args, &paths).await,
        Command::Runner(args) => runner::run(args, &paths).await,
        Command::Project(args) => run_crud(project::run(args, &paths).await),
        Command::Context(args) => run_crud(context::run(args, &paths).await),
        Command::Install(args) => install::run(args, &paths).await,
        Command::Uninstall(args) => uninstall::run(args, &paths).await,
        Command::Start(args) => start::run(args, &paths).await,
        Command::Stop(args) => stop::run(args, &paths).await,
        Command::Restart(args) => restart::run(args, &paths).await,
        Command::Reconnect(args) => reconnect::run(args, &paths).await,
        Command::Status(args) => status::run(args, &paths).await,
        Command::Tui(args) => tui::run(args, &paths).await,
        Command::Doctor(args) => doctor::run(args, &paths).await,
        Command::Update(args) => update::run(args, &paths).await,
        Command::Engine(args) => pidash_agent_cmd::run(args, &paths).await,
        Command::Remove(args) => remove::run(args, &paths).await,
        Command::Ai(args) => ai::run(args, &paths).await,
        Command::Issue(args) => run_crud(issue::run(args, &paths).await),
        Command::Docs(args) => run_crud(docs::run(args)),
        Command::Label(args) => run_crud(label::run(args, &paths).await),
        Command::Comment(args) => run_crud(comment::run(args, &paths).await),
        Command::Page(args) => run_crud(page::run(args, &paths).await),
        Command::State(args) => run_crud(state::run(args, &paths).await),
        Command::Scheduler(args) => run_crud(scheduler::run(args, &paths).await),
        Command::Workpad(args) => run_crud(workpad::run(args, &paths).await),
        Command::Workspace(args) => run_crud(workspace::run(args, &paths).await),
        Command::RunCmd(args) => run_crud(run_cmd::run(args, &paths).await),
        Command::Run(args) => run::run(args, &paths).await,
        Command::Managed(args) => managed::run(args, &paths).await,
        Command::EngineHelper(args) => pidash_agent_cmd::run_helper(args, &paths).await,
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
                workspace: None,
                force: false,
                device_code_stdin: false,
                engine: false,
            }),
        };
        return auth::run(args, paths).await;
    }
    // Inside an agent run the bare command lists only agent commands
    // (NEWPIDASH-320 §6.5), like `pidash --help` does.
    if in_agent_run() && !operator_bypassed() {
        print!("{}", agent_help());
    } else {
        Cli::command().print_help()?;
        println!();
    }
    Ok(())
}

fn run_crud(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        std::process::exit(code);
    }
}

#[cfg(test)]
mod prompt_contract;

#[cfg(test)]
mod tests {
    use super::{COMMAND_CLASSES, CommandClass, agent_help, classify_name, refusal_for};
    use clap::CommandFactory;

    /// Every name clap serves must appear in [`COMMAND_CLASSES`]. A new
    /// subcommand without a class fails here.
    #[test]
    fn every_subcommand_is_classified() {
        let mut missing = Vec::new();
        for sub in super::Cli::command().get_subcommands() {
            let name = sub.get_name();
            if !COMMAND_CLASSES.iter().any(|(entry, _)| *entry == name) {
                missing.push(name.to_string());
            }
        }
        assert!(
            missing.is_empty(),
            "unclassified subcommands (add them to COMMAND_CLASSES): {missing:?}"
        );
        // The table must not name commands that do not exist either.
        let top = super::Cli::command();
        let served: Vec<&str> = top.get_subcommands().map(|s| s.get_name()).collect();
        let stale: Vec<&&str> = COMMAND_CLASSES
            .iter()
            .map(|(name, _)| name)
            .filter(|name| !served.contains(name))
            .collect();
        assert!(stale.is_empty(), "stale classification entries: {stale:?}");
    }

    /// The table pins the required classes, including the three the
    /// design calls out: `ai` is operator, `run` and `docs` are agent.
    #[test]
    fn pinned_commands_classify_correctly() {
        for name in [
            "login",
            "auth",
            "config",
            "runner",
            "install",
            "uninstall",
            "start",
            "stop",
            "restart",
            "reconnect",
            "status",
            "tui",
            "doctor",
            "update",
            "engine",
            "remove",
            "ai",
            "workspace",
            "__run",
            "__managed",
            "__engine",
        ] {
            assert_eq!(classify_name(name), Some(CommandClass::Operator), "{name}");
        }
        for name in [
            "project",
            "context",
            "issue",
            "label",
            "comment",
            "page",
            "state",
            "scheduler",
            "workpad",
            "run",
            "docs",
        ] {
            assert_eq!(classify_name(name), Some(CommandClass::Agent), "{name}");
        }
        assert_eq!(classify_name("not-a-command"), None);
    }

    /// The pre-parse refusal fires for operator subcommands even when
    /// required flags are missing (clap would otherwise usage-error
    /// first). Help requests pass through; agent commands and runs
    /// outside runs pass.
    #[test]
    fn early_refusal_fires_before_parsing() {
        use super::early_refusal_for;
        let argv = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        // Operator, no required flags given — still the refusal JSON.
        assert_eq!(
            early_refusal_for(&argv(&["pidash", "runner", "add"]), Some("run-1"), false),
            Some(super::OPERATOR_REFUSED_ERROR)
        );
        assert_eq!(
            early_refusal_for(&argv(&["pidash", "ai"]), Some("run-1"), false),
            Some(super::OPERATOR_REFUSED_ERROR)
        );
        // Global flags before the subcommand do not hide it.
        assert_eq!(
            early_refusal_for(
                &argv(&["pidash", "--config-dir", "/tmp/x", "update"]),
                Some("run-1"),
                false
            ),
            Some(super::OPERATOR_REFUSED_ERROR)
        );
        // Help passes through to clap's rendering.
        assert_eq!(
            early_refusal_for(&argv(&["pidash", "runner", "--help"]), Some("run-1"), false),
            None
        );
        assert_eq!(
            early_refusal_for(&argv(&["pidash", "--help"]), Some("run-1"), false),
            None
        );
        // Agent commands, bypasses and outside-run invocations pass.
        assert_eq!(
            early_refusal_for(
                &argv(&["pidash", "issue", "get", "ENG-42"]),
                Some("run-1"),
                false
            ),
            None
        );
        assert_eq!(
            early_refusal_for(&argv(&["pidash", "runner", "add"]), Some("run-1"), true),
            None
        );
        assert_eq!(
            early_refusal_for(&argv(&["pidash", "runner", "add"]), None, false),
            None
        );
        assert_eq!(
            early_refusal_for(&argv(&["pidash"]), Some("run-1"), false),
            None
        );
    }

    /// The in-run refusal rule: operator commands refuse with the exact
    /// JSON unless bypassed; agent commands and runs outside runs pass.
    #[test]
    fn refusal_rule() {
        assert_eq!(
            refusal_for(Some(CommandClass::Operator), Some("run-1"), false),
            Some(super::OPERATOR_REFUSED_ERROR)
        );
        assert_eq!(
            refusal_for(Some(CommandClass::Operator), Some("run-1"), true),
            None
        );
        assert_eq!(refusal_for(Some(CommandClass::Operator), None, false), None);
        assert_eq!(
            refusal_for(Some(CommandClass::Operator), Some(""), false),
            None
        );
        assert_eq!(
            refusal_for(Some(CommandClass::Agent), Some("run-1"), false),
            None
        );
        assert_eq!(refusal_for(None, Some("run-1"), false), None);
    }

    /// Bare `pidash help` is the same top-level request as `pidash --help`
    /// (NEWPIDASH-320 review): it renders the filtered list in a run, while
    /// per-command `pidash help <cmd>` keeps clap's default rendering.
    #[test]
    fn filtered_help_covers_help_subcommand_form() {
        use super::wants_filtered_help_for;
        let argv = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        assert!(wants_filtered_help_for(
            &argv(&["pidash", "--help"]),
            true,
            false
        ));
        assert!(wants_filtered_help_for(
            &argv(&["pidash", "-h"]),
            true,
            false
        ));
        assert!(wants_filtered_help_for(
            &argv(&["pidash", "help"]),
            true,
            false
        ));
        assert!(!wants_filtered_help_for(
            &argv(&["pidash", "help", "issue"]),
            true,
            false
        ));
        assert!(!wants_filtered_help_for(
            &argv(&["pidash", "issue", "--help"]),
            true,
            false
        ));
        assert!(!wants_filtered_help_for(
            &argv(&["pidash", "--help"]),
            false,
            false
        ));
        assert!(!wants_filtered_help_for(
            &argv(&["pidash", "help"]),
            true,
            true
        ));
        assert!(!wants_filtered_help_for(&argv(&["pidash"]), true, false));
    }

    /// `pidash --help` inside a run lists agent commands and no operator
    /// command, plus the refusal note.
    #[test]
    fn agent_help_hides_operator_commands() {
        let help = agent_help();
        for agent in ["issue", "comment", "workpad", "state", "run", "docs"] {
            assert!(help.contains(agent), "help should list {agent}");
        }
        for operator in [
            "runner",
            "install",
            "update",
            "reconnect",
            "login",
            "auth",
            "config",
            "engine",
        ] {
            // Match the command entry line, not prose that names it.
            assert!(
                !help.lines().any(|l| l.trim_start().starts_with(operator)),
                "help should hide {operator}"
            );
        }
        assert!(help.contains("PIDASH_ALLOW_OPERATOR=1"));
    }
}
