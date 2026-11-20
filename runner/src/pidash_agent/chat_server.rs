//! One shared engine app-server for Pi Dash Agent chat (NEWPIDASH-333, part
//! of NEWPIDASH-328).
//!
//! Every desktop "This computer" chat used to spawn its own
//! `pidash-agent-engine` process and pay `initialize` per chat. This module
//! owns a single long-lived `pidash-agent-engine app-server` child and its
//! one stdio JSON-RPC connection; every chat session is a thread inside it.
//!
//! - [`EngineChatServer`] is the cloneable handle the IPC lane calls.
//!   Sessions are `chat_session_id → thread_id`; warm resumes (falling back
//!   to start, since the engine keeps no rollout for zero-turn threads —
//!   spike S1) and send runs a turn, both routed by `threadId`.
//! - One background task owns the child: it multiplexes RPCs by request id
//!   and fans notifications/requests out to per-thread subscriptions. Turn
//!   drivers never touch the child directly.
//! - Lifecycle: start on first warm/send when the engine can run; stop on
//!   sign-out, credential revoke, pause and daemon shutdown; restart with
//!   backoff on crash; restart-when-idle on model profile change (O4); at
//!   most [`MAX_RUNNING_CHAT_TURNS`] running turns, a send beyond it fails
//!   with `engine_busy` and no queue (O5).
//!
//! AgentRun never uses this component: every run keeps its own engine
//! process, `PIDASH_RUN_ID` and containment.

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use std::sync::Mutex as StdMutex;
use tokio::sync::{Mutex, Notify, mpsc, oneshot};
use uuid::Uuid;

use crate::agent::{ENGINE_STARTUP_TIMEOUT, ExitSnapshot, within_startup_deadline};
use crate::approval::engine_thread_settings;
use crate::cloud::protocol::{ApprovalDecision, ApprovalKind, ApprovalMode};
use crate::codex::app_server::AppServer;
use crate::codex::bridge::BridgeCursor;
use crate::codex::gateway::{
    CHAT_AGENT_FAILED, assistant_text_from_done_payload, gateway_chat_refusal,
    is_assistant_text_delta,
};
use crate::codex::jsonrpc::{self, Incoming, RpcId};
use crate::codex::schema::{
    ApprovalResponseParams, ClientInfo, InitializeParams, ThreadResumeParams, ThreadStartParams,
    TurnInputItem, TurnInterruptParams, TurnStartParams,
};

/// O5: at most this many running turns across every session of the shared
/// server. Open, idle sessions do not count. A send beyond the cap fails
/// with `chat_failed { code: "engine_busy" }`; there is no queue. A daemon
/// constant, not user configuration.
pub const MAX_RUNNING_CHAT_TURNS: usize = 5;

/// Sessions idle longer than this are unsubscribed so the engine can unload
/// their thread. The `chat_session_id → thread_id` mapping is kept, so the
/// next warm resumes the thread with `thread/resume`.
pub const CHAT_THREAD_IDLE_UNSUBSCRIBE: Duration = Duration::from_secs(30 * 60);

/// Backoff after a failed start or crash: `1s, 2s, 4s, …` capped here.
const RESTART_BACKOFF_INITIAL: Duration = Duration::from_secs(1);
const RESTART_BACKOFF_MAX: Duration = Duration::from_secs(30);

/// `ChatFailed.code` when a send arrives while five turns already run.
pub const ENGINE_BUSY: &str = "engine_busy";
/// `ChatFailed.code` failing the turns in flight when the child dies.
pub const ENGINE_RESTARTED: &str = "engine_restarted";
/// `ChatFailed.code` failing turns when the engine is signed out.
pub const ENGINE_SIGNED_OUT: &str = "engine_signed_out";
/// `ChatFailed.code` failing turns while the engine is paused.
pub const ENGINE_PAUSED: &str = "engine_paused";
/// `ChatFailed.code` failing turns on daemon shutdown.
pub const DAEMON_STOPPING: &str = "daemon_stopping";
/// `ChatFailed.code` when the child cannot be started.
pub const ENGINE_START_FAILED: &str = "engine_start_failed";

// ---------------------------------------------------------------------------
// Injection points: spawning and readiness.
// ---------------------------------------------------------------------------

/// Whether the engine can run right now, as the daemon sees it.
#[derive(Debug, Clone)]
pub struct ChatAvailability {
    pub can_run: bool,
    /// `ChatFailed.code` (or the engine's own reason code) when `!can_run`.
    pub code: String,
}

/// Decides whether the shared child may run. Built by the daemon from the
/// engine manager; tests return canned values.
pub type AvailabilitySource = Arc<dyn Fn() -> ChatAvailability + Send + Sync>;

pub(crate) type SpawnFuture = Pin<Box<dyn Future<Output = Result<AppServer>> + Send>>;

/// Spawns the shared child. Production spawns `pidash-agent-engine
/// app-server` the way `pidash_agent::agent::launch` does (binary, CODEX_HOME,
/// auth helper); tests substitute a fake app-server.
pub type SpawnSource = Arc<dyn Fn() -> SpawnFuture + Send + Sync>;

/// Spawn the production shared child: the pinned engine binary with the
/// daemon-supplied `CODEX_HOME` and auth helper, exactly as an engine runner
/// launches (via [`crate::pidash_agent::agent::prepare`]).
pub fn spawn_production_child(config: crate::config::schema::RunnerConfig) -> SpawnSource {
    Arc::new(move || {
        let config = config.clone();
        Box::pin(async move {
            let prepared = crate::pidash_agent::agent::prepare(&config).with_context(|| {
                format!(
                    "{}: the Pi Dash Agent cannot start shared chat",
                    super::api::reason::PROFILE_PENDING
                )
            })?;
            let env = crate::agent::agent_env_for_config(&prepared);
            let codex_home = env
                .codex_home
                .clone()
                .context("engine launch has no CODEX_HOME")?;
            crate::util::fs::create_dir_all_private(&codex_home)?;
            AppServer::spawn_with_env(&prepared.codex.binary, &codex_home, &env)
                .await
                .context("spawning pidash-agent-engine app-server")
        }) as SpawnFuture
    })
}

/// Lifecycle hooks the engine manager calls: stop on sign-out/revoke/pause,
/// restart-when-idle on profile change. Implemented by [`EngineChatServer`];
/// the manager holds it as a trait object so this module never names the
/// manager type.
pub trait ServerLifecycle: Send + Sync {
    /// Re-read readiness: stop the child when the engine cannot run, clear
    /// a previous stop when it can again.
    fn refresh_availability(&self);
    /// The model profile changed: restart the child now when no turn runs,
    /// otherwise restart once the running turns drain (O4).
    fn note_profile_changed(&self);
}

// ---------------------------------------------------------------------------
// Public handle.
// ---------------------------------------------------------------------------

/// What `warm` needs: the folder the thread runs in, the approval posture
/// captured for a new thread, and a previously saved thread to resume.
pub struct WarmSpec {
    pub workspace: PathBuf,
    pub mode: ApprovalMode,
    pub resume_id: Option<String>,
}

/// What `send` needs beyond [`WarmSpec`]: the user turn.
pub struct SendSpec {
    pub workspace: PathBuf,
    pub mode: ApprovalMode,
    pub resume_id: Option<String>,
    pub message_id: Uuid,
    pub content: String,
    pub model: Option<String>,
}

/// The terminal outcome of a shared turn, mapped to `ChatMessageCompleted`
/// / `ChatFailed` by the IPC lane.
#[derive(Debug)]
pub enum TurnTerminal {
    Completed {
        turn_id: Option<String>,
        assistant_message: Option<String>,
        status: String,
    },
    Failed {
        code: String,
        detail: Option<String>,
    },
}

/// An engine approval request surfaced mid-turn. Answered by the caller
/// (policy + local approval router live in the IPC lane).
pub struct ApprovalRequest {
    pub approval_id: String,
    pub kind: ApprovalKind,
    pub payload: serde_json::Value,
    pub reason: Option<String>,
}

/// What the caller's wait for an approval decision concluded.
pub enum ApprovalOutcome {
    Decided(ApprovalDecision),
    /// The turn was cancelled, the approval expired, or the router went
    /// away while parked. The driver interrupts the turn; the outcome
    /// carries the terminal failure, if any.
    TurnCancelled,
    TurnFailed {
        code: String,
        detail: Option<String>,
    },
}

/// The caller's side of a streaming turn. Implemented once, in the IPC lane,
/// so desktop chat frames stay byte-identical to the per-session lane.
pub trait TurnSink: Send {
    /// A warm found or created the thread; `seq` is the session's frame
    /// counter after this event.
    fn warmed(&mut self, thread_id: &str, seq: u64) -> impl Future<Output = Result<()>> + Send;
    /// A warm failed; the handler answers `Ack` after this. `seq` is the
    /// session's frame counter, unchanged by the failed warm.
    fn warm_failed(&mut self, detail: String, seq: u64) -> impl Future<Output = Result<()>> + Send;
    /// First frame for a session: mirrors `ChatStarted`.
    fn thread_started(&mut self, thread_id: &str) -> impl Future<Output = Result<()>> + Send;
    /// Mirrors `ChatMessageStarted`.
    fn message_started(&mut self, turn_id: &str) -> impl Future<Output = Result<()>> + Send;
    /// A streamed event: `assistant_delta` or `raw`, with the engine's
    /// `{ method, params }` payload. `seq` is the session's frame counter.
    fn event(
        &mut self,
        seq: u64,
        kind: &str,
        payload: serde_json::Value,
    ) -> impl Future<Output = Result<()>> + Send;
    /// An engine approval request. Blocks until the caller decides (policy,
    /// router, TTL) or the turn ends another way.
    fn decide_approval(
        &mut self,
        req: ApprovalRequest,
    ) -> impl Future<Output = ApprovalOutcome> + Send;
}

/// No-op sink for tests that only assert terminal outcomes.
pub struct NullSink;

impl TurnSink for NullSink {
    async fn warmed(&mut self, _thread_id: &str, _seq: u64) -> Result<()> {
        Ok(())
    }
    async fn warm_failed(&mut self, _detail: String, _seq: u64) -> Result<()> {
        Ok(())
    }
    async fn thread_started(&mut self, _thread_id: &str) -> Result<()> {
        Ok(())
    }
    async fn message_started(&mut self, _turn_id: &str) -> Result<()> {
        Ok(())
    }
    async fn event(&mut self, _seq: u64, _kind: &str, _payload: serde_json::Value) -> Result<()> {
        Ok(())
    }
    async fn decide_approval(&mut self, _req: ApprovalRequest) -> ApprovalOutcome {
        ApprovalOutcome::Decided(ApprovalDecision::Decline)
    }
}

