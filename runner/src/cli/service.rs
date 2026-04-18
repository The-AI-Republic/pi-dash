use anyhow::Result;
use clap::Subcommand;

use crate::util::paths::Paths;

#[derive(Debug, Subcommand)]
pub enum ServiceCmd {
    /// Generate and install an OS service unit (systemd user / launchd agent).
    Install,
    /// Start the installed service.
    Start,
    /// Stop the installed service.
    Stop,
    /// Uninstall the service unit.
    Uninstall,
    /// Report service status.
    Status,
}

pub async fn run(cmd: ServiceCmd, paths: &Paths) -> Result<()> {
    let svc = crate::service::detect();
    match cmd {
        ServiceCmd::Install => svc.install(paths).await,
        ServiceCmd::Start => svc.start().await,
        ServiceCmd::Stop => svc.stop().await,
        ServiceCmd::Uninstall => svc.uninstall(paths).await,
        ServiceCmd::Status => {
            let st = svc.status().await?;
            println!("{st}");
            Ok(())
        }
    }
}
