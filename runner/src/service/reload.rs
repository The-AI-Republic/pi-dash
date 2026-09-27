//! Restart-and-verify helper used after any `config.toml` mutation.
//!
//! Pattern: write file → kick the service → wait for the daemon to prove
//! it's healthy. "Healthy" means two things end to end:
//!
//! 1. IPC socket is reachable and serves `StatusGet`. This tells us the
//!    daemon started, loaded its config + credentials, and reached the
//!    serve-IPC step. If the config change was malformed (bad TOML, bad
//!    credentials, unknown agent), startup fails before this point.
//!
//! 2. The status snapshot shows the daemon reached the cloud. This is a
//!    three-way verdict, not a boolean (PDASHOSS01-222):
//!    - **all** runners hold a live cloud session → clean success;
//!    - the cloud is reachable (the daemon-level flag or at least one
//!      runner is connected) but some runners never opened their session
//!      within the window → success **with warnings** naming the stragglers.
//!      A per-runner session-open wedge is a cloud-side problem, not a
//!      failed restart, so it must not read as "your restart broke";
//!    - nothing reached the cloud at all → failure (wrong URL, invalid
//!      runner_secret, network trouble).
//!
//!    Note `daemon.connected` in the snapshot is really the *first
//!    configured runner's* session flag (`IpcServer.primary_state`), so a
//!    verdict keyed on it alone misreports a wedged first runner as a
//!    dead daemon — the exact failure this three-way split exists to fix.
//!
//! Callers (TUI Config save, CLI `pidash configure --<flag>`) turn the
//! `ReloadOutcome` into either success UI or a loud error so the user
//! immediately knows their edit broke the daemon, instead of discovering
//! it later via a silent background failure.
//!
//! Time budget: 5 s for IPC + an additional 30 s for cloud-connected
//! (35 s total worst case). The first stage is usually a few hundred ms on
//! Linux/macOS; the cloud handshake is what typically eats seconds after
//! package swaps, service-manager restarts, DNS, or cloud deploys.

use std::time::{Duration, Instant};

use crate::ipc::client::Client;
use crate::ipc::protocol::{Request, Response};
use crate::util::paths::Paths;

const IPC_TIMEOUT: Duration = Duration::from_secs(5);
const CLOUD_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const PROGRESS_INTERVAL: Duration = Duration::from_secs(5);

/// How many extra times `restart_and_verify` will re-issue the launchd start
/// when the freshly-exec'd daemon is SIGKILL'd before it can answer IPC.
/// Total start attempts = 1 initial + `MAX_STARTUP_RETRIES`. Sized for the
/// macOS AMFI code-signing race (PDASHOSS01-164): the second exec of a
/// just-swapped binary already reliably succeeds, so two retries is ample
/// headroom while keeping the failure path bounded. Only consumed when
/// `Service::recent_exit_is_retryable()` says the last exit was a retryable
/// SIGKILL, so non-macOS backends and genuine faults never spend them.
const MAX_STARTUP_RETRIES: u32 = 2;

/// Short pause between a SIGKILL'd startup and the next start attempt, to let
/// launchd finish reaping the dead instance and give AMFI's signature /
/// provenance cache a beat to settle before we re-exec.
const STARTUP_RETRY_BACKOFF: Duration = Duration::from_millis(750);

/// Outcome of a reload attempt. `ok = true` means the daemon is up and
/// talking to the cloud; a `false` value carries a message explaining which
/// stage failed plus any service-manager output we could capture.
#[derive(Debug, Clone)]
pub struct ReloadOutcome {
    pub ok: bool,
    /// Short, single-line summary suitable for a status banner.
    pub summary: String,
    /// Longer error detail — included only on failure. Safe to show in a
    /// popup or stderr.
    pub detail: Option<String>,
    /// Last-known service state string (`active`, `inactive`, `failed`,
    /// or the raw `launchctl list` row). Handy for the TUI even on success.
    pub service_state: String,
    /// Present only on a degraded success (`ok == true`): the daemon is up
    /// and the cloud is reachable, but one or more runners failed to open
    /// their own cloud session within the readiness window. Ready-to-print
    /// multi-line block: one `- <name> (<project>) — <why>` line per
    /// disconnected runner plus a closing "the daemon and the other N
    /// runners are connected" line. `None` on clean success and on failure.
    pub warnings: Option<String>,
}