// ---------------------------------------------------------------------------
// Shared state: one connection, many sessions.
// ---------------------------------------------------------------------------

/// A live session: which thread it owns plus per-session framing state.
/// The entry mutex is held across a whole warm/send so two sends on one
/// session serialize, as with the per-session lane's runtime lock.
struct SessionEntry {
    mutex: Mutex<SessionData>,
    /// Tripped by `cancel`/`close` (possibly from another connection).
    /// Warms in flight and running turns select on this.
    cancel: Arc<Notify>,
    /// Sticky cancel: set alongside the notify. `notify_waiters` wakes only
    /// waiters parked at that instant, so a cancel that lands before the
    /// turn parks its waiter would otherwise be lost and the turn would run
    /// to completion while `cancel()` waits on its session lock. Drivers
    /// check this before parking, so an early cancel is never missed.
    cancelled: AtomicBool,
}

struct SessionData {
    thread_id: Option<String>,
    /// The approval mode this session's thread was started under. Captured
    /// once, on the first warm, and held for the thread's life so a mode
    /// change mid-session cannot retune a turn in flight.
    approval_mode: ApprovalMode,
    workspace: Option<PathBuf>,
    /// Whether `thread_started` was already emitted for this session.
    started_reported: bool,
    /// The turn in flight, if any.
    running_turn_id: Option<String>,
    /// Monotonic frame counter for `ChatEvent.bridge_seq`, kept across
    /// turns of the session.
    seq: u64,
    last_used_at: Instant,
}

struct PendingRpc {
    reply: oneshot::Sender<Result<serde_json::Value>>,
    /// The method, so `turn/start` responses can index the new turn id.
    method: String,
    /// The thread the call targets, if any.
    thread_id: Option<String>,
}

struct Inner {
    spawn: SpawnSource,
    availability: AvailabilitySource,
    cmd_tx: mpsc::Sender<ConnCmd>,
    conn: Arc<Mutex<ConnShared>>,
    /// Backoff after failed starts: attempts since the last success. Zero
    /// backoff is injected by tests.
    backoff_initial: Duration,
    backoff_max: Duration,
    state: StdMutex<State>,
}

struct State {
    /// The connection task set this once `initialize` succeeds; cleared on
    /// crash and stop.
    up: bool,
    /// Sticky stop reason (`engine_signed_out`, `engine_paused`,
    /// `daemon_stopping`): set by `stop`/`refresh_availability`, cleared when
    /// the engine can run again. While set, warm/send fail without spawning.
    stopped_code: Option<String>,
    /// A profile change arrived while turns ran: restart once they drain.
    pending_restart: bool,
    failed_attempts: u32,
    backoff_until: Option<Instant>,
    running_turns: usize,
    sessions: HashMap<Uuid, Arc<SessionEntry>>,
    last_sweep_at: Option<Instant>,
}

impl State {
    fn backoff_for(&self, initial: Duration, max: Duration) -> Duration {
        let shift = self.failed_attempts.min(10);
        (initial * 2_u32.pow(shift)).min(max)
    }
}

enum ConnCmd {
    Start {
        reply: oneshot::Sender<Result<()>>,
    },
    /// Kill the child now, fail pending RPCs, drop subscriptions.
    StopServer,
    Rpc {
        id: u64,
        method: &'static str,
        params: serde_json::Value,
        reply: oneshot::Sender<Result<serde_json::Value>>,
    },
    /// A raw outbound line that expects no response (approval answers,
    /// `initialized`).
    SendRaw {
        line: String,
    },
    Subscribe {
        thread_id: String,
        tx: mpsc::UnboundedSender<Incoming>,
    },
}

/// The cloneable handle. All methods are safe to call concurrently; the one
/// background task owns the child.
#[derive(Clone)]
pub struct EngineChatServer {
    inner: Arc<Inner>,
}

impl EngineChatServer {
    /// Build the server and start its background task. The child itself is
    /// lazy: it spawns on the first warm/send that finds the engine runnable.
    pub fn new(spawn: SpawnSource, availability: AvailabilitySource) -> Self {
        Self::with_backoff(
            spawn,
            availability,
            RESTART_BACKOFF_INITIAL,
            RESTART_BACKOFF_MAX,
        )
    }

    pub(crate) fn with_backoff(
        spawn: SpawnSource,
        availability: AvailabilitySource,
        backoff_initial: Duration,
        backoff_max: Duration,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ConnCmd>(64);
        // Request ids start at 1: `start_child` uses 0 for `initialize`
        // while the connection loop is not yet routing.
        let shared = Arc::new(Mutex::new(ConnShared {
            next_id: 1,
            pending: HashMap::new(),
            subs: HashMap::new(),
            turns: HashMap::new(),
        }));
        let server = Self {
            inner: Arc::new(Inner {
                spawn,
                availability,
                cmd_tx,
                conn: shared.clone(),
                backoff_initial,
                backoff_max,
                state: StdMutex::new(State {
                    up: false,
                    stopped_code: None,
                    pending_restart: false,
                    failed_attempts: 0,
                    backoff_until: None,
                    running_turns: 0,
                    sessions: HashMap::new(),
                    last_sweep_at: None,
                }),
            }),
        };
        tokio::spawn(connection_task(cmd_rx, shared, server.inner.clone()));
        server
    }

    /// Whether `chat_session_id` is a session of this server (the IPC lane's
    /// `chat_target` check).
    pub async fn contains_session(&self, id: Uuid) -> bool {
        self.inner
            .state
            .lock()
            .expect("chat server state poisoned")
            .sessions
            .contains_key(&id)
    }

    /// The session's cancel signal, creating the session entry when needed.
    /// The IPC lane hands this to its per-request sink so a `ChatCancel`
    /// racing a warm or parked approval reaches the right wait.
    pub(crate) async fn session_cancel(&self, id: Uuid) -> Arc<Notify> {
        self.session(id).await.cancel.clone()
    }

    /// The folder the session's thread runs in, if it warmed one yet. The
    /// approval policy evaluates against this folder.
    pub(crate) async fn session_workspace(&self, id: Uuid) -> Option<PathBuf> {
        let entry = self
            .inner
            .state
            .lock()
            .expect("chat server state poisoned")
            .sessions
            .get(&id)
            .cloned()?;
        entry.mutex.lock().await.workspace.clone()
    }

    /// The session's frame counter, for failure frames emitted without
    /// contacting the server.
    pub(crate) async fn session_seq(&self, id: Uuid) -> u64 {
        let entry = self
            .inner
            .state
            .lock()
            .expect("chat server state poisoned")
            .sessions
            .get(&id)
            .cloned();
        match entry {
            Some(entry) => entry.mutex.lock().await.seq,
            None => 0,
        }
    }

    /// Re-read readiness: stop the child when the engine cannot run, clear a
    /// previous stop when it can again (sign-in after sign-out).
    pub fn refresh_availability(&self) {
        let availability = (self.inner.availability)();
        // `refresh_availability` is called from manager hooks that cannot
        // block; the state update itself is synchronous and the kill is
        // fire-and-forget through the command channel.
        if availability.can_run {
            let mut state = self.inner.state.lock().expect("chat server state poisoned");
            state.stopped_code = None;
        } else {
            let mut state = self.inner.state.lock().expect("chat server state poisoned");
            if state.stopped_code.is_none() {
                state.stopped_code = Some(availability.code.clone());
            }
            state.up = false;
            drop(state);
            let _ = self.inner.cmd_tx.try_send(ConnCmd::StopServer);
        }
    }

    /// Stop the child, failing running turns with `code`. Idempotent.
    pub fn stop(&self, code: &str) {
        {
            let mut state = self.inner.state.lock().expect("chat server state poisoned");
            state.stopped_code = Some(code.to_string());
            state.pending_restart = false;
            state.up = false;
        }
        let _ = self.inner.cmd_tx.try_send(ConnCmd::StopServer);
    }

    /// The daemon is shutting down: same as `stop`, with a best-effort wait
    /// for the child to exit.
    pub async fn shutdown(&self) {
        self.stop(DAEMON_STOPPING);
    }

    /// The model profile changed (O4): restart now when no turn runs,
    /// otherwise restart once the running turns drain. Turns that arrive
    /// while a restart waits are accepted as usual; the restart waits for
    /// them too.
    pub fn note_profile_changed(&self) {
        let restart_now = {
            let mut state = self.inner.state.lock().expect("chat server state poisoned");
            if state.running_turns == 0 {
                true
            } else {
                state.pending_restart = true;
                false
            }
        };
        if restart_now {
            self.restart_idle_child();
        }
    }

    /// Kill the idle child; the next warm/send starts it again. Best-effort:
    /// a failed restart only means the next warm pays the spawn.
    fn restart_idle_child(&self) {
        {
            let mut state = self.inner.state.lock().expect("chat server state poisoned");
            state.up = false;
            state.failed_attempts = 0;
            state.backoff_until = None;
        }
        let _ = self.inner.cmd_tx.try_send(ConnCmd::StopServer);
    }
}

impl ServerLifecycle for EngineChatServer {
    fn refresh_availability(&self) {
        EngineChatServer::refresh_availability(self);
    }

    fn note_profile_changed(&self) {
        EngineChatServer::note_profile_changed(self);
    }
}

/// Connection-level routing state, shared by the handle (id allocation,
/// subscriptions) and the background task (response/frame routing).
struct ConnShared {
    next_id: u64,
    pending: HashMap<u64, PendingRpc>,
    subs: HashMap<String, Vec<mpsc::UnboundedSender<Incoming>>>,
    /// `turn_id → thread_id`, learned from `turn/start` responses so frames
    /// that carry only a turn id still route.
    turns: HashMap<String, String>,
}

