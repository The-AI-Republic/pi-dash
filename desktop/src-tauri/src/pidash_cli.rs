// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Detection + installation of the `pidash` CLI (the OSS local runner).
//!
//! The OSS team ships `pidash` via cargo-dist; every `pidash-v*.*.*` tag in
//! github.com/The-AI-Republic/pi-dash produces per-platform binaries plus
//! `pidash-installer.sh` (sh/dash, macOS+Linux) and `pidash-installer.ps1`
//! (PowerShell, Windows) that download from that release into
//! `$HOME/.local/bin` (or `%LOCALAPPDATA%\Programs\pidash\`) and add the
//! directory to the user's PATH.
//!
//! The desktop wrapper consumes the runner over IPC for the local-runner
//! feature (desktop+runner on the same machine talk directly, bypassing the
//! cloud's WebSocket reroute). This module exposes two Tauri commands:
//!
//!   * [`detect_pidash_cli`] — non-destructive probe; returns whether and
//!     where the binary is installed plus its reported version.
//!   * [`install_pidash_cli`] — gated behind UI confirmation; downloads
//!     and runs the OSS installer for the current OS. Streams stdout/stderr
//!     line-by-line back to the webview as ``pidash-install-log`` events
//!     so the UI can render install progress.
//!
//! Security posture: the installer URL is hardcoded to a specific GitHub
//! release path. JS in the webview cannot influence which script is run
//! or where it downloads from — it can only `invoke('install_pidash_cli')`,
//! and the command itself decides the rest. This is the explicit reason
//! we did NOT add `tauri-plugin-shell` and surface raw shell execution.
//!
//! UI rendering contract: the install log lines emitted on
//! `pidash-install-log` events MUST be rendered as **plain text only** by
//! the UI — never via ``innerHTML`` / ``dangerouslySetInnerHTML`` / a
//! markdown component / any path that interprets HTML. The Rust side
//! does not sanitize. If the OSS installer is ever compromised, a
//! plain-text-only renderer is what stops a second-stage DOM injection
//! attack against the Tauri webview (which holds elevated invoke()
//! privileges). Strip ANSI escapes client-side if you want pretty output.
//!
//! Known gaps tracked for follow-ups:
//!   - No cancellation command. A spawned installer cannot be aborted
//!     from the UI; closing the window orphans the process. Wire a
//!     `cancel_pidash_install` command that holds the Child in shared
//!     state when this becomes user-visible.
//!   - stdout / stderr are read on two threads, so their events can arrive
//!     at the webview interleaved differently per render. Each `pidash-
//!     install-log` event now carries a monotonic `seq` (shared counter
//!     across both threads); the UI should order by it for a stable log.
//!     `seq` gives a deterministic total order, not true wall-clock
//!     chronology across the two pipes — that would need a single combined
//!     stream upstream.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

/// Result of [`detect_pidash_cli`].
#[derive(Debug, Serialize, Clone)]
pub struct PidashStatus {
    /// `true` if a usable `pidash` binary was found.
    pub installed: bool,
    /// Absolute path to the discovered binary, if any.
    pub path: Option<PathBuf>,
    /// Version string the binary reported via `--version`. Best-effort —
    /// some failure modes leave this empty even when `installed=true`.
    pub version: Option<String>,
}

impl PidashStatus {
    /// The "nothing found" result — shared by every detection fallback path so
    /// a new field can't be defaulted inconsistently across copy-pasted literals.
    fn not_installed() -> Self {
        Self {
            installed: false,
            path: None,
            version: None,
        }
    }
}

