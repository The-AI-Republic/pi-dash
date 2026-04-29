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
//! 2. The status snapshot reports `connected == true` — i.e. the WebSocket
//!    to the cloud handshook. If this never flips, the config is syntactically
//!    fine but semantically broken (wrong URL, invalid runner_secret, etc.).
//!
//! Callers (TUI Config save, CLI `pidash configure --<flag>`) turn the
//! `ReloadOutcome` into either success UI or a loud error so the user
//! immediately knows their edit broke the daemon, instead of discovering
//! it later via a silent background failure.
//!
//! Time budget: 5 s for IPC + an additional 5 s for cloud-connected
//! (10 s total worst case). The first stage is usually a few hundred ms on
//! Linux/macOS; the cloud handshake is what typically eats seconds.

use std::time::{Duration, Instant};

use crate::ipc::client::Client;
use crate::ipc::protocol::{Request, Response};
use crate::util::paths::Paths;

const IPC_TIMEOUT: Duration = Duration::from_secs(5);
const CLOUD_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

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
}

/// Write-then-restart flow. Starts the service if inactive, restarts if
/// active, then polls IPC and the cloud-connected flag. Always returns
/// something you can render — failures carry a `detail` string, successes
/// carry a summary of "<name> is connected".
pub async fn restart_and_verify(paths: &Paths) -> ReloadOutcome {
    let svc = crate::service::detect();

    // `enable_and_start` is idempotent *and* on systemd uses `restart`, so
    // this covers both "first start" and "reload after config change."
    if let Err(e) = svc.enable_and_start().await {
        let state = svc.status().await.unwrap_or_else(|_| "unknown".into());
        return ReloadOutcome {
            ok: false,
            summary: "failed to start runner service".into(),
            detail: Some(format!(
                "service manager rejected start/restart: {e:#}\n\
                 current state: {state}"
            )),
            service_state: state,
        };
    }

    // Stage 1: wait for IPC. A successful `StatusGet` means the daemon got
    // past config load, credential load, and agent init.
    let stage1 = wait_for_ipc(paths, IPC_TIMEOUT).await;
    let snapshot = match stage1 {
        Some(s) => s,
        None => {
            let state = svc.status().await.unwrap_or_else(|_| "unknown".into());
            let journal = capture_service_detail(&svc).await;
            return ReloadOutcome {
                ok: false,
                summary: "runner failed to come up".into(),
                detail: Some(format!(
                    "Daemon did not answer IPC within {}s after restart.\n\
                     Service state: {state}\n\n{journal}",
                    IPC_TIMEOUT.as_secs()
                )),
                service_state: state,
            };
        }
    };

    // Stage 2: wait for cloud-connected. Already connected? Return now.
    if snapshot.daemon.connected {
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
        };
    }
    match wait_for_cloud_connected(paths, CLOUD_TIMEOUT).await {
        Some(final_snap) => {
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
            }
        }
        None => {
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
            }
        }
    }
}

async fn wait_for_ipc(
    paths: &Paths,
    timeout: Duration,
) -> Option<crate::ipc::protocol::StatusSnapshot> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(mut c) = Client::connect(paths.ipc_socket_path()).await
            && let Ok(Response::Status(s)) = c.call(Request::StatusGet).await
        {
            return Some(s);
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn wait_for_cloud_connected(
    paths: &Paths,
    timeout: Duration,
) -> Option<crate::ipc::protocol::StatusSnapshot> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(mut c) = Client::connect(paths.ipc_socket_path()).await
            && let Ok(Response::Status(s)) = c.call(Request::StatusGet).await
            && s.daemon.connected
        {
            return Some(s);
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
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
