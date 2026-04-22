use anyhow::Result;
use std::path::Path;

use crate::util::paths::Paths;

pub mod launchd;
pub mod reload;
pub mod systemd;

/// Reject paths containing characters that would break the systemd unit /
/// launchd plist we generate. Newlines could inject extra directives; control
/// chars and characters with XML meaning would corrupt the plist.
pub(crate) fn validate_path_for_unit(path: &Path) -> Result<&str> {
    let s = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("path is not valid UTF-8: {path:?}"))?;
    if s.contains(['\n', '\r', '\0']) {
        anyhow::bail!("path contains a control character — refusing to write unit: {s:?}");
    }
    if s.chars().any(|c| c.is_control()) {
        anyhow::bail!("path contains a control character — refusing to write unit: {s:?}");
    }
    Ok(s)
}

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
    /// Write the unit file (systemd) or plist (launchd). Does not enable or
    /// start. Allows `pidash install` to gate activation on configuration.
    pub async fn write_unit(&self, paths: &Paths) -> Result<()> {
        match self {
            Service::Systemd => systemd::write_unit(paths).await,
            Service::Launchd => launchd::write_unit(paths).await,
        }
    }

    /// Enable at boot/login and start now. Must run after `write_unit`.
    pub async fn enable_and_start(&self) -> Result<()> {
        match self {
            Service::Systemd => systemd::enable_and_start().await,
            Service::Launchd => launchd::enable_and_start().await,
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
