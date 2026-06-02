use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
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

/// Bring the LaunchAgent up. Tries `bootstrap` first; falls back to
/// `kickstart -k` when the label is already loaded. Then waits until
/// `launchctl list` reports a fresh PID before returning, so callers
/// can treat a successful return as "daemon is actually exec'd."
///
/// # Why the fallback-and-wait dance
///
/// Both `launchctl bootstrap` and `launchctl kickstart -k` are
/// asynchronous: they return once launchd accepts the request, before
/// the new process is exec'd. The `kickstart -k` case is the painful
/// one — SIGTERM → exit → respawn can take 10+ s when the previous
/// daemon is wedged (active agent run, slow network call, post-wake
/// socket drain). Without the wait, `restart_and_verify`'s IPC clock
/// starts before the daemon exists and we surface a misleading
/// `Daemon did not answer IPC within 5s`.
///
/// The fallback exists because `launchctl bootstrap` reports the
/// "raced an in-flight teardown" case as the opaque
/// `Bootstrap failed: 5: Input/output error` rather than "already
/// loaded." `stop()` now waits for unload to complete so the race
/// is rare, but the `probe_loaded_pid` check stays as a safety net
/// for operators calling `pidash start` into a partially-loaded label.
///
/// We do NOT chain to kickstart unconditionally: kickstart on a
/// not-loaded service fails with exit 113, which would mask genuine
/// plist errors (missing, malformed, permission denied) behind a
/// confusing "kickstart failed."
pub async fn start() -> Result<()> {
    let uid = get_uid();
    let domain = format!("gui/{uid}");
    let target = format!("{domain}/{LABEL}");
    let plist = plist_path()?;

    // Precondition: the plist must exist before we hand its path to
    // launchctl. macOS reports "plist not found" as the opaque
    // `Bootstrap failed: 5: Input/output error` — indistinguishable
    // from the teardown-race EIO without out-of-band knowledge — so
    // surface it here with an actionable hint. `pidash restart` rewrites
    // the plist via `reload::restart_and_verify_with_progress` before
    // reaching this point; this bail only fires for direct `pidash start`
    // callers on a plist-less machine.
    ensure_plist_present(&plist)?;

    // Snapshot the pre-action PID so `wait_for_running` can tell a fresh
    // daemon from the one we're about to kick. None = no daemon before.
    // If another caller restarts the daemon between this snapshot and
    // our bootstrap/kickstart, the freshness predicate still works
    // correctly: any later PID we see will differ from this snapshot.
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

/// Poll `launchctl list LABEL` until [`is_fresh_pid`] accepts what we
/// see. Returns an error on timeout but does NOT signal the daemon: at
/// this point the caller has *just* asked launchd to start it, and any
/// failure to actually exec is launchd's to report (next `pidash status`
/// will pick up a non-zero LastExitStatus). Sending SIGKILL here would
/// race a daemon that's legitimately mid-init.
async fn wait_for_running(pre_pid: Option<i32>, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if is_fresh_pid(probe_loaded_pid().await, pre_pid) {
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

/// Decide whether the latest `launchctl list` probe shows a daemon we
/// should accept as the post-start instance. `current` is whatever
/// `probe_loaded_pid` just returned; `pre` is what we captured before
/// bootstrap/kickstart.
///
/// Accept = the daemon exists (PID > 0) AND its PID differs from the
/// pre-action snapshot — meaning launchd actually went through a
/// respawn cycle, not just kept the old process running.
///
/// Caveat: if launchd were to re-assign the *same* PID to the new
/// daemon (theoretical — macOS uses a pseudo-random PID walk and does
/// not reuse PIDs back-to-back), this would never return true and
/// `wait_for_running` would time out on a daemon that's actually
/// healthy. Not seen in practice and not worth pre-engineering for.
fn is_fresh_pid(current: Option<i32>, pre: Option<i32>) -> bool {
    matches!(current, Some(pid) if pid > 0 && Some(pid) != pre)
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
    parse_int_field(stdout, "PID")
}

/// Extract `LastExitStatus` from `launchctl list LABEL`'s plist-style
/// stdout. launchd exposes the wait-status integer: signal deaths live
/// in the low bits (`9` means SIGKILL), while ordinary exits are shifted
/// (`exit(78)` shows up as `19968`). Returns `None` when the field is
/// absent (label is loaded but has never exited).
fn parse_last_exit_status(stdout: &str) -> Option<i32> {
    parse_int_field(stdout, "LastExitStatus")
}

/// Shared parser for `"<KEY>" = <int>;` lines in launchctl's output.
fn parse_int_field(stdout: &str, key: &str) -> Option<i32> {
    let prefix = format!("\"{key}\" = ");
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(prefix.as_str()) {
            let val = rest.trim_end_matches(';').trim();
            if let Ok(n) = val.parse::<i32>() {
                return Some(n);
            }
        }
    }
    None
}

/// Inspect `launchctl list LABEL` and, if the daemon is currently NOT
/// running and its last exit looks problematic (signal-killed or
/// non-zero), return a human-readable diagnosis pointing the operator
/// at the likely cause. Returns `None` when the daemon is currently
/// running, when the label isn't loaded, or when the previous exit was
/// a clean `exit(0)`.
///
/// Called from `restart_and_verify` when the IPC verification times
/// out, so users see "AMFI killed the daemon — try resigning" instead
/// of the generic "Daemon did not answer IPC within 5s" when the
/// underlying problem is that the daemon crashed during startup.
pub async fn diagnose_recent_exit() -> Option<String> {
    let out = Command::new("launchctl")
        .args(["list", LABEL])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None; // label not loaded — nothing to diagnose
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let pid = parse_loaded_pid(&stdout);
    // If the daemon is currently running, this isn't a "daemon died" case.
    if matches!(pid, Some(p) if p > 0) {
        return None;
    }
    let raw_status = parse_last_exit_status(&stdout)?;
    let status = decode_launchd_exit_status(raw_status);
    if status == LaunchdExitStatus::Exited(0) {
        return None; // clean exit — IPC must have failed for another reason
    }
    Some(describe_exit_status(status, raw_status))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchdExitStatus {
    Exited(i32),
    Signaled { signal: i32, core_dumped: bool },
    Other(i32),
}

/// Decode launchd's `LastExitStatus` as the raw wait status it reports
/// in the per-label plist-style output. For example, SIGKILL is `9`,
/// SIGABRT with a core flag is `134`, and `exit(78)` is `19968`.
fn decode_launchd_exit_status(raw_status: i32) -> LaunchdExitStatus {
    // Be tolerant of callers passing the legacy `launchctl list` status
    // column form, where a signal can be represented as `-SIGNAL`.
    if raw_status < 0 {
        return LaunchdExitStatus::Signaled {
            signal: -raw_status,
            core_dumped: false,
        };
    }

    let termsig = raw_status & 0x7f;
    if termsig == 0 {
        return LaunchdExitStatus::Exited((raw_status >> 8) & 0xff);
    }
    if termsig != 0x7f {
        return LaunchdExitStatus::Signaled {
            signal: termsig,
            core_dumped: raw_status & 0x80 != 0,
        };
    }
    LaunchdExitStatus::Other(raw_status)
}

/// Translate a decoded launchd `LastExitStatus` into an
/// operator-actionable message. Pure function so it's unit-testable.
fn describe_exit_status(status: LaunchdExitStatus, raw_status: i32) -> String {
    match status {
        LaunchdExitStatus::Signaled { signal: 9, .. } => format!(
            "Daemon was killed by SIGKILL (LastExitStatus={raw_status}) shortly after launchd \
             exec'd it. On macOS this is almost always a code-signing rejection by \
             AMFI — common when a freshly built binary's signature isn't trusted \
             for launchd-spawned processes even though it runs fine from the shell. \
             Verify with: `codesign -v /Users/<you>/.local/bin/pidash`. \
             For details: `log show --last 5m --predicate \
             'subsystem == \"com.apple.kernel.amfi\"'`. \
             Re-sign locally with `codesign --force --sign - <binary>` if needed; \
             release builds shipped via `pidash-installer.sh` are Developer-ID \
             signed and unaffected."
        ),
        LaunchdExitStatus::Signaled {
            signal: 6,
            core_dumped,
        } => format!(
            "Daemon aborted (LastExitStatus={raw_status} = SIGABRT{}) shortly after launch. \
             Most likely a Rust panic or a libc assertion. Check the daemon's \
             stderr log at `~/Library/Application Support/so.pidash.pidash/logs/runner.err.log` \
             (macOS) for the panic message.",
            if core_dumped { ", core dumped" } else { "" }
        ),
        LaunchdExitStatus::Signaled {
            signal: 11,
            core_dumped,
        } => format!(
            "Daemon segfaulted (LastExitStatus={raw_status} = SIGSEGV{}) shortly after \
              launch. Likely a native crash. Check the stderr log and consider \
              running with `RUST_BACKTRACE=1` baked into the launchd plist.",
            if core_dumped { ", core dumped" } else { "" }
        ),
        LaunchdExitStatus::Signaled { signal, .. } => format!(
            "Daemon was killed by signal {signal} (LastExitStatus={raw_status}) shortly after \
             launch. Check the daemon's stderr log for context.",
        ),
        LaunchdExitStatus::Exited(code) => format!(
            "Daemon exited with non-zero status {code} (LastExitStatus={raw_status}) shortly \
             after launch. \
             Usually a config / credential error — check the daemon's stderr \
             log at `~/Library/Application Support/so.pidash.pidash/logs/runner.err.log`."
        ),
        LaunchdExitStatus::Other(raw) => format!(
            "Daemon stopped with unrecognized launchd LastExitStatus={raw}. \
             Check the daemon's stderr log at `~/Library/Application Support/so.pidash.pidash/logs/runner.err.log`."
        ),
    }
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

/// Pure path-existence check, extracted so the error message is unit-testable.
fn ensure_plist_present(plist: &Path) -> Result<()> {
    if plist.exists() {
        return Ok(());
    }
    anyhow::bail!(
        "launchd plist missing at {}. Run `pidash install` to (re)write it; \
         `pidash update` only swaps the binary and does not touch the plist.",
        plist.display()
    );
}

/// Self-heal entry point used by `reload::restart_and_verify_with_progress`:
/// returns `Ok(true)` when the plist was missing and got rewritten, `Ok(false)`
/// when it was already present. Lets `pidash restart` after `pidash update` on
/// a plist-less machine recover transparently instead of bailing with the
/// `ensure_plist_present` error the operator would have to act on by hand.
///
/// We only rewrite when missing (not unconditionally on every restart) so
/// operators hand-editing the plist for debugging don't get clobbered.
pub(crate) async fn rewrite_unit_if_missing(paths: &Paths) -> Result<bool> {
    let plist = plist_path()?;
    if plist.exists() {
        return Ok(false);
    }
    write_unit(paths).await?;
    Ok(true)
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
    fn is_fresh_pid_accepts_any_positive_pid_on_fresh_install() {
        // Install path: no daemon registered before `start()` ran. The
        // first PID launchd assigns is by definition fresh, whatever
        // its value.
        assert!(is_fresh_pid(Some(1234), None));
        assert!(is_fresh_pid(Some(2), None));
    }

    #[test]
    fn is_fresh_pid_accepts_only_a_different_pid_on_restart() {
        // Restart path: pre-action PID was 884. While we're still
        // looking at PID 884 (kickstart hasn't actually killed it yet,
        // or has killed it and respawned with the same PID), reject.
        // Once we see a different PID, accept.
        assert!(!is_fresh_pid(Some(884), Some(884)));
        assert!(is_fresh_pid(Some(12345), Some(884)));
    }

    #[test]
    fn is_fresh_pid_rejects_no_pid_yet() {
        // launchctl listed the label but hasn't assigned a PID — the
        // transient window between load and exec. Keep polling.
        assert!(!is_fresh_pid(Some(0), Some(884)));
        assert!(!is_fresh_pid(Some(0), None));
    }

    #[test]
    fn is_fresh_pid_rejects_label_gone() {
        // probe_loaded_pid returns None when launchctl reports the
        // label isn't in the domain at all — e.g. bootstrap silently
        // didn't take, or someone else just `bootout`'d it. Keep
        // polling: the bootstrap/kickstart we issued may still be in
        // flight.
        assert!(!is_fresh_pid(None, None));
        assert!(!is_fresh_pid(None, Some(884)));
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
    fn parse_last_exit_status_extracts_signal_value() {
        // Exact shape of `launchctl list so.pidash.daemon` after a
        // daemon dies from SIGKILL — the AMFI repro that motivated
        // the diagnosis path.
        let stdout = r#"{
	"Label" = "so.pidash.daemon";
	"OnDemand" = false;
	"LastExitStatus" = 9;
	"Program" = "/Users/u/.local/bin/pidash";
};
"#;
        assert_eq!(parse_last_exit_status(stdout), Some(9));
    }

    #[test]
    fn parse_last_exit_status_returns_none_when_absent() {
        // Fresh load, no prior exit yet — no LastExitStatus line at all.
        let stdout = r#"{
	"Label" = "so.pidash.daemon";
	"OnDemand" = false;
};
"#;
        assert_eq!(parse_last_exit_status(stdout), None);
    }

    #[test]
    fn parse_last_exit_status_handles_clean_zero() {
        let stdout = r#"{
	"Label" = "so.pidash.daemon";
	"LastExitStatus" = 0;
};
"#;
        assert_eq!(parse_last_exit_status(stdout), Some(0));
    }

    #[test]
    fn describe_exit_status_calls_out_sigkill_amfi() {
        let msg = describe_exit_status(decode_launchd_exit_status(9), 9);
        // The SIGKILL message MUST point at AMFI / codesign — that's
        // the load-bearing user-facing diagnosis. If someone edits
        // the message and loses that hint, this test should fail.
        assert!(msg.contains("SIGKILL"));
        assert!(msg.contains("AMFI") || msg.to_lowercase().contains("code-sign"));
        assert!(msg.contains("codesign"));
    }

    #[test]
    fn describe_exit_status_calls_out_sigabrt_panic() {
        let msg = describe_exit_status(decode_launchd_exit_status(6), 6);
        assert!(msg.contains("SIGABRT") || msg.contains("aborted"));
        assert!(
            msg.to_lowercase().contains("panic"),
            "SIGABRT message should mention Rust panic as the likely cause; got: {msg}"
        );
    }

    #[test]
    fn decode_launchd_exit_status_handles_raw_wait_statuses() {
        assert_eq!(decode_launchd_exit_status(0), LaunchdExitStatus::Exited(0));
        // launchd's plist-style LastExitStatus uses the raw wait status:
        // exit code 1 is 1 << 8, not plain 1.
        assert_eq!(
            decode_launchd_exit_status(256),
            LaunchdExitStatus::Exited(1)
        );
        assert_eq!(
            decode_launchd_exit_status(19968),
            LaunchdExitStatus::Exited(78)
        );
        // Signals occupy the low bits; the core-dump bit may also be set.
        assert_eq!(
            decode_launchd_exit_status(9),
            LaunchdExitStatus::Signaled {
                signal: 9,
                core_dumped: false
            }
        );
        assert_eq!(
            decode_launchd_exit_status(134),
            LaunchdExitStatus::Signaled {
                signal: 6,
                core_dumped: true
            }
        );
        assert_eq!(
            decode_launchd_exit_status(139),
            LaunchdExitStatus::Signaled {
                signal: 11,
                core_dumped: true
            }
        );
    }

    #[test]
    fn describe_exit_status_handles_arbitrary_signals_and_exit_codes() {
        // SIGTERM = 15: signal range, but no canned message — fall
        // through to the "killed by signal N" branch.
        let sig = describe_exit_status(decode_launchd_exit_status(15), 15);
        assert!(sig.contains("signal 15"));
        // Non-signal exit (e.g. config error, status 78 = EX_CONFIG).
        let cfg = describe_exit_status(decode_launchd_exit_status(19968), 19968);
        assert!(cfg.contains("78"));
        assert!(cfg.contains("19968"));
        assert!(
            cfg.to_lowercase().contains("config") || cfg.to_lowercase().contains("non-zero"),
            "high exit codes should suggest config/credential errors; got: {cfg}"
        );
    }

    #[test]
    fn ensure_plist_present_errors_with_install_hint_when_missing() {
        // The error must point the operator at `pidash install` (the
        // command that writes the plist) and include the missing path,
        // replacing the cryptic `Bootstrap failed: 5: Input/output error`
        // launchctl returns for a missing plist. Use a tempdir-scoped
        // path so parallel test invocations can't collide.
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("so.pidash.daemon.plist");
        let err = ensure_plist_present(&missing).expect_err("missing plist must error");
        let msg = format!("{err}");
        assert!(
            msg.contains("pidash install"),
            "missing-plist error must point at `pidash install`; got: {msg}"
        );
        assert!(
            msg.contains(&missing.display().to_string()),
            "missing-plist error must include the path; got: {msg}"
        );
    }

    #[test]
    fn ensure_plist_present_ok_when_file_exists() {
        // The function only checks `exists()` so the contents don't
        // matter. tempdir-scoped so parallel runs can't race on the
        // same path.
        let dir = tempfile::tempdir().expect("tempdir");
        let present = dir.path().join("so.pidash.daemon.plist");
        std::fs::write(&present, b"x").unwrap();
        assert!(ensure_plist_present(&present).is_ok());
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
