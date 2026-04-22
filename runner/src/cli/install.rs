//! `pidash install` — CI / provisioning entry point for the OS service unit.
//!
//! The interactive happy path lives in `pidash configure`, which handles
//! registration + service setup in one shot. `install` is the escape hatch
//! for the two cases `configure` can't cover:
//!
//! - **Unattended provisioning** (`--no-configure` or no TTY): write the
//!   unit at image-bake time without credentials. The daemon stays stopped
//!   until someone runs `pidash configure` on the real machine.
//! - **Re-install after upgrading the binary**: rewrite the unit (in case
//!   `ExecStart` or other fields moved between versions) and restart the
//!   daemon so it picks up the new binary. Safe to run repeatedly.
//!
//! For convenience, running `pidash install` on a fresh machine interactively
//! still chains into `pidash configure`, which from there takes over the
//! whole flow.

use anyhow::Result;
use clap::Args as ClapArgs;
use std::io::{BufRead, IsTerminal, Write};

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Skip the interactive `pidash configure` prompt on a fresh install.
    /// Writes the service unit, then exits without enabling the service.
    /// Use when running from CI / Ansible / Docker builds.
    #[arg(long)]
    pub no_configure: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let svc = crate::service::detect();

    // Treat a missing config.toml OR a missing credentials.toml as "fresh" —
    // `enable_and_start`ing against a partial state would crash-loop the daemon
    // on the `__run` missing-creds bail.
    let fresh = !paths.config_path().exists() || !paths.credentials_path().exists();

    if fresh {
        if args.no_configure || !std::io::stdin().is_terminal() {
            // Unattended path: just drop the unit file and stop. The service
            // stays disabled until someone runs `pidash configure`.
            svc.write_unit(paths).await?;
            print_unattended_hint(args.no_configure);
            return Ok(());
        }
        // Interactive path: configure owns the whole end-to-end flow
        // (register → persist → doctor → write unit → enable + start).
        let inputs = prompt_for_register_inputs(paths)?;
        crate::cli::configure::execute(inputs, paths).await?;
        return Ok(());
    }

    // Re-install path: credentials already exist, so the user is refreshing
    // the unit (e.g. after a binary upgrade). Rewrite + restart.
    svc.write_unit(paths).await?;
    svc.enable_and_start().await?;
    println!();
    println!("Service unit refreshed and daemon restarted.");
    println!();
    Ok(())
}

fn print_unattended_hint(explicit_opt_out: bool) {
    let reason = if explicit_opt_out {
        "--no-configure was set"
    } else {
        "no TTY detected (CI or piped input)"
    };
    println!();
    println!("Service unit installed but NOT enabled ({reason}).");
    println!();
    println!("Next step:");
    println!("  pidash configure --url <URL> --token <ONE_TIME_TOKEN>");
    println!("(this also starts the service)");
    println!();
}

/// Interactive prompts for URL + token + optional name. Stays minimal —
/// anything else (`working_dir`, `skip_doctor`) can be provided by re-running
/// `pidash configure` directly.
fn prompt_for_register_inputs(paths: &Paths) -> Result<crate::cli::configure::RegisterInputs> {
    println!();
    println!("No runner config found at {}.", paths.config_path().display());
    println!("Let's register this runner with Pi Dash cloud.");
    println!("(Press Ctrl+C to abort and run `pidash install --no-configure` instead.)");
    println!();

    let url = prompt_required("Pi Dash cloud URL [https://cloud.pidash.so]: ")?;
    let url = if url.trim().is_empty() {
        "https://cloud.pidash.so".to_string()
    } else {
        url.trim().to_string()
    };

    let token = prompt_required("One-time registration token: ")?;
    let token = token.trim().to_string();
    if token.is_empty() {
        anyhow::bail!("registration token is required; aborting install");
    }

    let name = prompt_required("Runner name (blank for host default): ")?;
    let name = name.trim();
    let name = if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    };

    Ok(crate::cli::configure::RegisterInputs {
        url,
        token,
        name,
        working_dir: None,
        // Leave `agent: None` so configure::execute handles the prompt in
        // one place (install is always interactive here).
        agent: None,
        skip_doctor: false,
        // Install's interactive chain delegates fully to configure — let it
        // write the unit + start the daemon as part of the same flow.
        skip_service: false,
    })
}

fn prompt_required(msg: &str) -> Result<String> {
    print!("{msg}");
    std::io::stdout().flush()?;
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    Ok(line)
}
