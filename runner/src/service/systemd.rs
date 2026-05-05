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
    let exe_str = super::validate_path_for_unit(&exe)?;
    let config_dir = super::validate_path_for_unit(&paths.config_dir)?;
    let data_dir = super::validate_path_for_unit(&paths.data_dir)?;
    // See `service::capture_install_time_path` for why we bake $PATH in.
    let path_env = super::capture_install_time_path();
    let body = render_unit(exe_str, config_dir, data_dir, path_env.as_deref());
    tokio::fs::write(&unit_path, body).await?;
    run_systemctl(&["daemon-reload"]).await?;
    println!("installed systemd unit at {}", unit_path.display());
    Ok(())
}

/// Render the user-unit body. Deliberately does NOT set `XDG_RUNTIME_DIR`:
/// systemd's user manager already exports it as `/run/user/$UID`, and the
/// daemon + CLI client both derive the socket path from that via
/// `ProjectDirs`. Overriding it here once caused a double `pidash/` path
/// component and left the client unable to reach the daemon.
///
/// `path_env`, when `Some`, is rendered as an `Environment=PATH=...` line
/// so the daemon (and every subprocess it forks) inherits the operator's
/// interactive PATH instead of `systemd --user`'s minimal default. See
/// `service::capture_install_time_path` for the full rationale.
fn render_unit(
    exe: &str,
    config_dir: &str,
    data_dir: &str,
    path_env: Option<&str>,
) -> String {
    let path_line = match path_env {
        Some(p) => format!("\nEnvironment=\"PATH={}\"", systemd_double_quoted_escape(p)),
        None => String::new(),
    };
    format!(
        r#"[Unit]
Description=Pi Dash Runner
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={exe} __run
Environment=PIDASH_CONFIG_DIR={config_dir}
Environment=PIDASH_DATA_DIR={data_dir}{path_line}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#
    )
}

/// Escape a value to be safe inside a systemd `Environment="KEY=VALUE"`
/// directive. Per `man systemd.syntax(7)`, the only valid C-style escapes
/// inside a double-quoted string are `\a \b \f \n \r \t \v \\ \" \' \s
/// \xXX \NNN`. `\$` and `` \` `` are *not* recognized: systemd-analyze
/// rejects them as `Invalid syntax, ignoring`, silently dropping the
/// entire directive. systemd does no `$VAR` or backtick expansion in
/// `Environment=` values either, so `$` and backtick pass through
/// verbatim and need no escaping.
///
/// We therefore only escape `\` and `"`, the two characters that are
/// genuinely special inside the double quotes. A pathological PATH entry
/// containing either still produces a valid unit; everything else is left
/// untouched.
fn systemd_double_quoted_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_body_does_not_set_xdg_runtime_dir() {
        // Regression: setting XDG_RUNTIME_DIR to the project-scoped runtime
        // dir caused `ProjectDirs::runtime_dir()` inside the daemon to append
        // `pidash/` a second time, producing a socket path the CLI client
        // couldn't reach.
        let body = render_unit(
            "/home/user/.cargo/bin/pidash",
            "/home/user/.config/pidash",
            "/home/user/.local/share/pidash",
            None,
        );
        assert!(
            !body.contains("XDG_RUNTIME_DIR"),
            "unit body must not set XDG_RUNTIME_DIR; got:\n{body}"
        );
    }

    #[test]
    fn unit_body_includes_exec_start_and_dirs() {
        let body = render_unit("/bin/pidash", "/etc/pidash", "/var/lib/pidash", None);
        assert!(body.contains("ExecStart=/bin/pidash __run"));
        assert!(body.contains("Environment=PIDASH_CONFIG_DIR=/etc/pidash"));
        assert!(body.contains("Environment=PIDASH_DATA_DIR=/var/lib/pidash"));
    }

    #[test]
    fn unit_body_omits_path_when_not_captured() {
        let body = render_unit("/bin/pidash", "/etc/pidash", "/var/lib/pidash", None);
        assert!(
            !body.contains("Environment=\"PATH=") && !body.contains("Environment=PATH="),
            "unit body must not declare PATH when path_env is None; got:\n{body}"
        );
    }

    #[test]
    fn unit_body_bakes_in_path_when_provided() {
        let body = render_unit(
            "/bin/pidash",
            "/etc/pidash",
            "/var/lib/pidash",
            Some("/home/user/.local/bin:/usr/local/bin:/usr/bin"),
        );
        assert!(
            body.contains(
                "Environment=\"PATH=/home/user/.local/bin:/usr/local/bin:/usr/bin\""
            ),
            "unit body must include captured PATH; got:\n{body}"
        );
    }

    #[test]
    fn systemd_escape_handles_specials() {
        // Inside `Environment="..."`, only `"` and `\` are valid C-style
        // escapes per `man systemd.syntax(7)`. `$` and backtick have no
        // special meaning in `Environment=` values (no expansion happens),
        // so they must pass through verbatim — escaping them as `\$` /
        // `` \` `` produces an unrecognized C-escape that systemd-analyze
        // rejects as "Invalid syntax, ignoring", silently dropping the
        // entire PATH assignment.
        assert_eq!(systemd_double_quoted_escape("a/b/c"), "a/b/c");
        assert_eq!(systemd_double_quoted_escape("/foo$bar"), "/foo$bar");
        assert_eq!(systemd_double_quoted_escape("/back`tick"), "/back`tick");
        assert_eq!(systemd_double_quoted_escape("/quo\"ted"), "/quo\\\"ted");
        assert_eq!(systemd_double_quoted_escape("a\\b"), "a\\\\b");
    }
}
