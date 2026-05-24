use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc, oneshot, watch};

use crate::agent::{AgentBridge, AgentCursor, BridgeEvent, RunPayload};
use crate::approval::policy::Policy;
use crate::approval::router::{ApprovalRecord, ApprovalRouter, ApprovalStatus, DecisionSource};
use crate::cloud::http::{
    AckEntry, AttachBody, CredentialsHandle, HttpLoop, InboundEnvelope, PollStatus,
    RunnerCloudClient, SharedHttpTransport,
};
use crate::cloud::protocol::{
    ClientMsg, FailureReason, RunnerStatus, ServerMsg, WIRE_VERSION, WorkspaceState,
};
use crate::config::schema::{AgentKind, Config, Credentials};
use crate::daemon::runner_instance::RunnerInstance;
use crate::daemon::runner_out::RunnerOut;
use crate::daemon::state::StateHandle;
use crate::history::index::{RunSummary, RunsIndex};
use crate::history::jsonl::{HistoryEntry, HistoryWriter};
use crate::ipc::protocol::{CurrentRunSummary, version_lt};
use crate::ipc::server::IpcServer;
use crate::util::paths::{Paths, RunnerPaths};

pub struct Supervisor {
    pub config: Config,
    pub creds: Credentials,
    pub paths: Paths,
    pub opts: crate::daemon::Options,
    pub state: StateHandle,
    pub approvals: ApprovalRouter,
}

type HelloRunner = (RunnerOut, StateHandle, Option<String>);
type HelloRunnerMap = HashMap<uuid::Uuid, HelloRunner>;

impl Supervisor {
    pub fn new(
        config: Config,
        creds: Credentials,
        paths: Paths,
        opts: crate::daemon::Options,
    ) -> Self {
        let state = StateHandle::new(config.clone());
        let approvals = ApprovalRouter::new();
        Self {
            config,
            creds,
            paths,
            opts,
            state,
            approvals,
        }
    }

    pub async fn run(self) -> Result<()> {
        self.paths.ensure()?;
        // Re-run config validation here so any future entry path that
        // bypasses `cli::run` (tests, IPC reload, embedded callers)
        // still gets the duplicate-name / nested-working-dir / cap
        // checks before we spawn N RunnerInstances. Cheap (O(n²) over
        // ≤50 runners) and idempotent with the cli::run gate.
        self.config
            .validate()
            .map_err(|e| anyhow::anyhow!("invalid config: {e}"))?;
        let Supervisor {
            config,
            creds: _creds,
            paths,
            opts,
            state,
            approvals: _supervisor_approvals,
        } = self;
        // Daemon-level state has no single runner_id any more — runners
        // come and go under one connection. Per-instance state still
        // holds its own runner_id (set in RunnerInstance::new).

        let transport = if opts.offline {
            None
        } else {
            Some(SharedHttpTransport::new(config.daemon.cloud_url.clone())?)
        };

        let mut instances: Vec<RunnerInstance> = Vec::new();
        for runner_cfg in &config.runners {
            let inst = if let Some(shared) = &transport {
                let runner_paths = paths.for_runner(runner_cfg.runner_id);
                let creds = load_runner_credentials(&runner_paths, &runner_cfg.name).await?;
                let client = RunnerCloudClient::new(runner_cfg.runner_id, creds, shared.clone());
                RunnerInstance::new_http(runner_cfg.clone(), &paths, client)
            } else {
                RunnerInstance::new_offline(runner_cfg.clone(), &paths)
            };
            inst.paths.ensure()?;
            instances.push(inst);
        }
        let mailboxes = Arc::new(RwLock::new(
            instances
                .iter()
                .map(|i| (i.runner_id, i.mailbox_tx.clone()))
                .collect(),
        ));
        let hello_runners = Arc::new(RwLock::new(
            instances
                .iter()
                .map(|i| {
                    (
                        i.runner_id,
                        (
                            i.out.clone(),
                            i.state.clone(),
                            i.config.project_slug.clone(),
                        ),
                    )
                })
                .collect::<HelloRunnerMap>(),
        ));

        // Primary runner for IPC's "default snapshot" — whichever runner
        // happens to be first in config.toml. None when the connection
        // has zero runners yet (a freshly enrolled dev machine), in which
        // case IPC falls back to the daemon-level state.
        let primary = instances.first().cloned();

        // Snapshot of every configured runner the IPC server can
        // route requests to. Built once at startup; runtime add /
        // remove (Phase 7 of the parent design) will mutate this map
        // when that work lands.
        let ipc_instances: HashMap<uuid::Uuid, RunnerInstance> = instances
            .iter()
            .cloned()
            .map(|i| (i.runner_id, i))
            .collect();
        let ipc = IpcServer {
            path: paths.ipc_socket_path(),
            primary_state: primary
                .as_ref()
                .map(|p| p.state.clone())
                .unwrap_or_else(|| state.clone()),
            paths: paths.clone(),
            instances: Arc::new(ipc_instances),
        };
        let ipc_handle = tokio::spawn(async move {
            if let Err(e) = ipc.run().await {
                tracing::error!("ipc server exited: {e:#}");
            }
        });

        if opts.offline {
            tracing::info!("offline mode: HTTP transport disabled");
        }

        // One RunnerLoop per instance. Each consumes from its mailbox.
        let mut loop_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
        let mut http_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
        let mut refresh_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
        for inst in &instances {
            let mailbox_rx = match inst.take_mailbox_rx().await {
                Some(rx) => rx,
                None => {
                    tracing::error!(
                        %inst.runner_id,
                        "mailbox already taken — refusing to spawn a duplicate RunnerLoop"
                    );
                    continue;
                }
            };
            let runner_paths = inst.paths.clone();
            let runner_config = inst.config.clone();
            let inst_state = inst.state.clone();
            let inst_approvals = inst.approvals.clone();
            let inst_out = inst.out.clone();
            let inst_ack_tx = inst.ack_tx.clone();
            let inst_remove_tx = inst.remove_tx.clone();
            let live_mailboxes = mailboxes.clone();
            let live_hello_runners = hello_runners.clone();
            let daemon_paths = paths.clone();
            let h = tokio::spawn(async move {
                let run = RunnerLoop {
                    runner_paths,
                    paths: daemon_paths,
                    runner_config,
                    out: inst_out,
                    state: inst_state,
                    approvals: inst_approvals,
                    inbound: mailbox_rx,
                    ack_tx: inst_ack_tx,
                    remove_tx: inst_remove_tx,
                    live_mailboxes,
                    live_hello_runners,
                    current_run: None,
                    current_chat: None,
                };
                if let Err(e) = run.run().await {
                    tracing::error!("runner loop exited: {e:#}");
                }
            });
            loop_handles.push(h);

            if let Some(client) = inst.client.clone() {
                let ack_rx = match inst.take_ack_rx().await {
                    Some(rx) => rx,
                    None => {
                        tracing::error!(
                            %inst.runner_id,
                            "ack receiver already taken — refusing to spawn a duplicate HttpLoop"
                        );
                        continue;
                    }
                };
                let http_loop = HttpLoop::new(
                    client.clone(),
                    inst.mailbox_tx.clone(),
                    ack_rx,
                    inst.state.rx_status.clone(),
                    inst.state.rx_in_flight.clone(),
                    inst.state.shutdown_notified(),
                    attach_body_for_instance(inst),
                )
                .with_state(inst.state.clone());
                let http_handle = tokio::spawn(async move {
                    if let Err(e) = http_loop.run().await {
                        tracing::error!("http loop exited: {e:#}");
                    }
                });
                http_handles.push(http_handle);

                let refresh_state = inst.state.clone();
                let refresh_handle = tokio::spawn(async move {
                    refresh_loop(client, refresh_state).await;
                });
                refresh_handles.push(refresh_handle);
            }
        }

        let shutdown = state.shutdown_notified();
        let sig = crate::util::signal::shutdown();
        tokio::select! {
            _ = shutdown.notified() => {
                tracing::info!("shutdown requested via IPC");
            }
            r = sig => {
                if let Err(e) = r { tracing::warn!("signal watcher failed: {e:#}"); }
            }
        }

        // Drain in-flight runs before tearing down: send RunFailed with
        // DaemonRestart so the cloud transitions each run to FAILED via
        // a deliberate signal instead of leaving them BUSY for the
        // heartbeat reaper to clean up after our successor reconnects.
        // Bounded by a 5s deadline so a slow / unreachable cloud can't
        // hang the shutdown — systemd's default TimeoutStopSec (90s)
        // would still SIGKILL us, but losing a clean error message is
        // better than losing the entire shutdown sequence.
        if let Err(_elapsed) = tokio::time::timeout(
            Duration::from_secs(5),
            drain_in_flight_runs(hello_runners.clone()),
        )
        .await
        {
            tracing::warn!("drain of in-flight runs timed out at 5s");
        }

        for h in loop_handles {
            h.abort();
        }
        for h in http_handles {
            h.abort();
        }
        for h in refresh_handles {
            h.abort();
        }
        ipc_handle.abort();
        Ok(())
    }
}

/// On daemon shutdown, send `RunFailed{DaemonRestart}` for every
/// runner that currently reports an in-flight run. Without this the
/// cloud is left guessing — its heartbeat reaper eventually fails the
/// run with the cryptic ``reaped by heartbeat: ... in_flight_run=
/// (none)`` message after the daemon restarts and the next session
/// Hello legitimately reports null. This sends a clean terminal
/// signal instead.
///
/// Returns the count of runs we successfully drained. Send errors are
/// logged at warn but do not abort the drain — one runner's
/// unreachable cloud client must not block another runner's drain.
///
/// Sends are issued in parallel via `join_all` and each is bounded by
/// a 2s timeout. The outer 5s timeout in `Supervisor::run` still caps
/// total wall-time, but with up to ~30 concurrent runners on one host
/// a serial loop (each attempt potentially walking through the shared
/// reqwest Client's 60s timeout) would starve later runners entirely.
async fn drain_in_flight_runs(runners: Arc<RwLock<HelloRunnerMap>>) -> usize {
    // Snapshot under the read lock so the lock is dropped before any
    // network I/O; concurrent writers (config reloads, etc.) are not
    // expected during shutdown but this keeps the contract clean.
    let snapshot: Vec<(uuid::Uuid, Option<uuid::Uuid>, RunnerOut)> = {
        let guard = runners.read().await;
        guard
            .iter()
            .map(|(rid, (out, state, _))| (*rid, *state.rx_in_flight.borrow(), out.clone()))
            .collect()
    };
    let now = Utc::now();
    let drains = snapshot.into_iter().filter_map(|(runner_id, in_flight, out)| {
        let run_id = in_flight?;
        let msg = ClientMsg::RunFailed {
            run_id,
            reason: FailureReason::DaemonRestart,
            detail: Some("daemon shutdown requested".to_string()),
            ended_at: now,
        };
        Some(async move {
            // Per-attempt timeout so one stuck cloud client can't keep
            // a parallel sibling from completing in time.
            match tokio::time::timeout(Duration::from_secs(2), out.send(msg)).await {
                Ok(Ok(())) => {
                    tracing::info!(%runner_id, %run_id, "drained in-flight run on shutdown");
                    true
                }
                Ok(Err(e)) => {
                    tracing::warn!(%runner_id, %run_id, "drain send failed: {e:#}");
                    false
                }
                Err(_) => {
                    tracing::warn!(%runner_id, %run_id, "drain send timed out at 2s");
                    false
                }
            }
        })
    });
    futures_util::future::join_all(drains)
        .await
        .into_iter()
        .filter(|ok| *ok)
        .count()
}

