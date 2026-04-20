use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::process::Command;

use crate::util::paths::Paths;

const LABEL: &str = "so.pidash.daemon";

/// Write the LaunchAgent plist. Does NOT bootstrap (load) it; that's deferred
/// to `enable_and_start` so `pidash install` can gate activation on
/// `pidash configure` completing first.
pub async fn write_unit(paths: &Paths) -> Result<()> {
    let plist_path = plist_path()?;
    if let Some(parent) = plist_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let exe = std::env::current_exe()?;
    let exe_str = xml_escape(super::validate_path_for_unit(&exe)?);
    let logs_dir = paths.logs_dir();
    let logs = xml_escape(super::validate_path_for_unit(&logs_dir)?);
    let config = xml_escape(super::validate_path_for_unit(&paths.config_dir)?);
    let data = xml_escape(super::validate_path_for_unit(&paths.data_dir)?);
    let runtime = xml_escape(super::validate_path_for_unit(&paths.runtime_dir)?);
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>__run</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PIDASH_CONFIG_DIR</key><string>{config}</string>
    <key>PIDASH_DATA_DIR</key><string>{data}</string>
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
        exe = exe_str,
    );
    tokio::fs::write(&plist_path, body).await?;
    println!("installed launchd agent at {}", plist_path.display());
    Ok(())
}

/// Load the LaunchAgent. `RunAtLoad=true` in the plist means `bootstrap`
/// also starts the process immediately. If already loaded, falls back to
/// `kickstart -k` so re-running `pidash install` after a crash-free run
/// doesn't fail — it just ensures the service is running.
pub async fn enable_and_start() -> Result<()> {
    let plist = plist_path()?;
    let uid = get_uid();
    let target = format!("gui/{uid}");
    let status = Command::new("launchctl")
        .args(["bootstrap", &target, &plist.display().to_string()])
        .status()
        .await
        .context("launchctl bootstrap")?;
    if status.success() {
        return Ok(());
    }
    // Already loaded → make sure it's running. `kickstart -k` restarts a
    // loaded service; on a fresh machine (no prior load) this branch isn't
    // taken because bootstrap succeeded above.
    start().await
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

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}
