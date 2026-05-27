use anyhow::{Context, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::process::Command;

use crate::util::paths::Paths;

const LABEL: &str = "so.pidash.daemon";

/// Upper bound on how long `stop()` will wait for `launchctl bootout` to
/// finish tearing the daemon down before escalating to SIGKILL. 10s is
/// generous compared to a healthy shutdown (typically <1s) but short
/// enough that an operator running `pidash restart` doesn't sit and wait
/// when the daemon is genuinely stuck.
const STOP_GRACE: Duration = Duration::from_secs(10);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(150);
const POST_KILL_GRACE: Duration = Duration::from_millis(500);

/// Upper bound on how long `start()` will wait for `launchctl bootstrap`
/// (or the `kickstart -k` fallback) to actually produce a running daemon
/// before giving up. 30s covers launchd's worst-case shutdown cycle for
/// the kickstart path: SIGTERM → up to TimeoutTerminateSec (20s default
/// on macOS) → SIGKILL → reap → exec, plus headroom. A healthy restart
/// returns in well under a second.
const START_GRACE: Duration = Duration::from_secs(30);
const START_POLL_INTERVAL: Duration = Duration::from_millis(150);

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

/// Bring the LaunchAgent up. Tries `bootstrap` first (which both loads and
/// starts the service, since `RunAtLoad=true`). Falls back to
/// `kickstart -k` when the label is still loaded in the user's gui
/// domain — either because launchctl told us so directly ("already
/// loaded"/"already bootstrapped") or because we asked `launchctl list`
/// and it confirmed the label is registered. Every other bootstrap
/// failure (plist missing, malformed, permission denied, …) is surfaced
/// with launchctl's own stderr text so the operator can diagnose it.
///
/// Once the launchctl call succeeds, this waits until `launchctl list`
/// reports a fresh PID before returning. Both `bootstrap` and
/// `kickstart -k` are asynchronous — they return as soon as launchd
/// accepts the request, *before* the new process has been exec'd. The
/// `kickstart -k` case is especially bad: launchctl returns immediately,
/// but the actual SIGTERM → exit → respawn cycle can take 10+ s when
/// the previous daemon is wedged (active agent run, slow network call,
/// post-wake socket drain). Callers like `restart_and_verify` then start
/// their IPC-verification clock from a "daemon is up" assumption that
/// wasn't true yet — surfacing a confusing
/// `Daemon did not answer IPC within 5s` even though the daemon comes up
/// fine a few seconds later. Waiting here moves that clock-start to the
/// moment the daemon is actually running.
///
/// Why the `launchctl list` check matters: `launchctl bootstrap` reports
/// the "you raced an in-flight teardown" case as the opaque
///   "Bootstrap failed: 5: Input/output error"
/// rather than "already loaded." Pre-fix, restart's
/// stop-then-immediately-start sequence would surface that EIO straight
/// to the user even though `kickstart -k` would have recovered it.
/// `stop()` now waits for unload to complete so the race is much rarer,
/// but the safety net stays because operators can also call
/// `pidash start` directly into a partially-loaded label.
///
/// Why kickstart isn't tried unconditionally: kickstart on a not-loaded
/// service fails with exit 113. Chaining there for *every* bootstrap
/// failure would mask the real cause behind a misleading "kickstart
/// failed" message — exactly what we want to avoid for genuine plist
/// problems.
pub async fn start() -> Result<()> {
    let uid = get_uid();
    let domain = format!("gui/{uid}");
    let target = format!("{domain}/{LABEL}");
    let plist = plist_path()?;

    // Snapshot the pre-action PID so `wait_for_running` can tell a fresh
    // daemon from the one we're about to kick. None = no daemon before.
    let pre_pid = probe_loaded_pid().await.filter(|p| *p > 0);

    let bootstrap = Command::new("launchctl")
        .arg("bootstrap")
        .arg(&domain)
        .arg(&plist)
        .output()
        .await
        .context("launchctl bootstrap")?;
    if bootstrap.status.success() {
        return wait_for_running(pre_pid, START_GRACE).await;
    }

    let bootstrap_stderr = String::from_utf8_lossy(&bootstrap.stderr);
    let label_loaded = pre_pid.is_some() || probe_loaded_pid().await.is_some();
    if !is_already_loaded_error(&bootstrap_stderr) && !label_loaded {
        anyhow::bail!(
            "launchctl bootstrap failed ({}): {}",
            bootstrap.status,
            bootstrap_stderr.trim()
        );
    }

    let kickstart = Command::new("launchctl")
        .args(["kickstart", "-k", &target])
        .output()
        .await
        .context("launchctl kickstart")?;
    if !kickstart.status.success() {
        let kickstart_stderr = String::from_utf8_lossy(&kickstart.stderr);
        anyhow::bail!(
            "launchctl kickstart failed ({}): {} (bootstrap also failed: {})",
            kickstart.status,
            kickstart_stderr.trim(),
            bootstrap_stderr.trim()
        );
    }
    wait_for_running(pre_pid, START_GRACE).await
}

/// Poll `launchctl list LABEL` until a fresh daemon is reported running:
/// a PID > 0 that differs from `pre_pid`. `pre_pid` of `None` means there
/// was no daemon registered when we called launchctl, so any PID > 0
/// counts as fresh (the install-time path).
///
/// Returns an error on timeout but does NOT signal the daemon: at this
/// point the caller has *just* asked launchd to start it, and any
/// failure to actually exec is launchd's to report (next `pidash status`
/// will pick up a non-zero LastExitStatus). Sending SIGKILL here would
/// race a daemon that's legitimately mid-init.
async fn wait_for_running(pre_pid: Option<i32>, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(pid) = probe_loaded_pid().await
            && pid > 0
            && Some(pid) != pre_pid
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(START_POLL_INTERVAL).await;
    }
    let pre_desc = pre_pid
        .map(|p| format!("pre-action pid was {p}"))
        .unwrap_or_else(|| "no daemon was registered before this start".to_string());
    anyhow::bail!(
        "service {LABEL} did not report a fresh PID within {}s of \
         bootstrap/kickstart ({pre_desc}). The launchd request was \
         accepted but the daemon never exec'd in time — check \
         `launchctl list {LABEL}` and the daemon's stderr log.",
        timeout.as_secs()
    );
}

