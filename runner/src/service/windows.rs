use anyhow::{Context, Result};
use std::path::Path;
use tokio::process::Command;

use crate::util::paths::Paths;

const TASK_NAME: &str = "PiDash";

/// Register a per-user scheduled task that starts the daemon at logon.
///
/// This mirrors the user-level systemd/launchd setup used on Unix-like hosts:
/// it does not require administrator privileges and keeps the daemon scoped to
/// the interactive user who installed Pi Dash.
pub async fn write_unit(paths: &Paths) -> Result<()> {
    paths.ensure()?;
    let exe = std::env::current_exe().context("resolving current pidash.exe path")?;
    let exe = validate_exe_path(&exe)?;
    let task_run = format!("\"{exe}\" __run");
    run_schtasks(&[
        "/Create", "/TN", TASK_NAME, "/SC", "ONLOGON", "/TR", &task_run, "/F",
    ])
    .await?;
    println!("installed Windows scheduled task {TASK_NAME}");
    Ok(())
}

pub async fn enable_and_start() -> Result<()> {
    start().await
}

pub async fn uninstall(_paths: &Paths) -> Result<()> {
    match run_schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]).await {
        Ok(_) => {
            println!("uninstalled Windows scheduled task {TASK_NAME}");
            Ok(())
        }
        Err(e) if looks_task_missing(&e) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Self-heal for `pidash restart`: re-register the task when it's gone.
///
/// Unlike systemd/launchd there is no file to stat, so existence is decided
/// by `schtasks /Query` succeeding. Deliberately *not* by classifying its
/// error text: schtasks.exe localizes its messages, so matching the English
/// "cannot find" (as `looks_task_missing` does, acceptably, for the tolerant
/// `uninstall` path) would leave this self-heal a silent no-op on a German or
/// Japanese host — precisely the machines it exists to repair.
///
/// So a clean exit means "present, leave it alone" and anything else means
/// "try the repair". That errs toward rewriting when the query fails for some
/// other reason (access denied, schtasks.exe missing), which is the safe
/// direction: `/Create /F` either restores a working task or fails with the
/// real reason, which the caller logs before the start attempt reports it.
pub(crate) async fn rewrite_unit_if_missing(paths: &Paths) -> Result<bool> {
    if run_schtasks(&["/Query", "/TN", TASK_NAME]).await.is_ok() {
        return Ok(false);
    }
    write_unit(paths).await?;
    Ok(true)
}

pub async fn start() -> Result<()> {
    run_schtasks(&["/Run", "/TN", TASK_NAME]).await?;
    Ok(())
}

pub async fn stop() -> Result<()> {
    match run_schtasks(&["/End", "/TN", TASK_NAME]).await {
        Ok(_) => Ok(()),
        Err(e) if looks_not_running(&e) => Ok(()),
        Err(e) => Err(e),
    }
}

pub async fn status() -> Result<String> {
    run_schtasks(&["/Query", "/TN", TASK_NAME, "/FO", "LIST", "/V"]).await
}

/// Stub on Windows — `schtasks /Query` doesn't surface a comparable
/// "LastExitStatus" field, and we don't have a Windows-specific
/// diagnostic library shipped yet. Returns `None` so the caller falls
/// back to its generic message.
pub async fn diagnose_recent_exit() -> Option<String> {
    None
}

fn validate_exe_path(path: &Path) -> Result<&str> {
    let s = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("path is not valid UTF-8: {path:?}"))?;
    if s.contains(['\n', '\r', '\0']) || s.chars().any(|c| c.is_control()) {
        anyhow::bail!("path contains a control character; refusing to write task: {s:?}");
    }
    Ok(s)
}

async fn run_schtasks(args: &[&str]) -> Result<String> {
    let out = Command::new("schtasks")
        .args(args)
        .output()
        .await
        .context("running schtasks.exe")?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if out.status.success() {
        return Ok(if stdout.is_empty() { stderr } else { stdout });
    }
    let detail = if stderr.is_empty() { stdout } else { stderr };
    anyhow::bail!("schtasks.exe failed: {detail}");
}

fn looks_task_missing(e: &anyhow::Error) -> bool {
    let s = e.to_string();
    s.contains("cannot find") || s.contains("The system cannot find")
}

fn looks_not_running(e: &anyhow::Error) -> bool {
    let s = e.to_string();
    s.contains("not currently running") || s.contains("cannot find")
}