fn params_thread_id(params: &serde_json::Value) -> Option<String> {
    params
        .get("threadId")
        .or_else(|| params.get("thread_id"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .or_else(|| {
            params
                .get("thread")
                .and_then(|t| t.get("id"))
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
}

/// The single background task: owns the child, sends every outbound line,
/// and routes every inbound frame. Ends when all handles are dropped.
async fn connection_task(
    mut cmd_rx: mpsc::Receiver<ConnCmd>,
    shared: Arc<Mutex<ConnShared>>,
    inner: Arc<Inner>,
) {
    let mut child: Option<AppServer> = None;
    // The exit watch is tracked apart from the child so the two `select!`
    // stream arms never borrow the child at once (one mutable, one not).
    let mut exit_rx: Option<tokio::sync::watch::Receiver<Option<ExitSnapshot>>> = None;
    loop {
        tokio::select! {
            biased;
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    // All handles dropped: shut the child down and exit.
                    if let Some(server) = child.take() {
                        let _ = server.shutdown(Duration::from_secs(5)).await;
                    }
                    return;
                };
                match cmd {
                    ConnCmd::Start { reply } => {
                        if child.is_some() {
                            let _ = reply.send(Ok(()));
                            continue;
                        }
                        let result = start_child(&inner.spawn).await;
                        match result {
                            Ok((server, early)) => {
                                child = Some(server);
                                exit_rx = child.as_ref().map(|s| s.process_handle().exit_rx);
                                let _ = reply.send(Ok(()));
                                for frame in early {
                                    route_frame(&shared, frame).await;
                                }
                            }
                            Err(e) => {
                                let _ = reply.send(Err(e));
                            }
                        }
                    }
                    ConnCmd::StopServer => {
                        if let Some(server) = child.take() {
                            let _ = server.shutdown(Duration::from_secs(5)).await;
                        }
                        exit_rx = None;
                        fail_all(&shared, "shared engine server stopped").await;
                    }
                    ConnCmd::Rpc { id, method, params, reply } => {
                        let Some(server) = child.as_mut() else {
                            let _ = reply.send(Err(anyhow::anyhow!("shared engine server is not running")));
                            continue;
                        };
                        let line = match jsonrpc::request(id, method, &params) {
                            Ok(line) => line,
                            Err(e) => {
                                let _ = reply.send(Err(e.into()));
                                continue;
                            }
                        };
                        {
                            let mut shared = shared.lock().await;
                            shared.pending.insert(
                                id,
                                PendingRpc {
                                    reply,
                                    method: method.to_string(),
                                    thread_id: params_thread_id(&params),
                                },
                            );
                        }
                        if let Err(e) = server.send_raw(&line).await {
                            let reply = shared.lock().await.pending.remove(&id).map(|p| p.reply);
                            if let Some(reply) = reply {
                                let _ = reply.send(Err(e));
                            }
                            connection_lost(&mut child, &mut exit_rx, &shared, &inner).await;
                        }
                    }
                    ConnCmd::SendRaw { line } => {
                        if let Some(server) = child.as_mut()
                            && let Err(_) = server.send_raw(&line).await
                        {
                            connection_lost(&mut child, &mut exit_rx, &shared, &inner).await;
                        }
                    }
                    ConnCmd::Subscribe { thread_id, tx } => {
                        shared.lock().await.subs.entry(thread_id).or_default().push(tx);
                    }
                }
            }
            frame = async {
                match child.as_mut() {
                    Some(server) => server.inbound.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                let Some(frame) = frame else {
                    // stdout closed: the child is gone.
                    connection_lost(&mut child, &mut exit_rx, &shared, &inner).await;
                    continue;
                };
                route_frame(&shared, frame).await;
            }
            _ = async {
                match exit_rx.as_mut() {
                    Some(rx) => {
                        let _ = rx.wait_for(|snap| snap.is_some()).await;
                    }
                    None => std::future::pending::<()>().await,
                }
            } => {
                connection_lost(&mut child, &mut exit_rx, &shared, &inner).await;
            }
        }
    }
}

/// Spawn the child and run `initialize` + `initialized`, so the first chat
/// finds the server ready. Returns the live server plus any frames that
/// arrived before the `initialize` response (routed once the child is
/// assigned to the connection loop).
async fn start_child(spawn: &SpawnSource) -> Result<(AppServer, Vec<Incoming>)> {
    let mut server = within_startup_deadline(spawn())
        .await
        .context("spawning the shared engine server")?;
    let id = 0;
    let line = jsonrpc::request(
        id,
        "initialize",
        &InitializeParams {
            client_info: ClientInfo {
                name: "pidash".to_string(),
                version: crate::RUNNER_VERSION.to_string(),
            },
        },
    )?;
    server.send_raw(&line).await?;
    // The engine answers `initialize` before anything else it emits; stash
    // any early notification and route it once the connection is up.
    let mut early = Vec::new();
    let deadline = tokio::time::Instant::now() + ENGINE_STARTUP_TIMEOUT;
    let response = loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let frame = tokio::time::timeout(remaining, server.inbound.recv())
            .await
            .context("shared engine server initialize timed out")?
            .context("shared engine server stdout closed during initialize")?;
        match frame {
            Incoming::Response {
                id: got,
                result,
                error,
                ..
            } if got == id => {
                if let Some(err) = error {
                    anyhow::bail!("engine initialize error {}: {}", err.code, err.message);
                }
                break result.unwrap_or(serde_json::Value::Null);
            }
            other => early.push(other),
        }
    };
    let _ = response;
    let line = jsonrpc::notification("initialized", &serde_json::Value::Null)?;
    server.send_raw(&line).await?;
    Ok((server, early))
}

/// Fail every pending RPC with `detail` and drop every subscription, waking
/// the turn drivers that observe their channel close.
async fn fail_all(shared: &Arc<Mutex<ConnShared>>, detail: &str) {
    let mut shared = shared.lock().await;
    for (_, pending) in shared.pending.drain() {
        let _ = pending.reply.send(Err(anyhow::anyhow!("{detail}")));
    }
    shared.subs.clear();
    shared.turns.clear();
}

/// The child is gone (crash or kill): fail pending RPCs, drop subscriptions
/// so running turns fail, and record the attempt for backoff. Sessions keep
/// their thread ids and resume on the next warm/send.
async fn connection_lost(
    child: &mut Option<AppServer>,
    exit_rx: &mut Option<tokio::sync::watch::Receiver<Option<ExitSnapshot>>>,
    shared: &Arc<Mutex<ConnShared>>,
    inner: &Arc<Inner>,
) {
    *exit_rx = None;
    let had_child = child.is_some();
    // Intentional stops (`StopServer`) clear `up` synchronously and fail
    // their own RPCs; a racing exit report after one must not add backoff.
    // Crash reports are idempotent the same way.
    let was_up = inner.state.lock().expect("chat server state poisoned").up;
    if !had_child && !was_up {
        return;
    }
    if had_child {
        // Reap the dead child without hanging the router: stdin is already
        // closed by the engine's exit, so this returns promptly.
        if let Some(server) = child.take() {
            let _ = server.shutdown(Duration::from_secs(1)).await;
        }
    }
    fail_all(shared, "shared engine server stopped").await;
    let mut state = inner.state.lock().expect("chat server state poisoned");
    state.up = false;
    state.failed_attempts = state.failed_attempts.saturating_add(1);
    state.backoff_until =
        Some(Instant::now() + state.backoff_for(inner.backoff_initial, inner.backoff_max));
}

/// Route one inbound frame: responses to their RPC waiter (indexing new
/// turn ids), notifications/requests to the thread's subscribers.
async fn route_frame(shared: &Arc<Mutex<ConnShared>>, frame: Incoming) {
    match frame {
        Incoming::Response {
            id, result, error, ..
        } => {
            let pending = shared.lock().await.pending.remove(&id);
            if let Some(pending) = pending {
                if let Some(err) = error {
                    let _ = pending.reply.send(Err(anyhow::anyhow!(
                        "engine rpc error {}: {}",
                        err.code,
                        err.message
                    )));
                    return;
                }
                let result = result.unwrap_or(serde_json::Value::Null);
                if pending.method == "turn/start"
                    && let (Some(thread_id), Some(turn_id)) = (
                        pending.thread_id.as_deref(),
                        result
                            .get("turn")
                            .and_then(|t| t.get("id"))
                            .and_then(|v| v.as_str()),
                    )
                {
                    shared
                        .lock()
                        .await
                        .turns
                        .insert(turn_id.to_string(), thread_id.to_string());
                }
                let _ = pending.reply.send(Ok(result));
            }
        }
        Incoming::Notification { method, params, .. } => {
            let target = lookup_target(shared, &params).await;
            deliver(
                shared,
                target,
                Incoming::Notification {
                    jsonrpc: Some("2.0".to_string()),
                    method,
                    params,
                },
            )
            .await;
        }
        Incoming::Request {
            id, method, params, ..
        } => {
            let target = lookup_target(shared, &params).await;
            deliver(
                shared,
                target,
                Incoming::Request {
                    jsonrpc: Some("2.0".to_string()),
                    id,
                    method,
                    params,
                },
            )
            .await;
        }
    }
}

/// Which thread a notification or server request belongs to: the `threadId`
/// in its params, else the thread that owns its `turnId` (learned from
/// `turn/start`), else nothing.
async fn lookup_target(
    shared: &Arc<Mutex<ConnShared>>,
    params: &serde_json::Value,
) -> Option<String> {
    if let Some(thread_id) = params_thread_id(params) {
        return Some(thread_id);
    }
    // Frames that carry only a turn id (some approval requests) resolve
    // through the turn index the router learns from `turn/start`.
    let turn_id = params
        .get("turnId")
        .or_else(|| params.get("turn").and_then(|t| t.get("id")))
        .and_then(|v| v.as_str())?;
    shared.lock().await.turns.get(turn_id).cloned()
}

/// Deliver a notification or server request to its thread's subscribers.
/// Frames without a routable id go to the single subscriber when there is
/// exactly one; otherwise they are dropped and logged, never broadcast —
/// a `turn/completed` for another thread must not end this one.
async fn deliver(shared: &Arc<Mutex<ConnShared>>, thread_id: Option<String>, frame: Incoming) {
    let mut shared = shared.lock().await;
    // Prune dead subscribers first: every turn subscribes fresh and drops
    // its receiver at turn end, so a kept dead sender would only leak and —
    // worse — a later turn could read this one's trailing frames.
    for subs in shared.subs.values_mut() {
        subs.retain(|tx| !tx.is_closed());
    }
    shared.subs.retain(|_, subs| !subs.is_empty());
    let targets: Vec<mpsc::UnboundedSender<Incoming>> = match thread_id
        .as_deref()
        .and_then(|t| shared.subs.get(t))
    {
        Some(subs) => subs.clone(),
        None if shared.subs.len() == 1 => shared.subs.values().next().cloned().unwrap_or_default(),
        None => {
            tracing::warn!("shared engine server dropped an unroutable frame");
            return;
        }
    };
    // Forget a finished turn's id once its completion is delivered.
    if frame_is_turn_completed(&frame)
        && let Some(tid) = thread_id.as_deref()
    {
        shared.turns.retain(|_, v| v != tid);
    }
    for tx in targets {
        let _ = tx.send(frame.clone());
    }
}

fn frame_is_turn_completed(frame: &Incoming) -> bool {
    matches!(frame, Incoming::Notification { method, .. } if method == "turn/completed")
}

// ---------------------------------------------------------------------------
// Sessions: warm / send / cancel / close.
// ---------------------------------------------------------------------------

impl EngineChatServer {
    /// Fetch-or-create the session entry. Never holds the state lock across
    /// an await that touches the entry.
    async fn session(&self, id: Uuid) -> Arc<SessionEntry> {
        let mut state = self.inner.state.lock().expect("chat server state poisoned");
        state
            .sessions
            .entry(id)
            .or_insert_with(|| {
                Arc::new(SessionEntry {
                    mutex: Mutex::new(SessionData {
                        thread_id: None,
                        approval_mode: ApprovalMode::default(),
                        workspace: None,
                        started_reported: false,
                        running_turn_id: None,
                        seq: 0,
                        last_used_at: Instant::now(),
                    }),
                    cancel: Arc::new(Notify::new()),
                    cancelled: AtomicBool::new(false),
                })
            })
            .clone()
    }

    /// Ensure the child is up and the engine can run, starting it when
    /// needed. Fails with the stop/restart code, never with a spawn when the
    /// engine is unavailable.
    async fn ensure_up(&self) -> Result<()> {
        {
            let state = self.inner.state.lock().expect("chat server state poisoned");
            if let Some(code) = state.stopped_code.as_deref() {
                anyhow::bail!("{code}");
            }
            if state.up {
                return Ok(());
            }
            if let Some(until) = state.backoff_until
                && Instant::now() < until
            {
                anyhow::bail!("{ENGINE_RESTARTED}: the shared engine server is backing off");
            }
        }
        let availability = (self.inner.availability)();
        if !availability.can_run {
            anyhow::bail!("{}", availability.code);
        }
        let (tx, rx) = oneshot::channel();
        self.inner
            .cmd_tx
            .send(ConnCmd::Start { reply: tx })
            .await
            .context("shared engine server task is gone")?;
        match rx.await {
            Ok(Ok(())) => {
                let mut state = self.inner.state.lock().expect("chat server state poisoned");
                state.up = true;
                state.failed_attempts = 0;
                state.backoff_until = None;
                Ok(())
            }
            Ok(Err(e)) => {
                let mut state = self.inner.state.lock().expect("chat server state poisoned");
                state.up = false;
                state.failed_attempts = state.failed_attempts.saturating_add(1);
                state.backoff_until = Some(
                    Instant::now()
                        + state.backoff_for(self.inner.backoff_initial, self.inner.backoff_max),
                );
                Err(e)
            }
            Err(_) => {
                let mut state = self.inner.state.lock().expect("chat server state poisoned");
                state.up = false;
                Err(anyhow::anyhow!("shared engine server task is gone"))
            }
        }
    }

    /// One multiplexed RPC: allocate the id here, send through the
    /// connection task, await the routed response.
    async fn rpc(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let id = {
            let mut shared = self.rpc_shared().lock().await;
            let id = shared.next_id;
            shared.next_id = shared.next_id.saturating_add(1).max(1);
            id
        };
        let (tx, rx) = oneshot::channel();
        self.inner
            .cmd_tx
            .send(ConnCmd::Rpc {
                id,
                method,
                params,
                reply: tx,
            })
            .await
            .context("shared engine server task is gone")?;
        // The oneshot resolves to the RPC result; a dropped sender means
        // the connection task is gone. `timeout` (not the startup helper:
        // the oneshot's error is a `RecvError`, not `anyhow::Error`).
        tokio::time::timeout(ENGINE_STARTUP_TIMEOUT, rx)
            .await
            .context("engine rpc timed out")?
            .context("shared engine server task is gone")?
    }

    fn rpc_shared(&self) -> &Arc<Mutex<ConnShared>> {
        &self.inner.conn
    }

    /// Answer an engine approval: an `rpc:` handle is a server request id
    /// answered with a JSON-RPC response, anything else is the
    /// `approval/response` notification. Mirrors `Bridge::send_approval`.
    async fn answer_approval(&self, approval_id: &str, decision: ApprovalDecision) -> Result<()> {
        if let Some(handle) = approval_id.strip_prefix("rpc:") {
            let (_, raw_id) = handle.split_once(':').context("invalid approval handle")?;
            let id: RpcId = serde_json::from_str(raw_id).context("invalid approval request id")?;
            let decision = match decision {
                ApprovalDecision::Accept => "accept",
                ApprovalDecision::Decline => "decline",
                ApprovalDecision::AcceptForSession => "acceptForSession",
            };
            let line = jsonrpc::response(id, serde_json::json!({ "decision": decision }))?;
            self.inner
                .cmd_tx
                .send(ConnCmd::SendRaw { line })
                .await
                .context("shared engine server task is gone")?;
            return Ok(());
        }
        let line = jsonrpc::notification(
            "approval/response",
            &ApprovalResponseParams {
                approval_id: approval_id.to_string(),
                decision: match decision {
                    ApprovalDecision::Accept => "accept".into(),
                    ApprovalDecision::Decline => "decline".into(),
                    ApprovalDecision::AcceptForSession => "accept_for_session".into(),
                },
            },
        )?;
        self.inner
            .cmd_tx
            .send(ConnCmd::SendRaw { line })
            .await
            .context("shared engine server task is gone")?;
        Ok(())
    }

    /// Send `turn/interrupt` for the session's running turn. Fire-and-forget:
    /// the driver never waits for the reply (the engine may be mid-turn and
    /// answer only later), and a missing turn id means the turn never started.
    async fn interrupt_turn(&self, thread_id: &str, turn_id: Option<&str>) {
        let Some(turn_id) = turn_id else { return };
        self.rpc_noreply(
            "turn/interrupt",
            serde_json::to_value(TurnInterruptParams {
                thread_id: thread_id.to_string(),
                turn_id: turn_id.to_string(),
            })
            .unwrap_or(serde_json::Value::Null),
        )
        .await;
    }

    /// An RPC whose response nobody reads (interrupts, unsubscribes). The
    /// router drops the reply when it arrives; the line still goes out over
    /// the single connection in order.
    async fn rpc_noreply(&self, method: &'static str, params: serde_json::Value) {
        let id = {
            let mut shared = self.rpc_shared().lock().await;
            let id = shared.next_id;
            shared.next_id = shared.next_id.saturating_add(1).max(1);
            id
        };
        let (tx, _rx) = oneshot::channel();
        let _ = self
            .inner
            .cmd_tx
            .send(ConnCmd::Rpc {
                id,
                method,
                params,
                reply: tx,
            })
            .await;
    }

    /// `ChatWarm`: make sure the server is up, then `thread/resume` (known
    /// thread) or `thread/start` (new). Returns the thread id.
    pub async fn warm<S: TurnSink>(
        &self,
        chat_session_id: Uuid,
        spec: WarmSpec,
        sink: &mut S,
    ) -> Result<String> {
        self.sweep_idle().await;
        let session = self.session(chat_session_id).await;
        let mut data = session.mutex.lock().await;
        // A warm races cancel/close the way the per-session lane does: a
        // hung engine must not hold the session lock for the whole startup
        // budget.
        // A cancel that landed before this waiter parked leaves no notify
        // to observe; the drive loop re-checks too, so a warm that wins
        // this check still cancels on its first turn iteration.
        let pre_cancelled = session.cancelled.load(Ordering::SeqCst);
        let cancel = session.cancel.notified();
        tokio::pin!(cancel);
        let warmed = if pre_cancelled {
            Err(anyhow::anyhow!("chat warm cancelled"))
        } else {
            tokio::select! {
                biased;
                _ = cancel.as_mut() => Err(anyhow::anyhow!("chat warm cancelled")),
                r = self.warm_locked(&mut data, &spec) => r,
            }
        };
        match warmed {
            Ok((thread_id, seq)) => {
                sink.warmed(&thread_id, seq).await?;
                Ok(thread_id)
            }
            Err(e) => {
                // `data` already holds the session lock; re-locking it here
                // self-deadlocks the turn (the guard lives until `warm`
                // returns).
                let seq = data.seq;
                sink.warm_failed(format!("{e:#}"), seq).await?;
                Err(e)
            }
        }
    }

    /// Warm with the session lock held. Returns the thread id and the
    /// session's frame counter after the warm event.
    async fn warm_locked(&self, data: &mut SessionData, spec: &WarmSpec) -> Result<(String, u64)> {
        self.ensure_up().await?;
        if data.workspace.is_none() {
            data.workspace = Some(spec.workspace.clone());
        }
        let workspace = data.workspace.clone().expect("workspace just set");
        if data.thread_id.is_none() {
            // Capture the mode for the whole thread the moment it is
            // created, as the per-session lane does.
            data.approval_mode = spec.mode;
        }
        let mode = data.approval_mode;
        let thread_id = if let Some(known) = data.thread_id.clone() {
            match self.resume_thread(&known).await {
                Ok(id) => id,
                Err(e) => {
                    tracing::warn!(
                        resume_thread_id = %known,
                        error = %format!("{e:#}"),
                        "thread/resume failed, falling back to thread/start"
                    );
                    self.start_thread(&workspace, spec, mode).await?
                }
            }
        } else if let Some(resume_id) = spec.resume_id.as_deref().filter(|s| !s.is_empty()) {
            match self.resume_thread(resume_id).await {
                Ok(id) => id,
                Err(e) => {
                    tracing::warn!(
                        resume_thread_id = %resume_id,
                        error = %format!("{e:#}"),
                        "thread/resume failed, falling back to thread/start"
                    );
                    self.start_thread(&workspace, spec, mode).await?
                }
            }
        } else {
            self.start_thread(&workspace, spec, mode).await?
        };
        data.thread_id = Some(thread_id.clone());
        data.last_used_at = Instant::now();
        data.seq = data.seq.saturating_add(1);
        let seq = data.seq;
        Ok((thread_id, seq))
    }

    async fn resume_thread(&self, thread_id: &str) -> Result<String> {
        let resp = self
            .rpc(
                "thread/resume",
                serde_json::to_value(ThreadResumeParams {
                    thread_id: thread_id.to_string(),
                })?,
            )
            .await?;
        thread_id_of(&resp).context("thread/resume missing threadId")
    }

    async fn start_thread(
        &self,
        workspace: &std::path::Path,
        spec: &WarmSpec,
        mode: ApprovalMode,
    ) -> Result<String> {
        let settings = engine_thread_settings(mode);
        let resp = self
            .rpc(
                "thread/start",
                serde_json::to_value(ThreadStartParams {
                    cwd: workspace.to_string_lossy().to_string(),
                    model: spec_model(spec),
                    sandbox: settings.sandbox.to_string(),
                    approval_policy: settings.approval_policy.to_string(),
                })?,
            )
            .await?;
        thread_id_of(&resp).context("thread/start missing threadId")
    }

    /// `ChatCancel`: interrupt the in-flight turn, if any. Best-effort.
    ///
    /// Never waits for the session lock: the holder is the turn this cancel
    /// must stop, and on one task that wait starves the turn it waits for
    /// (the turn's own timeout is only observed when the turn is polled).
    /// The driver owns the interrupt — it carries the thread and turn ids
    /// and checks `cancelled` before parking its waiter, so an early cancel
    /// is never missed.
    pub fn cancel(&self, chat_session_id: Uuid) {
        let entry = self
            .inner
            .state
            .lock()
            .expect("chat server state poisoned")
            .sessions
            .get(&chat_session_id)
            .cloned();
        let Some(entry) = entry else { return };
        // Sticky first: a cancel that lands before the turn parks its
        // waiter must still be observed (see `SessionEntry::cancelled`).
        entry.cancelled.store(true, Ordering::SeqCst);
        entry.cancel.notify_waiters();
    }

    /// `ChatClose`: interrupt the turn, unsubscribe the thread so the
    /// engine can unload it, and drop the session.
    pub async fn close(&self, chat_session_id: Uuid) {
        let entry = self
            .inner
            .state
            .lock()
            .expect("chat server state poisoned")
            .sessions
            .remove(&chat_session_id);
        let Some(entry) = entry else { return };
        entry.cancelled.store(true, Ordering::SeqCst);
        entry.cancel.notify_waiters();
        let (thread_id, turn_id) = {
            let data = entry.mutex.lock().await;
            (data.thread_id.clone(), data.running_turn_id.clone())
        };
        if let Some(thread_id) = thread_id {
            self.interrupt_turn(&thread_id, turn_id.as_deref()).await;
            // Unsubscribe is best-effort: the mapping is dropped either way,
            // and a failed unload only costs the engine memory. Sent without
            // waiting so a close never hangs on a busy engine.
            self.rpc_noreply(
                "thread/unsubscribe",
                serde_json::json!({ "threadId": thread_id }),
            )
            .await;
        }
    }

    /// Unsubscribe sessions idle past the unload delay (keeping their
    /// thread mapping for a later resume). Runs opportunistically on
    /// warm/send, at most every few minutes.
    async fn sweep_idle(&self) {
        let now = Instant::now();
        {
            let state = self.inner.state.lock().expect("chat server state poisoned");
            if state
                .last_sweep_at
                .is_some_and(|at| now.duration_since(at) < Duration::from_secs(5 * 60))
            {
                return;
            }
        }
        let idle: Vec<(Arc<SessionEntry>, String)> = {
            let state = self.inner.state.lock().expect("chat server state poisoned");
            let mut out = Vec::new();
            for entry in state.sessions.values() {
                let Ok(data) = entry.mutex.try_lock() else {
                    continue;
                };
                if data.running_turn_id.is_none()
                    && let Some(thread_id) = data.thread_id.clone()
                    && now.duration_since(data.last_used_at) > CHAT_THREAD_IDLE_UNSUBSCRIBE
                {
                    out.push((entry.clone(), thread_id));
                }
            }
            out
        };
        if idle.is_empty() {
            let mut state = self.inner.state.lock().expect("chat server state poisoned");
            state.last_sweep_at = Some(now);
            return;
        }
        for (_, thread_id) in &idle {
            self.rpc_noreply(
                "thread/unsubscribe",
                serde_json::json!({ "threadId": thread_id }),
            )
            .await;
        }
        let mut state = self.inner.state.lock().expect("chat server state poisoned");
        state.last_sweep_at = Some(now);
    }

    /// Answer a pending chat approval (the IPC lane's `ChatDecide` path
    /// resolves the router; the parked turn calls this to reply).
    pub async fn decide(&self, approval_id: &str, decision: ApprovalDecision) -> Result<()> {
        self.answer_approval(approval_id, decision).await
    }

    /// `ChatSend`: warm if needed, then run one turn on the session's
    /// thread, streaming through `sink`. Serializes with other sends on the
    /// same session via the entry mutex; the O5 cap counts across sessions.
    pub async fn send<S: TurnSink>(
        &self,
        chat_session_id: Uuid,
        spec: SendSpec,
        sink: &mut S,
    ) -> TurnTerminal {
        self.sweep_idle().await;
        let session = self.session(chat_session_id).await;
        let mut data = session.mutex.lock().await;

        // Warm inline (no `warmed` frame on this path, as today): resume the
        // saved thread or start a new one.
        let warm = WarmSpec {
            workspace: spec.workspace.clone(),
            mode: spec.mode,
            resume_id: spec.resume_id.clone(),
        };
        let cancel = session.cancel.notified();
        tokio::pin!(cancel);
        let thread_id = tokio::select! {
            biased;
            _ = cancel.as_mut() => {
                return TurnTerminal::Completed {
                    turn_id: None,
                    assistant_message: None,
                    status: "cancelled".into(),
                };
            }
            r = self.warm_locked(&mut data, &warm) => match r {
                Ok((thread_id, _)) => thread_id,
                Err(e) => return self.send_failed(e).await,
            },
        };

        if !data.started_reported {
            data.started_reported = true;
            if sink.thread_started(&thread_id).await.is_err() {
                // The turn never started and the cap was never consumed:
                // unwind the session state without touching the count.
                return self
                    .end_turn(&mut data, false)
                    .finish(TurnTerminal::Failed {
                        code: "internal".into(),
                        detail: Some("chat sink closed".into()),
                    });
            }
        }

        // O5: at most MAX_RUNNING_CHAT_TURNS across all sessions. Counted
        // after the warm so a warm-only session never consumes the cap.
        {
            let mut state = self.inner.state.lock().expect("chat server state poisoned");
            if state.running_turns >= MAX_RUNNING_CHAT_TURNS {
                return TurnTerminal::Failed {
                    code: ENGINE_BUSY.into(),
                    detail: Some(format!(
                        "the Pi Dash Agent is busy ({} turns running); try again shortly",
                        MAX_RUNNING_CHAT_TURNS
                    )),
                };
            }
            state.running_turns += 1;
        }

        // Subscribe before `turn/start` so no frame for this thread is
        // missed. The subscription is dropped at turn end; the router prunes
        // dead subscribers, so a later turn never reads this one's frames.
        let (tx, mut rx) = mpsc::unbounded_channel::<Incoming>();
        {
            let _ = self
                .inner
                .cmd_tx
                .send(ConnCmd::Subscribe {
                    thread_id: thread_id.clone(),
                    tx,
                })
                .await;
        }
        let turn_id = match self
            .rpc(
                "turn/start",
                serde_json::to_value(TurnStartParams {
                    thread_id: thread_id.clone(),
                    input: vec![TurnInputItem {
                        item_type: "text".into(),
                        text: spec.content.clone(),
                    }],
                    model: spec.model.clone(),
                    effort: None,
                })
                .unwrap_or(serde_json::Value::Null),
            )
            .await
            .context("starting the turn")
            .and_then(|resp| turn_id_of(&resp).context("turn/start missing turn id"))
        {
            Ok(id) => id,
            Err(e) => {
                return self
                    .end_turn(&mut data, true)
                    .finish(self.send_failed(e).await);
            }
        };
        data.running_turn_id = Some(turn_id.clone());
        if sink.message_started(&turn_id).await.is_err() {
            return self.end_turn(&mut data, true).finish(TurnTerminal::Failed {
                code: "internal".into(),
                detail: Some("chat sink closed".into()),
            });
        }

        let message_id = spec.message_id;
        let mut cursor = BridgeCursor {
            run_id: message_id,
            thread_id: thread_id.clone(),
            model: spec.model.clone(),
            seq: 0,
        };
        let cancel = session.cancel.notified();
        tokio::pin!(cancel);
        let terminal = loop {
            // Sticky cancel first: a cancel that landed before this
            // waiter parked leaves no notify to observe.
            if session.cancelled.load(Ordering::SeqCst) {
                self.interrupt_turn(&thread_id, Some(&turn_id)).await;
                break TurnTerminal::Completed {
                    turn_id: Some(turn_id.clone()),
                    assistant_message: None,
                    status: "cancelled".into(),
                };
            }
            tokio::select! {
                biased;
                _ = cancel.as_mut() => {
                    self.interrupt_turn(&thread_id, Some(&turn_id)).await;
                    break TurnTerminal::Completed {
                        turn_id: Some(turn_id.clone()),
                        assistant_message: None,
                        status: "cancelled".into(),
                    };
                }
                frame = rx.recv() => {
                    let Some(frame) = frame else {
                        // The connection task drops subscriptions on stop
                        // and on crash: only a stopped or restarted code
                        // can land here.
                        let (code, detail) = self.stopped_or_restarted("agent_stdout_closed").await;
                        break TurnTerminal::Failed { code, detail: Some(detail) };
                    };
                    match self
                        .drive_frame(&thread_id, &turn_id, &mut cursor, &mut data.seq, frame, sink)
                        .await
                    {
                        FrameOutcome::Continue => continue,
                        FrameOutcome::Terminal(t) => break t,
                    }
                }
            }
        };
        self.end_turn(&mut data, true).finish(terminal)
    }

    /// Map a warm/`turn/start` RPC failure to the terminal the caller sees:
    /// the stop code when stopped, `engine_restarted` after a crash, else a
    /// start failure.
    async fn send_failed(&self, e: anyhow::Error) -> TurnTerminal {
        let stopped = self
            .inner
            .state
            .lock()
            .expect("chat server state poisoned")
            .stopped_code
            .clone();
        if let Some(code) = stopped {
            return TurnTerminal::Failed {
                code,
                detail: Some(format!("{e:#}")),
            };
        }
        let up = self
            .inner
            .state
            .lock()
            .expect("chat server state poisoned")
            .up;
        if !up {
            return TurnTerminal::Failed {
                code: ENGINE_RESTARTED.into(),
                detail: Some(format!("{e:#}")),
            };
        }
        TurnTerminal::Failed {
            code: ENGINE_START_FAILED.into(),
            detail: Some(format!("{e:#}")),
        }
    }

    async fn stopped_or_restarted(&self, detail: &str) -> (String, String) {
        let state = self.inner.state.lock().expect("chat server state poisoned");
        if let Some(code) = state.stopped_code.clone() {
            (code, detail.to_string())
        } else {
            (ENGINE_RESTARTED.into(), detail.to_string())
        }
    }

    /// Turn end bookkeeping: clear the running turn, stamp use, drop the
    /// running count, and restart the child when a profile change waited for
    /// the drain (O4).
    fn end_turn(
        &self,
        data: &mut tokio::sync::MutexGuard<'_, SessionData>,
        counted: bool,
    ) -> EndTurn<'_> {
        data.running_turn_id = None;
        data.last_used_at = Instant::now();
        // Only turns that consumed the O5 cap give it back: an early return
        // before the increment must not decrement another turn's count (and
        // must not trip a pending restart while that turn still runs).
        let restart = counted && {
            let mut state = self.inner.state.lock().expect("chat server state poisoned");
            state.running_turns = state.running_turns.saturating_sub(1);
            state.running_turns == 0 && std::mem::take(&mut state.pending_restart)
        };
        EndTurn {
            server: self,
            restart,
        }
    }

    /// Drive one inbound frame for the running turn. Mirrors the
    /// per-session lane's translation: gateway refusals fail fast with the
    /// credit/auth code, deltas stream as `assistant_delta`, approvals park
    /// on the caller's sink, and completion ends the turn.
    async fn drive_frame<S: TurnSink>(
        &self,
        thread_id: &str,
        turn_id: &str,
        cursor: &mut BridgeCursor,
        seq: &mut u64,
        frame: Incoming,
        sink: &mut S,
    ) -> FrameOutcome {
        for event in cursor.translate(frame) {
            match event {
                crate::agent::BridgeEvent::Raw { method, params, .. } => {
                    if let Some((code, detail)) = gateway_chat_refusal(&method, &params) {
                        // The engine is about to re-send the whole request,
                        // five times over: stop the turn here rather than by
                        // a round trip, and never forward the retry notice.
                        self.interrupt_turn(thread_id, Some(turn_id)).await;
                        return FrameOutcome::Terminal(TurnTerminal::Failed {
                            code: code.into(),
                            detail: Some(detail),
                        });
                    }
                    let kind = if is_assistant_text_delta(&method, &params) {
                        "assistant_delta"
                    } else {
                        "raw"
                    };
                    let payload = serde_json::json!({ "method": method, "params": params });
                    *seq = seq.saturating_add(1);
                    if sink.event(*seq, kind, payload).await.is_err() {
                        return FrameOutcome::Terminal(TurnTerminal::Failed {
                            code: "internal".into(),
                            detail: Some("chat sink closed".into()),
                        });
                    }
                }
                crate::agent::BridgeEvent::ApprovalRequest {
                    approval_id,
                    kind,
                    payload,
                    reason,
                    ..
                } => {
                    match sink
                        .decide_approval(ApprovalRequest {
                            approval_id: approval_id.clone(),
                            kind,
                            payload,
                            reason,
                        })
                        .await
                    {
                        ApprovalOutcome::Decided(decision) => {
                            if let Err(e) = self.answer_approval(&approval_id, decision).await {
                                return FrameOutcome::Terminal(TurnTerminal::Failed {
                                    code: "approval_send_failed".into(),
                                    detail: Some(format!("{e:#}")),
                                });
                            }
                        }
                        ApprovalOutcome::TurnCancelled => {
                            self.interrupt_turn(thread_id, Some(turn_id)).await;
                            return FrameOutcome::Terminal(TurnTerminal::Completed {
                                turn_id: Some(turn_id.to_string()),
                                assistant_message: None,
                                status: "cancelled".into(),
                            });
                        }
                        ApprovalOutcome::TurnFailed { code, detail } => {
                            self.interrupt_turn(thread_id, Some(turn_id)).await;
                            return FrameOutcome::Terminal(TurnTerminal::Failed { code, detail });
                        }
                    }
                }
                crate::agent::BridgeEvent::Completed { done_payload, .. } => {
                    let assistant_message = assistant_text_from_done_payload(&done_payload);
                    return FrameOutcome::Terminal(TurnTerminal::Completed {
                        turn_id: Some(turn_id.to_string()),
                        assistant_message,
                        status: "completed".into(),
                    });
                }
                crate::agent::BridgeEvent::Failed { detail, .. } => {
                    return FrameOutcome::Terminal(TurnTerminal::Failed {
                        code: CHAT_AGENT_FAILED.into(),
                        detail,
                    });
                }
                crate::agent::BridgeEvent::RunStarted { .. }
                | crate::agent::BridgeEvent::AwaitingReauth { .. } => {}
            }
        }
        FrameOutcome::Continue
    }
}

enum FrameOutcome {
    Continue,
    Terminal(TurnTerminal),
}

/// Turn-end helper carrying the pending-restart flag out of the session
/// lock so the restart runs after the guard drops.
struct EndTurn<'a> {
    server: &'a EngineChatServer,
    restart: bool,
}