pub async fn stop() -> Result<()> {
    let uid = get_uid();
    let target = format!("gui/{uid}/{LABEL}");
    let out = Command::new("launchctl")
        .args(["bootout", &target])
        .output()
        .await
        .context("launchctl bootout")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        // Benign: bootout against a service that isn't loaded. macOS launchctl
        // has used different exit codes for this across versions (ESRCH=3 on
        // older releases, the same "Could not find service in domain" code as
        // kickstart on newer ones), so we match on the stderr text — stable in
        // launchctl's source — instead of pinning to a single exit code. Stay
        // quiet so `pidash restart` after an update / crash doesn't spam stderr
        // with a benign warning.
        if is_not_loaded_error(&stderr) {
            return Ok(());
        }
        // Real failure. Surface launchctl's diagnostic to stderr even when the
        // caller `.ok()`s the Result (which `restart`, `uninstall`,
        // `update --restart`, and `remove` all do): the old `.status()` path
        // inherited stderr live, and operators rely on seeing it to debug a
        // wedged bootout. Then propagate a structured error for callers that
        // do check the Result.
        eprintln!("{}", stderr.trim_end());
        anyhow::bail!("launchctl bootout failed ({})", out.status);
    }

    // launchctl bootout is *asynchronous*: it returns as soon as it queues
    // SIGTERM to the daemon, NOT after the process has actually exited.
    // A follow-up `bootstrap` in the same restart sequence then races the
    // still-mid-teardown label and launchctl reports the race with the
    // misleading "Bootstrap failed: 5: Input/output error" (errno EIO),
    // because launchd hasn't yet released the label.
    //
    // Wait until launchd has actually let go of the label before returning.
    // If the daemon is wedged (e.g. stuck in a long network call inside its
    // shutdown handler — exactly what we observed in production), escalate
    // to SIGKILL on the captured PID rather than letting `pidash restart`
    // fail with a launchctl error the operator can't act on.
    wait_for_unload(STOP_GRACE).await
}

