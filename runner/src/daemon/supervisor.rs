use anyhow::Result;
use chrono::Utc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::approval::policy::Policy;
use crate::approval::router::{ApprovalRecord, ApprovalRouter, ApprovalStatus, DecisionSource};
use crate::cloud::protocol::{
    ClientMsg, Envelope, FailureReason, RunnerStatus, ServerMsg, WIRE_VERSION, WorkspaceState,
};
use crate::cloud::ws::ConnectionLoop;
use crate::codex::bridge::{Bridge, BridgeCursor, BridgeEvent, RunPayload};
use crate::config::schema::{Config, Credentials};
use crate::daemon::state::StateHandle;
use crate::history::index::{RunSummary, RunsIndex};
use crate::history::jsonl::{HistoryEntry, HistoryWriter};
use crate::ipc::protocol::CurrentRunSummary;
use crate::ipc::server::IpcServer;
use crate::util::paths::Paths;

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
        let Supervisor {
            config,
            creds,
            paths,
            opts,
            state,
            approvals,
        } = self;
        state.set_runner_id(creds.runner_id).await;

        let ipc = IpcServer {
            path: paths.ipc_socket_path(),
            state: state.clone(),
            approvals: approvals.clone(),
            paths: paths.clone(),
        };
        let ipc_handle = tokio::spawn(async move {
            if let Err(e) = ipc.run().await {
                tracing::error!("ipc server exited: {e:#}");
            }
        });

        let (out_tx, out_rx) = mpsc::channel::<Envelope<ClientMsg>>(128);
        let (in_tx, in_rx) = mpsc::channel::<Envelope<ServerMsg>>(128);

        // Cloud loop (optional — offline mode skips it).
        let cloud_handle = if !opts.offline {
            let loop_ = ConnectionLoop {
                cloud_url: config.runner.cloud_url.clone(),
                creds: creds.clone(),
                outbound: out_rx,
                inbound: in_tx.clone(),
                status_snapshot: state.rx_status.clone(),
                in_flight: state.rx_in_flight.clone(),
            };
            Some(tokio::spawn(async move {
                if let Err(e) = loop_.run().await {
                    tracing::error!("cloud loop exited: {e:#}");
                }
            }))
        } else {
            tracing::info!("offline mode: cloud loop disabled");
            None
        };

        // Heartbeat.
        let out_tx_hb = out_tx.clone();
        let state_hb = state.clone();
        let hb_handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(25));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let now = Utc::now();
                state_hb.set_heartbeat(now).await;
                let status = { *state_hb.rx_status.borrow() };
                let in_flight = { *state_hb.rx_in_flight.borrow() };
                let frame = Envelope::new(ClientMsg::Heartbeat {
                    ts: now,
                    status,
                    in_flight_run: in_flight,
                });
                if out_tx_hb.send(frame).await.is_err() {
                    break;
                }
            }
        });

        // Runner loop — dispatches on inbound ServerMsg.
        let run = RunnerLoop {
            paths: paths.clone(),
            config: config.clone(),
            out: out_tx.clone(),
            state: state.clone(),
            approvals: approvals.clone(),
            inbound: in_rx,
            current_run_cancel: None,
        };

        let shutdown = state.shutdown_notified();
        let sig = crate::util::signal::shutdown();
        tokio::select! {
            _ = run.run() => {
                tracing::info!("runner loop ended");
            }
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

        hb_handle.abort();
        ipc_handle.abort();
        if let Some(h) = cloud_handle {
            h.abort();
        }
        Ok(())
    }
}

struct RunnerLoop {
    paths: Paths,
    config: Config,
    out: mpsc::Sender<Envelope<ClientMsg>>,
    state: StateHandle,
    approvals: ApprovalRouter,
    inbound: mpsc::Receiver<Envelope<ServerMsg>>,
    /// Cancel signal for the in-flight run, if any. Replaced on each Assign.
    current_run_cancel: Option<(uuid::Uuid, std::sync::Arc<tokio::sync::Notify>)>,
}

