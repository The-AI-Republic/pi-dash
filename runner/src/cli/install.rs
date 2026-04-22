//! `pidash install` — write the OS service unit and bring the service up.
//!
//! Three phases:
//!
//! 1. **Write unit** — always. `service::write_unit` is idempotent, so it's
//!    safe to re-run. On its own it doesn't activate anything.
//!
//! 2. **Fresh-install gate** — if `config.toml` doesn't exist, the daemon
//!    has no credentials to run with. On a TTY (and without `--no-configure`),
//!    prompt for URL + token and chain into `pidash configure`. Non-TTY or
//!    with `--no-configure`, print a next-step hint and exit without
//!    enabling the service. Nothing auto-starts a runner that isn't
//!    configured.
//!
//! 3. **Enable + start** — run once the machine has both a unit file and a
//!    valid `config.toml` + `credentials.toml`. Also prints the Linux
//!    `loginctl enable-linger` hint so on-boot autostart actually works.

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
    svc.write_unit(paths).await?;

    // Treat a missing config.toml OR a missing credentials.toml as "fresh" —
    // `enable_and_start`ing against a partial state would crash-loop the daemon
    // on the new `__run` missing-creds bail.
    let fresh = !paths.config_path().exists() || !paths.credentials_path().exists();
    if fresh {
        if args.no_configure || !std::io::stdin().is_terminal() {
            print_unattended_hint(args.no_configure);
            return Ok(());
        }
        // TTY + not opted-out → chain into configure interactively.
        let inputs = prompt_for_register_inputs(paths)?;
        crate::cli::configure::execute(inputs, paths, /* print_next_hint = */ false).await?;
    }

    svc.enable_and_start().await?;
    print_post_install_hints();
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
    println!("Next steps:");
    println!("  1. pidash configure --url <URL> --token <ONE_TIME_TOKEN>");
    println!("  2. pidash start");
    println!();
}

fn print_post_install_hints() {
    println!();
    println!("Service installed and running.");
    if cfg!(target_os = "linux") {
        println!();
        println!("For the service to start on OS boot (before you log in), run:");
        println!("  sudo loginctl enable-linger $USER");
        println!(
            "Without lingering, the service still starts at every user login and restarts on crash."
        );
    }
    println!();
    println!("Useful next commands:");
    println!("  pidash status         # service + daemon state");
    println!("  pidash tui            # interactive UI");
    println!("  pidash stop           # stop the service");
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