/// Poll `launchctl list LABEL` until the label is gone from the user's
/// domain. If the polite window expires, ask launchctl to SIGKILL the
/// service by label and re-check. Returns Err only if even SIGKILL
/// couldn't dislodge the label; the caller (`stop()`) currently propagates
/// that so `pidash restart` surfaces a clean "daemon would not exit"
/// rather than the cryptic EIO from the next bootstrap.
///
/// Kill is routed through `launchctl kill SIGKILL <target>` rather than a
/// direct `kill(pid, ...)` syscall: the PID we captured from `launchctl
/// list` could be stale by the time we'd signal it (sub-second teardown
/// race + PID reuse), and we don't want to risk killing an unrelated
/// process. `launchctl kill` resolves the target to the live process at
/// kill time, so the race vanishes.
async fn wait_for_unload(timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let mut last_pid: Option<i32> = None;
    loop {
        match probe_loaded_pid().await {
            None => return Ok(()),
            Some(pid) if pid > 0 => last_pid = Some(pid),
            Some(_) => {} // listed but no PID yet — keep polling
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(STOP_POLL_INTERVAL).await;
    }

    // Tell the operator we're escalating. A daemon that wedges its
    // shutdown handler is a real bug worth seeing; staying silent here
    // hid it for the original reporter.
    let pid_label = last_pid
        .map(|p| format!("pid {p}"))
        .unwrap_or_else(|| "no pid reported".to_string());
    eprintln!(
        "daemon ({pid_label}) did not exit within {}s of launchctl bootout; \
         escalating via `launchctl kill SIGKILL`",
        timeout.as_secs()
    );

    let target = format!("gui/{}/{LABEL}", get_uid());
    let _ = Command::new("launchctl")
        .args(["kill", "SIGKILL", &target])
        .status()
        .await;
    tokio::time::sleep(POST_KILL_GRACE).await;
    if probe_loaded_pid().await.is_none() {
        return Ok(());
    }
    let elapsed = timeout + POST_KILL_GRACE;
    anyhow::bail!(
        "service {LABEL} ({pid_label}) did not exit after bootout + SIGKILL within {}ms",
        elapsed.as_millis()
    );
}

/// Run `launchctl list LABEL` and return `Some(pid)` when the service is
/// loaded (pid 0 if launchd hasn't assigned one yet), or `None` when
/// launchctl exits non-zero (label not known in the user's domain).
async fn probe_loaded_pid() -> Option<i32> {
    let out = Command::new("launchctl")
        .args(["list", LABEL])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_loaded_pid(&String::from_utf8_lossy(&out.stdout)).unwrap_or(0))
}

/// Extract the PID from `launchctl list LABEL`'s plist-style stdout.
/// Returns `None` when the label is listed but no PID line is present
/// (registered but not yet exec'd, or just-exited). Pulled out as a pure
/// function so the parser has unit tests independent of launchctl.
fn parse_loaded_pid(stdout: &str) -> Option<i32> {
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("\"PID\" = ") {
            let pid_str = rest.trim_end_matches(';').trim();
            if let Ok(pid) = pid_str.parse::<i32>() {
                return Some(pid);
            }
        }
    }
    None
}

/// Pattern-match `launchctl bootstrap` stderr to decide whether the
/// failure is the benign "service is already loaded in this domain" case
/// (caller should fall back to `kickstart -k`) or a real configuration
/// problem that deserves a hard error. Matches on stderr text rather
/// than exit codes because the codes drift across macOS releases.
fn is_already_loaded_error(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    s.contains("already loaded")
        || s.contains("already bootstrapped")
        || s.contains("operation already in progress")
}

