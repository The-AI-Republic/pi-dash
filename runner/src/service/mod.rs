use anyhow::Result;
use std::path::Path;

use crate::util::paths::Paths;

#[cfg(target_os = "macos")]
pub mod launchd;
pub mod reload;
#[cfg(target_os = "linux")]
pub mod systemd;
#[cfg(windows)]
pub mod windows;

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

/// Snapshot the caller's `$PATH` so the unit/plist we write can hand it to
/// the daemon at startup. launchd (macOS) and `systemd --user` (Linux) both
/// give services a minimal default PATH (`/usr/bin:/bin:/usr/sbin:/sbin`)
/// that excludes nearly every PATH entry the operator added in their shell
/// rc — `/opt/homebrew/bin`, `~/.local/bin`, `~/.cargo/bin`, anything nvm /
/// pyenv / asdf injected, etc. The agent dispatch layer wraps subprocesses
/// in `bash -ilc` to recover that PATH (see `util::shell`), but `bash -i`
/// only sources `~/.bash_profile`/`~/.profile`/`~/.bashrc` — not the zsh /
/// fish files where most macOS users actually keep their PATH (zsh is the
/// default shell since 10.15). The wrapper therefore silently fails on a
/// stock macOS install: the daemon spawns `claude` and gets `not found`.
///
/// `pidash install` runs in the operator's interactive shell, so `$PATH`
/// here is the one we want. Capture it and bake it into the unit so the
/// daemon — and every subprocess it forks — inherits it deterministically,
/// without depending on which shell the operator uses.
///
/// Returns `None` (skip writing the env var) when:
/// - `$PATH` is unset or empty in the install-time environment, or
/// - the value contains a control char that would corrupt the unit / plist.
///
/// Each skip path emits a `tracing::warn!` so the operator can correlate a
/// later "claude: not found" failure with a known-bad install-time env.
pub(crate) fn capture_install_time_path() -> Option<String> {
    let value = match std::env::var("PATH") {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => {
            tracing::warn!(
                "$PATH is empty at install time; service unit will fall back to the manager's stripped PATH"
            );
            return None;
        }
        Err(e) => {
            tracing::warn!(
                "$PATH could not be read at install time ({e}); service unit will fall back to the manager's stripped PATH"
            );
            return None;
        }
    };
    if value.chars().any(|c| c.is_control()) {
        tracing::warn!(
            "$PATH contains a control character at install time; not baking into service unit"
        );
        return None;
    }
    Some(value)
}

pub enum Service {
    #[cfg(target_os = "linux")]
    Systemd,
    #[cfg(target_os = "macos")]
    Launchd,
    #[cfg(windows)]
    WindowsTask,
}

/// Outcome of `Service::ensure_boot_start`. Only two variants represent a
/// hard failure mode (`CheckFailed` / `EnableFailed`); the others are normal
/// and the caller just reports them in post-install hints.
#[derive(Debug)]
pub enum BootStartOutcome {
    /// Linger was already on — nothing to do.
    AlreadyEnabled,
    /// We just ran `sudo loginctl enable-linger` successfully.
    Enabled,
    /// Platform doesn't need / support a linger-style opt-in (macOS launchd).
    NotApplicable,
    /// No TTY available — sudo would hang, so we didn't try.
    NonInteractive,
    /// The user explicitly passed `--skip-linger`.
    Skipped,
    /// `loginctl show-user` failed; we can't tell whether linger is on.
    CheckFailed(String),
    /// The sudo/loginctl call itself exited non-zero (bad password, Ctrl+C, …).
    EnableFailed(String),
}

#[cfg(target_os = "linux")]
pub fn detect() -> Service {
    Service::Systemd
}

#[cfg(target_os = "macos")]
pub fn detect() -> Service {
    Service::Launchd
}

#[cfg(windows)]
pub fn detect() -> Service {
    Service::WindowsTask
}

impl Service {
    /// Write the unit file (systemd) or plist (launchd). Does not enable or
    /// start. Allows `pidash install` to gate activation on configuration.
    pub async fn write_unit(&self, paths: &Paths) -> Result<()> {
        match self {
            #[cfg(target_os = "linux")]
            Service::Systemd => systemd::write_unit(paths).await,
            #[cfg(target_os = "macos")]
            Service::Launchd => launchd::write_unit(paths).await,
            #[cfg(windows)]
            Service::WindowsTask => windows::write_unit(paths).await,
        }
    }

    /// Enable at boot/login and start now. Must run after `write_unit`.
    pub async fn enable_and_start(&self) -> Result<()> {
        match self {
            #[cfg(target_os = "linux")]
            Service::Systemd => systemd::enable_and_start().await,
            #[cfg(target_os = "macos")]
            Service::Launchd => launchd::enable_and_start().await,
            #[cfg(windows)]
            Service::WindowsTask => windows::enable_and_start().await,
        }
    }

    /// Ensure the daemon keeps running across reboots without a user login.
    /// On systemd this means `loginctl enable-linger`; on launchd user agents
    /// the equivalent is just "log in," which we can't automate. Never fails
    /// — the returned outcome drives post-install messaging.
    pub async fn ensure_boot_start(&self) -> BootStartOutcome {
        match self {
            #[cfg(target_os = "linux")]
            Service::Systemd => systemd::ensure_linger().await,
            #[cfg(target_os = "macos")]
            Service::Launchd => BootStartOutcome::NotApplicable,
            #[cfg(windows)]
            Service::WindowsTask => BootStartOutcome::NotApplicable,
        }
    }

    pub async fn uninstall(&self, paths: &Paths) -> Result<()> {
        match self {
            #[cfg(target_os = "linux")]
            Service::Systemd => systemd::uninstall(paths).await,
            #[cfg(target_os = "macos")]
            Service::Launchd => launchd::uninstall(paths).await,
            #[cfg(windows)]
            Service::WindowsTask => windows::uninstall(paths).await,
        }
    }

    pub async fn start(&self) -> Result<()> {
        match self {
            #[cfg(target_os = "linux")]
            Service::Systemd => systemd::start().await,
            #[cfg(target_os = "macos")]
            Service::Launchd => launchd::start().await,
            #[cfg(windows)]
            Service::WindowsTask => windows::start().await,
        }
    }

    pub async fn stop(&self) -> Result<()> {
        match self {
            #[cfg(target_os = "linux")]
            Service::Systemd => systemd::stop().await,
            #[cfg(target_os = "macos")]
            Service::Launchd => launchd::stop().await,
            #[cfg(windows)]
            Service::WindowsTask => windows::stop().await,
        }
    }

    pub async fn status(&self) -> Result<String> {
        match self {
            #[cfg(target_os = "linux")]
            Service::Systemd => systemd::status().await,
            #[cfg(target_os = "macos")]
            Service::Launchd => launchd::status().await,
            #[cfg(windows)]
            Service::WindowsTask => windows::status().await,
        }
    }
}
