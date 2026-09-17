//! Shared, always-on codex engine.
//!
//! [`super::bridge::Bridge`] can already multiplex many conversations/runs as
//! separate threads on one app-server process (`warm_session` / `run_session` /
//! `release_session` / `next_session_event`). But every method on it takes
//! `&mut self`, and `next_session_event` *awaits the engine's stdout* — so a
//! single shared `Bridge` behind a mutex would let whichever lane is parked in
//! `next_session_event` block the other lane from ever sending a turn. That
//! violates the "a chat turn and an issue run stream at the same time without
//! either stalling" requirement.
//!
//! [`SharedCodexEngine`] resolves that by owning the `Bridge` inside a single
//! actor task. The actor is the only place `&mut Bridge` lives: it `select!`s
//! between inbound commands (warm / run / approval / interrupt / release) and
//! the engine's next demuxed event, fanning each event out to the owning
//! session's own channel. Callers hold cheap, clonable [`EngineHandle`]s and
//! never touch the `Bridge` directly, so the chat lane and the issue-run lane
//! can drive one engine process concurrently — each in its own thread, each
//! reading only its own events — with no head-of-line blocking on streaming
//! frames.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

use crate::agent::{AgentProcessHandle, StderrRing, StderrSnapshot};
use crate::cloud::protocol::ApprovalDecision;
use crate::codex::bridge::{Bridge, BridgeEvent, RunPayload};
use crate::util::shell::AgentEnv;

/// A live session on the shared engine: the codex thread its turn runs on and a
/// stream carrying only that session's demultiplexed events.
///
/// The events channel closes (yields `None`) when the session is released or
/// the engine process exits, so a caller draining it observes the end of its
/// own conversation without seeing any other session's frames.
pub struct EngineSession {
    pub thread_id: String,
    pub events: mpsc::UnboundedReceiver<BridgeEvent>,
}

/// Commands the actor accepts. Each carries a `oneshot` the actor replies on so
/// callers get the same return values they would from calling `Bridge` directly.
enum EngineCommand {
    Warm {
        session: String,
        cwd: PathBuf,
        reply: oneshot::Sender<Result<String>>,
    },
    Run {
        session: String,
        payload: RunPayload,
        cwd: PathBuf,
        reply: oneshot::Sender<Result<EngineSession>>,
    },
    Approval {
        approval_id: String,
        decision: ApprovalDecision,
        reply: oneshot::Sender<Result<()>>,
    },
    Interrupt {
        reply: oneshot::Sender<Result<()>>,
    },
    Release {
        session: String,
        reply: oneshot::Sender<Option<String>>,
    },
    LiveThreadCount {
        reply: oneshot::Sender<usize>,
    },
}

/// A cheap, clonable handle to a [`SharedCodexEngine`]. Every lane that wants to
/// run work on the shared engine holds one; dropping the last handle (and the
/// owning `SharedCodexEngine`) shuts the actor — and the engine process — down.
#[derive(Clone)]
pub struct EngineHandle {
    cmd_tx: mpsc::Sender<EngineCommand>,
    /// Captured once at construction: pid + exit watch for the one engine
    /// process. Shared by every session (the engine is a shared crash domain),
    /// so a lane reads this exactly as it read the per-bridge process handle.
    process_handle: AgentProcessHandle,
    /// The engine process's stderr ring, so a lane can enrich a failure detail
    /// with recent stderr without an actor round-trip.
    stderr_ring: StderrRing,
    /// Engine-wide default model, so a session cursor can resolve the model it
    /// ran under the same way [`Bridge::run_session`] does (`payload.model` else
    /// this default).
    model_default: Option<String>,
}

impl EngineHandle {
    /// The shared engine process's observability handle (pid + exit watch).
    /// Every session shares one process, so this is the same handle regardless
    /// of which session asks.
    pub fn process_handle(&self) -> AgentProcessHandle {
        self.process_handle.clone()
    }

    /// Snapshot the shared engine's recent stderr (plus the dropped-noise
    /// tally), for enriching a failed turn's detail.
    pub async fn recent_stderr(&self) -> StderrSnapshot {
        self.stderr_ring.lock().await.snapshot()
    }