impl RunnerLoop {
    async fn run(mut self) -> Result<()> {
        while let Some(frame) = self.inbound.recv().await {
            match frame.body {
                ServerMsg::Welcome {
                    protocol_version, ..
                } => {
                    if protocol_version != WIRE_VERSION {
                        tracing::warn!(
                            server = protocol_version,
                            local = WIRE_VERSION,
                            "protocol version mismatch",
                        );
                    }
                    self.state.set_connected(true).await;
                }
                ServerMsg::Assign {
                    run_id,
                    prompt,
                    repo_url,
                    expected_codex_model,
                    ..
                } => {
                    if self.current_run_cancel.is_some() {
                        tracing::warn!(
                            %run_id,
                            "assign received while a run is already in flight; ignoring"
                        );
                        continue;
                    }
                    let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
                    self.current_run_cancel = Some((run_id, cancel.clone()));
                    let paths = self.paths.clone();
                    let config = self.config.clone();
                    let state = self.state.clone();
                    let approvals = self.approvals.clone();
                    let out = self.out.clone();
                    tokio::spawn(async move {
                        let mut worker = AssignWorker {
                            paths,
                            config,
                            state,
                            approvals,
                            out,
                            cancel,
                        };
                        if let Err(e) = worker
                            .run(run_id, prompt, repo_url, expected_codex_model)
                            .await
                        {
                            tracing::error!("run {run_id} failed: {e:#}");
                            let _ = worker
                                .out
                                .send(Envelope::new(ClientMsg::RunFailed {
                                    run_id,
                                    reason: FailureReason::Internal,
                                    detail: Some(format!("{e:#}")),
                                    ended_at: Utc::now(),
                                }))
                                .await;
                            worker.state.set_current_run(None).await;
                        }
                    });
                }
                ServerMsg::Cancel { run_id, reason } => {
                    tracing::info!(%run_id, ?reason, "cancel received");
                    if let Some((active, notify)) = &self.current_run_cancel {
                        if *active == run_id {
                            notify.notify_waiters();
                        } else {
                            tracing::warn!(
                                "cancel for run {run_id} but active run is {active}; ignoring"
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
                ServerMsg::Ping { ts } => {
                    let status = { *self.state.rx_status.borrow() };
                    let in_flight = { *self.state.rx_in_flight.borrow() };
                    let _ = self
                        .out
                        .send(Envelope::new(ClientMsg::Heartbeat {
                            ts,
                            status,
                            in_flight_run: in_flight,
                        }))
                        .await;
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
                ServerMsg::Revoke { reason } => {
                    tracing::error!("cloud revoked runner: {reason}");
                    if let Some((_, notify)) = &self.current_run_cancel {
                        notify.notify_waiters();
                    }
                    self.state.shutdown();
                    break;
                }
            }
            // Drop the cancel handle after the in-flight run completes. The
            // worker signals completion via `current_run` transitioning back
            // to None; we re-read it here lazily.
            if let Some((_, _)) = &self.current_run_cancel {
                let in_flight = { *self.state.rx_in_flight.borrow() };
                if in_flight.is_none() {
                    self.current_run_cancel = None;
                }
            }
        }
        self.state.set_connected(false).await;
        Ok(())
    }
}

/// Owns one `Assign`'s lifecycle. Spawned as a task from `RunnerLoop`, so the
/// message loop stays live and can deliver Cancel / Decide frames to us via
/// `self.cancel` and `self.approvals`.
struct AssignWorker {
    paths: Paths,
    config: Config,
    state: StateHandle,
    approvals: ApprovalRouter,
    out: mpsc::Sender<Envelope<ClientMsg>>,
    cancel: std::sync::Arc<tokio::sync::Notify>,
}

impl AssignWorker {
    async fn run(
        &mut self,
        run_id: uuid::Uuid,
        prompt: String,
        repo_url: Option<String>,
        expected_codex_model: Option<String>,
    ) -> Result<()> {
        self.handle_assign(run_id, prompt, repo_url, expected_codex_model)
            .await
    }

    async fn handle_assign(
        &mut self,
        run_id: uuid::Uuid,
        prompt: String,
        repo_url: Option<String>,
        expected_codex_model: Option<String>,
    ) -> Result<()> {
        // Resolve workspace.
        let wd = self.config.workspace.working_dir.clone();
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
                    | crate::workspace::ResolveError::NonEmptyNonRepo(_) => {
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
        let mut hist = HistoryWriter::open(&self.paths, run_id).await?;
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

        // Bridge to Codex.
        let mut bridge = match Bridge::spawn(
            &self.config.codex.binary,
            &workspace_path,
            self.config.codex.model_default.clone(),
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
                self.send(ClientMsg::RunFailed {
                    run_id,
                    reason: FailureReason::CodexCrash,
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
            resume_thread_id: None,
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
                self.send(ClientMsg::RunFailed {
                    run_id,
                    reason: FailureReason::CodexCrash,
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
            thread_id: cursor.thread_id.clone(),
            started_at: Utc::now(),
        })
        .await;
        hist.append(&HistoryEntry::Lifecycle {
            ts: Utc::now(),
            state: "started".to_string(),
            detail: Some(cursor.thread_id.clone()),
        })
        .await?;

        // Pump events until terminal.
        let outcome = self
            .pump_events(&mut bridge, &mut cursor, &mut hist, &workspace_path)
            .await?;

        bridge.server.shutdown(Duration::from_secs(5)).await.ok();

        let summary = RunSummary {
            run_id,
            work_item_id: None,
            status: outcome.status_label.clone(),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            title: None,
        };
        let mut idx = RunsIndex::load(&self.paths).unwrap_or_default();
        idx.upsert(summary);
        idx.save(&self.paths).ok();

        self.state.set_current_run(None).await;
        Ok(())
    }

    async fn pump_events(
        &mut self,
        bridge: &mut Bridge,
        cursor: &mut BridgeCursor,
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
                    cancelled = true;
                    bridge.interrupt().await.ok();
                    let _ = self.out.send(Envelope::new(ClientMsg::RunCancelled {
                        run_id: cursor.run_id,
                        cancelled_at: Utc::now(),
                    })).await;
                    hist.append(&HistoryEntry::Lifecycle {
                        ts: Utc::now(),
                        state: "cancelled".into(),
                        detail: None,
                    }).await.ok();
                    // Give Codex a short grace to wind down; if it doesn't, we
                    // exit and rely on the Bridge's shutdown to SIGKILL.
                    let _ = tokio::time::timeout(
                        Duration::from_secs(10),
                        bridge.server.inbound.recv(),
                    ).await;
                    return Ok(Outcome { status_label: "cancelled".into() });
                }
                frame = bridge.server.inbound.recv() => {
                    let Some(frame) = frame else {
                        self.send(ClientMsg::RunFailed {
                            run_id: cursor.run_id,
                            reason: FailureReason::CodexCrash,
                            detail: Some("codex stdout closed".to_string()),
                            ended_at: Utc::now(),
                        }).await;
                        hist.append(&HistoryEntry::Footer {
                            ts: Utc::now(),
                            final_status: "failed".into(),
                            done_payload: None,
                            error: Some("codex stdout closed".into()),
                        }).await?;
                        return Ok(Outcome { status_label: "failed".into() });
                    };
                    let events = cursor.translate(frame);
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
        bridge: &mut Bridge,
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
                tracing::trace!(%run_id, method, "codex event");
                Ok(None)
            }
            BridgeEvent::ApprovalRequest {
                run_id,
                approval_id,
                kind,
                payload,
                reason,
            } => {
                let policy = Policy::new(&self.config.approval_policy, workspace_root);
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

                // Wait for a decision (local or cloud).
                let mut rx = self.approvals.subscribe();
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
        let _ = self.out.send(Envelope::new(msg)).await;
    }
}

struct Outcome {
    status_label: String,
}

fn uuid_or(s: &str) -> uuid::Uuid {
    uuid::Uuid::parse_str(s).unwrap_or_else(|_| uuid::Uuid::new_v4())
}
