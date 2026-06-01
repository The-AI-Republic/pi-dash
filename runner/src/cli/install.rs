//! `pidash install` — write or refresh the OS service unit.
//!
//! Runner enrollment now lives in `pidash auth login` and
//! `pidash runner add`. `install` writes the systemd / launchd unit so
//! the daemon survives reboots. Safe to run repeatedly (e.g. after
//! upgrading the binary).

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Skip the `sudo loginctl enable-linger` step (Linux only). Without
    /// linger the daemon only starts at login, not at boot. Set this in
    /// CI / unattended installs where a sudo password prompt would hang.
    #[arg(long)]
    pub skip_linger: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let svc = crate::service::detect();
    let enrolled = machine_is_configured(paths);

    svc.write_unit(paths).await?;
    if !enrolled {
        println!("Service unit written.");
        println!();
        println!("This machine is not configured yet. Next:");
        println!("  pidash auth login --url <URL>");
        println!("  pidash runner add --project <PROJECT>");
        println!();
        return Ok(());
    }

    svc.enable_and_start().await?;
    if !args.skip_linger {
        let _ = svc.ensure_boot_start().await;
    }
    println!();
    println!("Service unit refreshed and daemon restarted.");
    println!();
    Ok(())
}

fn machine_is_configured(paths: &Paths) -> bool {
    let Ok(cfg) = crate::config::file::load_config(paths) else {
        return false;
    };
    if cfg.runners.is_empty() {
        return false;
    }
    let has_shared_machine_token = cfg
        .cli
        .as_ref()
        .and_then(|cli| cli.token.as_deref())
        .map(|token| token.starts_with("mt_"))
        .unwrap_or(false);
    has_shared_machine_token || paths.credentials_path().exists()
}
