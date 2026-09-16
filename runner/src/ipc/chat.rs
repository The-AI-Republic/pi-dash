//! Daemon-side handler for the local-chat IPC surface (PDASHOSS01-159,
//! slice 1b).
//!
//! The desktop host drives the built-in agent engine *directly* over the
//! daemon's local IPC socket rather than relaying chat through the Pi Dash
//! cloud. This module is the runner-side of that surface: it services the
//! `Chat{Warm,Send,Cancel,Close,Decide}` requests added to `pidash-ipc` in
//! slice 1a, spawning/driving an [`AgentBridge`] and streaming the turn's
//! events back as `Response::Chat*` frames on the same connection.
//!
//! ## Why a parallel lane, not the cloud `ChatWorker`
//!
//! The cloud chat runtime (`daemon/supervisor.rs::ChatWorker`) is bound to a
//! [`RunnerOut`](crate::daemon::runner_out::RunnerOut) sink that emits
//! `ClientMsg::Chat*` frames to the Pi Dash server. Reusing it here would mean
//! abstracting that concrete sink and teeing its output onto the IPC socket —
//! surgery on the live cloud path, at the cost of the "existing cloud chat is
//! unchanged" acceptance criterion. Instead this is a **self-contained lane**
//! that drives `AgentBridge` itself and writes IPC frames. The cloud
//! `ChatWorker` is left completely untouched; the translation here mirrors it
//! field-for-field (the IPC `Response::Chat*` frames are 1:1 with the cloud
//! `ClientMsg::Chat*` frames), minus the cloud-only `chat_timing` telemetry.
//!
//! ## Concurrency (issue AC6)
//!
//! A local chat and a managed issue run must never touch the same working copy
//! at the same time. The cloud loop's own guard lives on `RunnerLoop`
//! (`current_run` / `current_chat`), which the IPC server cannot see. So the
//! shared signal lives on [`RunnerInstance`]: `chat_active` (an `AtomicBool`
//! observed by the assign path) plus a read of `state.rx_in_flight` (set while
//! a managed run holds the working copy). [`try_begin_local_turn`] claims the
//! working copy for a chat turn or refuses when a run holds it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use tokio::io::{AsyncWriteExt, BufStream};
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

use super::protocol::Response;
use crate::agent::{AgentBridge, BridgeEvent, RunPayload};
use crate::approval::policy::Policy;
use crate::approval::router::{ApprovalRecord, ApprovalRouter, ApprovalStatus, DecisionSource};
use crate::cloud::protocol::{ApprovalDecision, ApprovalKind};
use crate::config::schema::RunnerConfig;
use crate::daemon::runner_instance::RunnerInstance;

/// How long a resolved approval / cancel wait is allowed to hold a turn.
const APPROVAL_TTL_MINUTES: i64 = 10;

// ---------------------------------------------------------------------------
// Sink abstraction — lets the turn driver stream to the live IPC socket in
// production and into a `Vec` in unit tests, without a real connection.
// ---------------------------------------------------------------------------

/// A destination for streamed `Response::Chat*` frames.
pub(crate) trait ChatSink {
    async fn emit(&mut self, frame: Response) -> Result<()>;
}

/// Production sink: newline-delimited JSON straight onto the IPC connection,
/// flushed per frame so the desktop sees deltas as they arrive.
pub(crate) struct SocketSink<'a, S> {
    pub buf: &'a mut BufStream<S>,
}