    /// The engine-wide default model, if configured.
    pub fn model_default(&self) -> Option<&str> {
        self.model_default.as_deref()
    }

    async fn send<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<T>) -> EngineCommand,
    ) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(make(tx))
            .await
            .map_err(|_| anyhow!("codex engine actor is gone"))?;
        rx.await
            .map_err(|_| anyhow!("codex engine actor dropped the reply"))
    }

    /// Acquire (or reuse) the codex thread bound to `session`, returning its
    /// thread id. Idempotent, mirroring [`Bridge::warm_session`].
    pub async fn warm_session(&self, session: &str, cwd: &Path) -> Result<String> {
        self.send(|reply| EngineCommand::Warm {
            session: session.to_string(),
            cwd: cwd.to_path_buf(),
            reply,
        })
        .await?
    }

    /// Start a turn for `session`, warming its thread first if needed, and
    /// return an [`EngineSession`] whose `events` receiver carries only this
    /// session's demuxed frames.
    pub async fn run_session(
        &self,
        session: &str,
        payload: &RunPayload,
        cwd: &Path,
    ) -> Result<EngineSession> {
        self.send(|reply| EngineCommand::Run {
            session: session.to_string(),
            payload: payload.clone(),
            cwd: cwd.to_path_buf(),
            reply,
        })
        .await?
    }

    /// Answer an approval request the engine raised.
    pub async fn send_approval(
        &self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> Result<()> {
        self.send(|reply| EngineCommand::Approval {
            approval_id: approval_id.to_string(),
            decision,
            reply,
        })
        .await?
    }

    /// Interrupt the current turn.
    pub async fn interrupt(&self) -> Result<()> {
        self.send(|reply| EngineCommand::Interrupt { reply }).await?
    }

    /// Close the thread for `session`, keeping the engine process alive.
    /// Returns the thread id that was released, if any (or `None` if the actor
    /// is already gone).
    pub async fn release_session(&self, session: &str) -> Option<String> {
        self.send(|reply| EngineCommand::Release {
            session: session.to_string(),
            reply,
        })
        .await
        .ok()
        .flatten()
    }

    /// Number of live threads currently multiplexed on the shared engine.
    pub async fn live_thread_count(&self) -> usize {
        self.send(|reply| EngineCommand::LiveThreadCount { reply })
            .await
            .unwrap_or(0)
    }
}

/// A codex engine shared across many conversations/runs. Owns one `Bridge`
/// (one app-server process) in an actor task; hand out [`EngineHandle`]s with
/// [`SharedCodexEngine::handle`].
pub struct SharedCodexEngine {
    handle: EngineHandle,
    actor: tokio::task::JoinHandle<()>,
}

impl SharedCodexEngine {
    /// Wrap an already-built `Bridge` (a live or fake app-server) in a shared
    /// engine, spawning the actor task that owns it.
    pub fn from_bridge(bridge: Bridge) -> Self {
        // Capture the process-wide observability handles and the default model
        // *before* the bridge moves into the actor, so a lane holding an
        // `EngineHandle` can read them without an actor round-trip — exactly as
        // it read them from a per-lane `AgentBridge` before the engine was
        // shared.
        let process_handle = bridge.server.process_handle();
        let stderr_ring = bridge.server.stderr_ring();
        let model_default = bridge.model_default.clone();
        // Modest buffer: commands are short-lived RPCs, not a data plane. The
        // event fan-out uses unbounded per-session channels instead, so a slow
        // event consumer never backs up into the command path.
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let actor = tokio::spawn(run_actor(bridge, cmd_rx));
        Self {
            handle: EngineHandle {
                cmd_tx,
                process_handle,
                stderr_ring,
                model_default,
            },
            actor,
        }
    }

    /// Spawn a real codex app-server and wrap it in a shared engine.
    pub async fn spawn(
        binary: &str,
        cwd: &Path,
        model_default: Option<String>,
        effort_default: Option<String>,
    ) -> Result<Self> {
        let bridge = Bridge::spawn(binary, cwd, model_default, effort_default).await?;
        Ok(Self::from_bridge(bridge))
    }