/// Watch the `connected` notify and re-emit one `Hello` per `RunnerInstance`
/// every time it fires. Driven by `ConnectionLoop`, which calls
/// `notify_one()` after each successful WS handshake (cold start and every
/// reconnect). Cloud-side `_handle_token_hello` is idempotent on re-Hello,
/// so a second emission for an already-authorised runner is harmless.
///
/// Also flips the daemon-level ``connected`` flag — the per-runner
/// Welcome handler also sets this on each Welcome, but with zero
/// runners there's no Hello/Welcome cycle to fall back on, so the
/// IPC / TUI would otherwise show "cloud offline" forever.
#[allow(dead_code)]
async fn hello_emitter(
    runners: Arc<RwLock<HelloRunnerMap>>,
    connected: Arc<tokio::sync::Notify>,
    daemon_state: StateHandle,
) {
    loop {
        connected.notified().await;
        daemon_state.set_connected(true).await;
        let current_runners: Vec<HelloRunner> =
            { runners.read().await.values().cloned().collect() };
        for (out, state, project_slug) in current_runners {
            let hello = ClientMsg::Hello {
                runner_id: out.runner_id(),
                version: crate::RUNNER_VERSION.to_string(),
                os: std::env::consts::OS.to_string(),
                arch: std::env::consts::ARCH.to_string(),
                status: *state.rx_status.borrow(),
                in_flight_run: *state.rx_in_flight.borrow(),
                protocol_version: crate::PROTOCOL_VERSION,
                project_slug,
            };
            // Channel-closed means the cloud loop exited; the next
            // reconnect will re-fire the notify and we'll retry then.
            // Best-effort: don't bring down the daemon over a single
            // failed Hello.
            let _ = out.send(hello).await;
        }
    }
}

