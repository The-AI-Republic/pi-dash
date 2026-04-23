use anyhow::{Context, Result};
use std::io::IsTerminal;
use std::path::PathBuf;
use tokio::process::Command;

use super::BootStartOutcome;
use crate::util::paths::Paths;

const UNIT_NAME: &str = "pidash.service";

/// Write the unit file and reload the systemd user manager. Does NOT enable
/// or start the unit — that's a separate step so `pidash install` can gate
/// it on `pidash configure` completing first.
pub async fn write_unit(paths: &Paths) -> Result<()> {
    let unit_path = unit_path()?;
    if let Some(parent) = unit_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let exe = std::env::current_exe()?;
    let exe_str = super::validate_path_for_unit(&exe)?.to_string();
    let config_dir = super::validate_path_for_unit(&paths.config_dir)?.to_string();
    let data_dir = super::validate_path_for_unit(&paths.data_dir)?.to_string();
    let runtime_dir = super::validate_path_for_unit(&paths.runtime_dir)?.to_string();
    let body = format!(
        r#"[Unit]
Description=Pi Dash Runner
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={exe} __run
Environment=PIDASH_CONFIG_DIR={config_dir}
Environment=PIDASH_DATA_DIR={data_dir}
Environment=XDG_RUNTIME_DIR={runtime_dir}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
        exe = exe_str,
    );
    tokio::fs::write(&unit_path, body).await?;
    run_systemctl(&["daemon-reload"]).await?;
    println!("installed systemd unit at {}", unit_path.display());
    Ok(())
}

/// Enable the unit at boot/login and bring it up. `restart` (not `start`) so
/// a re-configure against an already-running daemon forces it to reload the
/// freshly-written `credentials.toml`. On a stopped unit `restart` just
/// starts it — same net effect as before for the first-install path.
pub async fn enable_and_start() -> Result<()> {
    run_systemctl(&["enable", UNIT_NAME]).await?;
    run_systemctl(&["restart", UNIT_NAME]).await?;
    Ok(())
}

pub async fn uninstall(_: &Paths) -> Result<()> {
    run_systemctl(&["disable", UNIT_NAME]).await.ok();
    let p = unit_path()?;
    if p.exists() {
        tokio::fs::remove_file(&p).await?;
    }
    run_systemctl(&["daemon-reload"]).await.ok();
    println!("uninstalled systemd unit");
    Ok(())
}

pub async fn start() -> Result<()> {
    run_systemctl(&["start", UNIT_NAME]).await
}

pub async fn stop() -> Result<()> {
    run_systemctl(&["stop", UNIT_NAME]).await
}

/// Detect whether `loginctl enable-linger` has already been applied to the
/// current user. Linger is what makes the user-level systemd manager (and
/// therefore our user unit) start at boot rather than at first login.
pub async fn is_linger_enabled() -> Result<bool> {
    let user = std::env::var("USER").context("USER env not set")?;
    let out = Command::new("loginctl")
        .args(["show-user", &user, "--property=Linger"])
        .output()
        .await
        .context("invoking loginctl show-user")?;
    if !out.status.success() {
        anyhow::bail!(
            "loginctl show-user {user} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim() == "Linger=yes")
}

/// Shell out to `sudo loginctl enable-linger <user>`. sudo runs inline on
/// the caller's TTY, so any password prompt is surfaced to the user directly
/// — we don't feed stdin or try to cache credentials ourselves.
pub async fn enable_linger() -> Result<()> {
    let user = std::env::var("USER").context("USER env not set")?;
    let status = Command::new("sudo")
        .args(["loginctl", "enable-linger", &user])
        .status()
        .await
        .context("invoking sudo loginctl enable-linger")?;
    if !status.success() {
        anyhow::bail!("sudo loginctl enable-linger {user} exited with {status}");
    }
    Ok(())
}

/// Best-effort: enable linger so `pidash.service` survives reboot without a
/// login. Never fails the caller — every branch that can't apply linger maps
/// to an outcome the caller prints as a hint. Skips the sudo call entirely
/// when there's no TTY (sudo would hang or fail on non-interactive runs).
pub async fn ensure_linger() -> BootStartOutcome {
    match is_linger_enabled().await {
        Ok(true) => return BootStartOutcome::AlreadyEnabled,
        Ok(false) => {}
        Err(e) => return BootStartOutcome::CheckFailed(e.to_string()),
    }
    if !std::io::stdin().is_terminal() {
        return BootStartOutcome::NonInteractive;
    }
    match enable_linger().await {
        Ok(()) => BootStartOutcome::Enabled,
        Err(e) => BootStartOutcome::EnableFailed(e.to_string()),
    }
}

pub async fn status() -> Result<String> {
    let out = Command::new("systemctl")
        .args(["--user", "is-active", UNIT_NAME])
        .output()
        .await
        .context("invoking systemctl")?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn unit_path() -> Result<PathBuf> {
    let home = dirs_home()?;
    Ok(home.join(".config/systemd/user").join(UNIT_NAME))
}

async fn run_systemctl(args: &[&str]) -> Result<()> {
    let mut full = vec!["--user"];
    full.extend_from_slice(args);
    let status = Command::new("systemctl")
        .args(&full)
        .status()
        .await
        .context("invoking systemctl")?;
    if !status.success() {
        anyhow::bail!("systemctl {full:?} failed: {status}");
    }
    Ok(())
}

fn dirs_home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME not set")
}