/// Return plausible install paths for this OS, in priority order — the caller
/// probes them before falling back to `which`/`where`.
///
/// The order matters: prefer the path the OSS installer writes to so we always
/// find a self-installed binary before any user-managed one (brew, cargo, …).
///
/// We probe common package-manager locations *directly* rather than relying
/// only on the `which` fallback because a GUI app launched from Finder/Dock (or
/// the Windows shell) inherits a minimal PATH that omits `/opt/homebrew/bin`,
/// `~/.cargo/bin`, etc. Without this, detection returns a false negative on
/// those installs and the startup auto-install drops a *duplicate* copy over an
/// existing one.
fn known_install_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(unix)]
    if let Some(home) = dirs_home() {
        // OSS installer drops here on macOS + Linux. cargo-dist's default
        // ``install-path`` from dist-workspace.toml in OSS pi-dash. Gated
        // by cfg(unix) — Windows installer uses a different layout.
        out.push(home.join(".local").join("bin").join("pidash"));
        // `cargo install pidash` — likely for a Rust CLI, and not on a
        // Finder-launched app's PATH.
        out.push(home.join(".cargo").join("bin").join("pidash"));
    }
    // Homebrew / manual locations, absent from a GUI app's inherited PATH.
    #[cfg(target_os = "macos")]
    {
        out.push(PathBuf::from("/opt/homebrew/bin/pidash")); // Apple Silicon brew
        out.push(PathBuf::from("/usr/local/bin/pidash")); // Intel brew / manual
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        out.push(PathBuf::from("/usr/local/bin/pidash")); // manual install
        out.push(PathBuf::from("/home/linuxbrew/.linuxbrew/bin/pidash")); // linuxbrew
    }
    #[cfg(target_os = "windows")]
    if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
        out.push(
            PathBuf::from(local_appdata)
                .join("Programs")
                .join("pidash")
                .join("pidash.exe"),
        );
    }
    out
}

/// Tiny inline `dirs::home_dir` so we don't add the `dirs` crate just for
/// this. Falls back to `$HOME` (Unix) or `%USERPROFILE%` (Windows).
fn dirs_home() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

/// On Unix, verify the executable bit is set. On Windows, file extension
/// is what determines executability; we already only probe `.exe` paths.
/// Returns true if the file at `path` is plausibly runnable.
fn is_executable(path: &PathBuf) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Probe duration cap. A misbehaving or hung binary at the discovered
/// path must not stall the IPC thread indefinitely.
const PROBE_VERSION_TIMEOUT: Duration = Duration::from_secs(2);

/// Run the discovered binary with `--version` and return its trimmed
/// first line. None on any failure — including a > PROBE_VERSION_TIMEOUT
/// hang, which we kill the child on. Callers treat "version unknown" as
/// detected-but-version-unknown rather than failing the whole detection.
fn probe_version(path: &PathBuf) -> Option<String> {
    let mut child = match Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return None,
    };

    let started = Instant::now();
    let output = loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                // Process exited; collect output below. wait_with_output
                // takes ownership of the child handle which is what we
                // want anyway.
                break child.wait_with_output().ok()?;
            }
            Ok(None) => {
                if started.elapsed() >= PROBE_VERSION_TIMEOUT {
                    let _ = kill_child(&mut child);
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    };

    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next()?.trim();
    if first_line.is_empty() {
        None
    } else {
        Some(first_line.to_string())
    }
}

/// Convenience wrapper for `Child::kill` that swallows errors. We call
/// this when a child has exceeded its deadline; the only thing to do on
/// kill-failure is move on, since we no longer trust the process.
fn kill_child(child: &mut Child) -> std::io::Result<()> {
    child.kill()
}

/// Find `pidash` by name on PATH. Returns the absolute path resolved by
/// the OS's resolver. Used as a fallback after [`known_install_paths`]
/// to cover users who installed via brew / winget / a custom path.
fn which_pidash() -> Option<PathBuf> {
    let (cmd, arg) = if cfg!(windows) {
        ("where", "pidash")
    } else {
        ("which", "pidash")
    };
    let output = Command::new(cmd)
        .arg(arg)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // `which` and `where` both print one path per line; take the first.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout.lines().next()?.trim();
    if first.is_empty() {
        None
    } else {
        Some(PathBuf::from(first))
    }
}

/// Synchronous implementation of detection. Wrapped by the async Tauri
/// command so blocking subprocess calls don't stall the IPC dispatch
/// thread.
fn detect_pidash_cli_sync() -> PidashStatus {
    for candidate in known_install_paths() {
        if is_executable(&candidate) {
            let version = probe_version(&candidate);
            return PidashStatus {
                installed: true,
                path: Some(candidate),
                version,
            };
        }
    }
    if let Some(path) = which_pidash() {
        // `which` / `where` already resolved against an executable PATH
        // entry, so an exec-bit re-check is redundant; trust the OS
        // resolver. The probe handles any further "actually broken"
        // cases by reporting version=None.
        let version = probe_version(&path);
        return PidashStatus {
            installed: true,
            path: Some(path),
            version,
        };
    }
    PidashStatus::not_installed()
}