/// Write-then-restart flow. Starts the service if inactive, restarts if
/// active, then polls IPC and the cloud-connected flag. Always returns
/// something you can render — failures carry a `detail` string, successes
/// carry a summary of "<name> is connected".
pub async fn restart_and_verify(paths: &Paths) -> ReloadOutcome {
    restart_and_verify_with_progress(paths, |_| {}).await
}

pub async fn restart_and_verify_with_progress<F>(paths: &Paths, mut progress: F) -> ReloadOutcome
where
    F: FnMut(String),
{
    let svc = crate::service::detect();

    // Self-heal: rewrite the service unit if it's gone (manual cleanup, a
    // `pidash uninstall`/`remove`, or never written for the current install
    // location). Without this, `pidash restart` on a unit-less machine
    // hard-fails — `pidash update` only swaps the binary and doesn't touch
    // the unit, and operators should not need to know that `pidash install`
    // is the magic recovery command.
    //
    // All backends, not just launchd. This started as a launchd fix because
    // that's where it was first reported (launchctl bootstrap returning a
    // cryptic EIO), on the theory that systemd's clearer "Unit file does not
    // exist" left the operator able to recover unaided. It doesn't: a
    // legible error is still a hard failure, it names what's missing but not
    // the fix, and the fix isn't guessable — `pidash install` writes the unit
    // but then declines to start it when no runner is enrolled. `launchd::start`
    // keeps its precondition check as a last line of defense for direct
    // `pidash start` callers that don't pass through this self-heal.
    match svc.rewrite_unit_if_missing(paths).await {
        Ok(true) => progress("rewrote missing service unit".into()),
        Ok(false) => {}
        Err(e) => tracing::warn!("pre-restart service unit check failed: {e:#}"),
    }

    // Stage 1: start the service and wait for it to answer IPC, retrying the
    // launchd start when the freshly-exec'd daemon is SIGKILL'd before it can
    // serve IPC. On macOS the first launchd exec of a just-swapped binary is
    // intermittently killed by AMFI (a code-signing/provenance-caching race,
    // PDASHOSS01-164) even though the very next exec of the same binary comes
    // up fine. A bounded retry turns that scary first-attempt-after-update
    // "runner failed to come up" failure into a transparent recovery instead
    // of making the operator know to re-run `pidash restart` by hand.
    //
    // The retry is gated on `recent_exit_is_retryable()` (macOS SIGKILL only;
    // systemd/Windows always decline), so a config/credential error or a
    // panic still fails fast with its existing diagnosis rather than looping.
    let mut attempt: u32 = 0;
    let snapshot = loop {
        attempt += 1;

        progress("starting runner service".into());
        // `enable_and_start` is idempotent *and* on systemd uses `restart`, so
        // this covers both "first start" and "reload after config change."
        if let Err(e) = svc.enable_and_start().await {
            // A start that failed because the daemon was SIGKILL'd on exec is
            // the same AMFI race; retry it too when attempts remain.
            if attempt <= MAX_STARTUP_RETRIES && svc.recent_exit_is_retryable().await {
                emit_startup_retry_progress(attempt, &mut progress);
                tokio::time::sleep(STARTUP_RETRY_BACKOFF).await;
                continue;
            }
            let state = svc.status().await.unwrap_or_else(|_| "unknown".into());
            return ReloadOutcome {
                ok: false,
                summary: "failed to start runner service".into(),
                detail: Some(format!(
                    "service manager rejected start/restart: {e:#}\n\
                     current state: {state}"
                )),
                service_state: state,
                warnings: None,
            };
        }

        // Wait for IPC. A successful `StatusGet` means the daemon got past
        // config load, credential load, and agent init.
        progress(format!(
            "waiting up to {}s for daemon IPC",
            IPC_TIMEOUT.as_secs()
        ));
        if let Some(s) = wait_for_ipc(paths, IPC_TIMEOUT, &mut progress).await {
            break s;
        }

        // IPC never answered. If the daemon was SIGKILL'd on exec (the macOS
        // AMFI race) and attempts remain, re-kick and try again — the next
        // exec of the same binary reliably comes up.
        if attempt <= MAX_STARTUP_RETRIES && svc.recent_exit_is_retryable().await {
            emit_startup_retry_progress(attempt, &mut progress);
            tokio::time::sleep(STARTUP_RETRY_BACKOFF).await;
            continue;
        }

        let state = svc.status().await.unwrap_or_else(|_| "unknown".into());
        // Ask the backend whether the daemon died with a recognizable
        // signature (SIGKILL = AMFI on macOS, SIGABRT = panic, etc.).
        // Surface that ahead of the raw launchctl/systemctl dump so the
        // operator's first read points them at the actual cause rather
        // than "the IPC didn't answer."
        let diagnosis = svc.diagnose_recent_exit().await;
        let journal = capture_service_detail(&svc).await;
        let detail = match diagnosis {
            Some(diag) => format!(
                "Daemon did not answer IPC within {}s after restart.\n\n\
                 Diagnosis: {diag}\n\n\
                 Service state: {state}\n\n{journal}",
                IPC_TIMEOUT.as_secs()
            ),
            None => format!(
                "Daemon did not answer IPC within {}s after restart.\n\
                 Service state: {state}\n\n{journal}",
                IPC_TIMEOUT.as_secs()
            ),
        };
        return ReloadOutcome {
            ok: false,
            summary: "runner failed to come up".into(),
            detail: Some(detail),
            service_state: state,
            warnings: None,
        };
    };

    // Stage 2: wait for cloud-connected. Already fully connected? Return now.
    if matches!(
        classify_cloud_readiness(&snapshot),
        CloudReadiness::AllConnected
    ) {
        let state = svc.status().await.unwrap_or_else(|_| "unknown".into());
        return ReloadOutcome {
            ok: true,
            summary: format!(
                "{} — connected to {}",
                summarize_runners(&snapshot),
                snapshot.daemon.cloud_url
            ),
            detail: None,
            service_state: state,
            warnings: None,
        };
    }
    progress(format!(
        "waiting up to {}s for cloud connection",
        CLOUD_TIMEOUT.as_secs()
    ));
    match wait_for_cloud_ready(paths, CLOUD_TIMEOUT, &mut progress).await {
        CloudWait::AllConnected(final_snap) => {
            let state = svc.status().await.unwrap_or_else(|_| "unknown".into());
            ReloadOutcome {
                ok: true,
                summary: format!(
                    "{} — connected to {}",
                    summarize_runners(&final_snap),
                    final_snap.daemon.cloud_url
                ),
                detail: None,
                service_state: state,
                warnings: None,
            }
        }
        // The daemon reached the cloud but some runners never opened their
        // session. The restart itself worked — report success, name the
        // stragglers, and leave the exit code clean so automation that only
        // cares about the daemon keeps working (PDASHOSS01-222).
        CloudWait::Degraded(final_snap) => {
            let state = svc.status().await.unwrap_or_else(|_| "unknown".into());
            let failed = final_snap.runners.iter().filter(|r| !r.connected).count();
            ReloadOutcome {
                ok: true,
                summary: format!(
                    "restart succeeded with warnings: {failed} of {} runners \
                     failed to connect to the cloud within {}s",
                    final_snap.runners.len(),
                    CLOUD_TIMEOUT.as_secs()
                ),
                detail: None,
                service_state: state,
                warnings: Some(render_degraded_warnings(&final_snap)),
            }
        }
        CloudWait::NotConnected => {
            let state = svc.status().await.unwrap_or_else(|_| "unknown".into());
            ReloadOutcome {
                ok: false,
                summary: "daemon up but not connected to cloud".into(),
                detail: Some(format!(
                    "Runner started but did not reach the cloud within {}s.\n\
                     Common causes: wrong cloud_url, invalid runner_secret \
                     (try re-registering with `pidash configure --url ... --token ...`), \
                     or a network reachability problem.\n\
                     Service state: {state}",
                    CLOUD_TIMEOUT.as_secs()
                )),
                service_state: state,
                warnings: None,
            }
        }
    }
}

