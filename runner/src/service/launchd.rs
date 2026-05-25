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
    // See `service::capture_install_time_path` for why we bake $PATH in.
    let path_env = super::capture_install_time_path().map(|p| xml_escape(&p));
    let body = render_plist(&exe_str, &config, &data, &logs, path_env.as_deref());
    tokio::fs::write(&plist_path, body).await?;
    println!("installed launchd agent at {}", plist_path.display());
    Ok(())
}

/// Render the LaunchAgent plist body. Deliberately does NOT set
/// `XDG_RUNTIME_DIR`: on macOS `directories::ProjectDirs` ignores it (runtime
/// dir is derived from `data_dir`), so the env var is a no-op that only
/// obscures the real path contract between the daemon and the CLI client.
///
/// `path_env`, when `Some`, is rendered as a `<key>PATH</key>` entry inside
/// `EnvironmentVariables` so the daemon (and every subprocess it forks)
/// inherits the operator's interactive PATH instead of launchd's stripped
/// default. See `service::capture_install_time_path` for the full rationale.
fn render_plist(
    exe: &str,
    config: &str,
    data: &str,
    logs: &str,
    path_env: Option<&str>,
) -> String {
    let path_entry = match path_env {
        Some(p) => format!("\n    <key>PATH</key><string>{p}</string>"),
        None => String::new(),
    };
    format!(
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
    <key>PIDASH_DATA_DIR</key><string>{data}</string>{path_entry}
  </dict>
  <key>KeepAlive</key><true/>
  <key>RunAtLoad</key><true/>
  <key>StandardOutPath</key><string>{logs}/runner.out.log</string>
  <key>StandardErrorPath</key><string>{logs}/runner.err.log</string>
</dict>
</plist>
"#,
        label = LABEL,
    )
}

/// Load the LaunchAgent. Equivalent to `start()` now that `start` handles
/// both the "not yet loaded" and "already loaded" cases; kept as a named
/// entry point so the install flow reads as `write_unit` → `enable_and_start`.
pub async fn enable_and_start() -> Result<()> {
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

/// Bring the LaunchAgent up. Tries `bootstrap` first so we can recover from
/// a fully-unloaded state (the post-`stop()` shape, since `stop` does
/// `bootout`). `RunAtLoad=true` in the plist means bootstrap also starts the
/// process. If bootstrap fails because the service is already in the domain,
/// fall back to `kickstart -k` to restart the loaded copy.
///
/// Why this matters for `pidash restart`: restart = stop + start, and
/// `stop()` calls `bootout` which fully removes the service from the user's
/// gui domain. A plain `kickstart -k` after that fails with exit 113
/// ("Could not find service … in domain"), so start MUST be able to
/// re-bootstrap, not just kickstart.
pub async fn start() -> Result<()> {
    let uid = get_uid();
    let plist = plist_path()?;
    let domain = format!("gui/{uid}");
    let target = format!("{domain}/{LABEL}");

    let bootstrap_ok = Command::new("launchctl")
        .args(["bootstrap", &domain, &plist.display().to_string()])
        .status()
        .await
        .context("launchctl bootstrap")?
        .success();
    if bootstrap_ok {
        return Ok(());
    }

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
    let out = Command::new("launchctl")
        .args(["bootout", &target])
        .output()
        .await
        .context("launchctl bootout")?;
    if out.status.success() {
        return Ok(());
    }
    // launchctl exits 3 ("No such process") when the service isn't loaded.
    // That's not an error for our callers — `restart` tolerates it, and
    // `uninstall` is happy either way. Stay quiet so `pidash restart` after
    // an update doesn't spam stderr with a benign warning.
    if let Some(3) = out.status.code() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    anyhow::bail!("launchctl bootout failed ({}): {}", out.status, stderr.trim());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_body_does_not_set_xdg_runtime_dir() {
        // On macOS `directories::ProjectDirs` ignores XDG_RUNTIME_DIR — the
        // env var was a misleading no-op. Keep it out so both backends share
        // the same runtime-dir contract.
        let body = render_plist(
            "/usr/local/bin/pidash",
            "/Users/user/Library/Application Support/pidash",
            "/Users/user/Library/Application Support/pidash",
            "/Users/user/Library/Application Support/pidash/logs",
            None,
        );
        assert!(
            !body.contains("XDG_RUNTIME_DIR"),
            "plist body must not set XDG_RUNTIME_DIR; got:\n{body}"
        );
    }

    #[test]
    fn plist_body_includes_program_args_and_logs() {
        let body = render_plist("/bin/pidash", "/cfg", "/data", "/logs", None);
        assert!(body.contains("<string>/bin/pidash</string>"));
        assert!(body.contains("<string>__run</string>"));
        assert!(body.contains("<key>PIDASH_CONFIG_DIR</key><string>/cfg</string>"));
        assert!(body.contains("<key>PIDASH_DATA_DIR</key><string>/data</string>"));
        assert!(body.contains("<string>/logs/runner.out.log</string>"));
        assert!(body.contains("<string>/logs/runner.err.log</string>"));
    }

    #[test]
    fn plist_body_omits_path_when_not_captured() {
        // None means we couldn't (or shouldn't) snapshot $PATH at install
        // time. The plist must not contain a PATH key in that case — an
        // empty PATH would be worse than launchd's default.
        let body = render_plist("/bin/pidash", "/cfg", "/data", "/logs", None);
        assert!(
            !body.contains("<key>PATH</key>"),
            "plist body must not declare PATH when path_env is None; got:\n{body}"
        );
    }

    #[test]
    fn plist_body_bakes_in_path_when_provided() {
        let body = render_plist(
            "/bin/pidash",
            "/cfg",
            "/data",
            "/logs",
            Some("/Users/u/.local/bin:/opt/homebrew/bin:/usr/bin"),
        );
        assert!(
            body.contains(
                "<key>PATH</key><string>/Users/u/.local/bin:/opt/homebrew/bin:/usr/bin</string>"
            ),
            "plist body must include captured PATH inside EnvironmentVariables; got:\n{body}"
        );
        // PATH must sit *inside* the EnvironmentVariables dict, after the
        // existing keys, not at the top-level dict alongside Label/KeepAlive.
        let env_open = body.find("<key>EnvironmentVariables</key>").unwrap();
        let path_idx = body.find("<key>PATH</key>").unwrap();
        let dict_close = body[env_open..].find("</dict>").unwrap() + env_open;
        assert!(
            path_idx > env_open && path_idx < dict_close,
            "PATH key must live inside EnvironmentVariables dict"
        );
    }
}