impl<S> ChatSink for SocketSink<'_, S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    async fn emit(&mut self, frame: Response) -> Result<()> {
        let mut line = serde_json::to_vec(&frame)?;
        line.push(b'\n');
        self.buf.write_all(&line).await?;
        self.buf.flush().await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Per-runner session registry. Sessions persist across requests (and across
// connections) so a Codex thread survives between turns — the concrete Codex
// bridge has no resume-by-id, so continuity depends on keeping the same live
// bridge process for the session's lifetime.
// ---------------------------------------------------------------------------

/// A single live chat session: its engine bridge plus an out-of-band cancel
/// signal. `rt` serializes warm/turn/close for the one session; `cancel` is
/// tripped by `ChatCancel`/`ChatClose` arriving on a *different* connection
/// while a turn streams, and observed by the active turn's select loop.
struct ChatSession {
    rt: Mutex<SessionRuntime>,
    cancel: Notify,
}

#[derive(Default)]
struct SessionRuntime {
    bridge: Option<AgentBridge>,
    workspace: Option<PathBuf>,
    bridge_seq: u64,
    started_sent: bool,
}

/// Per-`RunnerInstance` registry of live chat sessions, keyed by
/// `chat_session_id`. Cloned by-Arc onto every `RunnerInstance` clone so all
/// IPC connections for a runner share one map.
#[derive(Clone, Default)]
pub struct ChatRegistry {
    sessions: Arc<Mutex<HashMap<Uuid, Arc<ChatSession>>>>,
}

impl ChatRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    async fn get_or_create(&self, id: Uuid) -> Arc<ChatSession> {
        let mut map = self.sessions.lock().await;
        map.entry(id)
            .or_insert_with(|| {
                Arc::new(ChatSession {
                    rt: Mutex::new(SessionRuntime::default()),
                    cancel: Notify::new(),
                })
            })
            .clone()
    }

    async fn get(&self, id: Uuid) -> Option<Arc<ChatSession>> {
        self.sessions.lock().await.get(&id).cloned()
    }

    async fn remove(&self, id: Uuid) -> Option<Arc<ChatSession>> {
        self.sessions.lock().await.remove(&id)
    }
}

// ---------------------------------------------------------------------------
// AC6 working-copy guard.
// ---------------------------------------------------------------------------

/// RAII claim on a runner's working copy for one local chat turn. Clears the
/// shared `chat_active` flag on drop so a crash / early-return can never leave
/// the runner wedged as "chat busy".
pub(crate) struct LocalTurnGuard {
    flag: Arc<AtomicBool>,
}

impl Drop for LocalTurnGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

/// Try to claim the runner's working copy for a local chat turn. Refuses when
/// a managed issue run holds it (`state.rx_in_flight`) or another local chat
/// turn is already active. The post-claim re-check of `rx_in_flight` closes
/// the window where a managed `Assign` lands between our two reads; the
/// assign lane's mirror check of `chat_active` (supervisor.rs) closes the
/// other direction. This is best-effort advisory exclusion, consistent with
/// the cloud loop's own `current_chat`/`current_run` checks.
pub(crate) fn try_begin_local_turn(inst: &RunnerInstance) -> std::result::Result<LocalTurnGuard, String> {
    if inst.state.rx_in_flight.borrow().is_some() {
        return Err("a managed issue run is using this runner's working copy".to_string());
    }
    if inst
        .chat_active
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("another local chat turn is already active on this runner".to_string());
    }
    if inst.state.rx_in_flight.borrow().is_some() {
        inst.chat_active.store(false, Ordering::SeqCst);
        return Err("a managed issue run is using this runner's working copy".to_string());
    }
    Ok(LocalTurnGuard {
        flag: inst.chat_active.clone(),
    })
}

// ---------------------------------------------------------------------------
// Request handlers, called from `IpcServer::dispatch`.
// ---------------------------------------------------------------------------

/// Arguments for a warm/send, threaded through from the IPC request.
pub(crate) struct WarmArgs {
    pub chat_session_id: Uuid,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub local_thread_id: Option<String>,
    pub local_session_id: Option<String>,
}

pub(crate) struct SendArgs {
    pub chat_session_id: Uuid,
    pub message_id: Uuid,
    pub content: String,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub local_thread_id: Option<String>,
    pub local_session_id: Option<String>,
}