impl EndTurn<'_> {
    fn finish(self, terminal: TurnTerminal) -> TurnTerminal {
        if self.restart {
            self.server.restart_idle_child();
        }
        terminal
    }
}

fn spec_model(_spec: &WarmSpec) -> Option<String> {
    // The model is the profile's (CODEX_HOME/config.toml), as for an engine
    // runner: a per-turn override could name one the gateway lacks.
    None
}

/// Extract the thread id from a `thread/start` / `thread/resume` response.
/// The app-server has used `threadId`, `thread.id` and `thread_id` across
/// versions, so accept all three (mirrors the bridge's parser).
fn thread_id_of(value: &serde_json::Value) -> Option<String> {
    value
        .get("threadId")
        .or_else(|| value.get("thread").and_then(|t| t.get("id")))
        .or_else(|| value.get("thread_id"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
}

fn turn_id_of(value: &serde_json::Value) -> Option<String> {
    value
        .get("turn")
        .and_then(|turn| turn.get("id"))
        .and_then(|id| id.as_str())
        .map(|id| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// Recording sink: captures every streamed frame per session.
    #[derive(Default)]
    struct RecSink {
        warms: Vec<(String, u64)>,
        warm_failed: Vec<String>,
        started: Vec<String>,
        messages: Vec<String>,
        events: Vec<(u64, String, serde_json::Value)>,
        approvals: Vec<String>,
        decide: Option<ApprovalDecision>,
    }

    impl TurnSink for RecSink {
        async fn warmed(&mut self, thread_id: &str, seq: u64) -> Result<()> {
            self.warms.push((thread_id.to_string(), seq));
            Ok(())
        }
        async fn warm_failed(&mut self, detail: String, _seq: u64) -> Result<()> {
            self.warm_failed.push(detail);
            Ok(())
        }
        async fn thread_started(&mut self, thread_id: &str) -> Result<()> {
            self.started.push(thread_id.to_string());
            Ok(())
        }
        async fn message_started(&mut self, turn_id: &str) -> Result<()> {
            self.messages.push(turn_id.to_string());
            Ok(())
        }
        async fn event(&mut self, seq: u64, kind: &str, payload: serde_json::Value) -> Result<()> {
            self.events.push((seq, kind.to_string(), payload));
            Ok(())
        }
        async fn decide_approval(&mut self, req: ApprovalRequest) -> ApprovalOutcome {
            self.approvals.push(req.approval_id.clone());
            ApprovalOutcome::Decided(self.decide.unwrap_or(ApprovalDecision::Decline))
        }
    }

    /// A fake app-server harness: `scripts[i]` is the shell served to the
    /// i-th child spawn (last one repeats). Requests append to a log file,
    /// PIDs to a pid file.
    struct Probe {
        _dir: tempfile::TempDir,
        dir: PathBuf,
        server: EngineChatServer,
        invocations: Arc<AtomicUsize>,
        can_run: Arc<AtomicBool>,
        code: Arc<StdMutex<String>>,
    }

    impl Probe {
        fn new(scripts: Vec<&str>) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let scripts: Vec<String> = scripts.into_iter().map(str::to_owned).collect();
            let invocations = Arc::new(AtomicUsize::new(0));
            let can_run = Arc::new(AtomicBool::new(true));
            let code = Arc::new(StdMutex::new(ENGINE_SIGNED_OUT.to_string()));
            let dir_path = dir.path().to_path_buf();
            let spawn: SpawnSource = {
                let invocations = invocations.clone();
                let dir_path = dir_path.clone();
                Arc::new(move || {
                    let i = invocations.fetch_add(1, Ordering::SeqCst);
                    let script = scripts.get(i).or(scripts.last()).cloned().unwrap();
                    let dir_path = dir_path.clone();
                    Box::pin(async move {
                        let mut cmd = tokio::process::Command::new("sh");
                        cmd.arg("-c")
                            .arg(&script)
                            .arg("fake")
                            .arg(&dir_path)
                            .env("PROBE_DIR", &dir_path);
                        AppServer::spawn_command(cmd).await
                    }) as SpawnFuture
                })
            };
            let availability: AvailabilitySource = {
                let can_run = can_run.clone();
                let code = code.clone();
                Arc::new(move || ChatAvailability {
                    can_run: can_run.load(Ordering::SeqCst),
                    code: code.lock().expect("probe code poisoned").clone(),
                })
            };
            let server =
                EngineChatServer::with_backoff(spawn, availability, Duration::ZERO, Duration::ZERO);
            Self {
                _dir: dir,
                dir: dir_path,
                server,
                invocations,
                can_run,
                code,
            }
        }

        fn log(&self) -> String {
            std::fs::read_to_string(self.dir.join("requests.log")).unwrap_or_default()
        }

        fn spawns(&self) -> usize {
            self.invocations.load(Ordering::SeqCst)
        }

        fn pids(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.join("pids.log"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }

        fn set_availability(&self, can_run: bool, code: &str) {
            self.can_run.store(can_run, Ordering::SeqCst);
            *self.code.lock().expect("probe code poisoned") = code.to_string();
        }

        async fn wait_running(&self, n: usize) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if self.running_count() == n {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!(
                "running turns did not reach {n}; count={} log:\n{}",
                self.running_count(),
                self.log()
            );
        }

        fn running_count(&self) -> usize {
            self.server
                .inner
                .state
                .lock()
                .expect("chat server state poisoned")
                .running_turns
        }

        fn workspace(&self) -> PathBuf {
            let dir = self.dir.join("work");
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        fn warm_spec(&self) -> WarmSpec {
            WarmSpec {
                workspace: self.workspace(),
                mode: ApprovalMode::Ask,
                resume_id: None,
            }
        }

        fn send_spec(&self, content: &str) -> SendSpec {
            SendSpec {
                workspace: self.workspace(),
                mode: ApprovalMode::Ask,
                resume_id: None,
                message_id: Uuid::new_v4(),
                content: content.to_string(),
                model: None,
            }
        }
    }

    const PRELUDE: &str = r#"
DIR="$1"
echo $$ >> "$DIR/pids.log"
rid() { printf '%s' "$1" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p'; }
tidp() { printf '%s' "$1" | sed -n 's/.*"threadId":"\([^"]*\)".*/\1/p'; }
read -r line; echo "$line" >> "$DIR/requests.log"
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"capabilities":{}}}'
read -r line; echo "$line" >> "$DIR/requests.log"
"#;

    /// Fake that completes every turn immediately with thread-specific text.
    fn happy_script() -> String {
        PRELUDE.to_string()
            + r#"
n=0
while read -r line; do
  echo "$line" >> "$DIR/requests.log"
  case "$line" in
    *'"method":"thread/start"'*)
      n=$((n+1)); id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"threadId":"%s"}}\n' "$id" "th-$n";;
    *'"method":"thread/resume"'*)
      id=$(rid "$line"); t=$(tidp "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"threadId":"%s"}}\n' "$id" "$t";;
    *'"method":"turn/start"'*)
      id=$(rid "$line"); t=$(tidp "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"turn":{"id":"turn-%s"}}}\n' "$id" "$t"
            printf '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"%s","text":"%s"}}\n' "$t" "hi-$t"
            printf '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"%s","turn":{"id":"%s","status":"completed"},"done":{"text":"%s"}}}\n' "$t" "turn-$t" "done-$t";;
    *'"method":"turn/interrupt"'*)
      id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id";;
    *'"method":"thread/unsubscribe"'*)
      id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"status":"unsubscribed"}}\n' "$id";;
  esac
