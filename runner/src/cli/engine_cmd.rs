//! `pidash engine` — the built-in agent engine that ships with the CLI.
//!
//! - `pidash engine install` places this release's `engine/<version>/`
//!   directory next to `pidash`. The cargo-dist shell and PowerShell
//!   installers only install executables onto `PATH` and drop the rest of
//!   the archive, so the `install.sh` / `install.ps1` wrappers run this right
//!   after them (see `crate::engine::install`). The MSI places the engine
//!   itself.
//! - `pidash engine path` prints where the daemon would find the engine.

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