/// `ChatWarm`: spawn/reuse+warm the engine without submitting a turn. Streams
/// a `ChatEvent{kind:"chat_warmed"}` (or `"chat_warm_failed"`), then `Ack`.
pub(crate) async fn handle_warm<S: ChatSink>(
    inst: &RunnerInstance,
    args: WarmArgs,
    sink: &mut S,
) -> Result<Response> {
    let session = inst.chat_sessions.get_or_create(args.chat_session_id).await;
    let mut rt = session.rt.lock().await;
    let chat_session_id = args.chat_session_id;

    let resume_id = chat_resume_id(
        args.local_session_id.as_deref(),
        args.local_thread_id.as_deref(),
    );
    let workspace = match ensure_bridge(
        inst,
        &mut rt,
        args.cwd.as_deref(),
        args.model.clone(),
        resume_id.as_deref(),
    )
    .await
    {
        Ok(w) => w,
        Err(e) => {
            sink.emit(Response::ChatEvent {
                chat_session_id,
                bridge_seq: rt.bridge_seq,
                kind: "chat_warm_failed".into(),
                payload: serde_json::json!({ "detail": format!("{e:#}") }),
            })
            .await?;
            return Ok(Response::Ack);
        }
    };

    let bridge = rt
        .bridge
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("chat bridge missing after spawn"))?;
    let warmed_thread_id = bridge.warm(&workspace).await?;
    if let Some(thread_id) = warmed_thread_id.clone()
        && !rt.started_sent
    {
        sink.emit(Response::ChatStarted {
            chat_session_id,
            local_thread_id: thread_id,
            local_session_id: warmed_thread_id.clone(),
            started_at: Utc::now(),
        })
        .await?;
        rt.started_sent = true;
    }
    rt.bridge_seq = rt.bridge_seq.saturating_add(1);
    sink.emit(Response::ChatEvent {
        chat_session_id,
        bridge_seq: rt.bridge_seq,
        kind: "chat_warmed".into(),
        payload: serde_json::json!({ "local_session_id": warmed_thread_id }),
    })
    .await?;
    Ok(Response::Ack)
}

/// `ChatSend`: submit a user turn. Streams `ChatMessageStarted`, then the
/// turn's `ChatEvent`/`ChatApprovalRequest` frames, and returns the terminal
/// `ChatMessageCompleted` (or `ChatFailed`) as the connection's final frame.
pub(crate) async fn handle_send<S: ChatSink>(
    inst: &RunnerInstance,
    args: SendArgs,
    sink: &mut S,
) -> Result<Response> {
    let chat_session_id = args.chat_session_id;

    // AC6: claim the working copy, or refuse cleanly.
    let _guard = match try_begin_local_turn(inst) {
        Ok(g) => g,
        Err(detail) => {
            return Ok(Response::ChatFailed {
                chat_session_id,
                code: "runner_busy".into(),
                detail: Some(detail),
                failed_at: Utc::now(),
            });
        }
    };

    let session = inst.chat_sessions.get_or_create(chat_session_id).await;
    let mut rt = session.rt.lock().await;

    let resume_id = chat_resume_id(
        args.local_session_id.as_deref(),
        args.local_thread_id.as_deref(),
    );
    let workspace = match ensure_bridge(
        inst,
        &mut rt,
        args.cwd.as_deref(),
        args.model.clone(),
        resume_id.as_deref(),
    )
    .await
    {
        Ok(w) => w,
        Err(e) => {
            return Ok(Response::ChatFailed {
                chat_session_id,
                code: "engine_start_failed".into(),
                detail: Some(format!("{e:#}")),
                failed_at: Utc::now(),
            });
        }
    };

    // Copy the streaming scalars into locals so the `&mut rt.bridge` borrow
    // (held across the turn's `.await`) doesn't collide with borrows of the
    // sibling fields; write them back once the turn returns.
    let mut bridge_seq = rt.bridge_seq;
    let mut started_sent = rt.started_sent;
    let outcome = {
        let bridge = rt
            .bridge
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("chat bridge missing after spawn"))?;
        drive_turn(
            DriveCtx {
                chat_session_id,
                message_id: args.message_id,
                content: args.content,
                model: args.model,
                runner_id: inst.config.runner_id,
                config: &inst.config,
                approvals: &inst.approvals,
                workspace: &workspace,
                cancel: &session.cancel,
            },
            bridge,
            &mut bridge_seq,
            &mut started_sent,
            sink,
        )
        .await
    };
    rt.bridge_seq = bridge_seq;
    rt.started_sent = started_sent;

    // A failed turn tears the bridge down so the next send respawns cleanly.
    let terminal = match outcome {
        Ok(t) => t,
        Err(e) => {
            if let Some(bridge) = rt.bridge.take() {
                bridge.shutdown(Duration::from_secs(5)).await.ok();
            }
            rt.started_sent = false;
            Response::ChatFailed {
                chat_session_id,
                code: "internal".into(),
                detail: Some(format!("{e:#}")),
                failed_at: Utc::now(),
            }
        }
    };
    Ok(terminal)
}

