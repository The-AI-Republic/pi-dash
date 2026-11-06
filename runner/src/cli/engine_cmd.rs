//! `pidash engine` — the built-in agent engine that ships with the CLI.
//!
//! - `pidash engine install` places this release's `engine/<version>/`
//!   directory next to `pidash`. The cargo-dist shell and PowerShell
//!   installers only install executables onto `PATH` and drop the rest of
//!   the archive, so the `install.sh` / `install.ps1` wrappers run this right
//!   after them (see `crate::engine::install`). The MSI places the engine
//!   itself.
//! - `pidash engine path` prints where the daemon would find the engine.
//! - `pidash engine pause` / `resume` set the per-machine "Pause the built-in
//!   engine" switch on the server (`DevMachine.engine_paused_by_owner`,
//!   design §8); engine runners created later are paused too.
//! - `pidash engine status` prints the daemon's machine-level engine state.
//! - `pidash __engine model-token` (hidden) is the engine's command-backed
//!   auth helper: it asks the daemon for the model token it holds in memory.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use crate::RUNNER_VERSION;
use crate::engine;
use crate::util::paths::Paths;

#[derive(Debug, Args)]
pub struct EngineArgs {
    #[command(subcommand)]
    pub command: EngineCommand,
}

#[derive(Debug, Subcommand)]
pub enum EngineCommand {
    /// Install the built-in engine of this pidash release next to pidash
    /// (downloads this release's archive and verifies its checksum).
    Install(InstallArgs),
    /// Print the engine binary the daemon would use, and exit non-zero if
    /// there is none.
    Path,
    /// Pause the built-in engine on this computer: its runners take no new
    /// work until `pidash engine resume`. Own-agent runners are unaffected.
    Pause,
    /// Resume the built-in engine after `pidash engine pause`.
    Resume,
    /// Show the built-in engine's state on this computer.
    Status(StatusArgs),
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Print JSON.
    #[arg(long)]
    pub json: bool,
}

/// `pidash __engine …`: internal verbs the engine runs.
#[derive(Debug, Args)]
pub struct EngineHelperArgs {
    #[command(subcommand)]
    pub command: EngineHelperCommand,
}

#[derive(Debug, Subcommand)]
pub enum EngineHelperCommand {
    /// Print the current model token (the engine's auth helper).
    ModelToken,
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Install the engine of another pidash release instead of this one
    /// (e.g. `0.1.30`).
    #[arg(long, value_name = "VERSION")]
    pub release: Option<String>,
}

pub async fn run(args: EngineArgs, paths: &Paths) -> Result<()> {
    match args.command {
        EngineCommand::Install(args) => install(args).await,
        EngineCommand::Path => path(paths),
        EngineCommand::Pause => set_paused(paths, true).await,
        EngineCommand::Resume => set_paused(paths, false).await,
        EngineCommand::Status(a) => status(paths, a.json).await,
    }
}

/// `pidash __engine model-token`: ask the daemon over the local socket and
/// print the token on stdout, the only thing the engine reads. Nothing is
/// written to disk.
pub async fn run_helper(args: EngineHelperArgs, paths: &Paths) -> Result<()> {
    match args.command {
        EngineHelperCommand::ModelToken => {
            let token = fetch_model_token(paths).await?;
            println!("{token}");
            Ok(())
        }
    }
}

/// The model token, from the daemon.
pub async fn fetch_model_token(paths: &Paths) -> Result<String> {
    use crate::ipc::protocol::{Request, Response};
    let mut client = crate::ipc::client::Client::connect(paths.ipc_socket_path())
        .await
        .context("the Pi Dash daemon is not running")?;
    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        client.call(Request::EngineModelToken),
    )
    .await
    .context("the daemon did not answer in time")??;
    match resp {
        Response::EngineModelToken { token, .. } => {
            if token.is_empty() || token.chars().any(|c| c.is_whitespace() || c.is_control()) {
                anyhow::bail!("the daemon returned a malformed model token");
            }
            Ok(token)
        }
        Response::Error(e) => anyhow::bail!("{}", e.message),
        other => anyhow::bail!("unexpected daemon response: {other:?}"),
    }
}

/// Set the pause switch: through the daemon when it runs (so it caches the
/// new state for `Hello` and `pidash engine status`), else straight to the
/// server.
async fn set_paused(paths: &Paths, paused: bool) -> Result<()> {
    use crate::ipc::protocol::{Request, Response};
    let via_daemon = match crate::ipc::client::Client::connect(paths.ipc_socket_path()).await {
        Ok(mut client) => match client.call(Request::EngineSetPaused { paused }).await? {
            Response::EngineStatus(st) => Some(st),
            // A daemon from before `EngineSetPaused` cannot parse it.
            Response::Error(e) if e.code == 400 => None,
            Response::Error(e) => anyhow::bail!("{}", e.message),
            other => anyhow::bail!("unexpected daemon response: {other:?}"),
        },
        Err(_) => None,
    };
    if via_daemon.is_none() {
        set_paused_direct(paths, paused).await?;
    }
    if paused {
        println!(
            "Paused the built-in engine on this computer. Its runners take no new work until `pidash engine resume`."
        );
    } else {
        println!("Resumed the built-in engine on this computer.");
    }
    if let Some(reason) = via_daemon.and_then(|st| st.auto_pause_reason) {
        println!("The server also paused it automatically: {reason}.");
    }
    Ok(())
}