async fn load_runner_credentials(
    runner_paths: &RunnerPaths,
    runner_name: &str,
) -> Result<CredentialsHandle> {
    crate::cloud::http::load_runner_credentials_from(runner_paths.credentials_path(), runner_name)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn attach_body_for_instance(inst: &RunnerInstance) -> AttachBody {
    let mut agent_versions = HashMap::new();
    agent_versions.insert(
        format!("{:?}", inst.config.agent.kind).to_ascii_lowercase(),
        crate::RUNNER_VERSION.to_string(),
    );
    AttachBody {
        version: crate::RUNNER_VERSION.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        status: PollStatus::from_wire(
            *inst.state.rx_status.borrow(),
            *inst.state.rx_in_flight.borrow(),
        )
        .status,
        in_flight_run: *inst.state.rx_in_flight.borrow(),
        project_slug: inst.config.project_slug.clone(),
        host_label: hostname().unwrap_or_else(|| inst.config.name.clone()),
        agent_versions,
    }
}

async fn refresh_loop(client: RunnerCloudClient, state: StateHandle) {
    // Acquire the shutdown Notify ONCE and pin it across the whole loop
    // so a `notify_one()` that fires between iterations isn't dropped on
    // the floor. Re-acquiring per-iteration would race: each iteration
    // obtains a fresh `Arc<Notify>` view, so a notify between two
    // iterations only wakes the previous (already-dropped) waiter.
    let shutdown = state.shutdown_notified();
    let shutdown_fut = shutdown.notified();
    tokio::pin!(shutdown_fut);
    loop {
        let sleep_for = match client.access_token_exp().await {
            Some(exp) => {
                let now = Utc::now();
                let safety = Duration::from_secs(300);
                exp.signed_duration_since(now)
                    .to_std()
                    .unwrap_or_default()
                    .saturating_sub(safety)
            }
            None => Duration::from_secs(60),
        };
        tokio::select! {
            biased;
            _ = &mut shutdown_fut => return,
            _ = tokio::time::sleep(sleep_for) => {}
        }
        if let Err(e) = client.refresh().await {
            tracing::error!(runner_id = %client.runner_id(), "scheduled refresh failed: {e:#}");
        }
    }
}

/// Spawn a small task that flips `agent_subprocess_alive` to false the
/// moment the bridge-owned wait task observes the agent subprocess
/// terminating. Independent of stdout-close detection so a `kill -9`
/// path that double-forks or otherwise keeps stdout open is still
/// caught for observability.
///
/// Scoped to `run_id`: if a new run has taken over the in-flight slot
/// by the time this task fires, the alive flag is left alone — the new
/// run's own watcher owns it. Without this guard, run A's exit could
/// stamp `alive=false` on run B's snapshot.
fn spawn_exit_watch(
    state: StateHandle,
    run_id: uuid::Uuid,
    mut exit_rx: tokio::sync::watch::Receiver<Option<crate::agent::ExitSnapshot>>,
) {
    tokio::spawn(async move {
        // A freshly-cloned watch::Receiver marks the current value as
        // "seen", so changed() only fires on values published *after*
        // we subscribed. Check borrow() first to catch the case where
        // the wait task already published Some(ExitSnapshot) before we
        // got here (e.g. agent binary segfaults on startup).
        let already_exited = exit_rx.borrow().is_some();
        if !already_exited {
            if exit_rx.changed().await.is_err() {
                return;
            }
            if exit_rx.borrow().is_none() {
                return;
            }
        }
        // Guard: only stamp alive=false if the in-flight run is still
        // ours. A new run may already have taken over and called
        // set_agent_alive(true); we must not stomp it.
        if *state.rx_in_flight.borrow() != Some(run_id) {
            return;
        }
        state.set_agent_alive(false).await;
    });
}

fn hostname() -> Option<String> {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

struct RunnerLoop {
    runner_paths: RunnerPaths,
    /// Daemon-level paths. Threaded in so `ServerMsg::RemoveRunner` can
    /// strip this runner's `[[runner]]` block from `config.toml` under
    /// the host-wide config lock.
    paths: Paths,
    runner_config: crate::config::schema::RunnerConfig,
    out: RunnerOut,
    state: StateHandle,
    approvals: ApprovalRouter,
    inbound: mpsc::Receiver<InboundEnvelope>,
    ack_tx: mpsc::UnboundedSender<AckEntry>,
    /// Latched before the loop exits on `ServerMsg::RemoveRunner` so
    /// background tasks can stop even if they were not already blocked
    /// on the signal.
    remove_tx: tokio::sync::watch::Sender<bool>,
    live_mailboxes: Arc<RwLock<HashMap<uuid::Uuid, mpsc::Sender<InboundEnvelope>>>>,
    live_hello_runners: Arc<RwLock<HelloRunnerMap>>,
    /// In-flight run, if any. Replaced on each Assign and cleared as soon as
    /// the worker task signals completion via `done_rx` — driven by
    /// `tokio::select!` so a new Assign isn't rejected while we wait for the
    /// next inbound frame.
    current_run: Option<CurrentRun>,
    current_chat: Option<CurrentChat>,
}

struct CurrentRun {
    run_id: uuid::Uuid,
    cancel: std::sync::Arc<tokio::sync::Notify>,
    done_rx: oneshot::Receiver<()>,
}

struct CurrentChat {
    chat_session_id: uuid::Uuid,
    tx: mpsc::Sender<ChatCommand>,
    active_rx: watch::Receiver<bool>,
    done_rx: oneshot::Receiver<()>,
}

#[derive(Debug)]
struct ChatTurn {
    message_id: uuid::Uuid,
    content: String,
    cwd: Option<String>,
    model: Option<String>,
    local_thread_id: Option<String>,
    local_session_id: Option<String>,
}

#[derive(Debug)]
struct ChatWarm {
    cwd: Option<String>,
    model: Option<String>,
    local_thread_id: Option<String>,
    local_session_id: Option<String>,
}

#[derive(Debug)]
enum ChatCommand {
    Warm(ChatWarm),
    Message(ChatTurn),
    Cancel { reason: Option<String> },
    Close { reason: Option<String> },
    Shutdown,
}

impl RunnerLoop {
    async fn stop_idle_chat_runtime(&mut self) {
        let Some(mut chat) = self.current_chat.take() else {
            return;
        };
        let _ = chat.tx.send(ChatCommand::Shutdown).await;
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut chat.done_rx).await;
    }

    async fn start_chat_runtime(
        &mut self,
        chat_session_id: uuid::Uuid,
    ) -> mpsc::Sender<ChatCommand> {
        let (tx, rx) = mpsc::channel(8);
        let (active_tx, active_rx) = watch::channel(false);
        let (done_tx, done_rx) = oneshot::channel();
        self.current_chat = Some(CurrentChat {
            chat_session_id,
            tx: tx.clone(),
            active_rx,
            done_rx,
        });
        let mut worker = ChatWorker {
            runner_paths: self.runner_paths.clone(),
            runner_config: self.runner_config.clone(),
            state: self.state.clone(),
            approvals: self.approvals.clone(),
            out: self.out.clone(),
            command_rx: rx,
            active_tx,
        };
        tokio::spawn(async move {
            worker.run(chat_session_id).await;
            let _ = done_tx.send(());
        });
        tx
    }

    async fn run(mut self) -> Result<()> {
        loop {
            let frame = {
                let inbound = self.inbound.recv();
                tokio::pin!(inbound);
                // `done_rx` exists only while a run is in flight; outside of that
                // window we wait on `pending()` so the select arm is inert.
                tokio::select! {
                    biased;
                    () = wait_done(&mut self.current_run) => {
                        self.current_run = None;
                        continue;
                    }
                    () = wait_chat_done(&mut self.current_chat) => {
                        self.current_chat = None;
                        continue;
                    }
                    f = &mut inbound => f,
                }
            };
            let Some(frame) = frame else { break };
            let stream_id = frame.stream_id.clone();
            let frame = frame.env;
            let mut should_break = false;

            match frame.body {
                ServerMsg::Welcome {
                    protocol_version,
                    heartbeat_interval_secs,
                    latest_runner_version,
                    min_runner_version,
                    ..
                } => {
                    if protocol_version != WIRE_VERSION {
                        tracing::warn!(
                            server = protocol_version,
                            local = WIRE_VERSION,
                            "protocol version mismatch",
                        );
                    }
                    if heartbeat_interval_secs > 0 {
                        let _ = self.state.tx_heartbeat_secs.send(heartbeat_interval_secs);
                    }
                    if latest_runner_version.is_some() || min_runner_version.is_some() {
                        tracing::info!(
                            latest = ?latest_runner_version,
                            min = ?min_runner_version,
                            "received runner version advisory from cloud",
                        );
                    }
                    self.state
                        .set_update_advisory(latest_runner_version.clone(), min_runner_version)
                        .await;
                    self.state.set_connected(true).await;

                    // Auto-update path: when the user has opted in, the
                    // cloud is announcing a newer version, no swap is
                    // already in flight, AND the on-disk binary is not
                    // already at the announced version, spawn a
                    // background task to swap. We deliberately do NOT
                    // restart the running daemon — it keeps its loaded
                    // copy until the next natural restart. See
                    // `runner/README.md` (Auto-update).
                    if let Some(latest) = latest_runner_version.as_deref()
                        && version_lt(crate::RUNNER_VERSION, latest)
                        && self.state.on_disk_version().await.as_deref() != Some(latest)
                        && self.state.auto_update_enabled().await
                        && self.state.try_claim_swap()
                    {
                        let st = self.state.clone();
                        let latest_owned = latest.to_string();
                        tokio::spawn(async move {
                            tracing::info!(
                                target = %latest_owned,
                                "auto-update: swapping pidash on disk",
                            );
                            match crate::cli::update::check_or_swap(false).await {
                                Ok(crate::cli::update::SwapOutcome::Swapped {
                                    new_version,
                                    ..
                                }) => {
                                    tracing::info!(
                                        %new_version,
                                        "auto-update: swap complete; restart to apply",
                                    );
                                    st.set_on_disk_version(new_version).await;
                                }
                                Ok(other) => {
                                    tracing::debug!(?other, "auto-update: no swap performed",);
                                }
                                Err(e) => {
                                    // Can be no-receipt (source build), a
                                    // GitHub network/rate-limit error, or a
                                    // failed installer run. Logged at info so
                                    // operators can see the cause without
                                    // editorialising about which it is.
                                    tracing::info!(
                                        error = %e,
                                        "auto-update: swap failed",
                                    );
                                }
                            }
                            st.release_swap();
                        });
                    }
                }
                ServerMsg::Assign {
                    run_id,
                    prompt,
                    repo_url,
                    git_work_branch,
                    expected_codex_model,
                    ..
                } => {
                    if self
                        .current_chat
                        .as_ref()
                        .is_some_and(|chat| *chat.active_rx.borrow())
                    {
                        tracing::warn!(
                            %run_id,
                            "assign received while chat is active; ignoring"
                        );
                        continue;
                    }
                    if self.current_chat.is_some() {
                        self.stop_idle_chat_runtime().await;
                    }
                    if self.current_run.is_some() {
                        tracing::warn!(
                            %run_id,
                            "assign received while a run is already in flight; ignoring"
                        );
                        continue;
                    }
                    let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
                    let (done_tx, done_rx) = oneshot::channel();
                    self.current_run = Some(CurrentRun {
                        run_id,
                        cancel: cancel.clone(),
                        done_rx,
                    });
                    // Stamp ``rx_in_flight = Some(run_id)`` synchronously, before
                    // the worker does anything slow. The cloud put this run
                    // into a BUSY status the moment it sent the Assign;
                    // ``reap_stale_busy_runs`` (services/session_service.py)
                    // will fail any BUSY run whose runner Hellos with
                    // ``in_flight_run=null``. Without this early stamp, a
                    // session reconnect during workspace setup (which can
                    // take 30s+ for a fresh clone) reports null truthfully
                    // and the run gets reaped before it ever starts.
                    // The worker re-stamps a fuller summary at the
                    // ``set_current_run`` site below; same run_id, so the
                    // watch channel doesn't toggle in/out of None.
                    self.state
                        .set_current_run(Some(CurrentRunSummary {
                            run_id,
                            thread_id: None,
                            status: "preparing".to_string(),
                            started_at: Utc::now(),
                            events: 0,
                        }))
                        .await;
                    let runner_paths = self.runner_paths.clone();
                    let daemon_paths = self.paths.clone();
                    let runner_config = self.runner_config.clone();
                    let state = self.state.clone();
                    let approvals = self.approvals.clone();
                    let out = self.out.clone();
                    tokio::spawn(async move {
                        let mut worker = AssignWorker {
                            runner_paths,
                            daemon_paths,
                            runner_config,
                            state,
                            approvals,
                            out,
                            cancel,
                        };
                        if let Err(e) = worker
                            .run(
                                run_id,
                                prompt,
                                repo_url,
                                git_work_branch,
                                expected_codex_model,
                            )
                            .await
                        {
                            tracing::error!("run {run_id} failed: {e:#}");
                            let _ = worker
                                .out
                                .send(ClientMsg::RunFailed {
                                    run_id,
                                    reason: FailureReason::Internal,
                                    detail: Some(format!("{e:#}")),
                                    ended_at: Utc::now(),
                                })
                                .await;
                            worker.state.set_current_run(None).await;
                        }
                        let _ = done_tx.send(());
                    });
                }
                ServerMsg::Cancel { run_id, reason } => {
                    tracing::info!(%run_id, ?reason, "cancel received");
                    if let Some(run) = &self.current_run {
                        if run.run_id == run_id {
                            run.cancel.notify_waiters();
                        } else {
                            tracing::warn!(
                                "cancel for run {run_id} but active run is {active}; ignoring",
                                active = run.run_id,
                            );
                        }
                    }
                }
                ServerMsg::Decide {
                    approval_id,
                    decision,
                    ..
                } => {
                    self.approvals
                        .decide(&approval_id.to_string(), decision, DecisionSource::Cloud)
                        .await;
                    let pending = self.approvals.list_pending().await.len();
                    self.state.set_approvals_pending(pending).await;
                }
                ServerMsg::ChatUserMessage {
                    chat_session_id,
                    message_id,
                    content,
                    content_parts: _,
                    local_thread_id,
                    local_session_id,
                    cwd,
                    model,
                } => {
                    if self.current_run.is_some() {
                        let _ = self
                            .out
                            .send(ClientMsg::ChatFailed {
                                chat_session_id,
                                code: "runner_busy".into(),
                                detail: Some("runner has an active task".into()),
                                failed_at: Utc::now(),
                            })
                            .await;
                        continue;
                    }
                    let turn = ChatTurn {
                        message_id,
                        content,
                        cwd,
                        model,
                        local_thread_id,
                        local_session_id,
                    };
                    let tx = if let Some(chat) = &self.current_chat {
                        if chat.chat_session_id == chat_session_id {
                            if *chat.active_rx.borrow() {
                                let _ = self
                                    .out
                                    .send(ClientMsg::ChatFailed {
                                        chat_session_id,
                                        code: "chat_turn_active".into(),
                                        detail: Some("runner has an active chat turn".into()),
                                        failed_at: Utc::now(),
                                    })
                                    .await;
                                continue;
                            }
                            chat.tx.clone()
                        } else if *chat.active_rx.borrow() {
                            let _ = self
                                .out
                                .send(ClientMsg::ChatFailed {
                                    chat_session_id,
                                    code: "chat_turn_active".into(),
                                    detail: Some("runner has an active chat turn".into()),
                                    failed_at: Utc::now(),
                                })
                                .await;
                            continue;
                        } else {
                            self.stop_idle_chat_runtime().await;
                            self.start_chat_runtime(chat_session_id).await
                        }
                    } else {
                        self.start_chat_runtime(chat_session_id).await
                    };
                    if tx.send(ChatCommand::Message(turn)).await.is_err() {
                        let _ = self
                            .out
                            .send(ClientMsg::ChatFailed {
                                chat_session_id,
                                code: "chat_runtime_closed".into(),
                                detail: Some("chat runtime closed before accepting message".into()),
                                failed_at: Utc::now(),
                            })
                            .await;
                    }
                }
                ServerMsg::ChatWarm {
                    chat_session_id,
                    local_thread_id,
                    local_session_id,
                    cwd,
                    model,
                } => {
                    if self.current_run.is_some() {
                        let _ = self
                            .out
                            .send(ClientMsg::ChatEvent {
                                chat_session_id,
                                bridge_seq: 0,
                                kind: "chat_warm_skipped".into(),
                                payload: serde_json::json!({
                                    "reason": "runner_busy",
                                }),
                            })
                            .await;
                        continue;
                    }
                    let warm = ChatWarm {
                        cwd,
                        model,
                        local_thread_id,
                        local_session_id,
                    };
                    let tx = if let Some(chat) = &self.current_chat {
                        if chat.chat_session_id == chat_session_id {
                            if *chat.active_rx.borrow() {
                                let _ = self
                                    .out
                                    .send(ClientMsg::ChatEvent {
                                        chat_session_id,
                                        bridge_seq: 0,
                                        kind: "chat_warm_skipped".into(),
                                        payload: serde_json::json!({
                                            "reason": "chat_turn_active",
                                        }),
                                    })
                                    .await;
                                continue;
                            }
                            chat.tx.clone()
                        } else if *chat.active_rx.borrow() {
                            let _ = self
                                .out
                                .send(ClientMsg::ChatEvent {
                                    chat_session_id,
                                    bridge_seq: 0,
                                    kind: "chat_warm_skipped".into(),
                                    payload: serde_json::json!({
                                        "reason": "chat_turn_active",
                                    }),
                                })
                                .await;
                            continue;
                        } else {
                            self.stop_idle_chat_runtime().await;
                            self.start_chat_runtime(chat_session_id).await
                        }
                    } else {
                        self.start_chat_runtime(chat_session_id).await
                    };
                    if tx.send(ChatCommand::Warm(warm)).await.is_err() {
                        let _ = self
                            .out
                            .send(ClientMsg::ChatEvent {
                                chat_session_id,
                                bridge_seq: 0,
                                kind: "chat_warm_failed".into(),
                                payload: serde_json::json!({
                                    "detail": "chat runtime closed before accepting warm request",
                                }),
                            })
                            .await;
                    }
                }
                ServerMsg::ChatCancel {
                    chat_session_id,
                    reason,
                } => {
                    tracing::info!(%chat_session_id, ?reason, "chat_cancel received");
                    match &self.current_chat {
                        Some(chat) if chat.chat_session_id == chat_session_id => {
                            let _ = chat.tx.send(ChatCommand::Cancel { reason }).await;
                        }
                        _ => {}
                    }
                }
                ServerMsg::ChatClose {
                    chat_session_id,
                    reason,
                } => {
                    tracing::info!(%chat_session_id, ?reason, "chat_close received");
                    if let Some(chat) = &self.current_chat {
                        if chat.chat_session_id == chat_session_id {
                            let _ = chat.tx.send(ChatCommand::Close { reason }).await;
                        }
                    } else {
                        let _ = self
                            .out
                            .send(ClientMsg::ChatClosed {
                                chat_session_id,
                                closed_at: Utc::now(),
                            })
                            .await;
                    }
                }
                ServerMsg::ChatDecide {
                    local_approval_id,
                    decision,
                    ..
                } => {
                    self.approvals
                        .decide(&local_approval_id, decision, DecisionSource::Cloud)
                        .await;
                    let pending = self.approvals.list_pending().await.len();
                    self.state.set_approvals_pending(pending).await;
                }
                ServerMsg::ConfigPush { .. } => {
                    tracing::info!("config_push received (deferred)");
                }
                ServerMsg::Ping { .. } => {
                    // Connection-scoped — handled by the supervisor's
                    // demux task before frames reach the per-runner
                    // mailbox. Should never fire here; if it does, the
                    // demux routing rule was violated.
                    tracing::warn!("Ping arrived at RunnerLoop; demux invariant violated");
                }
                ServerMsg::ResumeAck {
                    run_id,
                    last_seq,
                    status,
                    ..
                } => {
                    tracing::info!(
                        %run_id,
                        ?last_seq,
                        %status,
                        "cloud acked run resume"
                    );
                }
                ServerMsg::Revoke { .. } => {
                    // Per-runner revoke (HTTP transport). The HttpLoop
                    // already triggered shutdown for this runner, but
                    // we may receive the frame as a redelivery.
                    tracing::warn!("runner revoke received; loop will exit");
                    should_break = true;
                }
                ServerMsg::ForceRefresh { .. } => {
                    // Handled inline in HttpLoop. If we receive it
                    // here it's a bug or a redelivery; just log.
                    tracing::debug!(
                        "ForceRefresh arrived at RunnerLoop; HttpLoop should have handled it"
                    );
                }
                ServerMsg::RemoveRunner { runner_id, reason } => {
                    // Per-instance teardown: exit ONLY this RunnerLoop;
                    // the WS connection and other RunnerInstances stay
                    // up. The demux already routed by envelope.runner_id
                    // before we got here, so the rid-mismatch check
                    // below is defensive.
                    if runner_id != self.runner_paths.runner_id {
                        tracing::warn!(
                            "received RemoveRunner for {runner_id}, but this loop is \
                             {}; ignoring",
                            self.runner_paths.runner_id,
                        );
                        continue;
                    }
                    tracing::warn!(
                        "cloud removed runner {runner_id}: {}",
                        reason.as_deref().unwrap_or("(no reason)"),
                    );
                    if let Some(run) = &self.current_run {
                        run.cancel.notify_waiters();
                    }
                    // Tell the heartbeat task to exit before we drop
                    // out of the loop. Without this it would keep
                    // emitting frames carrying this runner's id and
                    // the cloud would drop each one with an
                    // `unknown rid` warning until the daemon restarts.
                    let _ = self.remove_tx.send(true);
                    self.live_mailboxes
                        .write()
                        .await
                        .remove(&self.runner_paths.runner_id);
                    self.live_hello_runners
                        .write()
                        .await
                        .remove(&self.runner_paths.runner_id);
                    // Best-effort cleanup of this runner's local data
                    // dir. The on-disk state is keyed by runner_id and
                    // is dead weight once the cloud-side row is gone.
                    let runner_dir = self.runner_paths.base_dir().to_path_buf();
                    if runner_dir.exists()
                        && let Err(e) = std::fs::remove_dir_all(&runner_dir)
                    {
                        tracing::warn!(
                            "failed to delete {:?}: {e:#} (file removal is best-effort)",
                            runner_dir,
                        );
                    }
                    // Strip the matching `[[runner]]` block from
                    // config.toml under the host-wide config lock so a
                    // concurrent `pidash runner add` can't lose its
                    // write. Without this the next daemon restart would
                    // re-Hello for the dead runner_id and the cloud
                    // would tear it down again on every boot.
                    //
                    // The systemd unit (`pidash.service`) hosts every
                    // runner under one process, so we deliberately
                    // never touch it here — only `pidash uninstall`
                    // does. If this was the last runner, log a hint
                    // so the operator knows they can remove the unit.
                    let target_runner_id = self.runner_paths.runner_id;
                    let runner_name = self.runner_config.name.clone();
                    let paths_for_strip = self.paths.clone();
                    let strip_result = tokio::task::spawn_blocking(move || {
                        crate::config::file::mutate_config(&paths_for_strip, |cfg| {
                            cfg.runners.retain(|r| r.runner_id != target_runner_id);
                            Ok(())
                        })
                    })
                    .await;
                    match strip_result {
                        Ok(Ok(post)) => {
                            tracing::info!(
                                runner = %runner_name,
                                runner_id = %target_runner_id,
                                remaining = post.runners.len(),
                                "stripped [[runner]] block from config.toml",
                            );
                            if post.runners.is_empty() {
                                tracing::warn!(
                                    "config.toml has no remaining runners; \
                                     run `pidash uninstall` to remove the \
                                     pidash.service systemd unit if you \
                                     no longer need the daemon on this host.",
                                );
                            }
                        }
                        Ok(Err(e)) => {
                            tracing::warn!(
                                runner = %runner_name,
                                runner_id = %target_runner_id,
                                "failed to strip [[runner]] block from \
                                 config.toml: {e:#}. Run \
                                 `pidash runner remove --local-only \
                                 --yes {runner_name}` to clean it up by \
                                 hand; otherwise the next daemon restart \
                                 will re-Hello this id.",
                            );
                        }
                        Err(join_err) => {
                            tracing::warn!("config-strip task panicked: {join_err:#}",);
                        }
                    }
                    should_break = true;
                }
            }
            if let Some(stream_id) = stream_id {
                let _ = self.ack_tx.send(AckEntry { stream_id });
            }
            if should_break {
                break;
            }
        }
        self.state.set_connected(false).await;
        Ok(())
    }
}

async fn wait_done(current: &mut Option<CurrentRun>) {
    match current {
        Some(run) => {
            let _ = (&mut run.done_rx).await;
        }
        None => std::future::pending().await,
    }
}

async fn wait_chat_done(current: &mut Option<CurrentChat>) {
    match current {
        Some(chat) => {
            let _ = (&mut chat.done_rx).await;
        }
        None => std::future::pending().await,
    }
}

fn chat_resume_id(
    local_session_id: Option<&str>,
    local_thread_id: Option<&str>,
) -> Option<String> {
    local_session_id
        .filter(|s| !s.is_empty())
        .or_else(|| local_thread_id.filter(|s| !s.is_empty()))
        .map(ToOwned::to_owned)
}

fn bridge_has_exited(bridge: &AgentBridge) -> bool {
    let handle = bridge.process_handle();
    handle.exit_rx.borrow().is_some()
}

fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

fn bridge_event_label(ev: &BridgeEvent) -> &'static str {
    match ev {
        BridgeEvent::RunStarted { .. } => "run_started",
        BridgeEvent::Raw { .. } => "raw",
        BridgeEvent::ApprovalRequest { .. } => "approval_request",
        BridgeEvent::AwaitingReauth { .. } => "awaiting_reauth",
        BridgeEvent::Completed { .. } => "completed",
        BridgeEvent::Failed { .. } => "failed",
    }
}