/// `ChatCancel`: interrupt the in-flight turn (if any). Best-effort — trips
/// the session's cancel signal, which the active turn observes.
pub(crate) async fn handle_cancel(inst: &RunnerInstance, chat_session_id: Uuid) -> Result<Response> {
    if let Some(session) = inst.chat_sessions.get(chat_session_id).await {
        session.cancel.notify_waiters();
    }
    Ok(Response::Ack)
}

/// `ChatClose`: interrupt any in-flight turn, tear the engine down, drop the
/// session, and emit a terminal `ChatClosed`.
pub(crate) async fn handle_close<S: ChatSink>(
    inst: &RunnerInstance,
    chat_session_id: Uuid,
    sink: &mut S,
) -> Result<Response> {
    if let Some(session) = inst.chat_sessions.get(chat_session_id).await {
        // Interrupt a streaming turn so it releases the runtime lock, then
        // acquire it to tear the bridge down.
        session.cancel.notify_waiters();
        let mut rt = session.rt.lock().await;
        if let Some(bridge) = rt.bridge.take() {
            bridge.shutdown(Duration::from_secs(5)).await.ok();
        }
        rt.started_sent = false;
    }
    inst.chat_sessions.remove(chat_session_id).await;
    sink.emit(Response::ChatClosed {
        chat_session_id,
        closed_at: Utc::now(),
    })
    .await?;
    Ok(Response::Ack)
}

/// `ChatDecide`: answer a pending chat approval. Routes into the runner's
/// shared approval router as `DecisionSource::Local`, unblocking the turn that
/// is parked on `approvals.subscribe()`.
pub(crate) async fn handle_decide(
    inst: &RunnerInstance,
    local_approval_id: &str,
    decision: ApprovalDecision,
) -> Result<Response> {
    let resolved = inst
        .approvals
        .decide(local_approval_id, decision, DecisionSource::Local)
        .await;
    if resolved.is_some() {
        Ok(Response::Ack)
    } else {
        anyhow::bail!("approval not found or already resolved");
    }
}

// ---------------------------------------------------------------------------
// Internals.
// ---------------------------------------------------------------------------

/// Ensure a live bridge exists on the session runtime, resolving the workspace
/// once and respawning if a prior bridge process exited. Returns the resolved
/// workspace path.
async fn ensure_bridge(
    inst: &RunnerInstance,
    rt: &mut SessionRuntime,
    cwd: Option<&str>,
    model: Option<String>,
    resume_id: Option<&str>,
) -> Result<PathBuf> {
    if rt.workspace.is_none() {
        rt.workspace = Some(resolve_chat_workspace(&inst.config, cwd)?);
    }
    let workspace = rt
        .workspace
        .clone()
        .ok_or_else(|| anyhow::anyhow!("chat workspace missing"))?;

    if rt.bridge.as_ref().is_some_and(bridge_has_exited) {
        if let Some(bridge) = rt.bridge.take() {
            bridge.shutdown(Duration::from_secs(1)).await.ok();
        }
        rt.started_sent = false;
    }
    if rt.bridge.is_none() {
        rt.bridge = Some(
            AgentBridge::spawn_from_config_with_resume(&inst.config, &workspace, model, resume_id)
                .await?,
        );
    }
    Ok(workspace)
}

struct DriveCtx<'a> {
    chat_session_id: Uuid,
    message_id: Uuid,
    content: String,
    model: Option<String>,
    runner_id: Uuid,
    config: &'a RunnerConfig,
    approvals: &'a ApprovalRouter,
    workspace: &'a Path,
    cancel: &'a Notify,
}