    /// Spawn a real codex app-server with a Pi Dash-controlled environment
    /// (managed `CODEX_HOME`, bundled CLI on `PATH`, model credential file) and
    /// wrap it in a shared engine. This is the constructor the daemon uses so
    /// the shared engine authenticates exactly like the per-lane bridge did.
    pub async fn spawn_with_env(
        binary: &str,
        cwd: &Path,
        model_default: Option<String>,
        effort_default: Option<String>,
        env: &AgentEnv,
    ) -> Result<Self> {
        let bridge =
            Bridge::spawn_with_env(binary, cwd, model_default, effort_default, env).await?;
        Ok(Self::from_bridge(bridge))
    }

    /// A fresh handle to this engine.
    pub fn handle(&self) -> EngineHandle {
        self.handle.clone()
    }

    /// The actor's join handle, so callers that want to observe engine exit
    /// (crash / stdout close) can await it.
    pub fn actor_handle(&self) -> &tokio::task::JoinHandle<()> {
        &self.actor
    }
}

impl Drop for SharedCodexEngine {
    fn drop(&mut self) {
        // Dropping our own handle lets the actor observe that all senders are
        // gone and exit; abort is a belt-and-braces stop if a stray clone
        // outlives us. The `Bridge`'s AppServer is `kill_on_drop`, so the
        // engine process is reaped once the actor's future is dropped.
        self.actor.abort();
    }
}

/// The single owner of `&mut Bridge`. Multiplexes command handling and event
/// fan-out over one `select!` loop so streaming frames on one thread never wait
/// behind another lane, while each RPC still gets exclusive stdio for the brief
/// window it needs to read its response.
async fn run_actor(mut bridge: Bridge, mut cmd_rx: mpsc::Receiver<EngineCommand>) {
    // thread id → the owning session's event sink.
    let mut routes: HashMap<String, mpsc::UnboundedSender<BridgeEvent>> = HashMap::new();

    loop {
        tokio::select! {
            // Bias command handling slightly: setup RPCs are quick and let a
            // new session start receiving before we park on the next frame.
            biased;

            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    // Every handle dropped: no one can ask for more work.
                    break;
                };
                match cmd {
                    EngineCommand::Warm { session, cwd, reply } => {
                        let _ = reply.send(bridge.warm_session(&session, &cwd).await);
                    }
                    EngineCommand::Run { session, payload, cwd, reply } => {
                        match bridge.run_session(&session, &payload, &cwd).await {
                            Ok(thread_id) => {
                                let (evt_tx, evt_rx) = mpsc::unbounded_channel();
                                routes.insert(thread_id.clone(), evt_tx);
                                let _ = reply.send(Ok(EngineSession { thread_id, events: evt_rx }));
                            }
                            Err(e) => {
                                let _ = reply.send(Err(e));
                            }
                        }
                    }
                    EngineCommand::Approval { approval_id, decision, reply } => {
                        let _ = reply.send(bridge.send_approval(&approval_id, decision).await);
                    }
                    EngineCommand::Interrupt { reply } => {
                        let _ = reply.send(bridge.interrupt().await);
                    }
                    EngineCommand::Release { session, reply } => {
                        let released = bridge.release_session(&session);
                        if let Some(thread_id) = &released {
                            // Dropping the sender closes the session's stream.
                            routes.remove(thread_id);
                        }
                        let _ = reply.send(released);
                    }
                    EngineCommand::LiveThreadCount { reply } => {
                        let _ = reply.send(bridge.live_thread_count());
                    }
                }
            }

            ev = bridge.next_session_event() => {
                match ev {
                    Some((thread_id, events)) => {
                        if let Some(tx) = routes.get(&thread_id) {
                            for e in events {
                                // Unbounded: a slow consumer must not stall the
                                // engine loop and starve other sessions.
                                let _ = tx.send(e);
                            }
                        }
                    }
                    None => {
                        // Engine stdout closed (exit / crash). Drop every route
                        // so live sessions see their stream end, then stop.
                        routes.clear();
                        break;
                    }
                }
            }
        }
    }

    // Best-effort graceful shutdown of the app-server process.
    let Bridge { server, .. } = bridge;
    let _ = server.shutdown(Duration::from_secs(2)).await;
}