fn assistant_delta_text(params: &serde_json::Value) -> Option<&str> {
    if let Some(delta) = params.get("delta") {
        if let Some(text) = delta.as_str() {
            return Some(text);
        }
        if let Some(text) = delta.get("text").and_then(|value| value.as_str()) {
            return Some(text);
        }
    }
    params.get("text").and_then(|value| value.as_str())
}

fn is_assistant_text_delta(method: &str, params: &serde_json::Value) -> bool {
    if method == "item/agentMessage/delta" {
        return assistant_delta_text(params).is_some();
    }
    method == "stream_event/content_block_delta"
        && params
            .get("delta")
            .and_then(|delta| delta.get("type"))
            .and_then(|value| value.as_str())
            == Some("text_delta")
        && params
            .get("delta")
            .and_then(|delta| delta.get("text"))
            .and_then(|value| value.as_str())
            .is_some()
}

fn timing_payload(stage: &str, mut payload: serde_json::Value) -> serde_json::Value {
    if !payload.is_object() {
        payload = serde_json::json!({});
    }
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("stage".into(), serde_json::json!(stage));
        obj.insert("runner_recorded_at".into(), serde_json::json!(Utc::now()));
    }
    payload
}

struct ChatWorker {
    runner_paths: RunnerPaths,
    runner_config: crate::config::schema::RunnerConfig,
    state: StateHandle,
    approvals: ApprovalRouter,
    out: RunnerOut,
    command_rx: mpsc::Receiver<ChatCommand>,
    active_tx: watch::Sender<bool>,
}

impl ChatWorker {
    async fn run(&mut self, chat_session_id: uuid::Uuid) {
        const CHAT_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

        let mut bridge: Option<AgentBridge> = None;
        let mut workspace_path: Option<std::path::PathBuf> = None;
        let mut bridge_seq = 0u64;
        let mut started_sent = false;

        loop {
            let command =
                match tokio::time::timeout(CHAT_IDLE_TIMEOUT, self.command_rx.recv()).await {
                    Ok(Some(command)) => command,
                    Ok(None) | Err(_) => break,
                };
            match command {
                ChatCommand::Warm(warm) => {
                    if let Err(e) = self
                        .handle_warm(
                            chat_session_id,
                            warm,
                            &mut bridge,
                            &mut workspace_path,
                            &mut bridge_seq,
                            &mut started_sent,
                        )
                        .await
                    {
                        let _ = self
                            .out
                            .send(ClientMsg::ChatEvent {
                                chat_session_id,
                                bridge_seq,
                                kind: "chat_warm_failed".into(),
                                payload: serde_json::json!({
                                    "detail": format!("{e:#}"),
                                }),
                            })
                            .await;
                        if let Some(bridge) = bridge.take() {
                            bridge.shutdown(Duration::from_secs(5)).await.ok();
                        }
                        workspace_path.take();
                        started_sent = false;
                    }
                }
                ChatCommand::Message(turn) => {
                    let _ = self.active_tx.send(true);
                    self.state.set_status(RunnerStatus::Busy).await;
                    let close_runtime = match self
                        .handle_turn(
                            chat_session_id,
                            turn,
                            &mut bridge,
                            &mut workspace_path,
                            &mut bridge_seq,
                            &mut started_sent,
                        )
                        .await
                    {
                        Ok(close_runtime) => close_runtime,
                        Err(e) => {
                            let _ = self
                                .out
                                .send(ClientMsg::ChatFailed {
                                    chat_session_id,
                                    code: "internal".into(),
                                    detail: Some(format!("{e:#}")),
                                    failed_at: Utc::now(),
                                })
                                .await;
                            if let Some(bridge) = bridge.take() {
                                bridge.shutdown(Duration::from_secs(5)).await.ok();
                            }
                            started_sent = false;
                            false
                        }
                    };
                    let _ = self.active_tx.send(false);
                    self.state.set_status(RunnerStatus::Idle).await;
                    if close_runtime {
                        break;
                    }
                }
                ChatCommand::Cancel { reason } => {
                    tracing::debug!(?reason, %chat_session_id, "chat cancel received while idle");
                }
                ChatCommand::Close { reason } => {
                    tracing::debug!(?reason, %chat_session_id, "chat close received while idle");
                    let _ = self
                        .out
                        .send(ClientMsg::ChatClosed {
                            chat_session_id,
                            closed_at: Utc::now(),
                        })
                        .await;
                    break;
                }
                ChatCommand::Shutdown => break,
            }
        }
        if let Some(bridge) = bridge.take() {
            bridge.shutdown(Duration::from_secs(5)).await.ok();
        }
        let _ = self.active_tx.send(false);
        self.state.set_status(RunnerStatus::Idle).await;
        let _ = &self.runner_paths;
    }

    async fn send_timing_event(
        &self,
        chat_session_id: uuid::Uuid,
        bridge_seq: &mut u64,
        stage: &str,
        payload: serde_json::Value,
    ) {
        *bridge_seq = (*bridge_seq).saturating_add(1);
        self.out
            .send(ClientMsg::ChatEvent {
                chat_session_id,
                bridge_seq: *bridge_seq,
                kind: "chat_timing".into(),
                payload: timing_payload(stage, payload),
            })
            .await
            .ok();
    }

    async fn handle_warm(
        &mut self,
        chat_session_id: uuid::Uuid,
        warm: ChatWarm,
        bridge: &mut Option<AgentBridge>,
        workspace_path: &mut Option<std::path::PathBuf>,
        bridge_seq: &mut u64,
        started_sent: &mut bool,
    ) -> Result<()> {
        self.send_timing_event(
            chat_session_id,
            bridge_seq,
            "runner_warm_received",
            serde_json::json!({
                "local_session_id": warm.local_session_id.as_deref(),
                "local_thread_id": warm.local_thread_id.as_deref(),
            }),
        )
        .await;

        if workspace_path.is_none() {
            *workspace_path = Some(self.resolve_chat_workspace(warm.cwd.as_deref()).await?);
        }
        let workspace_path = workspace_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("chat workspace missing"))?;

        if bridge.as_ref().is_some_and(bridge_has_exited) {
            if let Some(bridge) = bridge.take() {
                bridge.shutdown(Duration::from_secs(1)).await.ok();
            }
            *started_sent = false;
        }