/// Drive an engine turn to completion, streaming `Response::Chat*` frames to
/// `sink` and returning the terminal frame (`ChatMessageCompleted` or
/// `ChatFailed`). Mirrors the cloud `ChatWorker::handle_turn` translation,
/// minus the cloud-only `chat_timing` telemetry.
async fn drive_turn<S: ChatSink>(
    ctx: DriveCtx<'_>,
    bridge: &mut AgentBridge,
    bridge_seq: &mut u64,
    started_sent: &mut bool,
    sink: &mut S,
) -> Result<Response> {
    let DriveCtx {
        chat_session_id,
        message_id,
        content,
        model,
        runner_id,
        config,
        approvals,
        workspace,
        cancel,
    } = ctx;

    let payload = RunPayload {
        run_id: message_id,
        prompt: content,
        model: model.clone(),
    };
    let mut cursor = bridge.run(&payload, workspace).await?;
    let turn_id = cursor.thread_id().to_string();
    if !*started_sent {
        sink.emit(Response::ChatStarted {
            chat_session_id,
            local_thread_id: turn_id.clone(),
            local_session_id: Some(turn_id.clone()),
            started_at: Utc::now(),
        })
        .await?;
        *started_sent = true;
    }
    sink.emit(Response::ChatMessageStarted {
        chat_session_id,
        message_id,
        turn_id: Some(turn_id.clone()),
        started_at: Utc::now(),
    })
    .await?;

    let mut final_status = "completed".to_string();
    let mut assistant_message: Option<String> = None;

    // A single pinned cancel future: `notify_waiters()` stores no permit, so a
    // fresh future per iteration would miss a cancel that landed mid-poll.
    let cancelled = cancel.notified();
    tokio::pin!(cancelled);

    'turn: loop {
        tokio::select! {
            biased;
            _ = &mut cancelled => {
                bridge.interrupt().await.ok();
                final_status = "cancelled".into();
                break 'turn;
            }
            events = bridge.next_events(&mut cursor) => {
                let Some(events) = events else {
                    return Ok(Response::ChatFailed {
                        chat_session_id,
                        code: "agent_stdout_closed".into(),
                        detail: Some("agent stdout closed".into()),
                        failed_at: Utc::now(),
                    });
                };
                let mut done = false;
                for ev in events {
                    *bridge_seq = bridge_seq.saturating_add(1);
                    match ev {
                        BridgeEvent::Raw { method, params, .. } => {
                            let kind = if is_assistant_text_delta(&method, &params) {
                                "assistant_delta"
                            } else {
                                "raw"
                            };
                            sink.emit(Response::ChatEvent {
                                chat_session_id,
                                bridge_seq: *bridge_seq,
                                kind: kind.into(),
                                payload: serde_json::json!({ "method": method, "params": params }),
                            })
                            .await?;
                        }
                        BridgeEvent::ApprovalRequest {
                            approval_id,
                            kind,
                            payload,
                            reason,
                            ..
                        } => {
                            if let Some(term) = resolve_approval(
                                ApprovalCtx {
                                    chat_session_id,
                                    runner_id,
                                    message_id,
                                    config,
                                    approvals,
                                    workspace,
                                },
                                bridge,
                                &approval_id,
                                kind,
                                payload,
                                reason,
                                sink,
                            )
                            .await?
                            {
                                return Ok(term);
                            }
                        }
                        BridgeEvent::Completed { done_payload, .. } => {
                            assistant_message = assistant_text_from_done_payload(&done_payload);
                            final_status = "completed".into();
                            done = true;
                            break;
                        }
                        BridgeEvent::Failed { detail, .. } => {
                            return Ok(Response::ChatFailed {
                                chat_session_id,
                                code: "agent_failed".into(),
                                detail,
                                failed_at: Utc::now(),
                            });
                        }
                        BridgeEvent::RunStarted { .. } | BridgeEvent::AwaitingReauth { .. } => {}
                    }
                }
                if done {
                    break 'turn;
                }
            }
        }
    }

    Ok(Response::ChatMessageCompleted {
        chat_session_id,
        message_id,
        turn_id: Some(turn_id),
        assistant_message,
        status: final_status,
        completed_at: Utc::now(),
    })
}