done
"#
    }

    fn terminal_of(t: TurnTerminal) -> (String, Option<String>) {
        match t {
            TurnTerminal::Completed {
                status,
                assistant_message,
                ..
            } => (status, assistant_message),
            TurnTerminal::Failed { code, detail } => (format!("failed:{code}"), detail),
        }
    }

    async fn run_send(
        server: &EngineChatServer,
        id: Uuid,
        spec: SendSpec,
        sink: &mut RecSink,
    ) -> TurnTerminal {
        tokio::time::timeout(Duration::from_secs(15), server.send(id, spec, sink))
            .await
            .expect("send timed out")
    }

    #[tokio::test]
    async fn two_sessions_stream_concurrently_without_crossing_events() {
        let probe = Probe::new(vec![&happy_script()]);
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let (mut sa, mut sb) = (RecSink::default(), RecSink::default());
        let (ta, tb) = (
            run_send(&probe.server, a, probe.send_spec("hello a"), &mut sa),
            run_send(&probe.server, b, probe.send_spec("hello b"), &mut sb),
        );
        let (ta, tb) = tokio::join!(ta, tb);
        assert!(matches!(ta, TurnTerminal::Completed { .. }), "{ta:?}");
        assert!(matches!(tb, TurnTerminal::Completed { .. }), "{tb:?}");
        // One child served both sessions.
        assert_eq!(probe.spawns(), 1, "log:\n{}", probe.log());
        // Each sink only saw its own thread's frames (thread numbers
        // depend on scheduling, so read them back from the sinks).
        for sink in [&sa, &sb] {
            assert_eq!(sink.started.len(), 1);
            let tid = sink.started[0].clone();
            assert_eq!(sink.messages.len(), 1);
            let texts: Vec<String> = sink
                .events
                .iter()
                .filter(|(_, kind, _)| kind == "assistant_delta")
                .map(|(_, _, p)| p["params"]["text"].as_str().unwrap().to_string())
                .collect();
            assert_eq!(texts, [format!("hi-{tid}")], "events: {:?}", sink.events);
            let threads: Vec<&str> = sink
                .events
                .iter()
                .map(|(_, _, p)| p["params"]["threadId"].as_str().unwrap())
                .collect();
            assert!(threads.iter().all(|t| *t == tid), "{threads:?}");
        }
        // Frame counters are per-session and monotonic.
        for sink in [&sa, &sb] {
            let seqs: Vec<u64> = sink.events.iter().map(|(s, _, _)| *s).collect();
            assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
        }
    }

    #[tokio::test]
    async fn resume_falls_back_to_start_for_an_unknown_thread() {
        let script = PRELUDE.to_string()
            + r#"
while read -r line; do
  echo "$line" >> "$DIR/requests.log"
  case "$line" in
    *'"method":"thread/resume"'*)
      id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32600,"message":"no rollout found"}}\n' "$id";;
    *'"method":"thread/start"'*)
      id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"threadId":"%s"}}\n' "$id" "th-new";;
    *'"method":"turn/start"'*)
      id=$(rid "$line"); t=$(tidp "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"turn":{"id":"turn-x"}}}\n' "$id"
            printf '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"%s","text":"%s"}}\n' "$t" "hi-$t"
            printf '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"%s","turn":{"id":"%s","status":"completed"},"done":{"text":"%s"}}}\n' "$t" "turn-x" "done-$t";;
  esac
done
"#;
        let probe = Probe::new(vec![&script]);
        let id = Uuid::new_v4();
        let mut spec = probe.send_spec("hi");
        spec.resume_id = Some("th-unknown".into());
        let mut sink = RecSink::default();
        let terminal = run_send(&probe.server, id, spec, &mut sink).await;
        assert!(
            matches!(terminal, TurnTerminal::Completed { .. }),
            "{terminal:?}"
        );
        assert_eq!(sink.started, ["th-new"]);
        let log = probe.log();
        assert!(log.contains("\"method\":\"thread/resume\""), "{log}");
        assert!(log.contains("\"method\":\"thread/start\""), "{log}");
    }

    #[tokio::test]
    async fn close_unsubscribes_and_drops_the_session() {
        let probe = Probe::new(vec![&happy_script()]);
        let id = Uuid::new_v4();
        let mut sink = RecSink::default();
        probe
            .server
            .warm(id, probe.warm_spec(), &mut sink)
            .await
            .expect("warm");
        assert!(probe.server.contains_session(id).await);
        probe.server.close(id).await;
        assert!(!probe.server.contains_session(id).await);
        // Unsubscribe is fire-and-forget: poll the fake's log.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let log = probe.log();
            if log.contains("\"method\":\"thread/unsubscribe\"") && log.contains("th-1") {
                break;
            }
            assert!(Instant::now() < deadline, "no unsubscribe in log:\n{log}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Fake that withholds every turn's completion until five turns have
    /// started, then completes them all.
    fn gated_script() -> String {
        PRELUDE.to_string()
            + r#"
started=0
tids=""
while read -r line; do
  echo "$line" >> "$DIR/requests.log"
  case "$line" in
    *'"method":"thread/start"'*)
      n=$((n+1)); id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"threadId":"%s"}}\n' "$id" "th-$n";;
    *'"method":"turn/start"'*)
      id=$(rid "$line"); t=$(tidp "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"turn":{"id":"turn-%s"}}}\n' "$id" "$t"
      started=$((started+1)); tids="$tids $t"
      if [ "$started" -eq 5 ]; then
        ( while [ ! -f "$DIR/release" ]; do sleep 0.02; done
          for tt in $tids; do
              printf '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"%s","text":"%s"}}\n' "$tt" "hi-$tt"
              printf '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"%s","turn":{"id":"%s","status":"completed"},"done":{"text":"%s"}}}\n' "$tt" "turn-$tt" "done-$tt"
          done ) &
      fi;;
  esac
done
"#
    }

    #[tokio::test]
    async fn sixth_concurrent_send_gets_engine_busy() {
        let probe = Probe::new(vec![&gated_script()]);
        let ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
        let (mut s0, mut s1, mut s2, mut s3, mut s4): (
            RecSink,
            RecSink,
            RecSink,
            RecSink,
            RecSink,
        ) = Default::default();
        // Five sends race; each future borrows its own sink.
        let (t0, t1, t2, t3, t4) = (
            run_send(&probe.server, ids[0], probe.send_spec("m0"), &mut s0),
            run_send(&probe.server, ids[1], probe.send_spec("m1"), &mut s1),
            run_send(&probe.server, ids[2], probe.send_spec("m2"), &mut s2),
            run_send(&probe.server, ids[3], probe.send_spec("m3"), &mut s3),
            run_send(&probe.server, ids[4], probe.send_spec("m4"), &mut s4),
        );
        // Only after all five turns are actually running does the sixth
        // send arrive — otherwise it could slip in before the cap fills.
        let sixth = async {
            probe.wait_running(5).await;
            let mut sink = RecSink::default();
            let terminal = run_send(
                &probe.server,
                Uuid::new_v4(),
                probe.send_spec("m5"),
                &mut sink,
            )
            .await;
            // The five gated turns only complete once the rejection is in:
            // release their burst now.
            std::fs::write(probe.dir.join("release"), b"").unwrap();
            terminal
        };
        let (r0, r1, r2, r3, r4, sixth) = tokio::join!(t0, t1, t2, t3, t4, sixth);
        for (i, r) in [r0, r1, r2, r3, r4].into_iter().enumerate() {
            assert!(
                matches!(r, TurnTerminal::Completed { .. }),
                "turn {i}: {r:?}"
            );
        }
        match sixth {
            TurnTerminal::Failed { code, .. } => assert_eq!(code, ENGINE_BUSY),
            other => panic!("expected engine_busy, got {other:?}"),
        }
        // The rejected send never reached the engine.
        let log = probe.log();
        assert_eq!(log.matches("\"method\":\"turn/start\"").count(), 5, "{log}");
        // One child served all five.
        assert_eq!(probe.spawns(), 1, "{log}");
    }

    /// Fake for the crash test's first child: replies to the first
    /// `turn/start`, then dies mid-turn.
    fn crash_script() -> String {
        PRELUDE.to_string()
            + r#"
n=0
while read -r line; do
  echo "$line" >> "$DIR/requests.log"
  case "$line" in
    *'"method":"thread/start"'*)
      n=$((n+1)); id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"threadId":"%s"}}\n' "$id" "th-crash-$n";;
    *'"method":"turn/start"'*)
      id=$(rid "$line"); t=$(tidp "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"turn":{"id":"turn-%s"}}}\n' "$id" "$t"
      exit 1;;
  esac
done
"#
    }

    /// Fake for the crash test's second child (and any later spawn): resumes
    /// any thread and completes every turn.
    fn resume_script() -> String {
        PRELUDE.to_string()
            + r#"
while read -r line; do
  echo "$line" >> "$DIR/requests.log"
  case "$line" in
    *'"method":"thread/start"'*)
      n=$(printf '%s' "$DIR/requests.log" | grep -c '"method":"thread/start"')
      id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"threadId":"%s"}}\n' "$id" "th-after-$n";;
    *'"method":"thread/resume"'*)
      id=$(rid "$line"); t=$(tidp "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"threadId":"%s"}}\n' "$id" "$t";;
    *'"method":"turn/start"'*)
      id=$(rid "$line"); t=$(tidp "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"turn":{"id":"turn-%s"}}}\n' "$id" "$t"
            printf '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"%s","text":"%s"}}\n' "$t" "back-$t"
            printf '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"%s","turn":{"id":"%s","status":"completed"},"done":{"text":"%s"}}}\n' "$t" "$t" "resumed-$t";;
    *'"method":"turn/interrupt"'*)
      id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id";;
    *'"method":"thread/unsubscribe"'*)
      id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"status":"unsubscribed"}}\n' "$id";;
  esac
