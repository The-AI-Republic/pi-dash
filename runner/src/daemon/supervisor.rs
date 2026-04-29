use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc, oneshot};

use crate::agent::{AgentBridge, AgentCursor, BridgeEvent, RunPayload};
use crate::approval::policy::Policy;
use crate::approval::router::{ApprovalRecord, ApprovalRouter, ApprovalStatus, DecisionSource};
use crate::cloud::protocol::{
    ClientMsg, Envelope, FailureReason, RunnerStatus, ServerMsg, WIRE_VERSION, WorkspaceState,
};
use crate::cloud::ws::ConnectionLoop;
use crate::config::schema::{AgentKind, Config, Credentials};
use crate::daemon::runner_instance::RunnerInstance;
use crate::daemon::runner_out::RunnerOut;
use crate::daemon::state::StateHandle;
use crate::history::index::{RunSummary, RunsIndex};
use crate::history::jsonl::{HistoryEntry, HistoryWriter};
use crate::ipc::protocol::CurrentRunSummary;
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
            creds,
            paths,
            opts,
            state,
            approvals: _supervisor_approvals,
        } = self;
        state.set_runner_id(creds.runner_id).await;

        let (out_tx, out_rx) = mpsc::channel::<Envelope<ClientMsg>>(128);
        let (in_tx, in_rx) = mpsc::channel::<Envelope<ServerMsg>>(128);

        // Build one RunnerInstance per [[runner]] entry in config.toml.
        // Each owns its own paths, state, approvals, and mailbox.
        let mut instances: Vec<RunnerInstance> = Vec::new();
        for runner_cfg in &config.runners {
            let inst = RunnerInstance::new(runner_cfg.clone(), &paths, out_tx.clone());
            inst.paths.ensure()?;
            instances.push(inst);
        }
        // Map for the demux task to look up by runner_id.
        let mailboxes = Arc::new(RwLock::new(
            instances
                .iter()
                .map(|i| (i.runner_id, i.mailbox_tx.clone()))
                .collect::<HashMap<uuid::Uuid, mpsc::Sender<Envelope<ServerMsg>>>>(),
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
                .collect::<HashMap<uuid::Uuid, (RunnerOut, StateHandle, Option<String>)>>(),
        ));

        // Primary runner = the first configured runner. Used by IPC and
        // (legacy) heartbeat-status fields that haven't been split per-
        // instance yet. With one runner this is the only runner; with
        // many it's the daemon-level "default" view.
        let primary = instances
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("config.runners is empty; daemon refuses to start"))?;

        let ipc = IpcServer {
            path: paths.ipc_socket_path(),
            state: primary.state.clone(),
            approvals: primary.approvals.clone(),
            paths: paths.clone(),
            runner_paths: primary.paths.clone(),
        };
        let ipc_handle = tokio::spawn(async move {
            if let Err(e) = ipc.run().await {
                tracing::error!("ipc server exited: {e:#}");
            }
        });

        // Cloud loop + heartbeat are skipped in offline mode. Without a cloud
        // consumer, the heartbeat task would otherwise block forever once the
        // outbound channel filled (~53 min at 25s cadence).
        let (cloud_handle, hello_handle, hb_handles) = if !opts.offline {
            // `connected` fires every time the WS handshake completes —
            // first connect AND every reconnect. The hello-emitter task
            // waits on it and re-sends one Hello per instance, which is
            // what tells the cloud to (re-)populate `authorised_runners`
            // on the new consumer. Without this signal a reconnect
            // produces a live socket the cloud silently ignores.
            let connected = std::sync::Arc::new(tokio::sync::Notify::new());

            // Spawn the hello emitter BEFORE ConnectionLoop so we don't
            // race the first `notify_one` against a not-yet-scheduled
            // waiter. (`notify_one` latches one permit anyway, but
            // ordering keeps the code obvious.)
            let connected_for_hello = connected.clone();
            let hello_runners_for_task = hello_runners.clone();
            let hello_handle = tokio::spawn(async move {
                hello_emitter(hello_runners_for_task, connected_for_hello).await;
            });

            let shutdown_for_loop = state.shutdown_notified();
            let loop_ = ConnectionLoop {
                cloud_url: config.daemon.cloud_url.clone(),
                creds: creds.clone(),
                outbound: out_rx,
                inbound: in_tx.clone(),
                status_snapshot: state.rx_status.clone(),
                in_flight: state.rx_in_flight.clone(),
                shutdown: shutdown_for_loop,
                connected,
            };
            let cloud = tokio::spawn(async move {
                if let Err(e) = loop_.run().await {
                    tracing::error!("cloud loop exited: {e:#}");
                }
            });

            // Heartbeat task per instance — design.md §6.6 says per-runner
            // heartbeats carry rid so the cloud's per-runner record can
            // update last_seen_at independently for each. Each task also
            // watches `remove_signal` so a `ServerMsg::RemoveRunner` can
            // tear it down without dragging the daemon's other heartbeats
            // along — preventing zombie heartbeats with rid for a runner
            // the cloud-side row has already deleted.
            let mut hb_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
            for inst in &instances {
                let hb_out = inst.out.clone();
                let state_hb = inst.state.clone();
                let mut remove_rx = inst.remove_tx.subscribe();
                let runner_id = inst.runner_id;
                let h = tokio::spawn(async move {
                    let mut rx_interval = state_hb.rx_heartbeat_secs.clone();
                    let mut current_secs = (*rx_interval.borrow()).max(1);
                    let mut ticker = tokio::time::interval(Duration::from_secs(current_secs));
                    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tokio::select! {
                            changed = remove_rx.changed() => {
                                if changed.is_err() || *remove_rx.borrow() {
                                    tracing::info!(
                                        %runner_id,
                                        "heartbeat task exiting after RemoveRunner"
                                    );
                                    return;
                                }
                            }
                            _ = async {}, if *remove_rx.borrow() => {
                                tracing::info!(
                                    %runner_id,
                                    "heartbeat task exiting after RemoveRunner"
                                );
                                return;
                            }
                            _ = ticker.tick() => {
                                let now = Utc::now();
                                state_hb.set_heartbeat(now).await;
                                let status = { *state_hb.rx_status.borrow() };
                                let in_flight = { *state_hb.rx_in_flight.borrow() };
                                if hb_out
                                    .send(ClientMsg::Heartbeat {
                                        ts: now,
                                        status,
                                        in_flight_run: in_flight,
                                    })
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            changed = rx_interval.changed() => {
                                if changed.is_err() { break; }
                                let next = (*rx_interval.borrow()).max(1);
                                if next != current_secs {
                                    current_secs = next;
                                    ticker = tokio::time::interval(Duration::from_secs(current_secs));
                                    ticker.set_missed_tick_behavior(
                                        tokio::time::MissedTickBehavior::Delay,
                                    );
                                }
                            }
                        }
                    }
                });
                hb_handles.push(h);
            }
            (Some(cloud), Some(hello_handle), hb_handles)
        } else {
            tracing::info!("offline mode: cloud loop + heartbeat disabled");
            drop(out_rx);
            (None, None, Vec::new())
        };

        // Demux task — reads from in_rx (the connection's inbound) and
        // routes each frame by Envelope.runner_id to the matching
        // instance mailbox. Frames with rid = None are connection-scoped
        // (Ping reply, supervisor-driven concerns); we forward them to
        // every instance so they all see the connection event. With one
        // instance that's the same as today; with N it lets each runner
        // react to connection-level signals independently.
        let demux_state = state.clone();
        let demux_out = out_tx.clone();
        let mailboxes_for_demux = mailboxes.clone();
        let demux = tokio::spawn(async move {
            let mut in_rx = in_rx;
            while let Some(env) = in_rx.recv().await {
                let rid = env.runner_id;
                match rid {
                    Some(id) => {
                        let tx = { mailboxes_for_demux.read().await.get(&id).cloned() };
                        if let Some(tx) = tx {
                            let _ = tx.send(env).await;
                        } else {
                            tracing::warn!(
                                %id,
                                "frame for unknown runner; dropping"
                            );
                        }
                    }
                    None => {
                        // Connection-scoped frame. Today only Ping +
                        // connection-wide Revoke land here. Handle
                        // them inline rather than routing to instances.
                        match env.body {
                            ServerMsg::Ping { ts } => {
                                let status = *demux_state.rx_status.borrow();
                                let in_flight = *demux_state.rx_in_flight.borrow();
                                let _ = demux_out
                                    .send(Envelope::new(ClientMsg::Heartbeat {
                                        ts,
                                        status,
                                        in_flight_run: in_flight,
                                    }))
                                    .await;
                            }
                            ServerMsg::Revoke { reason } => {
                                tracing::error!("cloud revoked token: {reason}");
                                demux_state.shutdown();
                                break;
                            }
                            _ => {
                                tracing::warn!("unrouted connection-scoped frame: {:?}", env.body);
                            }
                        }
                    }
                }
            }
        });

        // One RunnerLoop per instance. Each consumes from its mailbox.
        let mut loop_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
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
            let inst_remove_tx = inst.remove_tx.clone();
            let live_mailboxes = mailboxes.clone();
            let live_hello_runners = hello_runners.clone();
            let h = tokio::spawn(async move {
                let run = RunnerLoop {
                    runner_paths,
                    runner_config,
                    out: inst_out,
                    state: inst_state,
                    approvals: inst_approvals,
                    inbound: mailbox_rx,
                    remove_tx: inst_remove_tx,
                    live_mailboxes,
                    live_hello_runners,
                    current_run: None,
                };
                if let Err(e) = run.run().await {
                    tracing::error!("runner loop exited: {e:#}");
                }
            });
            loop_handles.push(h);
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

        out_tx
            .send(Envelope::new(ClientMsg::Bye {
                reason: "shutdown".to_string(),
            }))
            .await
            .ok();

        for h in hb_handles {
            h.abort();
        }
        for h in loop_handles {
            h.abort();
        }
        demux.abort();
        ipc_handle.abort();
        if let Some(h) = hello_handle {
            h.abort();
        }
        if let Some(h) = cloud_handle {
            h.abort();
        }
        Ok(())
    }
}