/// Pattern-match `launchctl bootout` stderr to decide whether the
/// failure is the benign "service isn't loaded in the first place" case
/// (no-op the caller can ignore) or a real teardown failure.
fn is_not_loaded_error(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    s.contains("no such process")
        || s.contains("could not find service")
        || s.contains("not loaded")
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
    fn not_loaded_classifier_matches_observed_launchctl_text() {
        // The exact stderr the reporter saw (macOS Tahoe, errno-style):
        assert!(is_not_loaded_error("Boot-out failed: 3: No such process"));
        // Newer macOS phrasing — same wording kickstart uses for the
        // not-in-domain case; bootout has been observed to share it:
        assert!(is_not_loaded_error(
            "Could not find service \"so.pidash.daemon\" in domain for user gui: 501"
        ));
        // Variant seen on some releases:
        assert!(is_not_loaded_error("Service not loaded"));
        // Real failures we MUST surface, not silently absorb:
        assert!(!is_not_loaded_error("Operation not permitted"));
        assert!(!is_not_loaded_error("Bootstrap failed: 5: Input/output error"));
        assert!(!is_not_loaded_error(""));
    }

    #[test]
    fn already_loaded_classifier_matches_bootstrap_eexist_variants() {
        // EALREADY (37) is the canonical "already loaded" code; launchctl
        // prints its strerror:
        assert!(is_already_loaded_error(
            "Bootstrap failed: 37: Operation already in progress"
        ));
        // Verbose variants seen across macOS versions:
        assert!(is_already_loaded_error(
            "Service already loaded in this domain"
        ));
        assert!(is_already_loaded_error(
            "Service is already bootstrapped in domain for user gui: 501"
        ));
        // The classifier rejects EIO — the "label loaded but launchctl
        // returned EIO" recovery now flows through `probe_loaded_pid()`
        // in `start()`, not through this matcher. Keeping the classifier
        // strict here means a legitimately-bad bootstrap (plist missing,
        // malformed) still produces its own error rather than a
        // misleading "kickstart failed: exit status: 113".
        assert!(!is_already_loaded_error(
            "Bootstrap failed: 5: Input/output error"
        ));
        assert!(!is_already_loaded_error(
            "Load failed: 2: No such file or directory"
        ));
        assert!(!is_already_loaded_error("Path had bad ownership/permissions"));
        assert!(!is_already_loaded_error(""));
    }

    #[test]
    fn parse_loaded_pid_extracts_pid_from_launchctl_list_output() {
        // Exact shape of `launchctl list so.pidash.daemon` from the
        // reporter's session — the EIO repro we cut this fix from.
        let stdout = r#"{
	"StandardOutPath" = "/Users/irichard/Library/Application Support/so.pidash.pidash/logs/runner.out.log";
	"LimitLoadToSessionType" = "Aqua";
	"StandardErrorPath" = "/Users/irichard/Library/Application Support/so.pidash.pidash/logs/runner.err.log";
	"Label" = "so.pidash.daemon";
	"OnDemand" = false;
	"LastExitStatus" = 0;
	"PID" = 884;
	"Program" = "/Users/irichard/.local/bin/pidash";
};
"#;
        assert_eq!(parse_loaded_pid(stdout), Some(884));
    }

    #[test]
    fn parse_loaded_pid_returns_none_when_label_listed_without_pid() {
        // A label can be loaded but have no PID assigned yet (between
        // bootstrap and exec) or after the process has just exited but
        // launchd hasn't reaped the row. `wait_for_unload` treats this
        // as "still loaded, keep polling," which depends on parse
        // distinguishing "no PID line" from "PID = N".
        let stdout = r#"{
	"Label" = "so.pidash.daemon";
	"OnDemand" = false;
	"LastExitStatus" = 0;
};
"#;
        assert_eq!(parse_loaded_pid(stdout), None);
    }

    #[test]
    fn parse_loaded_pid_ignores_unrelated_quoted_keys() {
        // Defence against false positives — only the "PID" key parses
        // as a pid, not e.g. a "PPID" or "LastExitStatus" line.
        let stdout = r#"{
	"PPID" = 1;
	"LastExitStatus" = 0;
	"Label" = "so.pidash.daemon";
};
"#;
        assert_eq!(parse_loaded_pid(stdout), None);
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