/// Cloud-connectivity verdict derived from one status snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloudReadiness {
    /// Every configured runner holds a live cloud session (or the daemon
    /// hosts zero runners and its connection-level flag is up).
    AllConnected,
    /// The cloud is reachable — the daemon-level flag or at least one
    /// runner is connected — but one or more runners are not.
    Degraded,
    /// Nothing has reached the cloud.
    NotConnected,
}

fn classify_cloud_readiness(snap: &crate::ipc::protocol::StatusSnapshot) -> CloudReadiness {
    if snap.runners.is_empty() {
        // Zero-runner daemon (freshly enrolled machine): the per-runner
        // signal doesn't exist, so the connection-level flag is the only
        // evidence — same gate as before the three-way split.
        return if snap.daemon.connected {
            CloudReadiness::AllConnected
        } else {
            CloudReadiness::NotConnected
        };
    }
    let connected = snap.runners.iter().filter(|r| r.connected).count();
    if connected == snap.runners.len() {
        CloudReadiness::AllConnected
    } else if connected > 0 || snap.daemon.connected {
        CloudReadiness::Degraded
    } else {
        CloudReadiness::NotConnected
    }
}

/// Result of the stage-2 wait: the readiness verdict plus the snapshot it
/// was derived from, so the caller can name the disconnected runners.
enum CloudWait {
    AllConnected(crate::ipc::protocol::StatusSnapshot),
    Degraded(crate::ipc::protocol::StatusSnapshot),
    NotConnected,
}