done
"#
    }

    #[tokio::test]
    async fn killed_child_fails_only_running_turns_and_next_send_resumes() {
        let probe = Probe::new(vec![&crash_script(), &resume_script()]);
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        // Both sessions warm on the first child; only A starts a turn.
        let mut sink = RecSink::default();
        probe
            .server
            .warm(b, probe.warm_spec(), &mut sink)
            .await
            .expect("B warms");
        let b_thread = sink.warms.last().expect("B warmed").0.clone();
        let mut sa = RecSink::default();
        let send_a = run_send(&probe.server, a, probe.send_spec("doomed"), &mut sa);
        let terminal_a = send_a.await;
        let (code, _) = terminal_of(terminal_a);
        assert_eq!(
            code,
            format!("failed:{ENGINE_RESTARTED}"),
            "log:\n{}",
            probe.log()
        );
        // The idle session survived the crash with its thread id intact.
        assert!(probe.server.contains_session(b).await);
        // A's next send restarts the child and resumes A's thread.
        let mut sa2 = RecSink::default();
        let terminal = run_send(&probe.server, a, probe.send_spec("again"), &mut sa2).await;
        let (status, text) = terminal_of(terminal);
        assert_eq!(status, "completed", "log:\n{}", probe.log());
        assert!(
            text.as_ref().is_some_and(|t| t.contains("resumed-")),
            "{text:?}"
        );
        // No second warm was needed and no new thread was started for A.
        assert!(
            sa2.started.is_empty(),
            "resumed threads do not re-emit started"
        );
        assert_eq!(probe.spawns(), 2);
        // B resumes its own pre-crash thread on the new child too.
        let mut sb = RecSink::default();
        let terminal = run_send(&probe.server, b, probe.send_spec("b here"), &mut sb).await;
        assert!(
            matches!(terminal, TurnTerminal::Completed { .. }),
            "{terminal:?}"
        );
        assert!(
            sb.events.iter().all(|(_, _, p)| {
                p["params"]["threadId"]
                    .as_str()
                    .is_some_and(|t| t == b_thread)
            }),
            "B crossed threads: {:?}",
            sb.events
        );
        // Two thread starts on the first child, two resumes on the second —
        // no new thread was started for either resumed session.
        let log = probe.log();
        assert_eq!(
            log.matches("\"method\":\"thread/start\"").count(),
            2,
            "{log}"
        );
        assert_eq!(
            log.matches("\"method\":\"thread/resume\"").count(),
            2,
            "{log}"
        );
    }

    /// Fake whose first turn hangs until `$DIR/release` exists.
    fn release_gated_script() -> String {
        PRELUDE.to_string()
            + r#"
n=0
while read -r line; do
  echo "$line" >> "$DIR/requests.log"
  case "$line" in
    *'"method":"thread/start"'*)
      n=$((n+1)); id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"threadId":"%s"}}\n' "$id" "th-g-$n";;
    *'"method":"thread/resume"'*)
      id=$(rid "$line"); t=$(tidp "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"threadId":"%s"}}\n' "$id" "$t";;
    *'"method":"turn/start"'*)
      id=$(rid "$line"); t=$(tidp "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"turn":{"id":"turn-%s"}}}\n' "$id" "$t"
      while [ ! -f "$DIR/release" ]; do sleep 0.05; done
            printf '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"%s","text":"%s"}}\n' "$t" "late-$t"
            printf '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"%s","turn":{"id":"%s","status":"completed"},"done":{"text":"late-done"}}}\n' "$t" "$t";;
    *'"method":"turn/interrupt"'*)
      id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id";;
    *'"method":"thread/unsubscribe"'*)
      id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"status":"unsubscribed"}}\n' "$id";;
  esac
