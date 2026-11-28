//! Daemon-side handler for the local-chat IPC surface (PDASHOSS01-159,
//! slice 1b).
//!
//! The desktop host drives the Pi Dash Agent engine *directly* over the
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
//!
//! ## Two hosts (design §12.1, PO-8)
//!
//! A request is served by a [`ChatHost`]:
//!
//! - **A project runner** — the request names one (`runner`), or carries no
//!   absolute `cwd`: the runner's agent, its folder and the AC6 guard above.
//! - **The daemon's engine profile** ([`super::pidash_agent_chat::PidashAgentChatProfile`])
//!   — no `runner` selector and an absolute `cwd`: the machine-level built-in
//!   engine, with no project runner and no shared busy flag, so it works with
//!   zero or many runners and is never blocked by a run. Its folder must not
//!   be inside any runner's working dir.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use tokio::io::{AsyncWriteExt, BufStream};
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

use super::pidash_agent_chat::PidashAgentChatProfile;
use super::protocol::Response;
use crate::agent::{AgentBridge, BridgeEvent, RunPayload, within_startup_deadline};
use crate::approval::policy::Policy;
use crate::approval::router::{ApprovalRecord, ApprovalRouter, ApprovalStatus, DecisionSource};
use crate::cloud::protocol::{ApprovalDecision, ApprovalKind, ApprovalMode};
use crate::codex::gateway::{
    CHAT_AGENT_FAILED, assistant_text_from_done_payload, gateway_chat_refusal,
    gateway_refusal_code, gateway_status_text, is_assistant_text_delta,
};
#[cfg(test)]
use crate::codex::gateway::{CHAT_MODEL_AUTH_FAILED, CHAT_MODEL_CREDIT_EXHAUSTED};
use crate::config::schema::RunnerConfig;
use crate::daemon::runner_instance::RunnerInstance;
use crate::pidash_agent::chat_server::{
    ApprovalOutcome, ApprovalRequest as SharedApprovalRequest, SendSpec, TurnSink, TurnTerminal,
    WarmSpec,
};
use crate::util::chat_unit::ChatUnit;
use crate::util::containment::CleanupOutcome;
use crate::util::fs::create_dir_all_private;

/// How long a resolved approval / cancel wait is allowed to hold a turn.
pub(crate) const APPROVAL_TTL_MINUTES: i64 = 10;

// ---------------------------------------------------------------------------
// Sink abstraction — lets the turn driver stream to the live IPC socket in
// production and into a `Vec` in unit tests, without a real connection.
// ---------------------------------------------------------------------------

/// A destination for streamed `Response::Chat*` frames.
///
/// `emit` uses `impl Future` rather than `async fn` so the returned future
/// carries an explicit `Send` bound: native `async fn` in traits does not
/// propagate `Send`, and the turn drivers hold the future across threads.
pub(crate) trait ChatSink: Send {
    #[allow(clippy::manual_async_fn)]
    fn emit(&mut self, frame: Response) -> impl Future<Output = Result<()>> + Send;
}

/// Production sink: newline-delimited JSON straight onto the IPC connection,
/// flushed per frame so the desktop sees deltas as they arrive.
pub(crate) struct SocketSink<'a, S> {
    pub buf: &'a mut BufStream<S>,
}