        let already_warm = bridge.is_some();
        let resume_id = chat_resume_id(
            warm.local_session_id.as_deref(),
            warm.local_thread_id.as_deref(),
        );
        if bridge.is_none() {
            let spawn_started = Instant::now();
            *bridge = Some(
                AgentBridge::spawn_from_config_with_resume(
                    &self.runner_config,
                    workspace_path,
                    warm.model,
                    resume_id.as_deref(),
                )
                .await?,
            );
            self.send_timing_event(
                chat_session_id,
                bridge_seq,
                "bridge_spawned",
                serde_json::json!({
                    "operation": "warm",
                    "duration_ms": elapsed_ms(spawn_started),
                    "resume_id_present": resume_id.is_some(),
                }),
            )
            .await;
        } else {
            self.send_timing_event(
                chat_session_id,
                bridge_seq,
                "bridge_reused",
                serde_json::json!({
                    "operation": "warm",
                }),
            )
            .await;
        }
        let bridge = bridge
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("chat bridge missing"))?;
        let warm_started = Instant::now();
        let warmed_thread_id = bridge.warm(workspace_path).await?;
        self.send_timing_event(
            chat_session_id,
            bridge_seq,
            "bridge_warmed",
            serde_json::json!({
                "operation": "warm",
                "duration_ms": elapsed_ms(warm_started),
                "already_warm": already_warm,
                "local_session_id": warmed_thread_id.as_deref(),
            }),
        )
        .await;
        match warmed_thread_id.as_ref() {
            Some(thread_id) if !*started_sent => {
                self.out
                    .send(ClientMsg::ChatStarted {
                        chat_session_id,
                        local_thread_id: thread_id.clone(),
                        local_session_id: Some(thread_id.clone()),
                        started_at: Utc::now(),
                    })
                    .await
                    .ok();
                *started_sent = true;
            }
            _ => {}
        }

        *bridge_seq = (*bridge_seq).saturating_add(1);
        self.out
            .send(ClientMsg::ChatEvent {
                chat_session_id,
                bridge_seq: *bridge_seq,
                kind: "chat_warmed".into(),
                payload: serde_json::json!({
                    "already_warm": already_warm,
                    "local_session_id": warmed_thread_id,
                }),
            })
            .await
            .ok();
        Ok(())
    }

    async fn handle_turn(
        &mut self,
        chat_session_id: uuid::Uuid,
        turn: ChatTurn,
        bridge: &mut Option<AgentBridge>,
        workspace_path: &mut Option<std::path::PathBuf>,
        bridge_seq: &mut u64,
        started_sent: &mut bool,
    ) -> Result<bool> {
        let message_id = turn.message_id;
        self.send_timing_event(
            chat_session_id,
            bridge_seq,
            "runner_message_received",
            serde_json::json!({
                "message_id": message_id,
                "local_session_id": turn.local_session_id.as_deref(),
                "local_thread_id": turn.local_thread_id.as_deref(),
            }),
        )
        .await;

        if workspace_path.is_none() {
            *workspace_path = Some(self.resolve_chat_workspace(turn.cwd.as_deref()).await?);
        }
        let workspace_path = workspace_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("chat workspace missing"))?;

        if bridge.as_ref().is_some_and(bridge_has_exited) {
            if let Some(bridge) = bridge.take() {
                bridge.shutdown(Duration::from_secs(1)).await.ok();
            }
            *started_sent = false;
        }

        let resume_id = chat_resume_id(
            turn.local_session_id.as_deref(),
            turn.local_thread_id.as_deref(),
        );
        if bridge.is_none() {
            let spawn_started = Instant::now();
            *bridge = Some(
                AgentBridge::spawn_from_config_with_resume(
                    &self.runner_config,
                    workspace_path,
                    turn.model.clone(),
                    resume_id.as_deref(),
                )
                .await?,
            );
            self.send_timing_event(
                chat_session_id,
                bridge_seq,
                "bridge_spawned",
                serde_json::json!({
                    "operation": "turn",
                    "message_id": message_id,
                    "duration_ms": elapsed_ms(spawn_started),
                    "resume_id_present": resume_id.is_some(),
                }),
            )
            .await;
        } else {
            self.send_timing_event(
                chat_session_id,
                bridge_seq,
                "bridge_reused",
                serde_json::json!({
                    "operation": "turn",
                    "message_id": message_id,
                }),
            )
            .await;
        }
        let bridge = bridge
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("chat bridge missing"))?;
        let payload = RunPayload {
            run_id: turn.message_id,
            prompt: turn.content,
            model: turn.model,
        };
        let turn_started_at = Instant::now();
        let mut cursor = bridge.run(&payload, workspace_path).await?;
        let turn_id = cursor.thread_id().to_string();
        self.send_timing_event(
            chat_session_id,
            bridge_seq,
            "bridge_turn_started",
            serde_json::json!({
                "message_id": message_id,
                "turn_id": turn_id.as_str(),
                "duration_ms": elapsed_ms(turn_started_at),
            }),
        )
        .await;
        if !*started_sent {
            self.out
                .send(ClientMsg::ChatStarted {
                    chat_session_id,
                    local_thread_id: turn_id.clone(),
                    local_session_id: Some(turn_id.clone()),
                    started_at: Utc::now(),
                })
                .await
                .ok();
            *started_sent = true;
        }
        self.out
            .send(ClientMsg::ChatMessageStarted {
                chat_session_id,
                message_id: turn.message_id,
                turn_id: Some(turn_id.clone()),
                started_at: Utc::now(),
            })
            .await
            .ok();

        let mut final_status = "completed".to_string();
        let mut assistant_message: Option<String> = None;
        let mut close_after_turn = false;
        let mut first_agent_event_sent = false;
        let mut first_assistant_text_sent = false;
        loop {
            tokio::select! {
                biased;
                command = self.command_rx.recv() => {
                    match command {
                        Some(ChatCommand::Cancel { reason }) => {
                            tracing::info!(?reason, %chat_session_id, "cancelling active chat turn");
                            bridge.interrupt().await.ok();
                            final_status = "cancelled".into();
                            break;
                        }
                        Some(ChatCommand::Close { reason }) => {
                            tracing::info!(?reason, %chat_session_id, "closing active chat turn");
                            bridge.interrupt().await.ok();
                            final_status = "cancelled".into();
                            close_after_turn = true;
                            break;
                        }
                        Some(ChatCommand::Shutdown) | None => {
                            bridge.interrupt().await.ok();
                            final_status = "cancelled".into();
                            break;
                        }
                        Some(ChatCommand::Warm(_)) => {
                            tracing::debug!(
                                %chat_session_id,
                                "chat warm ignored while turn is active"
                            );
                        }
                        Some(ChatCommand::Message(_)) => {
                            tracing::warn!(
                                %chat_session_id,
                                "chat runtime received a second message while a turn is active"
                            );
                        }
                    }
                }
                events = bridge.next_events(&mut cursor) => {
                    let Some(events) = events else {
                        self.send_timing_event(
                            chat_session_id,
                            bridge_seq,
                            "agent_stdout_closed",
                            serde_json::json!({
                                "message_id": message_id,
                                "turn_id": turn_id.as_str(),
                                "since_turn_start_ms": elapsed_ms(turn_started_at),
                            }),
                        ).await;
                        self.out.send(ClientMsg::ChatFailed {
                            chat_session_id,
                            code: "agent_stdout_closed".into(),
                            detail: Some("agent stdout closed".into()),
                            failed_at: Utc::now(),
                        }).await.ok();
                        self.state.set_status(RunnerStatus::Idle).await;
                        return Ok(true);
                    };
                    let mut done = false;
                    for ev in events {
                        if !first_agent_event_sent {
                            self.send_timing_event(
                                chat_session_id,
                                bridge_seq,
                                "first_agent_event",
                                serde_json::json!({
                                    "message_id": message_id,
                                    "turn_id": turn_id.as_str(),
                                    "event": bridge_event_label(&ev),
                                    "since_turn_start_ms": elapsed_ms(turn_started_at),
                                }),
                            ).await;
                            first_agent_event_sent = true;
                        }
                        *bridge_seq = (*bridge_seq).saturating_add(1);
                        match ev {
                            BridgeEvent::Raw { method, params, .. } => {
                                let kind = if is_assistant_text_delta(&method, &params) {
                                    "assistant_delta"
                                } else {
                                    "raw"
                                };
                                let first_text_delta_chars =
                                    if !first_assistant_text_sent && kind == "assistant_delta" {
                                        assistant_delta_text(&params)
                                            .filter(|text| !text.is_empty())
                                            .map(|text| text.chars().count())
                                    } else {
                                        None
                                    };
                                self.out.send(ClientMsg::ChatEvent {
                                    chat_session_id,
                                    bridge_seq: *bridge_seq,
                                    kind: kind.into(),
                                    payload: serde_json::json!({
                                        "method": method.as_str(),
                                        "params": params,
                                    }),
                                }).await.ok();
                                if let Some(delta_chars) = first_text_delta_chars {
                                    self.send_timing_event(
                                        chat_session_id,
                                        bridge_seq,
                                        "first_assistant_text",
                                        serde_json::json!({
                                            "message_id": message_id,
                                            "turn_id": turn_id.as_str(),
                                            "method": method.as_str(),
                                            "delta_chars": delta_chars,
                                            "since_turn_start_ms": elapsed_ms(turn_started_at),
                                        }),
                                    ).await;
                                    first_assistant_text_sent = true;
                                }
                            }
                            BridgeEvent::ApprovalRequest {
                                approval_id,
                                kind,
                                payload,
                                reason,
                                ..
                            } => {
                                let policy = Policy::new(&self.runner_config.approval_policy, workspace_path);
                                let decision = policy.evaluate(kind, &payload);
                                if let Some(auto) = decision.into_cloud() {
                                    if let Err(e) = bridge.send_approval(&approval_id, auto).await {
                                        self.out.send(ClientMsg::ChatFailed {
                                            chat_session_id,
                                            code: "approval_send_failed".into(),
                                            detail: Some(format!("{e:#}")),
                                            failed_at: Utc::now(),
                                        }).await.ok();
                                        self.state.set_status(RunnerStatus::Idle).await;
                                        return Ok(true);
                                    }
                                    continue;
                                }
                                let rec = ApprovalRecord {
                                    approval_id: approval_id.clone(),
                                    runner_id: self.runner_config.runner_id,
                                    run_id: turn.message_id,
                                    kind,
                                    payload: payload.clone(),
                                    reason: reason.clone(),
                                    requested_at: Utc::now(),
                                    expires_at: Some(Utc::now() + chrono::Duration::minutes(10)),
                                    status: crate::approval::router::ApprovalStatus::Pending,
                                };
                                let mut rx = self.approvals.subscribe();
                                self.approvals.open(rec.clone()).await;
                                self.state
                                    .set_approvals_pending(self.approvals.list_pending().await.len())
                                    .await;
                                self.out.send(ClientMsg::ChatApprovalRequest {
                                    chat_session_id,
                                    local_approval_id: approval_id.clone(),
                                    kind,
                                    payload: payload.clone(),
                                    reason,
                                    expires_at: rec.expires_at,
                                }).await.ok();
                                loop {
                                    match rx.recv().await {
                                        Ok(ApprovalRecord {
                                            approval_id: aid,
                                            status:
                                                ApprovalStatus::Resolved {
                                                    decision, ..
                                                },
                                            ..
                                        }) if aid == approval_id => {
                                            if let Err(e) = bridge.send_approval(&approval_id, decision).await {
                                                self.out.send(ClientMsg::ChatFailed {
                                                    chat_session_id,
                                                    code: "approval_send_failed".into(),
                                                    detail: Some(format!("{e:#}")),
                                                    failed_at: Utc::now(),
                                                }).await.ok();
                                                self.state.set_status(RunnerStatus::Idle).await;
                                                return Ok(true);
                                            }
                                            self.state
                                                .set_approvals_pending(self.approvals.list_pending().await.len())
                                                .await;
                                            break;
                                        }
                                        Ok(_) => continue,
                                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                                        Err(_) => break,
                                    }
                                }
                            }
                            BridgeEvent::Completed { done_payload, .. } => {
                                assistant_message = assistant_text_from_done_payload(&done_payload);
                                self.send_timing_event(
                                    chat_session_id,
                                    bridge_seq,
                                    "agent_completed",
                                    serde_json::json!({
                                        "message_id": message_id,
                                        "turn_id": turn_id.as_str(),
                                        "since_turn_start_ms": elapsed_ms(turn_started_at),
                                    }),
                                ).await;
                                final_status = "completed".into();
                                done = true;
                                break;
                            }
                            BridgeEvent::Failed { detail, .. } => {
                                self.send_timing_event(
                                    chat_session_id,
                                    bridge_seq,
                                    "agent_failed",
                                    serde_json::json!({
                                        "message_id": message_id,
                                        "turn_id": turn_id.as_str(),
                                        "since_turn_start_ms": elapsed_ms(turn_started_at),
                                        "detail": detail.as_deref(),
                                    }),
                                ).await;
                                self.out.send(ClientMsg::ChatFailed {
                                    chat_session_id,
                                    code: "agent_failed".into(),
                                    detail,
                                    failed_at: Utc::now(),
                                }).await.ok();
                                self.state.set_status(RunnerStatus::Idle).await;
                                return Ok(true);
                            }
                            BridgeEvent::RunStarted { .. } | BridgeEvent::AwaitingReauth { .. } => {}
                        }
                    }
                    if done {
                        break;
                    }
                }
            }
        }
        self.send_timing_event(
            chat_session_id,
            bridge_seq,
            "turn_completion",
            serde_json::json!({
                "message_id": message_id,
                "turn_id": turn_id.as_str(),
                "status": final_status.as_str(),
                "has_assistant_message": assistant_message.is_some(),
                "since_turn_start_ms": elapsed_ms(turn_started_at),
            }),
        )
        .await;
        self.out
            .send(ClientMsg::ChatMessageCompleted {
                chat_session_id,
                message_id: turn.message_id,
                turn_id: Some(turn_id),
                assistant_message,
                status: final_status,
                completed_at: Utc::now(),
            })
            .await
            .ok();
        if close_after_turn {
            let _ = self
                .out
                .send(ClientMsg::ChatClosed {
                    chat_session_id,
                    closed_at: Utc::now(),
                })
                .await;
        }
        Ok(close_after_turn)
    }

    async fn resolve_chat_workspace(&self, cwd: Option<&str>) -> Result<std::path::PathBuf> {
        let workspace_path = self.runner_config.workspace.working_dir.clone();
        std::fs::create_dir_all(&workspace_path)?;
        if let Some(cwd) = cwd.filter(|s| !s.is_empty()) {
            let requested = std::path::PathBuf::from(cwd);
            let requested = if requested.is_absolute() {
                requested
            } else {
                workspace_path.join(requested)
            };
            if !requested.starts_with(&workspace_path) {
                anyhow::bail!("chat cwd is outside runner workspace");
            }
            return Ok(requested);
        }
        Ok(workspace_path)
    }
}