/// Human-readable warning block for a degraded restart: one line per
/// disconnected runner — name first, then project, then the best failure
/// hint the snapshot carries (the DISCONNECTED fields from `pidash status`:
/// consecutive session-open failures, never-connected) — plus a closing
/// line confirming what *did* connect and how to recheck.
fn render_degraded_warnings(snap: &crate::ipc::protocol::StatusSnapshot) -> String {
    let mut out = String::new();
    for r in snap.runners.iter().filter(|r| !r.connected) {
        let project = r.project_slug.as_deref().unwrap_or("no project");
        out.push_str(&format!(
            "  - {} ({project}) — {}\n",
            r.name,
            describe_disconnected(r)
        ));
    }
    let connected = snap.runners.iter().filter(|r| r.connected).count();
    match connected {
        0 => out.push_str(
            "The daemon is up but no runner has connected yet. \
             Re-run `pidash status` to recheck.",
        ),
        1 => out.push_str(
            "The daemon and the other runner are connected. \
             Re-run `pidash status` to recheck.",
        ),
        n => out.push_str(&format!(
            "The daemon and the other {n} runners are connected. \
             Re-run `pidash status` to recheck."
        )),
    }
    out
}

/// Best per-runner failure hint the status snapshot carries. There is no
/// per-attempt error classification on the IPC wire (timeout vs. 401 vs.
/// connect-refused), so this leans on the same fields `pidash status`
/// prints for DISCONNECTED runners.
fn describe_disconnected(r: &crate::ipc::protocol::RunnerStatusSnapshot) -> String {
    if r.consecutive_bootstrap_failures > 0 {
        let n = r.consecutive_bootstrap_failures;
        format!(
            "session-open failing ({n} consecutive failure{})",
            if n == 1 { "" } else { "s" }
        )
    } else if r.last_session_open.is_none() {
        "never connected since daemon start".to_string()
    } else {
        "not connected".to_string()
    }
}