impl<S> ChatSink for SocketSink<'_, S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
{
    #[allow(clippy::manual_async_fn)]
    fn emit(&mut self, frame: Response) -> impl Future<Output = Result<()>> + Send {
        async move {
            let mut line = serde_json::to_vec(&frame)?;
            line.push(b'\n');
            self.buf.write_all(&line).await?;
            self.buf.flush().await?;
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Per-runner session registry. Sessions persist across requests (and across
// connections) so a Codex thread survives between turns. When the in-memory
// bridge is gone (restart, Close, failed turn), a fresh bridge resumes the
// saved thread via `thread/resume`; unknown threads fall back to `thread/start`.
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
    /// The session's containment unit (PDASHOSS01-417): what its agents
    /// start lives until the session's runtime ends, not one turn. Created
    /// with the first bridge; finished by [`end_runtime`].
    unit: Option<ChatUnit>,
    workspace: Option<PathBuf>,
    bridge_seq: u64,
    started_sent: bool,
    /// Whether the live bridge already received the CLI guidance
    /// (NEWPIDASH-402). Set on the first turn of a bridge, cleared
    /// whenever the bridge is dropped so a revival in a fresh
    /// subprocess injects again.
    guidance_sent: bool,
    /// The approval mode this session's engine thread was spawned under.
    /// Captured once, when the bridge is first spawned, and then held for the
    /// life of the thread so a mode change mid-session cannot retune a turn
    /// already in flight — it takes effect on the next thread instead.
    approval_mode: ApprovalMode,
    /// When this session was last warmed or sent a turn. Stamped on
    /// creation and on every completed warm/turn; read by the registry
    /// reaper to evict idle sessions.
    last_used_at: Option<DateTime<Utc>>,
}

/// Per-`RunnerInstance` registry of live chat sessions, keyed by
/// `chat_session_id`. Cloned by-Arc onto every `RunnerInstance` clone so all
/// IPC connections for a runner share one map.
#[derive(Clone, Default)]
pub struct ChatRegistry {
    sessions: Arc<Mutex<HashMap<Uuid, Arc<ChatSession>>>>,
    /// Units of ended runtimes whose processes are not confirmed gone. The
    /// working copy is not handed to a run while any is left (see
    /// [`ChatRegistry::release_idle`]).
    lingering: Arc<Mutex<Vec<ChatUnit>>>,
    /// Evicted runtimes still being ended outside the map (see
    /// [`ChatRegistry::reap_idle`]). Their processes may still be using the
    /// working copy, so the handoff refuses while any is ending.
    ending: Arc<std::sync::atomic::AtomicUsize>,
}

/// Counts one evicted runtime in [`ChatRegistry::ending`] until dropped.
struct Ending(Arc<std::sync::atomic::AtomicUsize>);

impl Ending {
    fn new(count: &Arc<std::sync::atomic::AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(count.clone())
    }
}

impl Drop for Ending {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Shut a session runtime's bridge down, then stop what its agents left in
/// the session's unit. Returns the unit when that is unresolved: its
/// processes may still be using the working copy.
async fn end_runtime(rt: &mut SessionRuntime, grace: Duration) -> Option<ChatUnit> {
    if let Some(bridge) = rt.bridge.take() {
        bridge.shutdown(grace).await.ok();
    }
    rt.started_sent = false;
    rt.guidance_sent = false;
    let mut unit = rt.unit.take()?;
    (unit.finish().await == CleanupOutcome::Unresolved).then_some(unit)
}

impl ChatRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// [`end_runtime`], keeping an unresolved unit in [`Self::lingering`].
    async fn end_runtime(&self, rt: &mut SessionRuntime, grace: Duration) {
        if let Some(unit) = end_runtime(rt, grace).await {
            tracing::warn!(
                "local chat session's processes are not confirmed gone; the working copy stays held"
            );
            self.lingering.lock().await.push(unit);
        }
    }

    /// Free the working copy for a run (PDASHOSS01-417): end the agent of
    /// every idle session, stopping what it left running, and re-check the
    /// units of runtimes that ended unresolved. Sessions stay registered: the
    /// next warm/send resumes the thread in a fresh bridge. `true` only when
    /// no session is busy (a warm or turn holds its runtime) and nothing a
    /// session started may still be running.
    pub(crate) async fn release_idle(&self) -> bool {
        let sessions: Vec<Arc<ChatSession>> =
            self.sessions.lock().await.values().cloned().collect();
        let mut free = true;
        for session in sessions {
            let Ok(mut rt) = session.rt.try_lock() else {
                free = false;
                continue;
            };
            if rt.bridge.is_some() || rt.unit.is_some() {
                self.end_runtime(&mut rt, Duration::from_secs(5)).await;
            }
        }
        let mut lingering = self.lingering.lock().await;
        let mut left = Vec::new();
        for mut unit in lingering.drain(..) {
            if unit.finish().await == CleanupOutcome::Unresolved {
                left.push(unit);
            }
        }
        *lingering = left;
        // Read under the `lingering` lock: an evicted runtime that ends
        // unresolved is pushed there before it stops counting as ending,
        // so it is seen in one place or the other.
        let ending = self.ending.load(Ordering::SeqCst) > 0;
        free && lingering.is_empty() && !ending
    }

    async fn get_or_create(&self, id: Uuid) -> Arc<ChatSession> {
        // Bounds for the process-per-session runner lane only: the engine
        // profile lane keeps its threads on the shared server and unloads
        // them with `thread/unsubscribe` instead (NEWPIDASH-333).
        const MAX_LIVE_RUNNER_CHAT_SESSIONS: usize = 8;
        if let Some(existing) = self.get(id).await {
            if let Ok(mut rt) = existing.rt.try_lock() {
                rt.last_used_at = Some(Utc::now());
            }
            self.reap_idle(id, MAX_LIVE_RUNNER_CHAT_SESSIONS).await;
            return existing;
        }
        // Leave room for the session about to be inserted.
        self.reap_idle(id, MAX_LIVE_RUNNER_CHAT_SESSIONS - 1).await;
        let mut map = self.sessions.lock().await;
        map.entry(id)
            .or_insert_with(|| {
                Arc::new(ChatSession {
                    rt: Mutex::new(SessionRuntime {
                        last_used_at: Some(Utc::now()),
                        ..SessionRuntime::default()
                    }),
                    cancel: Notify::new(),
                })
            })
            .clone()
    }

    /// Evict idle sessions: those past their idle TTL first, then the
    /// least-recently-used ones while the registry exceeds `target`. The
    /// session being requested (`except`) and any session with a turn in
    /// flight (its runtime is locked) are never touched. Evicted bridges
    /// are shut down, tearing down their engine processes; the next
    /// warm/send for a dropped session respawns it.
    async fn reap_idle(&self, except: Uuid, target: usize) {
        let now = Utc::now();
        // Runner lane only (see `get_or_create`).
        let idle_limit = chrono::Duration::minutes(10);
        // Snapshot the eviction candidates under the map lock. `try_lock`
        // only — a locked runtime means a warm/turn is in flight and the
        // session must survive.
        let candidates: Vec<(Uuid, Arc<ChatSession>)> = {
            let map = self.sessions.lock().await;
            let mut candidates = Vec::new();
            let mut by_age: Vec<(Option<DateTime<Utc>>, Uuid, Arc<ChatSession>)> = Vec::new();
            for (id, session) in map.iter() {
                if *id == except {
                    continue;
                }
                let Ok(rt) = session.rt.try_lock() else {
                    continue;
                };
                if rt.last_used_at.is_none_or(|t| now - t > idle_limit) {
                    candidates.push((*id, session.clone()));
                } else {
                    by_age.push((rt.last_used_at, *id, session.clone()));
                }
            }
            let excess = map.len().saturating_sub(target);
            if excess > 0 {
                by_age.sort_by_key(|(at, _, _)| *at);
                by_age.truncate(excess);
                candidates.extend(by_age.into_iter().map(|(_, id, s)| (id, s)));
            }
            candidates
        };
        for (id, expected) in candidates {
            // Re-check under the map lock: only evict the exact session
            // snapshotted. If a concurrent `get_or_create` for the same id
            // already created a fresh entry, `expected` is stale and must be
            // left alone — removing/replacing the new entry would orphan the
            // caller's Arc (two live runtimes for one chat_session_id). A
            // busy runtime at this point means a turn started after the
            // snapshot: leave the session entirely untouched, no reinsert.
            let (bridge, _ending) = {
                let mut map = self.sessions.lock().await;
                let Some(current) = map.get(&id) else {
                    continue;
                };
                if !Arc::ptr_eq(current, &expected) {
                    continue;
                }
                let Ok(mut rt) = expected.rt.try_lock() else {
                    continue;
                };
                // Counted as ending before it leaves the map, so the
                // handoff never sees it in neither place.
                let ending = Ending::new(&self.ending);
                let session = map.remove(&id).expect("entry checked present");
                let _ = session;
                let rt = SessionRuntime {
                    bridge: rt.bridge.take(),
                    unit: rt.unit.take(),
                    ..SessionRuntime::default()
                };
                (rt, ending)
            };
            let mut ended = bridge;
            self.end_runtime(&mut ended, Duration::from_secs(5)).await;
        }
    }

    async fn get(&self, id: Uuid) -> Option<Arc<ChatSession>> {
        self.sessions.lock().await.get(&id).cloned()
    }

    async fn remove(&self, id: Uuid) -> Option<Arc<ChatSession>> {
        self.sessions.lock().await.remove(&id)
    }

    /// Whether `id` is a live session of this registry.
    pub(crate) async fn contains(&self, id: Uuid) -> bool {
        self.sessions.lock().await.contains_key(&id)
    }
}

// (reap_one removed: eviction is now atomic under the map lock in
// `reap_idle`, eliminating the blind-reinsert race where a busy session
// was re-inserted over a concurrently created entry for the same id.)

// ---------------------------------------------------------------------------
// Chat hosts.
// ---------------------------------------------------------------------------

/// What serves a local chat request on a project runner: its session
/// registry, approval router and agent. The daemon's own engine profile is
/// a different lane ([`EngineCtx`]): its sessions are threads on the shared
/// engine app-server, not per-session bridges.
pub(crate) struct ChatHost<'a> {
    sessions: &'a ChatRegistry,
    approvals: &'a ApprovalRouter,
    inst: &'a RunnerInstance,
}

impl<'a> ChatHost<'a> {
    pub(crate) fn runner(inst: &'a RunnerInstance) -> Self {
        Self {
            sessions: &inst.chat_sessions,
            approvals: &inst.approvals,
            inst,
        }
    }

    fn config(&self) -> &RunnerConfig {
        &self.inst.config
    }

    /// Claim the working copy for one turn.
    fn begin_turn(&self) -> std::result::Result<Option<LocalTurnGuard>, String> {
        try_begin_local_turn(self.inst).map(Some)
    }

    fn resolve_workspace(&self, cwd: Option<&str>) -> Result<PathBuf> {
        resolve_chat_workspace(&self.inst.config, cwd)
    }

    async fn spawn(
        &self,
        workspace: &Path,
        model: Option<String>,
        resume_id: Option<&str>,
        mode: ApprovalMode,
    ) -> Result<AgentBridge> {
        let logs_dir = self.inst.paths.logs_dir();
        AgentBridge::spawn_from_config_with_resume(
            &self.inst.config,
            workspace,
            model,
            resume_id,
            Some(mode),
            Some(logs_dir.as_path()),
        )
        .await
    }
}

/// What serves a local chat request on the daemon's own engine profile
/// (design §12.1): its shared server, approval router and config, plus the
/// runner dirs a chat folder may not be inside.
pub(crate) struct EngineCtx<'a> {
    pub(crate) profile: &'a PidashAgentChatProfile,
    /// `(runner name, working dir)` of every configured runner.
    pub(crate) runner_dirs: Vec<(String, PathBuf)>,
}

impl<'a> EngineCtx<'a> {
    fn resolve_workspace(&self, cwd: Option<&str>) -> Result<PathBuf> {
        super::pidash_agent_chat::resolve_engine_chat_folder(cwd, &self.runner_dirs)
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

/// Why an issue run still holds the runner's working copy: it is in flight,
/// or it finished but its run-end cleanup is unresolved, so its processes
/// may still be using the working copy (PDASHOSS01-417).
fn working_copy_busy(inst: &RunnerInstance) -> Option<String> {
    if inst.state.rx_in_flight.borrow().is_some() {
        return Some("a managed issue run is using this runner's working copy".to_string());
    }
    if inst.state.cleanup_reserved() {
        return Some(
            "the last issue run's processes are still being cleaned up on this runner's working copy"
                .to_string(),
        );
    }
    None
}

/// Try to claim the runner's working copy for a local chat turn. Refuses when
/// a managed issue run holds it (`state.rx_in_flight`), its run-end cleanup is
/// unresolved (`state.cleanup_reserved`), or another local chat
/// turn is already active. The post-claim re-check of `rx_in_flight` closes
/// the window where a managed `Assign` lands between our two reads; the
/// assign lane's mirror check of `chat_active` (supervisor.rs) closes the
/// other direction. This is best-effort advisory exclusion, consistent with
/// the cloud loop's own `current_chat`/`current_run` checks.
pub(crate) fn try_begin_local_turn(
    inst: &RunnerInstance,
) -> std::result::Result<LocalTurnGuard, String> {
    if let Some(busy) = working_copy_busy(inst) {
        return Err(busy);
    }
    if inst
        .chat_active
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("another local chat turn is already active on this runner".to_string());
    }
    if let Some(busy) = working_copy_busy(inst) {
        inst.chat_active.store(false, Ordering::SeqCst);
        return Err(busy);
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
    pub mode: Option<ApprovalMode>,
    pub local_thread_id: Option<String>,
    pub local_session_id: Option<String>,
}

pub(crate) struct SendArgs {
    pub chat_session_id: Uuid,
    pub message_id: Uuid,
    pub content: String,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub mode: Option<ApprovalMode>,
    pub local_thread_id: Option<String>,
    pub local_session_id: Option<String>,
}

/// `ChatWarm`: spawn/reuse+warm the engine without submitting a turn. Streams
/// a `ChatEvent{kind:"chat_warmed"}` (or `"chat_warm_failed"`), then `Ack`.
pub(crate) async fn handle_warm<S: ChatSink>(
    host: &ChatHost<'_>,
    args: WarmArgs,
    sink: &mut S,
) -> Result<Response> {
    let chat_session_id = args.chat_session_id;
    // A warm starts the agent in the working copy, so it claims the copy
    // like a turn (AC6, PDASHOSS01-417): never during an issue run or its
    // unresolved cleanup, and never after an Assign's handoff.
    let _guard = match host.begin_turn() {
        Ok(g) => g,
        Err(detail) => {
            sink.emit(Response::ChatEvent {
                chat_session_id,
                bridge_seq: 0,
                kind: "chat_warm_failed".into(),
                payload: serde_json::json!({ "detail": detail, "code": "runner_busy" }),
            })
            .await?;
            return Ok(Response::Ack);
        }
    };
    let session = host.sessions.get_or_create(chat_session_id).await;
    let mut rt = session.rt.lock().await;

    let resume_id = chat_resume_id(
        args.local_session_id.as_deref(),
        args.local_thread_id.as_deref(),
    );
    let workspace = match ensure_bridge(
        host,
        chat_session_id,
        &mut rt,
        args.cwd.as_deref(),
        args.model.clone(),
        resume_id.as_deref(),
        args.mode,
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
    // Race the warm against ChatCancel/ChatClose so a hung engine does not
    // hold the session's runtime lock for the whole startup budget.
    let cancelled = session.cancel.notified();
    let warmed = tokio::select! {
        biased;
        _ = cancelled => Err(anyhow::anyhow!("chat warm cancelled")),
        r = within_startup_deadline(bridge.warm(&workspace)) => r,
    };
    let warmed_thread_id = match warmed {
        Ok(id) => id,
        Err(e) => {
            // Mid-handshake bridge: discard it so the next warm/send respawns.
            if let Some(bridge) = rt.bridge.take() {
                bridge.shutdown(Duration::from_secs(5)).await.ok();
            }
            rt.started_sent = false;
            rt.guidance_sent = false;
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
    rt.last_used_at = Some(Utc::now());
    Ok(Response::Ack)
}

/// `ChatSend`: submit a user turn. Streams `ChatMessageStarted`, then the
/// turn's `ChatEvent`/`ChatApprovalRequest` frames, and returns the terminal
/// `ChatMessageCompleted` (or `ChatFailed`) as the connection's final frame.
pub(crate) async fn handle_send<S: ChatSink>(
    host: &ChatHost<'_>,
    args: SendArgs,
    sink: &mut S,
) -> Result<Response> {
    let chat_session_id = args.chat_session_id;

    // AC6: claim a project runner's working copy, or refuse cleanly. The
    // engine profile shares no folder with a run, so it claims nothing.
    let _guard = match host.begin_turn() {
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

    let session = host.sessions.get_or_create(chat_session_id).await;
    let mut rt = session.rt.lock().await;

    let resume_id = chat_resume_id(
        args.local_session_id.as_deref(),
        args.local_thread_id.as_deref(),
    );
    let workspace = match ensure_bridge(
        host,
        chat_session_id,
        &mut rt,
        args.cwd.as_deref(),
        args.model.clone(),
        resume_id.as_deref(),
        args.mode,
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
    let approval_mode = rt.approval_mode;
    // First turn of a bridge carries the CLI guidance (NEWPIDASH-402);
    // later turns on the same bridge send the user's text verbatim.
    let guidance = (!rt.guidance_sent).then(crate::chat_guidance::chat_guidance);
    let injected = guidance.is_some();
    let outcome = {
        let rt = &mut *rt;
        let bridge = rt
            .bridge
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("chat bridge missing after spawn"))?;
        // A one-shot engine spawns per turn: those processes join the
        // session's unit too (its process group, or the cgroup their login
        // shell joins before the agent starts).
        let turn = drive_turn(
            DriveCtx {
                chat_session_id,
                message_id: args.message_id,
                content: args.content,
                guidance,
                model: args.model,
                mode: approval_mode,
                runner_id: host.config().runner_id,
                config: host.config(),
                approvals: host.approvals,
                workspace: &workspace,
                cancel: &session.cancel,
            },
            bridge,
            &mut bridge_seq,
            &mut started_sent,
            sink,
        );
        match rt.unit.as_ref() {
            Some(unit) => unit.scoped(turn).await,
            None => turn.await,
        }
    };
    rt.bridge_seq = bridge_seq;
    rt.started_sent = started_sent;
    rt.last_used_at = Some(Utc::now());

    // A failed turn tears the bridge down so the next send respawns cleanly.
    let terminal = match outcome {
        Ok(t) => t,
        Err(e) => {
            if let Some(bridge) = rt.bridge.take() {
                bridge.shutdown(Duration::from_secs(5)).await.ok();
            }
            rt.started_sent = false;
            if e.is::<StartupCancelled>() {
                Response::ChatMessageCompleted {
                    chat_session_id,
                    message_id: args.message_id,
                    turn_id: None,
                    assistant_message: None,
                    status: "cancelled".into(),
                    completed_at: Utc::now(),
                }
            } else {
                Response::ChatFailed {
                    chat_session_id,
                    code: "internal".into(),
                    detail: Some(format!("{e:#}")),
                    failed_at: Utc::now(),
                }
            }
        }
    };
    // Protocol-level failures are successful Rust results too. They may end
    // our reader before the engine's turn ends (e.g. a metadata warning).
    // Never reuse that process: its unread reply would belong to this failed
    // turn, not the next message. Keep the stored thread id for a clean resume.
    if matches!(terminal, Response::ChatFailed { .. }) {
        if let Some(bridge) = rt.bridge.take() {
            bridge.shutdown(Duration::from_secs(5)).await.ok();
        }
        rt.started_sent = false;
    }
    // A dropped bridge re-arms guidance for the fresh subprocess; a live
    // bridge that just received it stays quiet (NEWPIDASH-402).
    rt.guidance_sent =
        crate::chat_guidance::next_guidance_sent(rt.guidance_sent, injected, rt.bridge.is_some());
    Ok(terminal)
}

/// `ChatCancel`: interrupt the in-flight turn (if any). Best-effort — trips
/// the session's cancel signal, which the active turn observes.
pub(crate) async fn handle_cancel(host: &ChatHost<'_>, chat_session_id: Uuid) -> Result<Response> {
    if let Some(session) = host.sessions.get(chat_session_id).await {
        session.cancel.notify_waiters();
    }
    Ok(Response::Ack)
}

/// `ChatClose`: interrupt any in-flight turn, tear the engine down, drop the
/// session, and emit a terminal `ChatClosed`.
pub(crate) async fn handle_close<S: ChatSink>(
    host: &ChatHost<'_>,
    chat_session_id: Uuid,
    sink: &mut S,
) -> Result<Response> {
    if let Some(session) = host.sessions.get(chat_session_id).await {
        // Interrupt a streaming turn so it releases the runtime lock, then
        // acquire it to tear the bridge down.
        session.cancel.notify_waiters();
        let mut rt = session.rt.lock().await;
        host.sessions
            .end_runtime(&mut rt, Duration::from_secs(5))
            .await;
    }
    host.sessions.remove(chat_session_id).await;
    sink.emit(Response::ChatClosed {
        chat_session_id,
        closed_at: Utc::now(),
    })
    .await?;
    Ok(Response::Ack)
}

/// `ChatDecide`: answer a pending chat approval. Routes into the lane's
/// shared approval router as `DecisionSource::Local`, unblocking the turn that
/// is parked on `approvals.subscribe()`.
pub(crate) async fn handle_decide(
    approvals: &ApprovalRouter,
    local_approval_id: &str,
    decision: ApprovalDecision,
) -> Result<Response> {
    let resolved = approvals
        .decide(local_approval_id, decision, DecisionSource::Local)
        .await;
    if resolved.is_some() {
        Ok(Response::Ack)
    } else {
        anyhow::bail!("approval not found or already resolved");
    }
}

// ---------------------------------------------------------------------------
// Engine-profile lane: desktop "This computer" chat on the shared server.
// ---------------------------------------------------------------------------
//
// The IPC protocol and its event frames are unchanged from the per-session
// lane, so the desktop app needs no change: warm still streams
// `ChatStarted` + `chat_warmed`, send still streams `ChatMessageStarted`,
// `ChatEvent` / `ChatApprovalRequest` and a terminal frame. Only the engine
// behind them changed — one shared app-server child instead of one process
// per session — and the 8-session / 10-minute caps are gone in favour of
// thread unsubscribe/unload.

/// `ChatWarm` on the engine profile: warm the session's thread on the
/// shared server without submitting a turn.
pub(crate) async fn handle_engine_warm<S: ChatSink>(
    ctx: &EngineCtx<'_>,
    args: WarmArgs,
    sink: &mut S,
) -> Result<Response> {
    let mode = args.mode.unwrap_or_default();
    let workspace = match ctx.resolve_workspace(args.cwd.as_deref()) {
        Ok(w) => w,
        Err(e) => {
            // Folder errors fail the warm the same way an engine that never
            // started does: `chat_warm_failed`, then `Ack`. No session is
            // created: the server was never contacted.
            let mut events = SharedSink::new(
                ctx,
                args.chat_session_id,
                mode,
                Arc::new(Notify::new()),
                sink,
            );
            events.workspace = PathBuf::from("/");
            let seq = ctx.profile.server().session_seq(args.chat_session_id).await;
            events.warm_failed(format!("{e:#}"), seq).await?;
            return Ok(Response::Ack);
        }
    };
    // The sink observes the session's own cancel signal so a `ChatCancel`
    // racing the warm still reaches an approval parked below it.
    let cancel = ctx
        .profile
        .server()
        .session_cancel(args.chat_session_id)
        .await;
    let mut events = SharedSink::new(ctx, args.chat_session_id, mode, cancel, sink);
    events.workspace = workspace.clone();
    let spec = WarmSpec {
        workspace,
        mode,
        resume_id: chat_resume_id(
            args.local_session_id.as_deref(),
            args.local_thread_id.as_deref(),
        ),
    };
    let _ = ctx
        .profile
        .server()
        .warm(args.chat_session_id, spec, &mut events)
        .await;
    // Errors already streamed as `chat_warm_failed` through the sink.
    Ok(Response::Ack)
}

/// `ChatSend` on the engine profile: submit a user turn on the session's
/// shared-server thread and stream its frames.
pub(crate) async fn handle_engine_send<S: ChatSink>(
    ctx: &EngineCtx<'_>,
    args: SendArgs,
    sink: &mut S,
) -> Result<Response> {
    let chat_session_id = args.chat_session_id;
    let message_id = args.message_id;
    let mode = args.mode.unwrap_or_default();
    let workspace = match ctx.resolve_workspace(args.cwd.as_deref()) {
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
    let cancel = ctx.profile.server().session_cancel(chat_session_id).await;
    let mut events = SharedSink::for_send(ctx, chat_session_id, message_id, mode, cancel, sink);
    events.workspace = workspace.clone();
    // Read the stored folder before the turn starts: the driver holds the
    // session lock for the whole turn, so the approval policy cannot look
    // it up mid-turn.
    events.stored_workspace = ctx
        .profile
        .server()
        .session_workspace(chat_session_id)
        .await;
    let spec = SendSpec {
        workspace,
        mode,
        resume_id: chat_resume_id(
            args.local_session_id.as_deref(),
            args.local_thread_id.as_deref(),
        ),
        message_id,
        content: args.content.clone(),
        model: args.model.clone(),
    };
    match ctx
        .profile
        .server()
        .send(chat_session_id, spec, &mut events)
        .await
    {
        TurnTerminal::Completed {
            turn_id,
            assistant_message,
            status,
        } => Ok(Response::ChatMessageCompleted {
            chat_session_id,
            message_id,
            turn_id,
            assistant_message,
            status,
            completed_at: Utc::now(),
        }),
        TurnTerminal::Failed { code, detail } => Ok(Response::ChatFailed {
            chat_session_id,
            code,
            detail,
            failed_at: Utc::now(),
        }),
    }
}

/// `ChatCancel` on the engine profile: interrupt the in-flight turn, if any.
pub(crate) async fn handle_engine_cancel(
    ctx: &EngineCtx<'_>,
    chat_session_id: Uuid,
) -> Result<Response> {
    ctx.profile.server().cancel(chat_session_id);
    Ok(Response::Ack)
}

/// `ChatClose` on the engine profile: interrupt the turn, unsubscribe the
/// thread so the engine can unload it, drop the session.
pub(crate) async fn handle_engine_close<S: ChatSink>(
    ctx: &EngineCtx<'_>,
    chat_session_id: Uuid,
    sink: &mut S,
) -> Result<Response> {
    ctx.profile.server().close(chat_session_id).await;
    sink.emit(Response::ChatClosed {
        chat_session_id,
        closed_at: Utc::now(),
    })
    .await?;
    Ok(Response::Ack)
}

/// The engine lane's [`TurnSink`]: maps the shared turn's stream onto the
/// same `Response::Chat*` frames the per-session lane emits, and answers
/// approval requests through the profile's approval router.
struct SharedSink<'a, 's, S: ChatSink> {
    ctx: &'a EngineCtx<'a>,
    chat_session_id: Uuid,
    message_id: Option<Uuid>,
    mode: ApprovalMode,
    cancel: Arc<Notify>,
    /// The folder this request resolved to. The approval policy evaluates
    /// against the session's stored folder (see `stored_workspace`); this
    /// is the fallback for a session that never warmed.
    workspace: PathBuf,
    /// The session's stored folder, read before the turn started. The turn
    /// driver holds the session lock for the whole turn, so the policy
    /// cannot look this up mid-turn (that re-lock self-deadlocks); the
    /// handler reads it up front instead.
    stored_workspace: Option<PathBuf>,
    sink: &'s mut S,
}

impl<'a, 's, S: ChatSink> SharedSink<'a, 's, S> {
    fn new(
        ctx: &'a EngineCtx<'a>,
        chat_session_id: Uuid,
        mode: ApprovalMode,
        cancel: Arc<Notify>,
        sink: &'s mut S,
    ) -> Self {
        Self {
            ctx,
            chat_session_id,
            message_id: None,
            mode,
            cancel,
            workspace: PathBuf::from("/"),
            stored_workspace: None,
            sink,
        }
    }

    fn for_send(
        ctx: &'a EngineCtx<'a>,
        chat_session_id: Uuid,
        message_id: Uuid,
        mode: ApprovalMode,
        cancel: Arc<Notify>,
        sink: &'s mut S,
    ) -> Self {
        Self {
            ctx,
            chat_session_id,
            message_id: Some(message_id),
            mode,
            cancel,
            workspace: PathBuf::from("/"),
            stored_workspace: None,
            sink,
        }
    }
}

impl<S: ChatSink> TurnSink for SharedSink<'_, '_, S> {
    async fn warmed(&mut self, thread_id: &str, seq: u64) -> Result<()> {
        self.sink
            .emit(Response::ChatEvent {
                chat_session_id: self.chat_session_id,
                bridge_seq: seq,
                kind: "chat_warmed".into(),
                payload: serde_json::json!({ "local_session_id": thread_id }),
            })
            .await
    }

    async fn warm_failed(&mut self, detail: String, seq: u64) -> Result<()> {
        self.sink
            .emit(Response::ChatEvent {
                chat_session_id: self.chat_session_id,
                bridge_seq: seq,
                kind: "chat_warm_failed".into(),
                payload: serde_json::json!({ "detail": detail }),
            })
            .await
    }

    async fn thread_started(&mut self, thread_id: &str) -> Result<()> {
        self.sink
            .emit(Response::ChatStarted {
                chat_session_id: self.chat_session_id,
                local_thread_id: thread_id.to_string(),
                local_session_id: Some(thread_id.to_string()),
                started_at: Utc::now(),
            })
            .await
    }

    async fn message_started(&mut self, turn_id: &str) -> Result<()> {
        self.sink
            .emit(Response::ChatMessageStarted {
                chat_session_id: self.chat_session_id,
                message_id: self.message_id.unwrap_or_else(Uuid::nil),
                turn_id: Some(turn_id.to_string()),
                started_at: Utc::now(),
            })
            .await
    }

    async fn event(&mut self, seq: u64, kind: &str, payload: serde_json::Value) -> Result<()> {
        self.sink
            .emit(Response::ChatEvent {
                chat_session_id: self.chat_session_id,
                bridge_seq: seq,
                kind: kind.to_string(),
                payload,
            })
            .await
    }

    async fn decide_approval(&mut self, req: SharedApprovalRequest) -> ApprovalOutcome {
        resolve_shared_approval(self, req).await
    }
}

/// Evaluate the approval policy and, when the policy asks the user, surface
/// a `ChatApprovalRequest` and block on the profile's approval router until
/// a `ChatDecide` resolves it. Mirrors the per-session lane's
/// `resolve_approval` (same policy, TTL and cancel semantics); only the
/// transport back to the engine differs, and the driver owns that.
async fn resolve_shared_approval<S: ChatSink>(
    sink: &mut SharedSink<'_, '_, S>,
    req: SharedApprovalRequest,
) -> ApprovalOutcome {
    // The policy evaluates against the session's stored folder (the folder
    // the thread runs in), falling back to this request's folder.
    let workspace = sink.stored_workspace.as_deref().unwrap_or(&sink.workspace);
    let policy = Policy::new(
        &sink.ctx.profile.config().approval_policy,
        workspace,
        sink.mode,
    );
    if let Some(auto) = policy.evaluate(req.kind, &req.payload).into_cloud() {
        return ApprovalOutcome::Decided(auto);
    }
    let message_id = sink.message_id.unwrap_or_else(Uuid::nil);
    let expires_at = Some(Utc::now() + chrono::Duration::minutes(APPROVAL_TTL_MINUTES));
    let rec = ApprovalRecord {
        approval_id: req.approval_id.clone(),
        runner_id: sink.ctx.profile.config().runner_id,
        run_id: message_id,
        kind: req.kind,
        payload: req.payload.clone(),
        reason: req.reason.clone(),
        requested_at: Utc::now(),
        expires_at,
        status: ApprovalStatus::Pending,
    };
    let approvals = &sink.ctx.profile.approvals;
    let mut rx = approvals.subscribe();
    approvals.open(rec).await;
    if sink
        .sink
        .emit(Response::ChatApprovalRequest {
            chat_session_id: sink.chat_session_id,
            local_approval_id: req.approval_id.clone(),
            kind: req.kind,
            payload: req.payload,
            reason: req.reason,
            expires_at,
        })
        .await
        .is_err()
    {
        approvals.expire(&req.approval_id).await;
        return ApprovalOutcome::TurnFailed {
            code: "internal".into(),
            detail: Some("chat sink closed".into()),
        };
    }
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(APPROVAL_TTL_MINUTES.max(0) as u64 * 60);
    loop {
        tokio::select! {
            biased;
            _ = sink.cancel.notified() => {
                approvals.expire(&req.approval_id).await;
                return ApprovalOutcome::TurnCancelled;
            }
            _ = tokio::time::sleep_until(deadline) => {
                approvals.expire(&req.approval_id).await;
                return ApprovalOutcome::TurnFailed {
                    code: "approval_timeout".into(),
                    detail: Some(format!(
                        "no approval decision within {APPROVAL_TTL_MINUTES} minutes"
                    )),
                };
            }
            recv = rx.recv() => {
                match recv {
                    Ok(ApprovalRecord {
                        approval_id: aid,
                        status: ApprovalStatus::Resolved { decision, .. },
                        ..
                    }) if aid == req.approval_id => {
                        return ApprovalOutcome::Decided(decision);
                    }
                    Ok(_) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => {
                        approvals.expire(&req.approval_id).await;
                        return ApprovalOutcome::TurnFailed {
                            code: "approval_channel_closed".into(),
                            detail: Some("approval router stopped before a decision arrived".into()),
                        };
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Internals.
// ---------------------------------------------------------------------------

/// Ensure a live bridge exists on the session runtime, resolving the workspace
/// once and respawning if a prior bridge process exited. Returns the resolved
/// workspace path.
async fn ensure_bridge(
    host: &ChatHost<'_>,
    chat_session_id: Uuid,
    rt: &mut SessionRuntime,
    cwd: Option<&str>,
    model: Option<String>,
    resume_id: Option<&str>,
    mode: Option<ApprovalMode>,
) -> Result<PathBuf> {
    if rt.workspace.is_none() {
        rt.workspace = Some(host.resolve_workspace(cwd)?);
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
        // The replacement below is a fresh subprocess: re-arm guidance.
        rt.guidance_sent = false;
    }
    if rt.bridge.is_none() {
        // Capture the mode for the whole thread the moment we spawn it. A
        // later warm/send that carries a different mode reuses this bridge and
        // does not disturb the captured value — the new mode only lands when a
        // fresh session spawns its own bridge.
        rt.approval_mode = mode.unwrap_or_default();
        let unit = rt
            .unit
            .get_or_insert_with(|| ChatUnit::new(chat_session_id));
        let bridge = unit
            .scoped(host.spawn(&workspace, model, resume_id, rt.approval_mode))
            .await?;
        unit.adopt(bridge.process_handle().pid);
        rt.bridge = Some(bridge);
    }
    Ok(workspace)
}

/// `drive_turn` was cancelled before the engine finished starting. The bridge
/// is left mid-handshake, so the caller discards it like any failed turn but
/// reports the turn as cancelled rather than failed.
#[derive(Debug)]
struct StartupCancelled;

impl std::fmt::Display for StartupCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("chat turn cancelled during engine startup")
    }
}

impl std::error::Error for StartupCancelled {}

struct DriveCtx<'a> {
    chat_session_id: Uuid,
    message_id: Uuid,
    content: String,
    /// Rendered CLI guidance for the first turn of a bridge
    /// (NEWPIDASH-402); `None` once the bridge has it. The user
    /// history never shows this: frames carry no user content.
    guidance: Option<String>,
    model: Option<String>,
    mode: ApprovalMode,
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
        guidance,
        model,
        mode,
        runner_id,
        config,
        approvals,
        workspace,
        cancel,
    } = ctx;

    let prompt = match guidance {
        Some(g) => crate::chat_guidance::prefix_with(&g, &content),
        None => content,
    };
    let payload = RunPayload {
        run_id: message_id,
        prompt,
        model: model.clone(),
    };
    // A single pinned cancel future, created before startup: `notify_waiters()`
    // stores no permit, so a cancel that lands while the engine is still
    // starting (or mid-poll later) must already have a registered waiter.
    let cancelled = cancel.notified();
    tokio::pin!(cancelled);

    let mut cursor = tokio::select! {
        biased;
        _ = cancelled.as_mut() => return Err(StartupCancelled.into()),
        r = within_startup_deadline(bridge.run(&payload, workspace)) => r?,
    };
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
                            if let Some((code, detail)) = gateway_chat_refusal(&method, &params) {
                                // The engine files a 402/401 under "stream
                                // disconnected" and is about to re-send the
                                // whole request, five times over. Its first
                                // retry is ~200ms out, so the turn is stopped
                                // here rather than by a round trip through the
                                // client, and the retry notice is not
                                // forwarded.
                                bridge.interrupt().await.ok();
                                return Ok(Response::ChatFailed {
                                    chat_session_id,
                                    code: code.into(),
                                    detail: Some(detail),
                                    failed_at: Utc::now(),
                                });
                            }
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
                                    mode,
                                    config,
                                    approvals,
                                    workspace,
                                },
                                bridge,
                                &approval_id,
                                kind,
                                payload,
                                reason,
                                &mut cancelled,
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
                            // An engine that gave up on its own (no retries
                            // left, or none configured) reports the same
                            // refusal as a bare string.
                            let refusal = detail
                                .as_deref()
                                .and_then(gateway_status_text)
                                .and_then(|(status, text)| Some((gateway_refusal_code(status)?, text)));
                            let (code, detail) = match refusal {
                                Some((code, text)) => (code, Some(text)),
                                None => (CHAT_AGENT_FAILED, detail),
                            };
                            return Ok(Response::ChatFailed {
                                chat_session_id,
                                code: code.into(),
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
    mode: ApprovalMode,
    config: &'a RunnerConfig,
    approvals: &'a ApprovalRouter,
    workspace: &'a Path,
}

/// Evaluate the approval policy; auto-decide when the policy allows, otherwise
/// surface a `ChatApprovalRequest` and block on the shared approval router
/// until a `ChatDecide` (on another connection) resolves it. Returns
/// `Some(terminal)` only when the approval path itself ended the turn — the
/// user cancelled (`cancelled`), the approval timed out, or sending the
/// decision to the engine failed.
///
/// The wait is not open-ended: an abandoned approval (the user closed the tab,
/// the host crashed, or the socket dropped without a `ChatCancel`) would
/// otherwise park the turn forever, and `chat_active` would stay `true` and
/// NACK every future managed `Assign` (issue AC1 interrupt + AC7 regression).
/// The `cancelled` arm handles an explicit interrupt; the `expires_at`
/// deadline is the backstop for a silent disconnect.
#[allow(clippy::too_many_arguments)]
async fn resolve_approval<S: ChatSink>(
    ctx: ApprovalCtx<'_>,
    bridge: &mut AgentBridge,
    approval_id: &str,
    kind: ApprovalKind,
    payload: serde_json::Value,
    reason: Option<String>,
    cancelled: &mut std::pin::Pin<&mut tokio::sync::futures::Notified<'_>>,
    sink: &mut S,
) -> Result<Option<Response>> {
    let policy = Policy::new(&ctx.config.approval_policy, ctx.workspace, ctx.mode);
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

    // The turn parked here must still observe an interrupt and a TTL. Reuse
    // the turn's single pinned `cancelled` future so a cancel that landed
    // mid-poll is not missed, and fail closed at the deadline.
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(APPROVAL_TTL_MINUTES.max(0) as u64 * 60);
    loop {
        tokio::select! {
            biased;
            _ = cancelled.as_mut() => {
                // Explicit interrupt (ChatCancel/ChatClose) while the approval
                // was outstanding: abort the engine turn and release the
                // working-copy guard rather than parking forever.
                bridge.interrupt().await.ok();
                ctx.approvals.expire(approval_id).await;
                return Ok(Some(Response::ChatMessageCompleted {
                    chat_session_id: ctx.chat_session_id,
                    message_id: ctx.message_id,
                    turn_id: None,
                    assistant_message: None,
                    status: "cancelled".into(),
                    completed_at: Utc::now(),
                }));
            }
            _ = tokio::time::sleep_until(deadline) => {
                // No decision within the TTL (silent host crash / socket drop
                // with no ChatCancel): expire the approval, interrupt the
                // engine, and fail the turn so the guard is released.
                bridge.interrupt().await.ok();
                ctx.approvals.expire(approval_id).await;
                return Ok(Some(Response::ChatFailed {
                    chat_session_id: ctx.chat_session_id,
                    code: "approval_timeout".into(),
                    detail: Some(format!(
                        "no approval decision within {APPROVAL_TTL_MINUTES} minutes"
                    )),
                    failed_at: Utc::now(),
                }));
            }
            recv = rx.recv() => {
                match recv {
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
                    Err(_) => {
                        // The router's broadcast channel closed, so no decision
                        // can ever arrive. Continuing would leave the engine
                        // parked on an approval with the TTL arm gone and the
                        // working-copy guard still held, so end the turn.
                        bridge.interrupt().await.ok();
                        ctx.approvals.expire(approval_id).await;
                        return Ok(Some(Response::ChatFailed {
                            chat_session_id: ctx.chat_session_id,
                            code: "approval_channel_closed".into(),
                            detail: Some("approval router stopped before a decision arrived".into()),
                            failed_at: Utc::now(),
                        }));
                    }
                }
            }
        }
    }
}

/// Resolve the directory a local chat turn runs in.
///
/// Two shapes are allowed, and the difference matters:
///
/// * **No `cwd`** — the runner's own `workspace.working_dir`, the same
///   directory an issue run uses. The AC6 guard (`try_begin_local_turn`) is
///   what keeps the two lanes off it at the same time.
/// * **An absolute `cwd`** — the caller's own working copy. The desktop host
///   passes the per-account chat working copy it owns
///   (`<app-data>/managed/chat/<account>/workdirs/...`, a deliberate *sibling*
///   of the runner's `workdirs` tree — see `ManagedPaths`), which satisfies
///   AC6 by construction rather than by locking. Refusing it, as an earlier
///   containment-only rule did, failed every desktop chat send with
///   "chat cwd is outside runner workspace".
///
/// A relative `cwd` still narrows to a subdirectory of the runner workspace,
/// and `..` is refused in both shapes: a lexical `starts_with` check would
/// otherwise accept `<workspace>/../escape`. The caller is trusted to the same
/// degree as any other IPC request — the control socket is owner-only and the
/// desktop host is its only client — so an absolute path is a directory
/// selection, not a privilege boundary. It must already exist, so a typo
/// fails loudly instead of silently creating a tree somewhere unexpected.
fn resolve_chat_workspace(config: &RunnerConfig, cwd: Option<&str>) -> Result<PathBuf> {
    let workspace_path = config.workspace.working_dir.clone();
    if let Some(cwd) = cwd.filter(|s| !s.is_empty()) {
        let requested_rel = PathBuf::from(cwd);
        if requested_rel
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            anyhow::bail!("chat cwd must not contain `..`");
        }
        if requested_rel.is_absolute() {
            if !requested_rel.is_dir() {
                anyhow::bail!(
                    "chat cwd {} does not exist (the caller owns this directory and must create it)",
                    requested_rel.display()
                );
            }
            return Ok(requested_rel);
        }
        create_dir_all_private(&workspace_path)?;
        let requested = workspace_path.join(requested_rel);
        if !requested.starts_with(&workspace_path) {
            anyhow::bail!("chat cwd is outside runner workspace");
        }
        return Ok(requested);
    }
    create_dir_all_private(&workspace_path)?;
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
        #[allow(clippy::manual_async_fn)]
        fn emit(&mut self, frame: Response) -> impl Future<Output = Result<()>> + Send {
            async move {
                self.frames.push(frame);
                Ok(())
            }
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
            antigravity: Default::default(),
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
    async fn drive_turn_cancel_during_engine_startup_is_not_lost() {
        // An engine that never answers its handshake. Before the startup race
        // the cancel fired with no registered waiter and was dropped, so the
        // turn sat out the whole startup budget.
        let cfg = config();
        let approvals = ApprovalRouter::new();
        let cancel = Notify::new();
        let mut bridge = fake_bridge("while read _; do :; done").await;
        let mut sink = VecSink::default();
        let (mut seq, mut started) = (0u64, false);
        let begin = std::time::Instant::now();
        let (result, ()) = tokio::join!(
            drive_turn(
                DriveCtx {
                    chat_session_id: Uuid::new_v4(),
                    message_id: Uuid::new_v4(),
                    content: "hi".into(),
                    guidance: None,
                    model: None,
                    mode: ApprovalMode::Ask,
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
            async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                cancel.notify_waiters();
            },
        );
        let err = result.expect_err("cancelled startup must not produce a turn");
        assert!(err.is::<StartupCancelled>(), "unexpected error: {err:#}");
        assert!(begin.elapsed() < Duration::from_secs(5));
        assert!(!started, "no ChatStarted before the engine came up");
        bridge.shutdown(Duration::from_secs(1)).await.ok();
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
                guidance: None,
                model: None,
                mode: ApprovalMode::Ask,
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

    /// A fake app-server that logs every request line to `$PROMPT_LOG`
    /// and completes two turns. Lets the test inspect the exact
    /// `turn/start` prompt the engine received.
    fn logging_script() -> &'static str {
        r#"
            set -e
            log() { printf '%s\n' "$1" >> "$PROMPT_LOG"; }
            read line; log "$line"
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
            read line; log "$line"
            read thread; log "$thread"
            case "$thread" in *'"method":"thread/start"'*) ;; *) exit 1;; esac
            printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_log"}}'
            for n in 1 2; do
                read turn; log "$turn"
                case "$turn" in *'"method":"turn/start"'*) ;; *) exit 1;; esac
                printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"text":"hello"}}'
                printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"conclusion":"success","done":{"status":"ok","summary":"done"}}}'
            done
            sleep 0.3
        "#
    }

    async fn fake_logging_bridge(script: &str, log: &std::path::Path) -> AgentBridge {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script).env("PROMPT_LOG", log);
        let server = AppServer::spawn_command(cmd)
            .await
            .expect("spawn fake codex");
        AgentBridge::Codex(Bridge::from_server(server, None))
    }

    fn turn_start_lines(log: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(log)
            .expect("prompt log written")
            .lines()
            .filter(|line| line.contains("\"method\":\"turn/start\""))
            .map(str::to_string)
            .collect()
    }

    #[tokio::test]
    async fn drive_turn_prefixes_guidance_only_on_the_first_turn() {
        // The bridge payload is the acceptance hook (NEWPIDASH-402):
        // the first turn of a bridge carries the CLI guidance, the
        // second sends the user's text verbatim, and no user-visible
        // frame leaks the injected text.
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = tmp.path().join("prompts.log");
        let cfg = config();
        let approvals = ApprovalRouter::new();
        let cancel = Notify::new();
        let mut bridge = fake_logging_bridge(logging_script(), &log).await;
        bridge.warm(&cfg.workspace.working_dir).await.expect("warm");

        let mut sink = VecSink::default();
        let (mut seq, mut started) = (0u64, false);
        drive_turn(
            DriveCtx {
                chat_session_id: Uuid::new_v4(),
                message_id: Uuid::new_v4(),
                content: "list the open issues".into(),
                guidance: Some("GUIDANCE-MARKER-402".into()),
                model: None,
                mode: ApprovalMode::Ask,
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
        .expect("first turn");
        drive_turn(
            DriveCtx {
                chat_session_id: Uuid::new_v4(),
                message_id: Uuid::new_v4(),
                content: "second question".into(),
                guidance: None,
                model: None,
                mode: ApprovalMode::Ask,
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
        .expect("second turn");

        let turns = turn_start_lines(&log);
        assert_eq!(turns.len(), 2, "two turn/start prompts logged");
        assert!(
            turns[0].contains("GUIDANCE-MARKER-402") && turns[0].contains("list the open issues"),
            "first turn carries guidance plus user text"
        );
        assert!(
            !turns[1].contains("GUIDANCE-MARKER-402") && turns[1].contains("second question"),
            "second turn sends the user's text verbatim"
        );
        assert!(
            !format!("{:?}", sink.frames).contains("GUIDANCE-MARKER-402"),
            "no user-visible frame leaks the injected text"
        );
        bridge.shutdown(Duration::from_secs(1)).await.ok();
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
                    guidance: None,
                    model: None,
                    mode: ApprovalMode::Ask,
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

    /// A fake app-server that treats `turn/interrupt` the way the bundled
    /// engine does: it needs the thread and turn ids, and answers anything
    /// else with an error while the turn keeps running. A turn that was not
    /// interrupted delivers its reply (`OLD`) when the next one starts.
    fn interrupt_script() -> &'static str {
        r#"
            read -r _
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
            read -r _
            read -r _
            printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_stop"}}'
            read -r _
            printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"turn":{"id":"turn_1"}}}'
            printf '%s\n' '{"jsonrpc":"2.0","method":"turn/started","params":{"threadId":"th_stop","turn":{"id":"turn_1"}}}'
            read -r interrupt
            case "$interrupt" in
                *'"threadId":"th_stop"'*'"turnId":"turn_1"'*)
                    printf '%s\n' '{"jsonrpc":"2.0","id":4,"result":{}}'
                    printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"th_stop","turn":{"id":"turn_1","status":"interrupted"}}}'
                    read -r _
                    ;;
                *)
                    printf '%s\n' '{"jsonrpc":"2.0","id":4,"error":{"code":-32600,"message":"Invalid request: missing field `threadId`"}}'
                    read -r _
                    printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"text":"OLD"}}'
                    printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"th_stop","turn":{"id":"turn_1","status":"completed"}}}'
                    ;;
            esac
            printf '%s\n' '{"jsonrpc":"2.0","id":5,"result":{"turn":{"id":"turn_2"}}}'
            printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"text":"NEW"}}'
            printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"th_stop","turn":{"id":"turn_2","status":"completed"}}}'
            sleep 0.3
        "#
    }

    #[tokio::test]
    async fn stopped_turn_does_not_leak_into_the_next_one() {
        let cfg = config();
        let approvals = ApprovalRouter::new();
        let cancel = Arc::new(Notify::new());
        let mut bridge = fake_bridge(interrupt_script()).await;
        bridge.warm(&cfg.workspace.working_dir).await.expect("warm");
        let (mut seq, mut started) = (0u64, false);

        let cancel2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            cancel2.notify_waiters();
        });

        let mut statuses = Vec::new();
        let mut sink = VecSink::default();
        for content in ["slow", "next"] {
            sink.frames.clear();
            let terminal = tokio::time::timeout(
                Duration::from_secs(3),
                drive_turn(
                    DriveCtx {
                        chat_session_id: Uuid::new_v4(),
                        message_id: Uuid::new_v4(),
                        content: content.into(),
                        guidance: None,
                        model: None,
                        mode: ApprovalMode::Ask,
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
            .expect("turn should not hang")
            .expect("drive turn");
            match terminal {
                Response::ChatMessageCompleted { status, .. } => statuses.push(status),
                other => panic!("expected ChatMessageCompleted, got {other:?}"),
            }
        }
        assert_eq!(statuses, ["cancelled", "completed"]);

        // Only the second turn's own reply may reach its stream.
        let deltas: Vec<&str> = sink
            .frames
            .iter()
            .filter_map(|f| match f {
                Response::ChatEvent { kind, payload, .. } if kind == "assistant_delta" => {
                    payload["params"]["text"].as_str()
                }
                _ => None,
            })
            .collect();
        assert_eq!(deltas, ["NEW"], "frames: {:?}", sink.frames);
    }

    /// A fake app-server whose gateway answers `status` to the first turn: it
    /// announces a retry the way the bundled engine does and, unless it is
    /// interrupted first, goes on to announce a second one (`2/5`).
    fn gateway_refusal_script(status: u16, body: &str) -> String {
        let retry = |attempt: u8| {
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "error",
                "params": {
                    "error": {
                        "message": format!("Reconnecting... {attempt}/5"),
                        "codexErrorInfo": {"responseStreamDisconnected": {"httpStatusCode": status}},
                        "additionalDetails": format!(
                            "unexpected status {status} {body}, url: https://gateway.test/v1/responses, request id: req_1"
                        ),
                    },
                    "willRetry": true,
                    "threadId": "th_refused",
                    "turnId": "turn_1",
                },
            })
        };
        format!(
            r#"
            read -r _
            printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"capabilities":{{}}}}}}'
            read -r _
            read -r _
            printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"threadId":"th_refused"}}}}'
            read -r _
            printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"turn":{{"id":"turn_1"}}}}}}'
            printf '%s\n' '{{"jsonrpc":"2.0","method":"turn/started","params":{{"threadId":"th_refused","turn":{{"id":"turn_1"}}}}}}'
            printf '%s\n' '{first}'
            read -r interrupt
            case "$interrupt" in
                *'"method":"turn/interrupt"'*'"turnId":"turn_1"'*)
                    printf '%s\n' '{{"jsonrpc":"2.0","id":4,"result":{{}}}}'
                    printf '%s\n' '{{"jsonrpc":"2.0","method":"turn/completed","params":{{"threadId":"th_refused","turn":{{"id":"turn_1","status":"interrupted"}}}}}}'
                    read -r _
                    printf '%s\n' '{{"jsonrpc":"2.0","id":5,"result":{{"turn":{{"id":"turn_2"}}}}}}'
                    printf '%s\n' '{{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{{"text":"NEW"}}}}'
                    printf '%s\n' '{{"jsonrpc":"2.0","method":"turn/completed","params":{{"threadId":"th_refused","turn":{{"id":"turn_2","status":"completed"}}}}}}'
                    ;;
                *)
                    printf '%s\n' '{second}'
                    ;;
            esac
            sleep 0.3
        "#,
            first = retry(1),
            second = retry(2),
        )
    }

    /// Drive one turn against `bridge` and return its terminal frame plus
    /// everything streamed before it.
    async fn run_turn(bridge: &mut AgentBridge, content: &str) -> (Response, Vec<Response>) {
        let cfg = config();
        let approvals = ApprovalRouter::new();
        let cancel = Notify::new();
        let mut sink = VecSink::default();
        let (mut seq, mut started) = (0u64, true);
        let terminal = tokio::time::timeout(
            Duration::from_secs(3),
            drive_turn(
                DriveCtx {
                    chat_session_id: Uuid::new_v4(),
                    message_id: Uuid::new_v4(),
                    content: content.into(),
                    guidance: None,
                    model: None,
                    mode: ApprovalMode::Ask,
                    runner_id: cfg.runner_id,
                    config: &cfg,
                    approvals: &approvals,
                    workspace: &cfg.workspace.working_dir,
                    cancel: &cancel,
                },
                bridge,
                &mut seq,
                &mut started,
                &mut sink,
            ),
        )
        .await
        .expect("turn should not hang")
        .expect("drive turn");
        (terminal, sink.frames)
    }

    fn raw_methods(frames: &[Response]) -> Vec<String> {
        frames
            .iter()
            .filter_map(|f| match f {
                Response::ChatEvent { payload, .. } => {
                    payload["method"].as_str().map(str::to_owned)
                }
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn out_of_credit_stops_the_turn_before_the_engine_retries() {
        let cfg = config();
        let body = "Payment Required: Daily free quota used and LLM credit balance is insufficient; add credit or upgrade your plan.";
        let mut bridge = fake_bridge(&gateway_refusal_script(402, body)).await;
        bridge.warm(&cfg.workspace.working_dir).await.expect("warm");

        let (terminal, frames) = run_turn(&mut bridge, "hello").await;
        match terminal {
            Response::ChatFailed { code, detail, .. } => {
                assert_eq!(code, CHAT_MODEL_CREDIT_EXHAUSTED);
                assert_eq!(
                    detail.as_deref(),
                    Some(
                        "Daily free quota used and LLM credit balance is insufficient; add credit or upgrade your plan."
                    ),
                );
            }
            other => panic!("expected ChatFailed, got {other:?}"),
        }
        // Neither the retry notice nor its URL / request id reaches the client.
        assert!(
            !raw_methods(&frames).contains(&"error".to_string()),
            "frames: {frames:?}"
        );

        // The engine was interrupted (the fake only answers a second turn if
        // it was), and the stopped turn does not leak into the next one.
        let (terminal, frames) = run_turn(&mut bridge, "again").await;
        assert!(
            matches!(&terminal, Response::ChatMessageCompleted { status, .. } if status == "completed"),
            "second turn: {terminal:?} frames: {frames:?}"
        );
        assert!(
            !raw_methods(&frames).contains(&"error".to_string()),
            "frames: {frames:?}"
        );
    }

    #[tokio::test]
    async fn rejected_credential_stops_the_turn_as_an_auth_failure() {
        let cfg = config();
        let mut bridge =
            fake_bridge(&gateway_refusal_script(401, "Unauthorized: Invalid JWT")).await;
        bridge.warm(&cfg.workspace.working_dir).await.expect("warm");

        let (terminal, frames) = run_turn(&mut bridge, "hello").await;
        match terminal {
            Response::ChatFailed { code, detail, .. } => {
                assert_eq!(code, CHAT_MODEL_AUTH_FAILED);
                assert_eq!(detail.as_deref(), Some("Invalid JWT"));
            }
            other => panic!("expected ChatFailed, got {other:?}"),
        }
        assert!(
            !raw_methods(&frames).contains(&"error".to_string()),
            "frames: {frames:?}"
        );
    }

    #[test]
    fn only_refusals_the_engine_would_retry_in_vain_are_recognised() {
        let notice = |status: u64, will_retry: bool| {
            serde_json::json!({
                "error": {
                    "message": "Reconnecting... 1/5",
                    "codexErrorInfo": {"responseStreamDisconnected": {"httpStatusCode": status}},
                    "additionalDetails": format!("unexpected status {status} Whatever: body text, url: http://x/v1/responses"),
                },
                "willRetry": will_retry,
            })
        };
        assert_eq!(
            gateway_chat_refusal("error", &notice(402, true)),
            Some((CHAT_MODEL_CREDIT_EXHAUSTED, "body text".to_string()))
        );
        assert_eq!(
            gateway_chat_refusal("error", &notice(401, true)),
            Some((CHAT_MODEL_AUTH_FAILED, "body text".to_string()))
        );
        // A server error or a rate limit may well succeed on retry.
        assert_eq!(gateway_chat_refusal("error", &notice(503, true)), None);
        assert_eq!(gateway_chat_refusal("error", &notice(429, true)), None);
        // The terminal notice is the bridge's to turn into a failure.
        assert_eq!(gateway_chat_refusal("error", &notice(402, false)), None);
        // A dropped stream carries no HTTP status at all.
        let dropped = serde_json::json!({
            "error": {
                "message": "Reconnecting... 1/5",
                "codexErrorInfo": {"responseStreamDisconnected": {"httpStatusCode": null}},
                "additionalDetails": "stream disconnected before completion: Incomplete response returned, reason: max_output_tokens",
            },
            "willRetry": true,
        });
        assert_eq!(gateway_chat_refusal("error", &dropped), None);
        assert_eq!(gateway_chat_refusal("warning", &notice(402, true)), None);

        assert_eq!(
            gateway_status_text(
                "unexpected status 402 Payment Required: add credit, url: https://g/v1/responses, request id: r1"
            ),
            Some((402, "add credit".to_string()))
        );
        assert_eq!(
            gateway_status_text("stream disconnected before completion"),
            None
        );
    }

    #[tokio::test]
    async fn refusal_the_engine_gave_up_on_keeps_its_code() {
        // No retry notice at all: the engine fails the turn itself, as it does
        // once its retries are spent.
        let script = r#"
            read -r _
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
            read -r _
            read -r _
            printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_final"}}'
            read -r _
            printf '%s\n' '{"jsonrpc":"2.0","method":"error","params":{"error":{"message":"unexpected status 402 Payment Required: add credit, url: https://g/v1/responses","codexErrorInfo":"other"},"willRetry":false}}'
            sleep 0.3
        "#;
        let cfg = config();
        let mut bridge = fake_bridge(script).await;
        bridge.warm(&cfg.workspace.working_dir).await.expect("warm");
        let (terminal, _) = run_turn(&mut bridge, "hello").await;
        match terminal {
            Response::ChatFailed { code, detail, .. } => {
                assert_eq!(code, CHAT_MODEL_CREDIT_EXHAUSTED);
                assert_eq!(detail.as_deref(), Some("add credit"));
            }
            other => panic!("expected ChatFailed, got {other:?}"),
        }
    }

    /// A fake app-server that warms, starts a turn, then requests a command
    /// approval and hangs — the turn can only end via the approval wait's
    /// cancel arm or its TTL backstop, never on its own.
    fn approval_hang_script() -> &'static str {
        r#"
            set -e
            read _
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
            read _
            read _
            printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_appr"}}'
            read _
            printf '%s\n' '{"jsonrpc":"2.0","method":"item/commandExecution/requestApproval","params":{"approval_id":"appr1","command":"danger-cmd","cwd":"/tmp","item_id":"i1","reason":"needs approval"}}'
            sleep 5
        "#
    }

    #[tokio::test]
    async fn abandoned_approval_is_interrupted_by_cancel() {
        // H1: an outstanding approval must not park the turn forever. A cancel
        // arriving while the turn waits on the approval must unwind it — before
        // this fix the wait loop had no cancel arm and would hang.
        let cfg = config();
        let approvals = ApprovalRouter::new();
        let cancel = Arc::new(Notify::new());
        let mut bridge = fake_bridge(approval_hang_script()).await;
        bridge.warm(&cfg.workspace.working_dir).await.expect("warm");

        let cancel2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
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
                    content: "run it".into(),
                    guidance: None,
                    model: None,
                    mode: ApprovalMode::Ask,
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
        .expect("turn must not park on an abandoned approval")
        .expect("drive turn");

        // The approval surfaced to the UI first...
        assert!(
            sink.frames
                .iter()
                .any(|f| matches!(f, Response::ChatApprovalRequest { .. })),
            "missing ChatApprovalRequest: {:?}",
            sink.frames
        );
        // ...and the cancel unwound the parked turn as cancelled.
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
            machine_context_dir: base.join("config"),
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

    /// Review item 1 on #3499: a finished run whose cleanup is unresolved
    /// may still have processes using the working copy, so a desktop chat
    /// turn must not claim it until cleanup resolves.
    #[tokio::test]
    async fn local_turn_guard_refuses_while_run_cleanup_is_unresolved() {
        let inst = offline_instance();
        inst.state.set_cleanup_reserved(true).await;
        assert!(try_begin_local_turn(&inst).is_err());
        assert!(!inst.chat_active.load(Ordering::SeqCst));
        inst.state.set_cleanup_reserved(false).await;
        let _g = try_begin_local_turn(&inst).expect("claim after cleanup resolves");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn process_alive(pid: u32) -> bool {
        crate::util::procinfo::info(pid as i32).is_some_and(|p| !p.zombie)
    }

    /// A session whose unit holds a process its agent left running.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    async fn session_with_leftover(registry: &ChatRegistry) -> (Arc<ChatSession>, u32) {
        let id = Uuid::new_v4();
        let mut unit = ChatUnit::new(id);
        let mut leftover = unit
            .scoped(async {
                crate::util::shell::login_shell_command("sleep", &["300"], None)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .await;
        let pid = leftover.id().unwrap();
        unit.adopt(Some(pid));
        tokio::spawn(async move { leftover.wait().await });
        let session = registry.get_or_create(id).await;
        session.rt.lock().await.unit = Some(unit);
        (session, pid)
    }

    /// PDASHOSS01-417 (#2952 review item 3): an idle local chat session's
    /// processes are part of the working-copy handoff. A run may take the
    /// working copy only once they are stopped; the session itself stays,
    /// so the next turn resumes its thread.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn release_idle_stops_what_an_idle_local_session_left_running() {
        let registry = ChatRegistry::new();
        let (session, pid) = session_with_leftover(&registry).await;
        assert!(registry.release_idle().await);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while process_alive(pid) && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            !process_alive(pid),
            "the idle session's process outlived the handoff"
        );
        assert!(session.rt.lock().await.unit.is_none());
        assert_eq!(registry.sessions.lock().await.len(), 1, "session dropped");
    }

    /// A session with a warm or turn in flight holds the working copy: the
    /// handoff refuses and leaves its processes alone.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn release_idle_refuses_while_a_local_session_is_busy() {
        let registry = ChatRegistry::new();
        let (session, pid) = session_with_leftover(&registry).await;
        let busy = session.rt.lock().await;
        let released = registry.release_idle().await;
        let alive = process_alive(pid);
        drop(busy);
        assert!(
            !released,
            "handed over while a local turn holds the session"
        );
        assert!(alive, "a busy session's processes were stopped");
        assert!(registry.release_idle().await);
    }

    /// A closed session whose processes cannot be proven gone keeps the
    /// working copy held, and nothing unproven is signalled.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn release_idle_refuses_while_a_closed_sessions_processes_are_unresolved() {
        use std::os::unix::process::CommandExt;
        let mut stranger = std::process::Command::new("sleep")
            .arg("300")
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = stranger.id();
        let info = crate::util::procinfo::info(pid as i32).unwrap();
        let recorded = crate::util::procinfo::ProcInfo {
            start: info.start - 1,
            ..info
        };
        let registry = ChatRegistry::new();
        let mut rt = SessionRuntime {
            unit: Some(ChatUnit::from_group(
                crate::util::containment::RunGroup::claimed_for_test(&recorded),
            )),
            ..SessionRuntime::default()
        };
        registry.end_runtime(&mut rt, Duration::from_secs(1)).await;
        let released = registry.release_idle().await;
        let alive = process_alive(pid);
        let _ = stranger.kill();
        let _ = stranger.wait();
        assert!(
            !released,
            "handed over while a closed session is unresolved"
        );
        assert!(alive, "an unproven group was signalled");
        assert!(
            registry.release_idle().await,
            "gone processes still hold the copy"
        );
    }

    /// An idle session being evicted keeps the working copy held until its
    /// processes are stopped: while its runtime is ending outside the map,
    /// the handoff must still see it.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn release_idle_refuses_while_an_evicted_session_is_still_ending() {
        let registry = ChatRegistry::new();
        let id = Uuid::new_v4();
        let mut unit = ChatUnit::new(id);
        // Ignores TERM, so its unit's cleanup lasts the whole grace.
        let mut leftover = unit
            .scoped(async {
                crate::util::shell::login_shell_command(
                    "sh",
                    &["-c", "trap '' TERM; exec sleep 300"],
                    None,
                )
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap()
            })
            .await;
        let pid = leftover.id().unwrap();
        unit.adopt(Some(pid));
        tokio::spawn(async move { leftover.wait().await });
        let session = registry.get_or_create(id).await;
        {
            let mut rt = session.rt.lock().await;
            rt.unit = Some(unit);
            rt.last_used_at = None; // past its idle TTL
        }
        // Let the shell reach its `trap` before the eviction signals it.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let reaper = registry.clone();
        let reap = tokio::spawn(async move { reaper.reap_idle(Uuid::new_v4(), 8).await });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while registry.contains(id).await && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!registry.contains(id).await, "the session was not evicted");
        let released = registry.release_idle().await;
        let alive = process_alive(pid);
        reap.await.unwrap();
        assert!(alive, "the eviction ended before the handoff check");
        assert!(
            !released,
            "handed over while an evicted session's processes were still running"
        );
        assert!(
            registry.release_idle().await,
            "the ended session still holds the copy"
        );
    }

    /// A warm starts an agent in the working copy, so it is refused while
    /// an issue run (or its unresolved cleanup) holds the copy, like a turn.
    #[tokio::test]
    async fn warm_is_refused_while_an_issue_run_holds_the_working_copy() {
        let inst = offline_instance();
        inst.state.set_cleanup_reserved(true).await;
        let host = ChatHost::runner(&inst);
        let id = Uuid::new_v4();
        let mut sink = VecSink::default();
        let response = handle_warm(
            &host,
            WarmArgs {
                chat_session_id: id,
                cwd: None,
                model: None,
                mode: None,
                local_thread_id: None,
                local_session_id: None,
            },
            &mut sink,
        )
        .await
        .unwrap();
        assert!(matches!(response, Response::Ack));
        let detail = sink.frames.iter().find_map(|f| match f {
            Response::ChatEvent { kind, payload, .. } if kind == "chat_warm_failed" => {
                payload["detail"].as_str().map(str::to_owned)
            }
            _ => None,
        });
        assert!(
            detail.as_deref().is_some_and(|d| d.contains("cleaned up")),
            "warm was not refused as busy: {:?}",
            sink.frames
        );
        if let Some(session) = inst.chat_sessions.get(id).await {
            let rt = session.rt.lock().await;
            assert!(
                rt.bridge.is_none() && rt.unit.is_none(),
                "warm started an agent"
            );
        }
        assert!(!inst.chat_active.load(Ordering::SeqCst));
    }

    #[test]
    fn resolve_chat_workspace_confines_to_workspace() {
        let cfg = config();
        std::fs::create_dir_all(&cfg.workspace.working_dir).unwrap();
        // An absolute path is the caller's own working copy (the desktop's
        // per-account chat tree is a sibling of the runner's `workdirs`), so it
        // is accepted when it exists...
        let outside = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_chat_workspace(&cfg, Some(outside.path().to_str().unwrap())).unwrap(),
            outside.path()
        );
        // ...and refused when it does not, rather than being created blind.
        assert!(
            resolve_chat_workspace(&cfg, Some(&format!("{}/nope", outside.path().display())))
                .is_err()
        );
        // No cwd resolves to the workspace root itself.
        assert_eq!(
            resolve_chat_workspace(&cfg, None).unwrap(),
            cfg.workspace.working_dir
        );
        // A relative subdir under the workspace is accepted and stays under it.
        let ok = resolve_chat_workspace(&cfg, Some("sub")).unwrap();
        assert!(ok.starts_with(&cfg.workspace.working_dir));
        // `..` traversal must be refused: a lexical `starts_with` check would
        // otherwise accept `<workspace>/../escape`, which resolves outside.
        assert!(resolve_chat_workspace(&cfg, Some("../escape")).is_err());
        assert!(resolve_chat_workspace(&cfg, Some("sub/../../escape")).is_err());
    }

    #[tokio::test]
    async fn warmed_chat_sessions_are_reaped_once_the_registry_grows() {
        // PIDESKAPP-75: every local chat the user views calls `chat_warm`,
        // which spawns a long-lived engine process keyed by `chat_session_id`
        // and nothing ever reaped the entries. N distinct warms must not
        // leave N live sessions: the least-recently-used ones are evicted
        // (their bridges torn down) once the registry passes its cap, and
        // the next warm/send respawns them.
        let registry = ChatRegistry::new();
        let first = Uuid::new_v4();
        registry.get_or_create(first).await;
        // View more chats than the live cap allows.
        for _ in 0..12 {
            let id = Uuid::new_v4();
            registry.get_or_create(id).await;
        }
        assert!(
            !registry.contains(first).await,
            "the least-recently-warmed chat session must be evicted once the registry exceeds its cap"
        );
    }

    #[tokio::test]
    async fn a_session_with_a_turn_in_flight_is_never_reaped() {
        // A warm/turn holds the session's runtime lock; the reaper must
        // leave that session alone no matter how far past the cap the
        // registry grows.
        let registry = ChatRegistry::new();
        let busy = Uuid::new_v4();
        let busy_session = registry.get_or_create(busy).await;
        // Simulate an in-flight warm/turn holding the runtime lock.
        let _in_flight = busy_session.rt.lock().await;
        for _ in 0..12 {
            let id = Uuid::new_v4();
            registry.get_or_create(id).await;
        }
        drop(_in_flight);
        assert!(
            registry.contains(busy).await,
            "a session with a turn in flight must never be reaped"
        );
    }

    #[tokio::test]
    async fn an_idle_session_is_reaped_on_the_next_warm() {
        // A session left untouched past the idle TTL must go on the next
        // registry access, even before the cap is reached.
        let registry = ChatRegistry::new();
        let stale = Uuid::new_v4();
        let stale_session = registry.get_or_create(stale).await;
        {
            let mut rt = stale_session.rt.lock().await;
            rt.last_used_at = Some(Utc::now() - chrono::Duration::minutes(10 + 1)); // runner-lane idle TTL, see get_or_create
        }
        registry.get_or_create(Uuid::new_v4()).await;
        assert!(
            !registry.contains(stale).await,
            "a session idle past its TTL must be reaped"
        );
    }

    /// After the reaper drops a session, the next warm/send respawns a
    /// fresh bridge that resumes the saved Codex thread (PIDESKAPP-74 path),
    /// so the conversation continues on the same thread id. This is the
    /// thread-continuity half of PIDESKAPP-75's acceptance: without
    /// `thread/resume` the respawned bridge minted a new id and the engine
    /// lost all prior turns.
    #[tokio::test]
    async fn reaped_session_respawn_resumes_saved_thread() {
        // Fake app-server: expects thread/resume with the saved id, not
        // thread/start.
        let script = r#"
            set -e
            read _
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
            read _
            read resume
            case "$resume" in *'"method":"thread/resume"'*) ;; *) printf '%s\n' '{"jsonrpc":"2.0","id":2,"error":{"code":-32600,"message":"expected thread/resume"}}'; exit 0;; esac
            case "$resume" in *'"threadId":"th_saved"'*) ;; *) printf '%s\n' '{"jsonrpc":"2.0","id":2,"error":{"code":-32600,"message":"expected th_saved"}}'; exit 0;; esac
            printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"th_saved"}}}'
            sleep 0.1
        "#;
        // Simulate a reaped session: the registry entry is gone, the next
        // warm for the same chat_session_id spawns a fresh bridge carrying
        // the saved thread id from the desktop. The bridge must resume it.
        let registry = ChatRegistry::new();
        let chat_id = Uuid::new_v4();
        let old = registry.get_or_create(chat_id).await;
        {
            let mut rt = old.rt.lock().await;
            rt.last_used_at = Some(Utc::now() - chrono::Duration::minutes(10 + 1)); // runner-lane idle TTL, see get_or_create
        }
        // Trigger reap on a different id so `chat_id` is dropped.
        registry.get_or_create(Uuid::new_v4()).await;
        assert!(
            !registry.contains(chat_id).await,
            "stale session must be reaped before respawn"
        );
        // Respawn: fresh registry entry with a new Arc (old was dropped).
        let fresh = registry.get_or_create(chat_id).await;
        assert!(
            registry.contains(chat_id).await,
            "respawned session must be live"
        );
        assert!(
            !Arc::ptr_eq(&old, &fresh),
            "respawn must allocate a fresh runtime, not resurrect the reaped one"
        );
        // Fresh bridge carrying the saved thread must resume it.
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        let server = AppServer::spawn_command(cmd)
            .await
            .expect("spawn fake codex");
        let mut bridge = Bridge::from_server(server, None);
        bridge.set_resume_thread_id(Some("th_saved"));
        let cwd = std::env::temp_dir();
        let thread_id = bridge.warm(&cwd).await.expect("respawn must resume");
        assert_eq!(
            thread_id, "th_saved",
            "post-reap warm must resume, not restart"
        );
    }

    /// Concurrent `get_or_create` for the same `chat_session_id` must share
    /// a single `ChatSession` Arc — the reaper's blind-reinsert race
    /// produced two live runtimes for one chat when a reap overlapped a
    /// warm. The atomic ptr_eq removal keeps the registry single-sourced.
    #[tokio::test]
    async fn concurrent_get_or_create_shares_single_runtime() {
        let registry = ChatRegistry::new();
        let id = Uuid::new_v4();
        let mut handles = Vec::new();
        for _ in 0..8 {
            let r = registry.clone();
            handles.push(tokio::spawn(async move { r.get_or_create(id).await }));
        }
        let mut arcs = Vec::new();
        for h in handles {
            arcs.push(h.await.expect("spawned get_or_create"));
        }
        for a in arcs.iter().skip(1) {
            assert!(
                Arc::ptr_eq(&arcs[0], a),
                "concurrent get_or_create must share one runtime"
            );
        }
        // And the concurrently obtained Arc must match a fresh lookup.
        let c = registry.get(id).await.expect("concurrent session present");
        assert!(
            Arc::ptr_eq(&arcs[0], &c),
            "concurrent get_or_create must not duplicate the runtime"
        );
    }
}