fn assistant_text_from_done_payload(payload: &serde_json::Value) -> Option<String> {
    if let Some(text) = payload.as_str() {
        return Some(text.to_string());
    }
    let obj = payload.as_object()?;
    for key in ["result", "message", "text", "output", "summary", "content"] {
        if let Some(text) = obj.get(key).and_then(|value| value.as_str())
            && !text.is_empty()
        {
            return Some(text.to_string());
        }
    }
    None
}

/// Owns one `Assign`'s lifecycle. Spawned as a task from `RunnerLoop`, so the
/// message loop stays live and can deliver Cancel / Decide frames to us via
/// `self.cancel` and `self.approvals`.
struct AssignWorker {
    runner_paths: RunnerPaths,
    daemon_paths: Paths,
    runner_config: crate::config::schema::RunnerConfig,
    state: StateHandle,
    approvals: ApprovalRouter,
    out: RunnerOut,
    cancel: std::sync::Arc<tokio::sync::Notify>,
}

impl AssignWorker {
    /// Pick the right `FailureReason` when the agent subprocess crashes or
    /// exits abnormally. Codex stays on `CodexCrash` so dashboards that
    /// already filter on `"codex_crash"` keep working; Claude (and any
    /// future non-Codex agent) surfaces as the agent-neutral `AgentCrash`
    /// so the two aren't conflated in telemetry.
    fn crash_reason(&self) -> FailureReason {
        match self.runner_config.agent.kind {
            AgentKind::Codex => FailureReason::CodexCrash,
            AgentKind::ClaudeCode => FailureReason::AgentCrash,
        }
    }

    async fn run(
        &mut self,
        run_id: uuid::Uuid,
        prompt: String,
        repo_url: Option<String>,
        git_work_branch: Option<String>,
        expected_codex_model: Option<String>,
    ) -> Result<()> {
        self.handle_assign(
            run_id,
            prompt,
            repo_url,
            git_work_branch,
            expected_codex_model,
        )
        .await
    }

