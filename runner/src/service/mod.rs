use anyhow::Result;

use crate::util::paths::Paths;

pub mod launchd;
pub mod systemd;

pub enum Service {
    Systemd,
    Launchd,
}

pub fn detect() -> Service {
    if cfg!(target_os = "macos") {
        Service::Launchd
    } else {
        Service::Systemd
    }
}

impl Service {
    pub async fn install(&self, paths: &Paths) -> Result<()> {
        match self {
            Service::Systemd => systemd::install(paths).await,
            Service::Launchd => launchd::install(paths).await,
        }
    }

    pub async fn uninstall(&self, paths: &Paths) -> Result<()> {
        match self {
            Service::Systemd => systemd::uninstall(paths).await,
            Service::Launchd => launchd::uninstall(paths).await,
        }
    }

    pub async fn start(&self) -> Result<()> {
        match self {
            Service::Systemd => systemd::start().await,
            Service::Launchd => launchd::start().await,
        }
    }

    pub async fn stop(&self) -> Result<()> {
        match self {
            Service::Systemd => systemd::stop().await,
            Service::Launchd => launchd::stop().await,
        }
    }

    pub async fn status(&self) -> Result<String> {
        match self {
            Service::Systemd => systemd::status().await,
            Service::Launchd => launchd::status().await,
        }
    }
}