#[tauri::command]
pub async fn detect_pidash_cli() -> PidashStatus {
    // The sync helper makes blocking subprocess calls. Hand it to the
    // blocking pool rather than running it on the Tauri async runtime
    // thread (which is shared with all other IPC dispatch). A misbehaving
    // CLI binary that survives the per-probe timeout in probe_version
    // would still stall the blocking pool slot but not the IPC channel.
    tauri::async_runtime::spawn_blocking(detect_pidash_cli_sync)
        .await
        .unwrap_or_else(|_| PidashStatus::not_installed())
}

/// URL of the cargo-dist-generated installer for the trusted `pidash`
/// release used by this desktop build.
///
/// Hardcoded so the webview cannot influence which script is fetched.
///
/// The URL and SHA-256 must be updated together. Pinning the release tag
/// closes the `releases/latest` substitution risk; verifying the script
/// body before execution closes corrupted or replaced asset downloads.
///
/// `INSTALLER_RELEASE_TAG` is read only by the tests below: it exists so a
/// version bump that updates one URL and forgets the other fails the suite
/// instead of shipping a mismatched pair.
#[cfg(test)]
const INSTALLER_RELEASE_TAG: &str = "pidash-v0.1.15";
const INSTALLER_URL_SH: &str =
    "https://github.com/The-AI-Republic/pi-dash/releases/download/pidash-v0.1.15/pidash-installer.sh";
const INSTALLER_URL_PS1: &str =
    "https://github.com/The-AI-Republic/pi-dash/releases/download/pidash-v0.1.15/pidash-installer.ps1";
const INSTALLER_SHA256_SH: &str =
    "c1303efd44d180675ceee445ef0091a71264f326447fb32dadda8ec02c39868e";
const INSTALLER_SHA256_PS1: &str =
    "8e7e7fe78672740c446ac4e7a9819ac1273c8ddeae05d7740dc9bbebc69cc4f4";

/// Module-level lock preventing concurrent install runs. Two rapid invokes
/// (double-click, accidental retry), or a manual install colliding with the
/// startup auto-install, would otherwise spawn parallel installer processes
/// racing on the same destination — corrupted binary, duplicated shell-rc
/// PATH entries, etc. Shared by both the manual command and the background
/// auto-install; released when the guard below drops.
static INSTALL_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// User-facing message when an install can't start because one is already
/// running. Phrased to be accurate whether the in-flight install is a
/// double-click or the silent startup auto-install (which the user never
/// explicitly triggered), so it doesn't read as "you did something wrong".
const INSTALL_BUSY_MESSAGE: &str =
    "Pi Dash is already setting up the pidash CLI — please wait for it to finish, then try again.";

/// RAII guard over [`INSTALL_IN_PROGRESS`]. [`acquire`](Self::acquire) returns
/// `None` when an install is already in flight (the `compare_exchange` failed);
/// the returned guard clears the flag on drop, so any early return from the
/// install path releases the lock rather than leaking a stuck one. Single
/// definition shared by both install entry points (was duplicated per-fn).
struct InstallInProgressGuard;

impl InstallInProgressGuard {
    fn acquire() -> Option<Self> {
        INSTALL_IN_PROGRESS
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then_some(InstallInProgressGuard)
    }
}

impl Drop for InstallInProgressGuard {
    fn drop(&mut self) {
        INSTALL_IN_PROGRESS.store(false, Ordering::Release);
    }
}

/// Event payload streamed during install. Stream chunks rather than a
/// single final blob so the UI can show progress for installers that
/// take 10-30s (download + extract).
///
/// `line` is raw bytes from the installer process decoded as UTF-8
/// (lossily on invalid bytes — common on Windows PowerShell which pipes
/// in the OEM codepage). The UI MUST render this as plain text only —
/// see the module-level "UI rendering contract" doc note.
///
/// `seq` is a monotonic counter shared by both reader threads, assigned as
/// each line is consumed. stdout and stderr are read on separate threads, so
/// events can otherwise arrive at the webview interleaved differently on each
/// render; the UI should order by `seq` for a stable log. (This gives a
/// deterministic total order, not true wall-clock chronology across the two
/// pipes — that would require a single combined stream upstream.)
#[derive(Debug, Serialize, Clone)]
struct InstallLogLine {
    stream: &'static str, // "stdout" | "stderr"
    line: String,
    seq: u64,
}