async fn set_paused_direct(paths: &Paths, paused: bool) -> Result<()> {
    let cfg = crate::config::file::load_config_opt(paths)?.ok_or_else(|| {
        anyhow::anyhow!("this computer is not signed in; run `pidash auth login`")
    })?;
    let machine_id = cfg
        .daemon
        .dev_machine_id
        .context("this computer has no dev-machine id yet; run `pidash auth login`")?;
    let token = cfg
        .cli
        .as_ref()
        .and_then(|c| {
            c.workspace_tokens()
                .into_iter()
                .map(|(_, t)| t)
                .find(|t| t.starts_with("mt_"))
        })
        .context("this computer holds no daemon credential; run `pidash auth login`")?;
    let api = engine::api::EngineApi::new(&cfg.daemon.cloud_url)?;
    api.set_paused(&token, machine_id, paused)
        .await
        .map_err(|e| anyhow::anyhow!("the server did not change the engine pause: {e}"))?;
    Ok(())
}

async fn status(paths: &Paths, json: bool) -> Result<()> {
    use crate::ipc::protocol::{Request, Response};
    let st = match crate::ipc::client::Client::connect(paths.ipc_socket_path()).await {
        Ok(mut client) => match client.call(Request::EngineStatus).await? {
            Response::EngineStatus(st) => st,
            Response::Error(e) => anyhow::bail!("daemon refused: {}", e.message),
            other => anyhow::bail!("unexpected daemon response: {other:?}"),
        },
        // No daemon: compute it the way the daemon would.
        Err(_) => engine::provision::status_without_daemon(paths).await?,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&st)?);
    } else {
        println!("{}", describe_status(&st));
    }
    Ok(())
}

/// One line for `pidash status` / `pidash engine status`.
pub fn describe_status(st: &crate::ipc::protocol::EngineStatusInfo) -> String {
    use crate::ipc::protocol::EngineState;
    let state = match st.state {
        EngineState::Available => "available".to_string(),
        EngineState::Expiring => format!(
            "available, sign-in expiring{}",
            st.credential_expires_at
                .map(|e| format!(" on {}", e.format("%Y-%m-%d")))
                .unwrap_or_default()
        ),
        EngineState::SignedOut => format!(
            "signed out ({}) — run `pidash login --engine`",
            st.sign_in_state.as_str()
        ),
        EngineState::Unavailable => format!(
            "unavailable ({})",
            st.reason_code.as_deref().unwrap_or("unknown")
        ),
    };
    let model = st
        .model
        .as_deref()
        .map(|m| format!(", model {m}"))
        .unwrap_or_default();
    format!("engine: {state}{model}{}", describe_pause(st))
}

/// The pause part of [`describe_status`]: empty when not paused or unknown.
fn describe_pause(st: &crate::ipc::protocol::EngineStatusInfo) -> String {
    let mut parts = Vec::new();
    if st.paused_by_owner == Some(true) {
        parts.push("paused by you — `pidash engine resume` to resume".to_string());
    }
    if let Some(reason) = &st.auto_pause_reason {
        parts.push(format!("paused automatically ({reason})"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("; {}", parts.join("; "))
    }
}

async fn install(args: InstallArgs) -> Result<()> {
    let release = args.release.as_deref().unwrap_or(RUNNER_VERSION);
    let root = engine::install::install_root_for_current_exe()?;
    // This release's engine version is known without downloading anything.
    if release == RUNNER_VERSION {
        let version = engine::engine_version();
        if root
            .join(version)
            .join(engine::ENGINE_BINARY_NAME)
            .is_file()
        {
            println!("engine {version} is already installed");
            return Ok(());
        }
    }
    println!(
        "installing the built-in engine of pidash v{release} into {}",
        root.display()
    );
    let outcome = engine::install::install_release(release, &root)
        .await
        .context("installing the built-in engine")?;
    for v in &outcome.installed {
        println!("installed engine {v}");
    }
    for v in &outcome.already_present {
        println!("engine {v} is already installed");
    }
    Ok(())
}

fn path(paths: &Paths) -> Result<()> {
    let section = crate::config::file::load_config_opt(paths)?
        .map(|c| c.engine)
        .unwrap_or_default();
    let resolved = engine::locate::resolve(&section)?;
    println!("{}", resolved.binary.display());
    Ok(())
}

#[cfg(test)]
mod describe_tests {
    use super::*;
    use crate::ipc::protocol::{EngineSignInState, EngineState, EngineStatusInfo};

    #[test]
    fn status_line_says_who_paused_the_engine() {
        let mut st = EngineStatusInfo {
            state: EngineState::Available,
            reason_code: None,
            sign_in_state: EngineSignInState::Active,
            credential_expires_at: None,
            model: Some("gpt-5.5".into()),
            binary: None,
            profile_fetched_at: None,
            paused_by_owner: Some(false),
            auto_pause_reason: None,
            pause_checked_at: Some(chrono::Utc::now()),
        };
        assert_eq!(describe_status(&st), "engine: available, model gpt-5.5");
        st.paused_by_owner = Some(true);
        assert!(describe_status(&st).contains("paused by you"));
        st.paused_by_owner = Some(false);
        st.auto_pause_reason = Some("wallet_empty".into());
        assert!(
            describe_status(&st).ends_with("paused automatically (wallet_empty)"),
            "{}",
            describe_status(&st)
        );
    }
}