    async fn handle_assign(
        &mut self,
        run_id: uuid::Uuid,
        prompt: String,
        repo_url: Option<String>,
        git_work_branch: Option<String>,
        expected_codex_model: Option<String>,
    ) -> Result<()> {
        // Resolve workspace.
        let wd = self.runner_config.workspace.working_dir.clone();
        let resolution = crate::workspace::resolve(&wd, repo_url.as_deref()).await;
        let workspace_path = match resolution {
            Ok(r) => match r {
                crate::workspace::Resolution::ExistingRepo(p)
                | crate::workspace::Resolution::Cloned(p) => p,
            },
            Err(e) => {
                let reason = match &e {
                    crate::workspace::ResolveError::Clone(_) => FailureReason::GitAuth,
                    crate::workspace::ResolveError::MissingRepoUrl
                    | crate::workspace::ResolveError::NonEmptyNonRepo(_)
                    | crate::workspace::ResolveError::UnsupportedScheme(_) => {
                        FailureReason::WorkspaceSetup
                    }
                    crate::workspace::ResolveError::Io(_) => FailureReason::WorkspaceSetup,
                };
                self.send(ClientMsg::RunFailed {
                    run_id,
                    reason,
                    detail: Some(e.to_string()),
                    ended_at: Utc::now(),
                })
                .await;
                // The supervisor stamped ``rx_in_flight = Some(run_id)``
                // synchronously when the Assign arrived; clear it now so
                // a session reconnect after the failure doesn't claim a
                // run that no longer exists.
                self.state.set_current_run(None).await;
                return Ok(());
            }
        };
        if let Some(project_ref) = self.runner_config.project_slug.as_deref()
            && let Err(err) = crate::cli::context::write_context_for_project(
                &self.daemon_paths,
                &workspace_path,
                project_ref,
            )
            .await
        {
            tracing::debug!(
                error = %err,
                "skipping .pidash/context.md refresh after workspace resolution",
            );
        }

        // Pre-flight checkout: if the issue pins an existing branch, land on
        // it before the agent runs so it commits onto that branch directly.
        // When not set, the agent handles branch creation per the prompt.
        if let Some(branch) = git_work_branch.as_deref().filter(|s| !s.is_empty())
            && let Err(e) =
                crate::workspace::git::checkout_work_branch(&workspace_path, branch).await
        {
            self.send(ClientMsg::RunFailed {
                run_id,
                reason: FailureReason::WorkspaceSetup,
                detail: Some(format!("checkout {branch}: {e:#}")),
                ended_at: Utc::now(),
            })
            .await;
            // Same reason as above: clear the early-stamp on failure.
            self.state.set_current_run(None).await;
            return Ok(());
        }

        let ws_state = crate::workspace::git::workspace_state(&workspace_path)
            .await
            .unwrap_or(WorkspaceState {
                branch: None,
                head: None,
                dirty: false,
            });
        self.send(ClientMsg::Accept {
            run_id,
            workspace_state: ws_state,
        })
        .await;

        // History writer.
        let mut hist = HistoryWriter::open(&self.runner_paths, run_id).await?;
        hist.append(&HistoryEntry::Header {
            run_id,
            work_item_id: None,
            prompt_preview: prompt.chars().take(160).collect(),
            started_at: Utc::now(),
            repo_url,
        })
        .await?;

        // Update state with current run.
        self.state
            .set_current_run(Some(CurrentRunSummary {
                run_id,
                thread_id: None,
                status: "starting".to_string(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;

        // Bridge to the configured agent (Codex or Claude Code). `AgentBridge`
        // hides which CLI is actually being driven from the rest of this
        // worker; the event flow below is identical for both.
        let mut bridge = match AgentBridge::spawn_from_config(
            &self.runner_config,
            &workspace_path,
            expected_codex_model.clone(),
        )
        .await
        {
            Ok(b) => b,
            Err(e) => {
                hist.append(&HistoryEntry::Footer {
                    ts: Utc::now(),
                    final_status: "failed".to_string(),
                    done_payload: None,
                    error: Some(e.to_string()),
                })
                .await
                .ok();
                let reason = self.crash_reason();
                self.send(ClientMsg::RunFailed {
                    run_id,
                    reason,
                    detail: Some(format!("{e:#}")),
                    ended_at: Utc::now(),
                })
                .await;
                self.state.set_current_run(None).await;
                return Ok(());
            }
        };
        // Capture the bridge-owned process handle now that the subprocess
        // is spawned. Subscribe to its `exit_rx` so the live-state snapshot
        // flips `agent_subprocess_alive=false` the moment the wait task
        // observes termination — independent of stdout-close detection.
        // See `.ai_design/runner_agent_bridge/design.md` §4.4.
        let process_handle = bridge.process_handle();
        self.state.set_agent_pid(process_handle.pid).await;
        self.state.set_agent_alive(true).await;
        spawn_exit_watch(self.state.clone(), run_id, process_handle.exit_rx.clone());

        let payload = RunPayload {
            run_id,
            prompt,
            model: expected_codex_model,
        };
        let mut cursor = match bridge.run_one_shot(&payload, &workspace_path).await {
            Ok(c) => c,
            Err(e) => {
                hist.append(&HistoryEntry::Footer {
                    ts: Utc::now(),
                    final_status: "failed".to_string(),
                    done_payload: None,
                    error: Some(e.to_string()),
                })
                .await
                .ok();
                self.send(ClientMsg::RunFailed {
                    run_id,
                    reason: self.crash_reason(),
                    detail: Some(format!("{e:#}")),
                    ended_at: Utc::now(),
                })
                .await;
                self.state.set_current_run(None).await;
                return Ok(());
            }
        };
        self.send(ClientMsg::RunStarted {
            run_id,
            thread_id: cursor.thread_id().to_string(),
            started_at: Utc::now(),
        })
        .await;
        hist.append(&HistoryEntry::Lifecycle {
            ts: Utc::now(),
            state: "started".to_string(),
            detail: Some(cursor.thread_id().to_string()),
        })
        .await?;

        // Pump events until terminal.
        let outcome = self
            .pump_events(&mut bridge, &mut cursor, &mut hist, &workspace_path)
            .await?;

        bridge.shutdown(Duration::from_secs(5)).await.ok();

        let summary = RunSummary {
            run_id,
            work_item_id: None,
            status: outcome.status_label.clone(),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            title: None,
        };
        let mut idx = RunsIndex::load(&self.runner_paths).unwrap_or_default();
        idx.upsert(summary);
        idx.save(&self.runner_paths).ok();

        self.state.set_current_run(None).await;
        Ok(())
    }

    async fn pump_events(
        &mut self,
        bridge: &mut AgentBridge,
        cursor: &mut AgentCursor,
        hist: &mut HistoryWriter,
        workspace_root: &std::path::Path,
    ) -> Result<Outcome> {
        // Stall watchdog: if the agent emits no frames for this long AND
        // no approval is in flight (a human-wait is legitimate silence),
        // give up and fail the run. Without this, an unrecognised wait —
        // e.g. a future codex protocol change — leaves the runner blocked
        // on stdout and the cloud showing "running" indefinitely.
        //
        // Threshold is per-agent because Codex emits continuous token-
        // count events while Claude can be silent for the full
        // duration of a single tool call (see `AgentKind::stall_timeout`).
        let stall_timeout = self.runner_config.agent.kind.stall_timeout();

        let shutdown = self.state.shutdown_notified();
        let cancel = self.cancel.clone();
        let mut cancelled = false;
        loop {
            // Re-evaluate at every loop entry: an approval that opened on
            // the previous iteration disarms the watchdog; one that just
            // resolved re-arms it.
            let stall_deadline = if self.approvals.list_pending().await.is_empty() {
                Some(tokio::time::Instant::now() + stall_timeout)
            } else {
                None
            };
            tokio::select! {
                biased;
                _ = shutdown.notified(), if !cancelled => {
                    cancelled = true;
                    bridge.interrupt().await.ok();
                }
                _ = cancel.notified(), if !cancelled => {
                    bridge.interrupt().await.ok();
                    let _ = self.out.send(ClientMsg::RunCancelled {
                        run_id: cursor.run_id(),
                        cancelled_at: Utc::now(),
                    }).await;
                    hist.append(&HistoryEntry::Lifecycle {
                        ts: Utc::now(),
                        state: "cancelled".into(),
                        detail: None,
                    }).await.ok();
                    // Give the agent a short grace to wind down; if it doesn't, we
                    // exit and rely on the bridge's shutdown to SIGKILL.
                    let _ = tokio::time::timeout(
                        Duration::from_secs(10),
                        bridge.next_events(cursor),
                    ).await;
                    return Ok(Outcome { status_label: "cancelled".into() });
                }
                events = bridge.next_events(cursor) => {
                    let Some(events) = events else {
                        let reason = self.crash_reason();
                        let detail = self
                            .build_failure_detail("agent stdout closed", bridge)
                            .await;
                        self.send(ClientMsg::RunFailed {
                            run_id: cursor.run_id(),
                            reason,
                            detail: Some(detail.clone()),
                            ended_at: Utc::now(),
                        }).await;
                        hist.append(&HistoryEntry::Footer {
                            ts: Utc::now(),
                            final_status: "failed".into(),
                            done_payload: None,
                            error: Some(detail),
                        }).await?;
                        return Ok(Outcome { status_label: "failed".into() });
                    };
                    for ev in events {
                        if let Some(out) = self
                            .handle_bridge_event(ev, bridge, hist, workspace_root)
                            .await?
                        {
                            return Ok(out);
                        }
                    }
                }
                _ = async {
                    match stall_deadline {
                        Some(d) => tokio::time::sleep_until(d).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    let mins = stall_timeout.as_secs() / 60;
                    let base = format!("no agent frames for {mins} minutes");
                    let detail = self.build_failure_detail(&base, bridge).await;
                    self.send(ClientMsg::RunFailed {
                        run_id: cursor.run_id(),
                        reason: FailureReason::Timeout,
                        detail: Some(detail.clone()),
                        ended_at: Utc::now(),
                    }).await;
                    hist.append(&HistoryEntry::Footer {
                        ts: Utc::now(),
                        final_status: "failed".into(),
                        done_payload: None,
                        error: Some(detail),
                    }).await?;
                    return Ok(Outcome { status_label: "failed".into() });
                }
            }
        }
    }

    /// Build a `RunFailed.detail` string that includes whatever local
    /// context might help the cloud / UI explain what went wrong:
    /// - the supervisor's own classifier message (e.g. `"no agent frames
    ///   for 5 minutes"`),
    /// - the most recent shell command the agent kicked off,
    /// - the last few lines of agent stderr.
    ///
    /// All inputs are optional and the assembly degrades gracefully when
    /// they're missing — for a healthy code-edit task the result is just
    /// the base string. The total payload is bounded so a runaway stderr
    /// dump can't balloon a `RunFailed` body.
    async fn build_failure_detail(&self, base: &str, bridge: &AgentBridge) -> String {
        const DETAIL_BYTES_CAP: usize = 4096;
        const STDERR_TAIL_LINES: usize = 10;

        let last_cmd = self.state.observability_snapshot().await.last_exec_command;
        let stderr = bridge.recent_stderr().await;

        let mut parts: Vec<String> = vec![base.to_string()];
        if let Some(cmd) = last_cmd {
            let elapsed = (Utc::now() - cmd.started_at).num_seconds().max(0);
            let cwd = cmd
                .cwd
                .as_deref()
                .map(|c| format!(" in `{c}`"))
                .unwrap_or_default();
            parts.push(format!(
                "last command: `{}`{cwd} (started {elapsed}s ago)",
                cmd.command
            ));
        }
        // The stderr ring has already filtered codex tracing noise;
        // `stderr.dropped` is the running count of lines we rejected so
        // the user has an honest signal of how much was suppressed.
        // Emit the section if either we have content to show OR a
        // non-zero dropped count (the latter alone tells the operator
        // "the agent was emitting noise but no signal").
        if !stderr.lines.is_empty() || stderr.dropped > 0 {
            let shown = stderr.lines.len().min(STDERR_TAIL_LINES);
            let tail = stderr
                .lines
                .iter()
                .rev()
                .take(STDERR_TAIL_LINES)
                .rev()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n  ");
            let suffix = if stderr.dropped > 0 {
                format!(
                    " (plus {} noise line(s) filtered from codex tracing)",
                    stderr.dropped
                )
            } else {
                String::new()
            };
            if shown > 0 {
                parts.push(format!("stderr tail ({shown} line(s){suffix}):\n  {tail}"));
            } else {
                // Filter dropped everything — surface that fact alone
                // rather than emitting an empty stderr block.
                parts.push(format!("stderr: empty after filtering{suffix}"));
            }
        }
        let mut joined = parts.join("; ");
        if joined.len() > DETAIL_BYTES_CAP {
            // Truncate on a char boundary, leaving a sentinel so consumers
            // can tell the body was clipped.
            let mut end = DETAIL_BYTES_CAP;
            while !joined.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            joined.truncate(end);
            joined.push('…');
        }
        joined
    }

    async fn handle_bridge_event(
        &mut self,
        ev: BridgeEvent,
        bridge: &mut AgentBridge,
        hist: &mut HistoryWriter,
        workspace_root: &std::path::Path,
    ) -> Result<Option<Outcome>> {
        self.state.incr_current_run_events().await;
        // Observability: every bridge event bumps last_event_at + stamps
        // a structure-only kind/summary. The summary is sanitised inside
        // `summary_of` — never includes prompt or model output.
        // Opt-in: extract codex token / turn metrics from Raw frames so
        // operators see them on the runner-status panel.
        let kind = crate::daemon::observability::kind_of(&ev);
        let summary = crate::daemon::observability::summary_of(&ev);
        self.state
            .note_agent_event(Utc::now(), kind, Some(summary))
            .await;
        if let BridgeEvent::Raw { method, params, .. } = &ev {
            match method.as_str() {
                "codex/event/token_count" => {
                    if let Some(usage) =
                        crate::daemon::observability::parse_codex_token_count(params)
                    {
                        self.state.set_tokens(usage).await;
                    }
                }
                "turn/started" => {
                    self.state.incr_turn().await;
                }
                "item/started" | "assistant/message" => {
                    // Failure-detail enrichment: capture the most recent
                    // shell command the agent kicked off. Routing for
                    // codex (`item/started` → commandExecution) and
                    // Claude (`assistant/message` → Bash tool_use) lives
                    // in `extract_exec_command_hint` so it's
                    // unit-testable without a live bridge — a typo in
                    // either method name is caught by tests rather than
                    // by silently absent `last command:` fields in
                    // production failure details.
                    if let Some(hint) = crate::daemon::observability::extract_exec_command_hint(
                        method.as_str(),
                        params,
                    ) {
                        self.state
                            .note_exec_command(crate::daemon::state::ExecCommandSnapshot {
                                command: hint.command,
                                cwd: hint.cwd,
                                started_at: Utc::now(),
                            })
                            .await;
                    }
                }
                _ => {}
            }
        }
        match ev {
            BridgeEvent::RunStarted { .. } => Ok(None),
            BridgeEvent::Raw {
                method,
                params,
                run_id,
            } => {
                hist.append(&HistoryEntry::CodexEvent {
                    ts: Utc::now(),
                    method: method.clone(),
                    params: params.clone(),
                })
                .await
                .ok();
                // Only lifecycle-ish events are mirrored to cloud; raw deltas stay local.
                tracing::trace!(%run_id, method, "agent event");
                Ok(None)
            }
            BridgeEvent::ApprovalRequest {
                run_id,
                approval_id,
                kind,
                payload,
                reason,
            } => {
                let policy = Policy::new(&self.runner_config.approval_policy, workspace_root);
                let decision = policy.evaluate(kind, &payload);
                if let Some(auto) = decision.into_cloud() {
                    bridge.send_approval(&approval_id, auto).await.ok();
                    hist.append(&HistoryEntry::Approval {
                        ts: Utc::now(),
                        approval_id: approval_id.clone(),
                        status: format!("auto:{auto:?}"),
                        payload: payload.clone(),
                    })
                    .await
                    .ok();
                    return Ok(None);
                }
                let rec = ApprovalRecord {
                    approval_id: approval_id.clone(),
                    runner_id: self.runner_config.runner_id,
                    run_id,
                    kind,
                    payload: payload.clone(),
                    reason,
                    requested_at: Utc::now(),
                    expires_at: Some(Utc::now() + chrono::Duration::minutes(10)),
                    status: crate::approval::router::ApprovalStatus::Pending,
                };
                // Subscribe BEFORE opening so a Decide that races in between
                // open() and subscribe() doesn't strand the worker waiting on
                // an event it'll never see.
                let mut rx = self.approvals.subscribe();
                self.approvals.open(rec.clone()).await;
                self.state
                    .set_approvals_pending(self.approvals.list_pending().await.len())
                    .await;
                self.send(ClientMsg::ApprovalRequest {
                    run_id,
                    approval_id: uuid_or(&approval_id),
                    kind,
                    payload: payload.clone(),
                    reason: rec.reason.clone(),
                    expires_at: rec.expires_at,
                })
                .await;
                hist.append(&HistoryEntry::Approval {
                    ts: Utc::now(),
                    approval_id: approval_id.clone(),
                    status: "pending".into(),
                    payload,
                })
                .await
                .ok();

                // Wait for a decision (local or cloud). Subscribed before
                // open() above; ApprovalRouter::open also resolves inline if
                // a Decide already arrived for this id.

                loop {
                    match rx.recv().await {
                        Ok(ApprovalRecord {
                            approval_id: aid,
                            status:
                                ApprovalStatus::Resolved {
                                    decision, source, ..
                                },
                            ..
                        }) if aid == approval_id => {
                            bridge.send_approval(&approval_id, decision).await.ok();
                            hist.append(&HistoryEntry::Approval {
                                ts: Utc::now(),
                                approval_id: approval_id.clone(),
                                status: format!("resolved:{source:?}:{decision:?}"),
                                payload: serde_json::Value::Null,
                            })
                            .await
                            .ok();
                            let remaining = self.approvals.list_pending().await.len();
                            self.state.set_approvals_pending(remaining).await;
                            return Ok(None);
                        }
                        Ok(_) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
                Ok(None)
            }
            BridgeEvent::AwaitingReauth { run_id, detail } => {
                self.state.set_status(RunnerStatus::AwaitingReauth).await;
                self.send(ClientMsg::RunAwaitingReauth {
                    run_id,
                    detail: detail.clone(),
                })
                .await;
                hist.append(&HistoryEntry::Lifecycle {
                    ts: Utc::now(),
                    state: "awaiting_reauth".into(),
                    detail,
                })
                .await
                .ok();
                Ok(None)
            }
            BridgeEvent::Completed {
                run_id,
                done_payload,
            } => {
                self.send(ClientMsg::RunCompleted {
                    run_id,
                    done_payload: done_payload.clone(),
                    ended_at: Utc::now(),
                })
                .await;
                hist.append(&HistoryEntry::Footer {
                    ts: Utc::now(),
                    final_status: "completed".into(),
                    done_payload: Some(done_payload),
                    error: None,
                })
                .await
                .ok();
                Ok(Some(Outcome {
                    status_label: "completed".into(),
                }))
            }
            BridgeEvent::Failed {
                run_id,
                reason,
                detail,
            } => {
                // Codex / claude-detected failures (e.g.
                // "turn/completed without conclusion", `error` notifications
                // with `willRetry=false`) reach us with whatever bare
                // string the bridge produced. Run them through the same
                // enrichment helper the watchdog and stdout-close paths
                // use so the user sees `last command:` and a stderr
                // tail in the issue activity comment, not just the
                // bridge's classifier text.
                let base = detail
                    .clone()
                    .unwrap_or_else(|| "agent reported failure".to_string());
                let enriched = self.build_failure_detail(&base, bridge).await;
                self.send(ClientMsg::RunFailed {
                    run_id,
                    reason,
                    detail: Some(enriched.clone()),
                    ended_at: Utc::now(),
                })
                .await;
                hist.append(&HistoryEntry::Footer {
                    ts: Utc::now(),
                    final_status: "failed".into(),
                    done_payload: None,
                    error: Some(enriched),
                })
                .await
                .ok();
                Ok(Some(Outcome {
                    status_label: "failed".into(),
                }))
            }
        }
    }

    async fn send(&self, msg: ClientMsg) {
        let _ = self.out.send(msg).await;
    }
}

struct Outcome {
    status_label: String,
}

/// Best-effort: if codex hands us a non-UUID approval id (it shouldn't, per
/// schema), derive a stable v5 UUID from it so the cloud sees the same id on
/// retries instead of a fresh random one each time.
fn uuid_or(s: &str) -> uuid::Uuid {
    if let Ok(u) = uuid::Uuid::parse_str(s) {
        return u;
    }
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, s.as_bytes())
}

#[cfg(test)]
mod tests {
    //! In-module tests: cover the Supervisor's *internal* helpers that
    //! integration tests in ``runner/tests/`` cannot reach (private
    //! functions). The wire-format / connection-loop side is covered by
    //! ``runner/tests/cloud_ws_fake.rs``.

    use super::*;
    use crate::cloud::protocol::Envelope;
    use crate::config::schema::{
        AgentSection, ApprovalPolicySection, ClaudeCodeSection, CodexSection, RunnerConfig,
        WorkspaceSection,
    };
    use std::path::PathBuf;
    use tokio::sync::Notify;

    fn paths_for(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
        }
    }

    fn runner_config(name: &str, project_slug: &str, working_dir: PathBuf) -> RunnerConfig {
        RunnerConfig {
            name: name.into(),
            runner_id: uuid::Uuid::new_v4(),
            workspace_slug: Some("acme".into()),
            project_slug: Some(project_slug.into()),
            pod_id: None,
            workspace: WorkspaceSection { working_dir },
            agent: AgentSection::default(),
            codex: CodexSection::default(),
            claude_code: ClaudeCodeSection::default(),
            approval_policy: ApprovalPolicySection::default(),
        }
    }

    /// Build the supervisor's ``hello_runners`` map from a list of
    /// ``RunnerInstance`` exactly the way ``Supervisor::run`` does, then
    /// drive ``hello_emitter`` once and assert each instance's Hello
    /// frame carries the correct per-runner ``project_slug``.
    ///
    /// This is the integration point the original review asked for:
    /// production code calls ``hello_emitter`` to construct the frames
    /// the cloud cross-checks against ``runner.pod.project``. If the
    /// supervisor were to misroute ``project_slug`` between instances,
    /// this test catches it. The wire-format-only test in
    /// ``cloud_ws_fake.rs`` cannot.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hello_emitter_emits_one_hello_per_instance_with_project_slug() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);

        // Two instances, distinct project_slugs and disjoint working_dirs
        // (`Config::validate()` rejects overlap; we mirror that here).
        let inst_web = RunnerInstance::new(
            runner_config("laptop-web", "WEB", tmp.path().join("wd-web")),
            &paths,
            out_tx.clone(),
        );
        let inst_api = RunnerInstance::new(
            runner_config("laptop-api", "API", tmp.path().join("wd-api")),
            &paths,
            out_tx.clone(),
        );
        let runner_web = inst_web.runner_id;
        let runner_api = inst_api.runner_id;

        let runners: Arc<RwLock<HelloRunnerMap>> = Arc::new(RwLock::new(
            [&inst_web, &inst_api]
                .into_iter()
                .map(|i| {
                    (
                        i.runner_id,
                        (
                            i.out.clone(),
                            i.state.clone(),
                            i.config.project_slug.clone(),
                        ),
                    )
                })
                .collect(),
        ));

        let connected = Arc::new(Notify::new());
        let connected_for_task = connected.clone();
        // Daemon-level state handle — the test only cares about Hello
        // emission, but the emitter signature now takes a StateHandle so
        // the IPC's connected flag can be flipped on each WS handshake.
        let daemon_state = StateHandle::new(crate::config::schema::Config {
            version: 2,
            daemon: Default::default(),
            runners: vec![],
            cli: None,
        });
        let task = tokio::spawn(async move {
            hello_emitter(runners, connected_for_task, daemon_state).await;
        });

        connected.notify_one();

        // Collect both Hellos. Supervisor iterates the map; HashMap order
        // is unspecified so we sort by runner_id before asserting.
        let mut received: Vec<(uuid::Uuid, Option<String>)> = Vec::new();
        for _ in 0..2 {
            let env = tokio::time::timeout(std::time::Duration::from_secs(2), out_rx.recv())
                .await
                .expect("timed out waiting for Hello")
                .expect("channel closed before Hello arrived");
            assert_eq!(env.runner_id, env.runner_id); // sanity
            match env.body {
                ClientMsg::Hello {
                    runner_id,
                    project_slug,
                    ..
                } => {
                    received.push((runner_id, project_slug));
                }
                other => panic!("expected Hello, got {other:?}"),
            }
        }

        task.abort();

        received.sort_by_key(|(id, _)| *id);
        let mut expected: Vec<(uuid::Uuid, Option<String>)> = vec![
            (runner_web, Some("WEB".into())),
            (runner_api, Some("API".into())),
        ];
        expected.sort_by_key(|(id, _)| *id);
        assert_eq!(received, expected);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn drain_in_flight_runs_sends_run_failed_for_busy_runners_only() {
        // Two runners; only one has an in-flight run. Drain should
        // emit RunFailed{DaemonRestart} for that one and skip the idle one.
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);

        let inst_busy = RunnerInstance::new(
            runner_config("busy-runner", "WEB", tmp.path().join("wd-busy")),
            &paths,
            out_tx.clone(),
        );
        let inst_idle = RunnerInstance::new(
            runner_config("idle-runner", "API", tmp.path().join("wd-idle")),
            &paths,
            out_tx.clone(),
        );
        let busy_runner_id = inst_busy.runner_id;
        let active_run_id = uuid::Uuid::new_v4();

        // Stamp Some(rid) on the busy instance, leave idle as None.
        inst_busy
            .state
            .set_current_run(Some(CurrentRunSummary {
                run_id: active_run_id,
                thread_id: None,
                status: "running".into(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;

        let runners: Arc<RwLock<HelloRunnerMap>> = Arc::new(RwLock::new(
            [&inst_busy, &inst_idle]
                .into_iter()
                .map(|i| {
                    (
                        i.runner_id,
                        (
                            i.out.clone(),
                            i.state.clone(),
                            i.config.project_slug.clone(),
                        ),
                    )
                })
                .collect(),
        ));

        let sent = drain_in_flight_runs(runners).await;
        assert_eq!(sent, 1, "exactly one runner had an in-flight run");

        // Receive the one RunFailed envelope.
        let env = tokio::time::timeout(std::time::Duration::from_secs(2), out_rx.recv())
            .await
            .expect("timed out waiting for RunFailed")
            .expect("channel closed before RunFailed arrived");
        assert_eq!(env.runner_id, Some(busy_runner_id));
        match env.body {
            ClientMsg::RunFailed {
                run_id,
                reason,
                detail,
                ..
            } => {
                assert_eq!(run_id, active_run_id);
                assert!(matches!(reason, FailureReason::DaemonRestart));
                assert_eq!(detail.as_deref(), Some("daemon shutdown requested"));
            }
            other => panic!("expected RunFailed, got {other:?}"),
        }

        // No further messages — the idle runner must not have produced one.
        let stray =
            tokio::time::timeout(std::time::Duration::from_millis(100), out_rx.recv()).await;
        assert!(
            stray.is_err(),
            "idle runner unexpectedly produced a frame: {stray:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn drain_in_flight_runs_no_op_when_all_idle() {
        // Common path: nothing in flight, drain should send nothing.
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let inst = RunnerInstance::new(
            runner_config("idle", "WEB", tmp.path().join("wd")),
            &paths,
            out_tx,
        );
        let runners: Arc<RwLock<HelloRunnerMap>> = Arc::new(RwLock::new(
            std::iter::once(&inst)
                .map(|i| {
                    (
                        i.runner_id,
                        (
                            i.out.clone(),
                            i.state.clone(),
                            i.config.project_slug.clone(),
                        ),
                    )
                })
                .collect(),
        ));

        let sent = drain_in_flight_runs(runners).await;
        assert_eq!(sent, 0);
        let stray =
            tokio::time::timeout(std::time::Duration::from_millis(100), out_rx.recv()).await;
        assert!(
            stray.is_err(),
            "idle drain produced a stray frame: {stray:?}"
        );
    }
}