/// Spawn the OSS installer and stream its output to the webview.
///
/// Returns Ok(()) on installer exit code 0. Returns Err(message) on any
/// failure mode the UI should surface (bad spawn, non-zero exit, missing
/// `curl`/`powershell`, install already in progress). The UI is expected
/// to follow up with a fresh [`detect_pidash_cli`] regardless — even on
/// success we don't trust the installer's exit code as proof of
/// installation.
#[tauri::command]
pub async fn install_pidash_cli<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    // Concurrency guard — rejects re-entry (double-click, or a collision with
    // the background startup auto-install) rather than spawning a second
    // installer against the same destination. The guard clears the flag on
    // drop, covering every early return below.
    let Some(_guard) = InstallInProgressGuard::acquire() else {
        return Err(INSTALL_BUSY_MESSAGE.to_string());
    };

    let (program, args) = installer_command();

    // Run on a blocking-friendly task so we can use std::process::Command
    // (simpler than tokio::process for line-buffered I/O) without blocking
    // the tauri async runtime.
    let app_clone = app.clone();
    let join =
        tauri::async_runtime::spawn_blocking(move || run_installer(&app_clone, &program, &args));
    match join.await {
        Ok(result) => result,
        Err(e) => Err(format!("install task panicked: {e}")),
    }
}