/// Watch the `connected` notify and re-emit one `Hello` per `RunnerInstance`
/// every time it fires. Driven by `ConnectionLoop`, which calls
/// `notify_one()` after each successful WS handshake (cold start and every
/// reconnect). Cloud-side `_handle_token_hello` is idempotent on re-Hello,
/// so a second emission for an already-authorised runner is harmless.
async fn hello_emitter(
    runners: Arc<RwLock<HashMap<uuid::Uuid, (RunnerOut, StateHandle, Option<String>)>>>,
    connected: Arc<tokio::sync::Notify>,
) {
    loop {
        connected.notified().await;
        let current_runners: Vec<(RunnerOut, StateHandle, Option<String>)> =
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

struct RunnerLoop {
    runner_paths: RunnerPaths,
    runner_config: crate::config::schema::RunnerConfig,
    out: RunnerOut,
    state: StateHandle,
    approvals: ApprovalRouter,
    inbound: mpsc::Receiver<Envelope<ServerMsg>>,
    /// Latched before the loop exits on `ServerMsg::RemoveRunner` so
    /// background tasks can stop even if they were not already blocked
    /// on the signal.
    remove_tx: tokio::sync::watch::Sender<bool>,
    live_mailboxes: Arc<RwLock<HashMap<uuid::Uuid, mpsc::Sender<Envelope<ServerMsg>>>>>,
    live_hello_runners:
        Arc<RwLock<HashMap<uuid::Uuid, (RunnerOut, StateHandle, Option<String>)>>>,
    /// In-flight run, if any. Replaced on each Assign and cleared as soon as
    /// the worker task signals completion via `done_rx` — driven by
    /// `tokio::select!` so a new Assign isn't rejected while we wait for the
    /// next inbound frame.
    current_run: Option<CurrentRun>,
}

struct CurrentRun {
    run_id: uuid::Uuid,
    cancel: std::sync::Arc<tokio::sync::Notify>,
    done_rx: oneshot::Receiver<()>,
}

impl RunnerLoop {
    async fn run(mut self) -> Result<()> {
        loop {
            let inbound = self.inbound.recv();
            tokio::pin!(inbound);
            // `done_rx` exists only while a run is in flight; outside of that
            // window we wait on `pending()` so the select arm is inert.
            let frame = tokio::select! {
                biased;
                () = wait_done(&mut self.current_run) => {
                    self.current_run = None;
                    continue;
                }
                f = &mut inbound => f,
            };
            let Some(frame) = frame else { break };

            match frame.body {
                ServerMsg::Welcome {
                    protocol_version,
                    heartbeat_interval_secs,
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
                    self.state.set_connected(true).await;
                }
                ServerMsg::Assign {
                    run_id,
                    prompt,
                    repo_url,
                    git_work_branch,
                    expected_codex_model,
                    resume_thread_id,
                    ..
                } => {
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
                    let runner_paths = self.runner_paths.clone();
                    let runner_config = self.runner_config.clone();
                    let state = self.state.clone();
                    let approvals = self.approvals.clone();
                    let out = self.out.clone();
                    tokio::spawn(async move {
                        let mut worker = AssignWorker {
                            runner_paths,
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
                                resume_thread_id,
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
                    // Connection-scoped (token revocation) — supervisor's
                    // demux handles it. Reaching this arm means the
                    // demux routed by mistake.
                    tracing::warn!("Revoke arrived at RunnerLoop; demux invariant violated");
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
                    // NOTE: the [[runner]] entry in config.toml is not
                    // cleaned up here — operators run
                    // `pidash token remove-runner --name <name>` to
                    // also strip config.toml so the next daemon
                    // restart doesn't re-Hello for this runner_id.
                    // Surface this loudly so the operator notices
                    // before the next restart turns into a silent
                    // re-Hello → RemoveRunner loop.
                    tracing::warn!(
                        runner = %self.runner_config.name,
                        runner_id = %runner_id,
                        "runner removed cloud-side; \
                         run `pidash token remove-runner --name {}` to \
                         strip the [[runner]] block from config.toml. \
                         Otherwise the daemon will re-Hello this id on \
                         next restart and the cloud will tear it down again.",
                        self.runner_config.name,
                    );
                    break;
                }
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

/// Owns one `Assign`'s lifecycle. Spawned as a task from `RunnerLoop`, so the
/// message loop stays live and can deliver Cancel / Decide frames to us via
/// `self.cancel` and `self.approvals`.
struct AssignWorker {
    runner_paths: RunnerPaths,
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
        resume_thread_id: Option<String>,
    ) -> Result<()> {
        self.handle_assign(
            run_id,
            prompt,
            repo_url,
            git_work_branch,
            expected_codex_model,
            resume_thread_id,
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
        resume_thread_id: Option<String>,
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
                return Ok(());
            }
        };

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
            resume_thread_id.as_deref(),
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
        let payload = RunPayload {
            run_id,
            prompt,
            model: expected_codex_model,
            resume_thread_id,
        };
        let mut cursor = match bridge.run(&payload, &workspace_path).await {
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
                // Distinguish "agent CLI couldn't find the session id" from a
                // generic agent crash. Cloud's reaction differs: drop the pin
                // and re-queue with no resume hint, vs. mark the run failed.
                let reason = if e
                    .downcast_ref::<crate::agent::ResumeUnavailable>()
                    .is_some()
                {
                    FailureReason::ResumeUnavailable
                } else {
                    self.crash_reason()
                };
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
        let shutdown = self.state.shutdown_notified();
        let cancel = self.cancel.clone();
        let mut cancelled = false;
        loop {
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
                        self.send(ClientMsg::RunFailed {
                            run_id: cursor.run_id(),
                            reason,
                            detail: Some("agent stdout closed".to_string()),
                            ended_at: Utc::now(),
                        }).await;
                        hist.append(&HistoryEntry::Footer {
                            ts: Utc::now(),
                            final_status: "failed".into(),
                            done_payload: None,
                            error: Some("agent stdout closed".into()),
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
            }
        }
    }

    async fn handle_bridge_event(
        &mut self,
        ev: BridgeEvent,
        bridge: &mut AgentBridge,
        hist: &mut HistoryWriter,
        workspace_root: &std::path::Path,
    ) -> Result<Option<Outcome>> {
        self.state.incr_current_run_events().await;
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
                self.send(ClientMsg::RunFailed {
                    run_id,
                    reason,
                    detail: detail.clone(),
                    ended_at: Utc::now(),
                })
                .await;
                hist.append(&HistoryEntry::Footer {
                    ts: Utc::now(),
                    final_status: "failed".into(),
                    done_payload: None,
                    error: detail,
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