done
"#
    }

    #[tokio::test]
    async fn profile_change_during_turn_restarts_only_after_it_ends() {
        let probe = Probe::new(vec![&release_gated_script()]);
        let id = Uuid::new_v4();
        let mut sink = RecSink::default();
        let send = tokio::time::timeout(
            Duration::from_secs(15),
            probe.server.send(id, probe.send_spec("slow"), &mut sink),
        );
        tokio::pin!(send);
        // Drive the send until its turn is running, then note the profile
        // change: it must only arm the restart, not restart yet.
        let noted_at = loop {
            tokio::select! {
                biased;
                r = &mut send => panic!("send ended before the profile change: {r:?}"),
                _ = tokio::time::sleep(Duration::from_millis(20)) => {
                    if probe.running_count() == 1 {
                        probe.server.note_profile_changed();
                        break Instant::now();
                    }
                }
            }
        };
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(probe.spawns(), 1, "child restarted while a turn ran");
        assert_eq!(probe.pids().len(), 1);
        // Ending the turn runs the delayed restart: the old child is gone
        // and the next warm spawns a new one.
        std::fs::write(probe.dir.join("release"), "go").unwrap();
        let terminal = send.await.expect("send timed out");
        assert!(
            matches!(terminal, TurnTerminal::Completed { .. }),
            "{terminal:?}"
        );
        assert!(noted_at.elapsed() < Duration::from_secs(14));
        // The restart killed the old child; lazily, so no new spawn yet.
        assert_eq!(probe.spawns(), 1);
        assert_eq!(probe.running_count(), 0);
        let mut sink = RecSink::default();
        probe
            .server
            .warm(Uuid::new_v4(), probe.warm_spec(), &mut sink)
            .await
            .expect("warm after restart");
        assert_eq!(probe.spawns(), 2);
        assert_eq!(probe.pids().len(), 2);
        assert_ne!(probe.pids()[0], probe.pids()[1]);
    }

    #[tokio::test]
    async fn sign_out_stops_the_child_and_fails_the_running_turn() {
        let probe = Probe::new(vec![&release_gated_script()]);
        let id = Uuid::new_v4();
        let mut sink = RecSink::default();
        let send = tokio::time::timeout(
            Duration::from_secs(15),
            probe.server.send(id, probe.send_spec("slow"), &mut sink),
        );
        tokio::pin!(send);
        // Drive the send until its turn is running, then sign out: the
        // running turn fails and the child stops.
        loop {
            tokio::select! {
                biased;
                r = &mut send => panic!("send ended before sign-out: {r:?}"),
                _ = tokio::time::sleep(Duration::from_millis(20)) => {
                    if probe.running_count() == 1 {
                        probe.set_availability(false, ENGINE_SIGNED_OUT);
                        probe.server.refresh_availability();
                        break;
                    }
                }
            }
        }
        let terminal = send.await.expect("send timed out");
        let (code, _) = terminal_of(terminal);
        assert_eq!(
            code,
            format!("failed:{ENGINE_SIGNED_OUT}"),
            "log:\n{}",
            probe.log()
        );
        // No new child, and a warm fails without spawning one.
        assert_eq!(probe.spawns(), 1);
        std::fs::write(probe.dir.join("release"), "go").unwrap();
        let mut sink = RecSink::default();
        let err = probe
            .server
            .warm(Uuid::new_v4(), probe.warm_spec(), &mut sink)
            .await
            .expect_err("warm must fail while signed out");
        assert!(format!("{err:#}").contains(ENGINE_SIGNED_OUT), "{err:#}");
        assert_eq!(probe.spawns(), 1, "warm spawned while signed out");
        assert!(!sink.warm_failed.is_empty());
    }

    #[tokio::test]
    async fn cancel_interrupts_the_running_turn() {
        let probe = Probe::new(vec![&release_gated_script()]);
        let id = Uuid::new_v4();
        let mut sink = RecSink::default();
        let send = tokio::time::timeout(
            Duration::from_secs(15),
            probe.server.send(id, probe.send_spec("slow"), &mut sink),
        );
        tokio::pin!(send);
        // Drive the send until its turn is running, then cancel it.
        loop {
            tokio::select! {
                biased;
                r = &mut send => panic!("send ended before cancel: {r:?}"),
                _ = tokio::time::sleep(Duration::from_millis(20)) => {
                    if probe.running_count() == 1 {
                        probe.server.cancel(id);
                        break;
                    }
                }
            }
        }
        let terminal = send.await.expect("send timed out");
        match terminal {
            TurnTerminal::Completed { status, .. } => assert_eq!(status, "cancelled"),
            other => panic!("expected cancelled, got {other:?}"),
        }
        // The interrupt reached the engine; release the fake afterwards so
        // the child can exit cleanly.
        std::fs::write(probe.dir.join("release"), "go").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let log = probe.log();
            if log.contains("\"method\":\"turn/interrupt\"") {
                break;
            }
            assert!(Instant::now() < deadline, "no interrupt in log:\n{log}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn approval_request_routes_by_thread_and_answer_returns() {
        let script = PRELUDE.to_string()
            + r#"
while read -r line; do
  echo "$line" >> "$DIR/requests.log"
  case "$line" in
    *'"method":"thread/start"'*)
      id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"threadId":"%s"}}\n' "$id" "th-appr";;
    *'"method":"turn/start"'*)
      id=$(rid "$line"); t=$(tidp "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{"turn":{"id":"turn-appr"}}}\n' "$id"
            printf '{"jsonrpc":"2.0","id":77,"method":"item/commandExecution/requestApproval","params":{"threadId":"%s","command":"rm -rf /"}}\n' "$t"
      read -r ans; echo "$ans" >> "$DIR/requests.log"
            printf '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"%s","turn":{"id":"turn-appr","status":"completed"},"done":{"text":"ok"}}}\n' "$t";;
    *'"method":"turn/interrupt"'*)
      id=$(rid "$line")
            printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id";;
  esac
done
"#;
        let probe = Probe::new(vec![&script]);
        let id = Uuid::new_v4();
        let mut sink = RecSink {
            decide: Some(ApprovalDecision::Accept),
            ..Default::default()
        };
        let terminal = run_send(&probe.server, id, probe.send_spec("do it"), &mut sink).await;
        assert!(
            matches!(terminal, TurnTerminal::Completed { .. }),
            "{terminal:?}"
        );
        assert_eq!(sink.approvals.len(), 1, "log:\n{}", probe.log());
        let log = probe.log();
        assert!(
            log.contains("\"id\":77") && log.contains("\"decision\":\"accept\""),
            "engine never got the approval answer:\n{log}"
        );
    }
}