/// Build the platform-specific installer command.
///
/// Hardening choices:
///   * `--proto =https --proto-redir =https` forces curl to refuse any
///     non-HTTPS scheme, including a 30x redirect to plain http.
///   * On Unix, avoid `curl | sh`. We download to a temp file first, then
///     execute it only if `curl` succeeded and the file matches the
///     bundled SHA-256. This keeps the script POSIX-sh compatible on both
///     Linux and macOS without relying on Bash-only `pipefail`.
///   * On Windows, resolve powershell.exe by absolute path through
///     %SystemRoot% so a malicious `powershell.exe` earlier in PATH cannot
///     hijack execution, and download the installer to a temp file instead
///     of piping `Invoke-RestMethod` into `Invoke-Expression`. The temp
///     file is SHA-256 verified before the child PowerShell executes it.
fn installer_command() -> (PathBuf, Vec<String>) {
    if cfg!(windows) {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let powershell_path = PathBuf::from(system_root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        return (
            powershell_path,
            vec![
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-Command".into(),
                windows_installer_script(),
            ],
        );
    }

    (
        PathBuf::from("/bin/sh"),
        vec![
            "-c".into(),
            format!(
                "set -eu; \
                 command -v curl >/dev/null || \
                     {{ echo 'curl is required' >&2; exit 127; }}; \
                 tmp=$(mktemp \"${{TMPDIR:-/tmp}}/pidash-installer.XXXXXX\"); \
                 trap 'rm -f \"$tmp\"' EXIT HUP INT TERM; \
                 curl --proto '=https' --proto-redir '=https' -fsSL '{url}' -o \"$tmp\"; \
                 if command -v sha256sum >/dev/null 2>&1; then \
                     actual=$(sha256sum < \"$tmp\" | awk '{{print $1}}'); \
                 elif command -v shasum >/dev/null 2>&1; then \
                     actual=$(shasum -a 256 < \"$tmp\" | awk '{{print $1}}'); \
                 else \
                     echo 'sha256sum or shasum is required' >&2; exit 127; \
                 fi; \
                 if [ \"$actual\" != '{sha256}' ]; then \
                     echo \"installer checksum mismatch: expected {sha256}, got $actual\" >&2; \
                     exit 1; \
                 fi; \
                 /bin/sh \"$tmp\"",
                url = INSTALLER_URL_SH,
                sha256 = INSTALLER_SHA256_SH
            ),
        ],
    )
}

fn windows_installer_script() -> String {
    // `-UseBasicParsing` on Invoke-WebRequest: PowerShell 5.1 (still the
    // default on shipping Windows SKUs) otherwise routes IWR through the
    // legacy IE parser, which hangs or errors on systems where the IE
    // first-run UX has never been completed (Server, fresh Win11, IE
    // removed). The response is a `.ps1`, not HTML — basic parsing is
    // strictly correct.
    //
    // Executing the downloaded script in a *child* powershell process
    // via `-File $tmp` instead of the call operator `& $tmp`: cargo-dist's
    // installer.ps1 ends with `exit`, which under `& $tmp` terminates the
    // current PowerShell session and bypasses the `finally` cleanup,
    // leaking the temp `.ps1`. A child process means `exit` only kills
    // the child, so the parent runs `finally` and removes the temp file.
    // `$PSHOME\powershell.exe` is the same interpreter that's already
    // executing us (resolved by absolute path from %SystemRoot% on the
    // Rust side), so PATH-shadowing remains closed off.
    format!(
        "$ErrorActionPreference = 'Stop'; \
         [System.Net.ServicePointManager]::SecurityProtocol = \
           [System.Net.SecurityProtocolType]::Tls12; \
         $tmp = Join-Path ([System.IO.Path]::GetTempPath()) \
           ('pidash-installer-' + [System.Guid]::NewGuid().ToString('N') + '.ps1'); \
         try {{ \
           Invoke-WebRequest -Uri '{url}' -OutFile $tmp -UseBasicParsing; \
           $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $tmp).Hash.ToLowerInvariant(); \
           if ($actual -ne '{sha256}') {{ \
             throw \"installer checksum mismatch: expected {sha256}, got $actual\" \
           }} \
           & \"$PSHOME\\powershell.exe\" -NoProfile -ExecutionPolicy Bypass -File $tmp; \
           if ($LASTEXITCODE -ne $null -and $LASTEXITCODE -ne 0) {{ exit $LASTEXITCODE }} \
         }} finally {{ \
           Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue \
         }}",
        url = INSTALLER_URL_PS1,
        sha256 = INSTALLER_SHA256_PS1
    )
}

fn run_installer<R: Runtime>(
    app: &AppHandle<R>,
    program: &PathBuf,
    args: &[String],
) -> Result<(), String> {
    use std::io::{BufRead, BufReader};

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", program.display()))?;

    // Take stdout/stderr before wait() so we can stream them in parallel.
    let stdout = child.stdout.take().ok_or("missing stdout pipe")?;
    let stderr = child.stderr.take().ok_or("missing stderr pipe")?;

    // Monotonic sequence shared by both reader threads: each emitted line gets
    // the next value, giving the UI a stable total order to sort by despite the
    // two threads racing to emit (see InstallLogLine::seq).
    let seq = Arc::new(AtomicU64::new(0));

    let app_stdout = app.clone();
    let seq_stdout = Arc::clone(&seq);
    let stdout_thread = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = app_stdout.emit(
                "pidash-install-log",
                InstallLogLine {
                    stream: "stdout",
                    line,
                    seq: seq_stdout.fetch_add(1, Ordering::Relaxed),
                },
            );
        }
    });
    let app_stderr = app.clone();
    let seq_stderr = Arc::clone(&seq);
    let stderr_thread = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = app_stderr.emit(
                "pidash-install-log",
                InstallLogLine {
                    stream: "stderr",
                    line,
                    seq: seq_stderr.fetch_add(1, Ordering::Relaxed),
                },
            );
        }
    });

    let status = child
        .wait()
        .map_err(|e| format!("installer wait failed: {e}"))?;
    // Join readers so all output has flushed before we report exit. If a
    // reader panics we log the panic at stderr (the desktop's stderr, not
    // the webview log stream) — useful in debug builds; harmless in
    // release because the panic doesn't surface to the user.
    if let Err(e) = stdout_thread.join() {
        eprintln!("pidash install: stdout reader panicked: {e:?}");
    }
    if let Err(e) = stderr_thread.join() {
        eprintln!("pidash install: stderr reader panicked: {e:?}");
    }

    if !status.success() {
        return Err(format!(
            "installer exited with status {} — see the install log for details",
            status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "<signal>".to_string())
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_seq_counter_is_unique_and_contiguous_across_threads() {
        // Mirrors run_installer's two-reader-thread pattern: both threads pull
        // seq values from one shared AtomicU64. The UI sorts log lines by seq,
        // so the load-bearing property is that no two lines ever get the same
        // seq (and none is skipped) regardless of thread interleaving.
        let seq = Arc::new(AtomicU64::new(0));
        let per_thread = 1000u64;
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let s = Arc::clone(&seq);
                std::thread::spawn(move || {
                    (0..per_thread)
                        .map(|_| s.fetch_add(1, Ordering::Relaxed))
                        .collect::<Vec<u64>>()
                })
            })
            .collect();
        let mut all: Vec<u64> = handles
            .into_iter()
            .flat_map(|h| h.join().expect("thread joins"))
            .collect();
        all.sort_unstable();
        // Exactly 0..2000, each exactly once — unique and contiguous.
        assert_eq!(all, (0..2 * per_thread).collect::<Vec<u64>>());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn known_install_paths_probe_homebrew_on_macos() {
        // A Finder/Dock-launched app inherits a minimal PATH without Homebrew
        // dirs, so detection must probe them directly (else auto-install drops
        // a duplicate over an existing brew install).
        let paths = known_install_paths();
        assert!(paths.contains(&PathBuf::from("/opt/homebrew/bin/pidash")));
        assert!(paths.contains(&PathBuf::from("/usr/local/bin/pidash")));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn known_install_paths_probe_common_dirs_on_linux() {
        let paths = known_install_paths();
        // ~/.local/bin (OSS installer) is probed first for priority.
        assert!(paths.first().is_some_and(|p| p.ends_with("pidash")));
        assert!(paths.contains(&PathBuf::from("/usr/local/bin/pidash")));
        assert!(paths.contains(&PathBuf::from("/home/linuxbrew/.linuxbrew/bin/pidash")));
    }

    #[cfg(unix)]
    #[test]
    fn unix_installer_uses_posix_sh_without_pipeline() {
        let (program, args) = installer_command();
        assert_eq!(program, PathBuf::from("/bin/sh"));
        assert_eq!(args.first().map(String::as_str), Some("-c"));
        let script = args.get(1).expect("script arg");
        assert!(script.contains("set -eu"));
        assert!(!script.contains("pipefail"));
        assert!(!script.contains("| sh"));
        assert!(script.contains("-o \"$tmp\""));
        assert!(script.contains("--proto '=https'"));
        assert!(script.contains(INSTALLER_URL_SH));
        assert!(script.contains(INSTALLER_RELEASE_TAG));
        assert!(!script.contains("releases/latest"));
        assert!(script.contains(INSTALLER_SHA256_SH));
        assert!(script.contains("sha256sum < \"$tmp\""));
        assert!(script.contains("shasum -a 256 < \"$tmp\""));
        assert!(script.contains("installer checksum mismatch"));
        let verify_idx = script.find("installer checksum mismatch").unwrap();
        let exec_idx = script.find("/bin/sh \"$tmp\"").unwrap();
        assert!(verify_idx < exec_idx);
    }

    #[test]
    fn windows_installer_script_downloads_before_execution() {
        let script = windows_installer_script();
        assert!(script.contains("Invoke-WebRequest"));
        assert!(script.contains("-OutFile $tmp"));
        // PS 5.1 requires -UseBasicParsing or IWR routes through the IE
        // parser and hangs on systems where IE first-run never ran.
        assert!(script.contains("-UseBasicParsing"));
        assert!(script.contains(INSTALLER_URL_PS1));
        assert!(script.contains(INSTALLER_RELEASE_TAG));
        assert!(!script.contains("releases/latest"));
        assert!(script.contains(INSTALLER_SHA256_PS1));
        assert!(script.contains("Get-FileHash -Algorithm SHA256"));
        assert!(script.contains("installer checksum mismatch"));
        // Execute the downloaded script in a CHILD powershell process so
        // an `exit` in the installer doesn't tear down the parent before
        // the `finally` block can remove the temp file.
        assert!(script.contains("$PSHOME\\powershell.exe"));
        assert!(script.contains("-File $tmp"));
        let verify_idx = script.find("Get-FileHash").unwrap();
        let exec_idx = script.find("-File $tmp").unwrap();
        assert!(verify_idx < exec_idx);
        // Regression guard: ensure we never go back to the in-process
        // call-operator form, which bypasses `finally` on `exit`.
        assert!(!script.contains("& $tmp;"));
        assert!(script.contains("Remove-Item -LiteralPath $tmp"));
        assert!(!script.contains("iex"));
        assert!(!script.contains("Invoke-Expression"));
    }
}
