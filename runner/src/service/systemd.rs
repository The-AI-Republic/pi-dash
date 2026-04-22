use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::process::Command;

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