async fn wait_for_ipc(
    paths: &Paths,
    timeout: Duration,
    progress: &mut impl FnMut(String),
) -> Option<crate::ipc::protocol::StatusSnapshot> {
    let start = Instant::now();
    let deadline = start + timeout;
    let mut next_progress = start + PROGRESS_INTERVAL;
    loop {
        if let Ok(mut c) = Client::connect(paths.ipc_socket_path()).await
            && let Ok(Response::Status(s)) = c.call(Request::StatusGet).await
        {
            return Some(s);
        }
        if Instant::now() >= deadline {
            return None;
        }
        maybe_emit_wait_progress("daemon IPC", start, timeout, &mut next_progress, progress);
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Poll status until every runner is cloud-connected or the window closes.
/// Returns early only on `AllConnected`; on timeout the verdict comes from
/// the freshest snapshot observed — `Degraded` when the cloud was reached
/// but stragglers remain, `NotConnected` when nothing connected (or IPC
/// never answered during the window).
async fn wait_for_cloud_ready(
    paths: &Paths,
    timeout: Duration,
    progress: &mut impl FnMut(String),
) -> CloudWait {
    let start = Instant::now();
    let deadline = start + timeout;
    let mut next_progress = start + PROGRESS_INTERVAL;
    let mut last: Option<crate::ipc::protocol::StatusSnapshot> = None;
    loop {
        if let Ok(mut c) = Client::connect(paths.ipc_socket_path()).await
            && let Ok(Response::Status(s)) = c.call(Request::StatusGet).await
        {
            if classify_cloud_readiness(&s) == CloudReadiness::AllConnected {
                return CloudWait::AllConnected(s);
            }
            last = Some(s);
        }
        if Instant::now() >= deadline {
            return match last {
                Some(s) if classify_cloud_readiness(&s) == CloudReadiness::Degraded => {
                    CloudWait::Degraded(s)
                }
                _ => CloudWait::NotConnected,
            };
        }
        maybe_emit_wait_progress(
            "cloud connection",
            start,
            timeout,
            &mut next_progress,
            progress,
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

fn maybe_emit_wait_progress(
    label: &str,
    start: Instant,
    timeout: Duration,
    next_progress: &mut Instant,
    progress: &mut impl FnMut(String),
) {
    let now = Instant::now();
    if now < *next_progress {
        return;
    }
    let elapsed = now.saturating_duration_since(start).as_secs();
    progress(format!(
        "still waiting for {label} ({elapsed}s elapsed, {}s max)",
        timeout.as_secs()
    ));
    *next_progress = now + PROGRESS_INTERVAL;
}

/// Emit the operator-facing progress line when we re-issue a launchd start
/// after the daemon was SIGKILL'd on exec. Names the likely macOS AMFI
/// code-signing race so the retry reads as a deliberate recovery, not the
/// tool flailing. `attempt` is the number of the start that just failed
/// (1-based); the line advertises the next attempt out of the total.
fn emit_startup_retry_progress(attempt: u32, progress: &mut impl FnMut(String)) {
    progress(format!(
        "daemon was killed on startup before answering IPC — likely the macOS \
         AMFI code-signing race on a freshly-swapped binary; retrying start \
         (attempt {} of {})",
        attempt + 1,
        MAX_STARTUP_RETRIES + 1
    ));
}

/// Best-effort collection of service-manager output to show in the error
/// popup. We ignore failures — if this can't run, the outer error already
/// said "daemon didn't come up," and we don't want to obscure it.
async fn capture_service_detail(svc: &crate::service::Service) -> String {
    match svc.status().await {
        Ok(s) => format!("service status:\n{s}"),
        Err(e) => format!("(could not read service status: {e})"),
    }
}

/// Render a one-liner of configured-runner names for `pidash install` /
/// reload outcomes. Single-runner returns the bare name; multi-runner
/// joins with `, ` and reports the count.
fn summarize_runners(snap: &crate::ipc::protocol::StatusSnapshot) -> String {
    match snap.runners.len() {
        0 => "(no runners)".to_string(),
        1 => snap.runners[0].name.clone(),
        n => format!(
            "{n} runners ({})",
            snap.runners
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::{DaemonInfo, RunnerStatusSnapshot, StatusSnapshot};
    use pidash_ipc::RunnerStatus;

    fn runner(name: &str, project: Option<&str>, connected: bool, fails: u32) -> RunnerStatusSnapshot {
        RunnerStatusSnapshot {
            runner_id: uuid::Uuid::new_v4(),
            name: name.to_string(),
            project_slug: project.map(str::to_string),
            pod_id: None,
            status: RunnerStatus::Idle,
            connected,
            current_run: None,
            approvals_pending: 0,
            last_heartbeat: None,
            last_session_open: None,
            consecutive_bootstrap_failures: fails,
            observability: None,
        }
    }

    fn snapshot(daemon_connected: bool, runners: Vec<RunnerStatusSnapshot>) -> StatusSnapshot {
        StatusSnapshot {
            daemon: DaemonInfo {
                cloud_url: "https://cloud.example".into(),
                connected: daemon_connected,
                uptime_secs: 1,
                update: None,
            },
            runners,
        }
    }

    #[test]
    fn all_runners_connected_is_a_clean_success() {
        let snap = snapshot(true, vec![runner("a", Some("P1"), true, 0)]);
        assert_eq!(classify_cloud_readiness(&snap), CloudReadiness::AllConnected);
    }

    #[test]
    fn wedged_first_runner_no_longer_reads_as_daemon_down() {
        // The incident shape from PDASHOSS01-222: `daemon.connected` mirrors
        // the *first* runner (IpcServer.primary_state), so a wedged first
        // runner used to fail the whole restart even with the rest healthy.
        let snap = snapshot(
            false,
            vec![
                runner("claude_macmini01", Some("PDASHOSS01"), false, 5),
                runner("healthy_one", Some("PDASHOSS01"), true, 0),
                runner("healthy_two", Some("PRIVATEPI1"), true, 0),
            ],
        );
        assert_eq!(classify_cloud_readiness(&snap), CloudReadiness::Degraded);
    }

    #[test]
    fn connected_first_runner_with_stragglers_is_degraded_not_clean() {
        // Mirror bug: the old gate returned clean success as soon as the
        // first runner (== daemon.connected) was up, hiding dead stragglers.
        let snap = snapshot(
            true,
            vec![
                runner("first", Some("P1"), true, 0),
                runner("stuck", Some("P2"), false, 3),
            ],
        );
        assert_eq!(classify_cloud_readiness(&snap), CloudReadiness::Degraded);
    }

    #[test]
    fn nothing_connected_is_a_hard_failure() {
        let snap = snapshot(
            false,
            vec![runner("a", Some("P1"), false, 2), runner("b", None, false, 0)],
        );
        assert_eq!(classify_cloud_readiness(&snap), CloudReadiness::NotConnected);
    }

    #[test]
    fn zero_runner_daemon_keeps_the_connection_flag_gate() {
        assert_eq!(
            classify_cloud_readiness(&snapshot(true, vec![])),
            CloudReadiness::AllConnected
        );
        assert_eq!(
            classify_cloud_readiness(&snapshot(false, vec![])),
            CloudReadiness::NotConnected
        );
    }

    #[test]
    fn degraded_warnings_name_runner_project_and_failure_hint() {
        let snap = snapshot(
            false,
            vec![
                runner("claude_macmini01", Some("PDASHOSS01"), false, 5),
                runner("private_pidash_codex001", Some("PRIVATEPI1"), false, 1),
                runner("no_project_runner", None, false, 0),
                runner("healthy_a", Some("PDASHOSS01"), true, 0),
                runner("healthy_b", Some("PDASHOSS01"), true, 0),
            ],
        );
        let w = render_degraded_warnings(&snap);
        // Named by runner name + project so the message is actionable.
        assert!(w.contains("claude_macmini01 (PDASHOSS01) — session-open failing (5 consecutive failures)"));
        assert!(w.contains("private_pidash_codex001 (PRIVATEPI1) — session-open failing (1 consecutive failure)"));
        assert!(w.contains("no_project_runner (no project) — never connected since daemon start"));
        // Healthy runners are not in the warning list.
        assert!(!w.contains("healthy_a ("));
        // Closing line confirms what did connect and how to recheck.
        assert!(w.contains("the other 2 runners are connected"));
        assert!(w.contains("`pidash status`"));
    }

    #[test]
    fn degraded_warnings_singular_connected_runner() {
        let snap = snapshot(
            false,
            vec![
                runner("stuck", Some("P1"), false, 2),
                runner("healthy", Some("P1"), true, 0),
            ],
        );
        let w = render_degraded_warnings(&snap);
        assert!(w.contains("The daemon and the other runner are connected"));
    }

    #[test]
    fn startup_retry_progress_names_amfi_and_counts_attempts() {
        let mut lines = Vec::new();
        // `attempt` is the 1-based number of the start that just failed.
        emit_startup_retry_progress(1, &mut |m| lines.push(m));
        emit_startup_retry_progress(2, &mut |m| lines.push(m));

        // The retry message must point at the AMFI code-signing race so an
        // operator watching the restart understands the retry is deliberate.
        assert!(lines[0].to_uppercase().contains("AMFI"));
        // First retry advertises attempt 2, last retry attempt 3 (1 initial
        // + MAX_STARTUP_RETRIES). Guards against the off-by-one drifting.
        let total = MAX_STARTUP_RETRIES + 1;
        assert!(lines[0].contains(&format!("attempt 2 of {total}")));
        assert!(lines[1].contains(&format!("attempt 3 of {total}")));
    }
}
