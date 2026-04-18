use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::process::Command;

use crate::util::paths::Paths;

const LABEL: &str = "so.apple-pi-dash.runner";

pub async fn install(paths: &Paths) -> Result<()> {
    let plist_path = plist_path()?;
    if let Some(parent) = plist_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let exe = std::env::current_exe()?;
    let logs = paths.logs_dir().display().to_string();
    let config = paths.config_dir.display().to_string();
    let data = paths.data_dir.display().to_string();
    let runtime = paths.runtime_dir.display().to_string();
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>start</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>APPLE_PI_RUNNER_CONFIG_DIR</key><string>{config}</string>
    <key>APPLE_PI_RUNNER_DATA_DIR</key><string>{data}</string>
    <key>XDG_RUNTIME_DIR</key><string>{runtime}</string>
  </dict>
  <key>KeepAlive</key><true/>
  <key>RunAtLoad</key><true/>
  <key>StandardOutPath</key><string>{logs}/runner.out.log</string>
  <key>StandardErrorPath</key><string>{logs}/runner.err.log</string>
</dict>
</plist>
"#,
        label = LABEL,
        exe = exe.display(),
    );
    tokio::fs::write(&plist_path, body).await?;
    let uid = get_uid();
    let target = format!("gui/{uid}");
    let status = Command::new("launchctl")
        .args(["bootstrap", &target, &plist_path.display().to_string()])
        .status()
        .await
        .context("launchctl bootstrap")?;
    if !status.success() {
        anyhow::bail!("launchctl bootstrap failed: {status}");
    }
    println!("installed launchd agent at {}", plist_path.display());
    Ok(())
}

pub async fn uninstall(_: &Paths) -> Result<()> {
    let uid = get_uid();
    let target = format!("gui/{uid}/{LABEL}");
    Command::new("launchctl")
        .args(["bootout", &target])
        .status()
        .await
        .ok();
    let p = plist_path()?;
    if p.exists() {
        tokio::fs::remove_file(&p).await?;
    }
    println!("uninstalled launchd agent");
    Ok(())
}

pub async fn start() -> Result<()> {
    let uid = get_uid();
    let target = format!("gui/{uid}/{LABEL}");
    let status = Command::new("launchctl")
        .args(["kickstart", "-k", &target])
        .status()
        .await
        .context("launchctl kickstart")?;
    if !status.success() {
        anyhow::bail!("launchctl kickstart failed: {status}");
    }
    Ok(())
}

pub async fn stop() -> Result<()> {
    let uid = get_uid();
    let target = format!("gui/{uid}/{LABEL}");
    Command::new("launchctl")
        .args(["bootout", &target])
        .status()
        .await
        .context("launchctl bootout")?;
    Ok(())
}

pub async fn status() -> Result<String> {
    let out = Command::new("launchctl")
        .args(["list", LABEL])
        .output()
        .await?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn plist_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME not set")?;
    Ok(home
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

fn get_uid() -> u32 {
    nix::unistd::geteuid().as_raw()
}
