//! `pidash install` — write or refresh the OS service unit.
//!
//! In the connection-first design, enrollment lives in
//! `pidash connect` and runner CRUD in `pidash runner add|list|remove`.
//! `install` is the small piece in between: write the systemd / launchd
//! unit so the daemon survives reboots. Safe to run repeatedly (e.g.
//! after upgrading the binary).

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
    let enrolled = paths.config_path().exists() && paths.credentials_path().exists();

    svc.write_unit(paths).await?;
    if !enrolled {
        println!("Service unit written.");
        println!();
        println!("This machine is not enrolled yet. Next:");
        println!("  pidash connect --url <URL> --token <ONE_TIME_TOKEN>");
        println!("  pidash runner add --name <NAME> --project <PROJECT>");
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