struct ApprovalCtx<'a> {
    chat_session_id: Uuid,
    runner_id: Uuid,
    message_id: Uuid,
    config: &'a RunnerConfig,
    approvals: &'a ApprovalRouter,
    workspace: &'a Path,
}

/// Evaluate the approval policy; auto-decide when the policy allows, otherwise
/// surface a `ChatApprovalRequest` and block on the shared approval router
/// until a `ChatDecide` (on another connection) resolves it. Returns
/// `Some(terminal)` only when the approval path itself failed the turn.
async fn resolve_approval<S: ChatSink>(
    ctx: ApprovalCtx<'_>,
    bridge: &mut AgentBridge,
    approval_id: &str,
    kind: ApprovalKind,
    payload: serde_json::Value,
    reason: Option<String>,
    sink: &mut S,
) -> Result<Option<Response>> {
    let policy = Policy::new(&ctx.config.approval_policy, ctx.workspace);
    if let Some(auto) = policy.evaluate(kind, &payload).into_cloud() {
        if let Err(e) = bridge.send_approval(approval_id, auto).await {
            return Ok(Some(Response::ChatFailed {
                chat_session_id: ctx.chat_session_id,
                code: "approval_send_failed".into(),
                detail: Some(format!("{e:#}")),
                failed_at: Utc::now(),
            }));
        }
        return Ok(None);
    }

    let expires_at = Some(Utc::now() + chrono::Duration::minutes(APPROVAL_TTL_MINUTES));
    let rec = ApprovalRecord {
        approval_id: approval_id.to_string(),
        runner_id: ctx.runner_id,
        run_id: ctx.message_id,
        kind,
        payload: payload.clone(),
        reason: reason.clone(),
        requested_at: Utc::now(),
        expires_at,
        status: ApprovalStatus::Pending,
    };
    // Subscribe before opening so a `ChatDecide` that races ahead is not lost.
    let mut rx = ctx.approvals.subscribe();
    ctx.approvals.open(rec).await;
    sink.emit(Response::ChatApprovalRequest {
        chat_session_id: ctx.chat_session_id,
        local_approval_id: approval_id.to_string(),
        kind,
        payload,
        reason,
        expires_at,
    })
    .await?;

    loop {
        match rx.recv().await {
            Ok(ApprovalRecord {
                approval_id: aid,
                status: ApprovalStatus::Resolved { decision, .. },
                ..
            }) if aid == approval_id => {
                if let Err(e) = bridge.send_approval(approval_id, decision).await {
                    return Ok(Some(Response::ChatFailed {
                        chat_session_id: ctx.chat_session_id,
                        code: "approval_send_failed".into(),
                        detail: Some(format!("{e:#}")),
                        failed_at: Utc::now(),
                    }));
                }
                return Ok(None);
            }
            Ok(_) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => return Ok(None),
        }
    }
}

/// One working dir per runner: chat resolves to the same `workspace.working_dir`
/// an issue run uses (the AC6 guard keeps the two lanes mutually exclusive). An
/// optional `cwd` may narrow to a subdirectory but may not escape the
/// workspace root.
fn resolve_chat_workspace(config: &RunnerConfig, cwd: Option<&str>) -> Result<PathBuf> {
    let workspace_path = config.workspace.working_dir.clone();
    std::fs::create_dir_all(&workspace_path)?;
    if let Some(cwd) = cwd.filter(|s| !s.is_empty()) {
        let requested = PathBuf::from(cwd);
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

fn chat_resume_id(local_session_id: Option<&str>, local_thread_id: Option<&str>) -> Option<String> {
    local_session_id
        .filter(|s| !s.is_empty())
        .or_else(|| local_thread_id.filter(|s| !s.is_empty()))
        .map(ToOwned::to_owned)
}

fn bridge_has_exited(bridge: &AgentBridge) -> bool {
    bridge.process_handle().exit_rx.borrow().is_some()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::app_server::AppServer;
    use crate::codex::bridge::Bridge;
    use tokio::process::Command;

    /// Collecting sink for driving `drive_turn` without a socket.
    #[derive(Default)]
    struct VecSink {
        frames: Vec<Response>,
    }
    impl ChatSink for VecSink {
        async fn emit(&mut self, frame: Response) -> Result<()> {
            self.frames.push(frame);
            Ok(())
        }
    }

    fn config() -> RunnerConfig {
        use crate::config::schema::{
            AgentSection, ApprovalPolicySection, ClaudeCodeSection, CodexSection,
            CursorAgentSection, WorkspaceSection,
        };
        RunnerConfig {
            name: "chat-test".into(),
            runner_id: Uuid::new_v4(),
            workspace_slug: Some("ws".into()),
            project_slug: Some("PROJ".into()),
            pod_id: None,
            workspace: WorkspaceSection {
                working_dir: std::env::temp_dir().join("pidash-chat-drive-test"),
            },
            agent: AgentSection::default(),
            codex: CodexSection::default(),
            claude_code: ClaudeCodeSection::default(),
            cursor_agent: CursorAgentSection::default(),
            openclaw: Default::default(),
            grok: Default::default(),
            muse_code: Default::default(),
            approval_policy: ApprovalPolicySection::default(),
        }
    }

    async fn fake_bridge(script: &str) -> AgentBridge {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        let server = AppServer::spawn_command(cmd)
            .await
            .expect("spawn fake codex");
        AgentBridge::Codex(Bridge::from_server(server, None))
    }

    /// A fake app-server that warms a thread and completes one turn, emitting a
    /// single assistant delta.
    fn happy_script() -> &'static str {
        r#"
            set -e
            read _
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
            read _
            read thread
            case "$thread" in *'"method":"thread/start"'*) ;; *) exit 1;; esac
            printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_drive"}}'
            read _
            printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"text":"hello"}}'
            printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"conclusion":"success","done":{"status":"ok","summary":"done"}}}'
            sleep 0.3
        "#
    }

    #[tokio::test]
    async fn drive_turn_streams_started_delta_and_completed() {
        let cfg = config();
        let approvals = ApprovalRouter::new();
        let cancel = Notify::new();
        let mut bridge = fake_bridge(happy_script()).await;
        // Warm first so the thread id exists (mirrors the send path).
        bridge.warm(&cfg.workspace.working_dir).await.expect("warm");

        let mut sink = VecSink::default();
        let (mut seq, mut started) = (0u64, false);
        let chat_session_id = Uuid::new_v4();
        let message_id = Uuid::new_v4();
        let terminal = drive_turn(
            DriveCtx {
                chat_session_id,
                message_id,
                content: "hi".into(),
                model: None,
                runner_id: cfg.runner_id,
                config: &cfg,
                approvals: &approvals,
                workspace: &cfg.workspace.working_dir,
                cancel: &cancel,
            },
            &mut bridge,
            &mut seq,
            &mut started,
            &mut sink,
        )
        .await
        .expect("drive turn");

        // Terminal frame is a completed message for this turn.
        match terminal {
            Response::ChatMessageCompleted {
                status,
                message_id: mid,
                ..
            } => {
                assert_eq!(status, "completed");
                assert_eq!(mid, message_id);
            }
            other => panic!("expected ChatMessageCompleted, got {other:?}"),
        }
        // The stream carried ChatMessageStarted then an assistant_delta.
        assert!(
            sink.frames
                .iter()
                .any(|f| matches!(f, Response::ChatMessageStarted { .. })),
            "missing ChatMessageStarted: {:?}",
            sink.frames
        );
        assert!(
            sink.frames.iter().any(|f| matches!(
                f,
                Response::ChatEvent { kind, .. } if kind == "assistant_delta"
            )),
            "missing assistant_delta ChatEvent: {:?}",
            sink.frames
        );
    }

    #[tokio::test]
    async fn cancel_signal_interrupts_a_running_turn() {
        // A script that warms, then hangs on the turn (never completes) until
        // its stdin closes — so only the cancel path can end the turn.
        let script = r#"
            set -e
            read _
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
            read _
            read _
            printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_hang"}}'
            read _
            # never emit turn/completed; idle until interrupted/closed
            sleep 5
        "#;
        let cfg = config();
        let approvals = ApprovalRouter::new();
        let cancel = Arc::new(Notify::new());
        let mut bridge = fake_bridge(script).await;
        bridge.warm(&cfg.workspace.working_dir).await.expect("warm");

        let cancel2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            cancel2.notify_waiters();
        });

        let mut sink = VecSink::default();
        let (mut seq, mut started) = (0u64, false);
        let terminal = tokio::time::timeout(
            Duration::from_secs(3),
            drive_turn(
                DriveCtx {
                    chat_session_id: Uuid::new_v4(),
                    message_id: Uuid::new_v4(),
                    content: "hang".into(),
                    model: None,
                    runner_id: cfg.runner_id,
                    config: &cfg,
                    approvals: &approvals,
                    workspace: &cfg.workspace.working_dir,
                    cancel: &cancel,
                },
                &mut bridge,
                &mut seq,
                &mut started,
                &mut sink,
            ),
        )
        .await
        .expect("turn should not hang past cancel")
        .expect("drive turn");

        match terminal {
            Response::ChatMessageCompleted { status, .. } => assert_eq!(status, "cancelled"),
            other => panic!("expected cancelled ChatMessageCompleted, got {other:?}"),
        }
    }

    fn offline_instance() -> RunnerInstance {
        use crate::config::schema::DaemonConfig;
        use crate::util::paths::Paths;
        let base = std::env::temp_dir().join(format!("pidash-chat-guard-{}", Uuid::new_v4()));
        let paths = Paths {
            config_dir: base.join("config"),
            data_dir: base.join("data"),
            runtime_dir: base.join("runtime"),
        };
        RunnerInstance::new_offline(config(), &paths, DaemonConfig::default())
    }

    #[tokio::test]
    async fn local_turn_guard_is_mutually_exclusive() {
        let inst = offline_instance();

        // First claim succeeds; a second concurrent claim is refused.
        let g1 = try_begin_local_turn(&inst).expect("first claim");
        assert!(inst.chat_active.load(Ordering::SeqCst));
        assert!(
            try_begin_local_turn(&inst).is_err(),
            "a second local turn must be refused while one is active"
        );

        // Releasing the guard clears the flag and re-enables claiming.
        drop(g1);
        assert!(!inst.chat_active.load(Ordering::SeqCst));
        let _g2 = try_begin_local_turn(&inst).expect("claim after release");
    }

    #[tokio::test]
    async fn local_turn_guard_refuses_while_managed_run_in_flight() {
        use pidash_ipc::protocol::CurrentRunSummary;
        let inst = offline_instance();
        inst.state
            .set_current_run(Some(CurrentRunSummary {
                run_id: Uuid::new_v4(),
                thread_id: None,
                status: "running".into(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;

        // A managed run holds the working copy → a local chat turn is refused,
        // and the shared flag is left clear so the assign lane isn't blocked.
        assert!(try_begin_local_turn(&inst).is_err());
        assert!(!inst.chat_active.load(Ordering::SeqCst));

        // Once the run clears, a local turn can claim the working copy.
        inst.state.set_current_run(None).await;
        let _g = try_begin_local_turn(&inst).expect("claim after run clears");
    }

    #[test]
    fn resolve_chat_workspace_confines_to_workspace() {
        let cfg = config();
        std::fs::create_dir_all(&cfg.workspace.working_dir).unwrap();
        // An absolute path outside the workspace is rejected.
        assert!(resolve_chat_workspace(&cfg, Some("/etc")).is_err());
        // No cwd resolves to the workspace root itself.
        assert_eq!(
            resolve_chat_workspace(&cfg, None).unwrap(),
            cfg.workspace.working_dir
        );
        // A relative subdir under the workspace is accepted and stays under it.
        let ok = resolve_chat_workspace(&cfg, Some("sub")).unwrap();
        assert!(ok.starts_with(&cfg.workspace.working_dir));
    }
}
