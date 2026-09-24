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
//!
//! ## Crash / respawn (a shared crash domain that recovers)
//!
//! One process serving every session means one crash takes them all down — an
//! accepted cost (see the issue's "Decisions"). What makes it acceptable is
//! recovery. When the engine process exits, the actor observes its stdout close
//! and:
//!
//! 1. **fails the in-flight turns** — every live session's event stream closes
//!    (`next_events` yields `None`), which each lane already surfaces as a
//!    failed turn, so the user sees a failed turn rather than a silent hang;
//! 2. **stays up** — the actor keeps the same command channel, so every
//!    [`EngineHandle`] a lane holds stays valid across the replacement;
//! 3. **respawns lazily** — the next `warm`/`run` rebuilds the app-server from
//!    the retained spawn recipe, so an idle crashed engine isn't respun on a
//!    hot loop and the failing lane can read the dead process's stderr for its
//!    failure detail *before* the process is replaced;
//! 4. **resumes each live thread from its stored id** — the actor retains the
//!    `session → (thread_id, cwd)` map across the crash and issues
//!    `thread/resume` for each on the fresh process, so a chat continues its
//!    conversation on the next message instead of starting over.
//!
//! A permanently-broken engine (respawn fails, or the fresh process dies again
//! before any frame flows) is capped at [`MAX_RESPAWNS_WITHOUT_PROGRESS`]
//! consecutive attempts, after which the actor gives up and every subsequent
//! command errors — the daemon then reports failed turns rather than spinning.
//!
//! ## Idle-RSS recycle (a leak safety net)
//!
//! A long-lived process accretes memory. A fresh idle engine measures ~115 MB
//! RSS on the reference machine; that figure is of a *fresh* process, so a
//! process kept warm for hours could sit far higher. To bound that, the actor
//! samples the engine's RSS on a slow timer and, when the process is **idle**
//! (zero live threads — every chat closed and every run ended, so there is no
//! turn to lose and nothing to resume) *and* its RSS is at or above
//! [`RECYCLE_RSS_THRESHOLD_BYTES`], recycles it: the current process is
//! force-killed to free its memory immediately, and the next `warm`/`run`
//! respawns a fresh one through the same lazy path a crash uses. The recycle is
//! deliberately confined to the idle case — while any thread is live the
//! process is never killed out from under it, so an active or between-turns
//! session is never disrupted by the safety net (only by a real crash, which
//! resume already covers). The threshold is a generous multiple of the fresh
//! baseline so ordinary operation never trips it; it exists to catch runaway
//! growth, not to churn a healthy process.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

use crate::agent::{AgentProcessHandle, StderrRing, StderrSnapshot};
use crate::cloud::protocol::ApprovalDecision;
use crate::codex::bridge::{Bridge, BridgeEvent, RunPayload};
use crate::util::shell::AgentEnv;

/// How many times the engine will respawn back-to-back without any evidence of
/// progress (a frame flowing, or a warm/run succeeding) before it gives up.
/// Bounds a crash loop on a permanently-broken engine so the daemon reports
/// failed turns instead of spinning; a healthy engine resets the counter the
/// moment work flows again, so ordinary crash recovery is never affected.
const MAX_RESPAWNS_WITHOUT_PROGRESS: u32 = 5;

/// A short pause before rebuilding the app-server after a crash, so a process
/// that dies immediately on startup can't be respun in a tight busy loop.
const RESPAWN_BACKOFF: Duration = Duration::from_millis(250);

/// How often the actor samples the engine's RSS to decide whether an idle
/// process has grown enough to recycle. Deliberately slow: this is a leak
/// safety net, not a hot control loop, and each sample shells out to `ps`.
const RECYCLE_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// The resident-set-size at or above which an *idle* engine is recycled.
///
/// A fresh idle engine measures ~115 MB RSS on the reference machine, so 1 GiB
/// is roughly nine times the fresh baseline: comfortably above anything normal
/// operation produces, yet low enough to reclaim a genuinely runaway process
/// before it starves the host. The check only ever fires while the engine is
/// idle (zero live threads), so a recycle costs nothing but a respawn on the
/// next use — hence a generous, conservative threshold rather than a tight one.
const RECYCLE_RSS_THRESHOLD_BYTES: u64 = 1024 * 1024 * 1024;

/// Samples a process's resident set size in bytes from its pid, or `None` when
/// the pid is unknown or the platform sample failed. A missing sample must
/// never *force* a recycle — `None` simply means "don't recycle this tick".
/// Injectable so tests can drive a recycle deterministically without a real
/// bloated process; production uses [`default_rss_sampler`].
pub type RssSampler = Arc<dyn Fn(u32) -> Option<u64> + Send + Sync>;

/// Tunables for the idle-RSS recycle safety net (see the module docs). Exposed
/// so tests can drive a recycle deterministically with a stub sampler and a
/// short interval; production uses [`RecyclePolicy::default`].
#[derive(Clone)]
pub struct RecyclePolicy {
    /// RSS at or above which an idle engine is recycled.
    pub threshold_bytes: u64,
    /// How often to sample RSS.
    pub check_interval: Duration,
    /// Maps a pid to its RSS in bytes.
    pub sampler: RssSampler,
}

impl Default for RecyclePolicy {
    fn default() -> Self {
        Self {
            threshold_bytes: RECYCLE_RSS_THRESHOLD_BYTES,
            check_interval: RECYCLE_CHECK_INTERVAL,
            sampler: default_rss_sampler(),
        }
    }
}

/// The production RSS sampler: `ps -o rss= -p <pid>` reports the resident set
/// size in kibibytes on both macOS and Linux. On non-unix (no portable sample
/// without an extra dependency; dev machines are macOS/Linux) it always returns
/// `None`, disabling the recycle safety net there.
fn default_rss_sampler() -> RssSampler {
    Arc::new(sample_rss)
}

#[cfg(unix)]
fn sample_rss(pid: u32) -> Option<u64> {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let kib: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kib * 1024)
}

#[cfg(not(unix))]
fn sample_rss(_pid: u32) -> Option<u64> {
    None
}

/// Builds a fresh [`Bridge`] (a new app-server process) when the engine needs
/// to respawn after its process exits. `None` means the engine was constructed
/// from a pre-built bridge with no recipe to rebuild from, so it dies on exit
/// rather than recovering.
pub type RespawnFactory =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<Bridge>> + Send>> + Send + Sync>;

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

/// The process-scoped observability the engine surfaces to lanes: the current
/// app-server's pid + exit watch, and its stderr ring. Held behind a mutex and
/// swapped by the actor on respawn so a lane re-reading after a crash sees the
/// *new* process, while a lane reading between the crash and the next respawn
/// still sees the dead process's stderr for its failure detail.
struct Observability {
    process_handle: AgentProcessHandle,
    stderr_ring: StderrRing,
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
    /// Current process's pid/exit-watch + stderr ring. Shared with the actor,
    /// which swaps its contents on respawn so a lane reading after a crash sees
    /// the replacement process (the engine is a shared crash domain, but a
    /// recoverable one).
    obs: Arc<std::sync::Mutex<Observability>>,
    /// Engine-wide default model, so a session cursor can resolve the model it
    /// ran under the same way [`Bridge::run_session`] does (`payload.model` else
    /// this default). Fixed for the life of the engine (part of the spawn
    /// recipe), so it is not behind the respawn mutex.
    model_default: Option<String>,
}

impl EngineHandle {
    /// The shared engine process's observability handle (pid + exit watch).
    /// Every session shares one process, so this is the same handle regardless
    /// of which session asks; after a respawn it reflects the new process.
    pub fn process_handle(&self) -> AgentProcessHandle {
        self.obs
            .lock()
            .expect("engine observability mutex poisoned")
            .process_handle
            .clone()
    }

    /// Snapshot the shared engine's recent stderr (plus the dropped-noise
    /// tally), for enriching a failed turn's detail. Reads the *current*
    /// process's ring — between a crash and the next respawn that is still the
    /// dead process's ring, so a failure detail built on `next_events → None`
    /// captures the crash output.
    pub async fn recent_stderr(&self) -> StderrSnapshot {
        let ring = {
            self.obs
                .lock()
                .expect("engine observability mutex poisoned")
                .stderr_ring
                .clone()
        };
        ring.lock().await.snapshot()
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
    /// engine, spawning the actor task that owns it. The engine cannot respawn
    /// (no recipe), so it dies on process exit — use [`Self::spawn_with_env`] /
    /// [`Self::spawn`] for a recoverable engine, or
    /// [`Self::from_bridge_with_factory`] to supply a respawn recipe in tests.
    pub fn from_bridge(bridge: Bridge) -> Self {
        Self::from_bridge_with_respawn(bridge, None, RecyclePolicy::default())
    }

    /// [`Self::from_bridge`] with a caller-supplied respawn factory. Used in
    /// tests to make a fake app-server recoverable: `factory` builds the
    /// replacement `Bridge` (e.g. a second fake process) the way
    /// [`Self::spawn_with_env`] rebuilds a real one.
    pub fn from_bridge_with_factory<F, Fut>(bridge: Bridge, factory: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Bridge>> + Send + 'static,
    {
        Self::from_bridge_with_factory_and_recycle(bridge, factory, RecyclePolicy::default())
    }

    /// [`Self::from_bridge_with_factory`] with a caller-supplied recycle policy.
    /// Used in tests to drive the idle-RSS recycle deterministically: a stub
    /// sampler reports an over-threshold RSS and a short interval fires the
    /// check quickly, so a test need not wait for the production cadence or grow
    /// a real process past a gibibyte.
    pub fn from_bridge_with_factory_and_recycle<F, Fut>(
        bridge: Bridge,
        factory: F,
        recycle: RecyclePolicy,
    ) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Bridge>> + Send + 'static,
    {
        let respawn: RespawnFactory = Arc::new(move || Box::pin(factory()));
        Self::from_bridge_with_respawn(bridge, Some(respawn), recycle)
    }

    fn from_bridge_with_respawn(
        bridge: Bridge,
        respawn: Option<RespawnFactory>,
        recycle: RecyclePolicy,
    ) -> Self {
        // Capture the process-wide observability handles and the default model
        // *before* the bridge moves into the actor, so a lane holding an
        // `EngineHandle` can read them without an actor round-trip — exactly as
        // it read them from a per-lane `AgentBridge` before the engine was
        // shared. The observability is shared with the actor so it can swap in
        // the replacement process's handles on respawn.
        let process_handle = bridge.server.process_handle();
        let stderr_ring = bridge.server.stderr_ring();
        let model_default = bridge.model_default.clone();
        let obs = Arc::new(std::sync::Mutex::new(Observability {
            process_handle,
            stderr_ring,
        }));
        // Modest buffer: commands are short-lived RPCs, not a data plane. The
        // event fan-out uses unbounded per-session channels instead, so a slow
        // event consumer never backs up into the command path.
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let actor = tokio::spawn(run_actor(bridge, cmd_rx, obs.clone(), respawn, recycle));
        Self {
            handle: EngineHandle {
                cmd_tx,
                obs,
                model_default,
            },
            actor,
        }
    }

    /// Spawn a real codex app-server and wrap it in a recoverable shared engine.
    pub async fn spawn(
        binary: &str,
        cwd: &Path,
        model_default: Option<String>,
        effort_default: Option<String>,
    ) -> Result<Self> {
        Self::spawn_with_env(binary, cwd, model_default, effort_default, &AgentEnv::default()).await
    }

    /// Spawn a real codex app-server with a Pi Dash-controlled environment
    /// (managed `CODEX_HOME`, bundled CLI on `PATH`, model credential file) and
    /// wrap it in a recoverable shared engine. This is the constructor the
    /// daemon uses so the shared engine authenticates exactly like the per-lane
    /// bridge did — and rebuilds itself the same way after a crash.
    pub async fn spawn_with_env(
        binary: &str,
        cwd: &Path,
        model_default: Option<String>,
        effort_default: Option<String>,
        env: &AgentEnv,
    ) -> Result<Self> {
        let bridge = Bridge::spawn_with_env(
            binary,
            cwd,
            model_default.clone(),
            effort_default.clone(),
            env,
        )
        .await?;
        // The recipe, captured by value so a respawn can rebuild an identical
        // process long after the caller's references have gone.
        let binary = binary.to_string();
        let cwd = cwd.to_path_buf();
        let env = env.clone();
        let factory: RespawnFactory = Arc::new(move || {
            let binary = binary.clone();
            let cwd = cwd.clone();
            let model_default = model_default.clone();
            let effort_default = effort_default.clone();
            let env = env.clone();
            Box::pin(async move {
                Bridge::spawn_with_env(&binary, &cwd, model_default, effort_default, &env).await
            })
        });
        Ok(Self::from_bridge_with_respawn(
            bridge,
            Some(factory),
            RecyclePolicy::default(),
        ))
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

/// Per-session bookkeeping the actor keeps *outside* the `Bridge` so it survives
/// a process replacement: the codex thread the session last ran on and the cwd
/// it ran in. Both are needed to `thread/resume` the session on a fresh process.
type ResumeMap = HashMap<String, (String, PathBuf)>;

/// The single owner of `&mut Bridge`. Multiplexes command handling and event
/// fan-out over one `select!` loop so streaming frames on one thread never wait
/// behind another lane, while each RPC still gets exclusive stdio for the brief
/// window it needs to read its response. Recovers from a process crash by
/// respawning lazily and resuming live threads (see the module docs).
async fn run_actor(
    mut bridge: Bridge,
    mut cmd_rx: mpsc::Receiver<EngineCommand>,
    obs: Arc<std::sync::Mutex<Observability>>,
    respawn: Option<RespawnFactory>,
    recycle: RecyclePolicy,
) {
    // thread id → the owning session's event sink.
    let mut routes: HashMap<String, mpsc::UnboundedSender<BridgeEvent>> = HashMap::new();
    // session → (thread id, cwd), retained across a crash to resume threads.
    let mut resume_map: ResumeMap = HashMap::new();
    // Is `bridge` a live process? Flips to false when its stdout closes, back to
    // true once a respawn succeeds.
    let mut alive = true;
    // Consecutive respawns with no intervening progress; see the constant.
    let mut respawns_since_progress: u32 = 0;

    // Idle-RSS recycle timer. The first tick fires immediately; that first
    // sample is of a fresh process (well under threshold), so it is a no-op.
    let mut recycle_check = tokio::time::interval(recycle.check_interval);
    recycle_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

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
                        if !ensure_alive(
                            &mut bridge, &obs, &respawn, &mut resume_map,
                            &mut alive, &mut respawns_since_progress,
                        ).await {
                            let _ = reply.send(Err(anyhow!(
                                "codex engine is down and could not be respawned"
                            )));
                            continue;
                        }
                        let res = bridge.warm_session(&session, &cwd).await;
                        if let Ok(thread_id) = &res {
                            resume_map.insert(session, (thread_id.clone(), cwd));
                            respawns_since_progress = 0;
                        }
                        let _ = reply.send(res);
                    }
                    EngineCommand::Run { session, payload, cwd, reply } => {
                        if !ensure_alive(
                            &mut bridge, &obs, &respawn, &mut resume_map,
                            &mut alive, &mut respawns_since_progress,
                        ).await {
                            let _ = reply.send(Err(anyhow!(
                                "codex engine is down and could not be respawned"
                            )));
                            continue;
                        }
                        match bridge.run_session(&session, &payload, &cwd).await {
                            Ok(thread_id) => {
                                let (evt_tx, evt_rx) = mpsc::unbounded_channel();
                                routes.insert(thread_id.clone(), evt_tx);
                                resume_map.insert(session, (thread_id.clone(), cwd));
                                respawns_since_progress = 0;
                                let _ = reply.send(Ok(EngineSession { thread_id, events: evt_rx }));
                            }
                            Err(e) => {
                                let _ = reply.send(Err(e));
                            }
                        }
                    }
                    EngineCommand::Approval { approval_id, decision, reply } => {
                        // The turn an approval answers only exists on a live
                        // process; if the engine crashed the turn is already
                        // dead, so a dropped approval is moot — don't respawn
                        // just to deliver it.
                        let res = if alive {
                            bridge.send_approval(&approval_id, decision).await
                        } else {
                            Ok(())
                        };
                        let _ = reply.send(res);
                    }
                    EngineCommand::Interrupt { reply } => {
                        // Nothing to interrupt on a dead process; its turns are
                        // already failing. Reply Ok without respawning.
                        let res = if alive { bridge.interrupt().await } else { Ok(()) };
                        let _ = reply.send(res);
                    }
                    EngineCommand::Release { session, reply } => {
                        // Map removal only — safe whether the process is alive,
                        // dead, or freshly respawned. Drop the resume entry too
                        // so a later crash doesn't resurrect a closed session.
                        resume_map.remove(&session);
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

            // Only poll the engine's stdout while the process is alive; a dead
            // bridge would return `None` immediately and busy-loop. When dead,
            // the actor parks on `cmd_rx` and respawns on the next warm/run.
            ev = bridge.next_session_event(), if alive => {
                match ev {
                    Some((thread_id, events)) => {
                        // A frame flowed: the engine is working, so forgive any
                        // accumulated respawn attempts.
                        respawns_since_progress = 0;
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
                        // so live sessions see their stream end (a failed turn),
                        // and mark the process dead — but keep the actor (and so
                        // every EngineHandle) alive to respawn on next use.
                        routes.clear();
                        alive = false;
                    }
                }
            }

            // Idle-RSS recycle safety net. Only meaningful while the process is
            // alive and idle: killing it out from under a live thread would lose
            // work, so a bloated-but-busy engine is left alone until it drains.
            _ = recycle_check.tick(), if alive => {
                if let Some((pid, bytes)) = idle_engine_to_recycle(&bridge, &recycle).await {
                    tracing::info!(
                        rss_bytes = bytes,
                        pid,
                        threshold = recycle.threshold_bytes,
                        "recycling idle codex engine above RSS threshold"
                    );
                    // Free the memory now; the next warm/run respawns a fresh
                    // process through the same lazy path a crash uses. A
                    // deliberate recycle is not a crash, so it leaves the respawn
                    // cap counter untouched (already 0 after any successful
                    // warm/run).
                    bridge.server.request_force_kill();
                    routes.clear();
                    alive = false;
                }
            }
        }
    }

    // Best-effort graceful shutdown of the app-server process.
    let Bridge { server, .. } = bridge;
    let _ = server.shutdown(Duration::from_secs(2)).await;
}

/// Decide whether the engine should be recycled this tick. Returns
/// `Some((pid, rss_bytes))` only when the process is idle (zero live threads)
/// *and* its sampled RSS is at or above the policy threshold; `None` otherwise
/// (busy, no pid, or the sample failed — a missing sample never forces a
/// recycle). The RSS sample runs on a blocking pool since the default sampler
/// shells out to `ps`.
async fn idle_engine_to_recycle(bridge: &Bridge, recycle: &RecyclePolicy) -> Option<(u32, u64)> {
    if bridge.live_thread_count() != 0 {
        return None;
    }
    let pid = bridge.server.process_handle().pid?;
    let sampler = recycle.sampler.clone();
    let bytes = tokio::task::spawn_blocking(move || sampler(pid))
        .await
        .ok()
        .flatten()?;
    (bytes >= recycle.threshold_bytes).then_some((pid, bytes))
}

/// Ensure `bridge` is a live process, respawning it (and resuming every live
/// thread from its stored id) if it crashed. Returns `true` when the engine is
/// ready to serve, `false` when it cannot be recovered (no respawn recipe, the
/// respawn cap was hit, or the rebuild itself failed) — the caller then errors
/// the command so the lane reports a failed turn instead of hanging.
async fn ensure_alive(
    bridge: &mut Bridge,
    obs: &Arc<std::sync::Mutex<Observability>>,
    respawn: &Option<RespawnFactory>,
    resume_map: &mut ResumeMap,
    alive: &mut bool,
    respawns_since_progress: &mut u32,
) -> bool {
    if *alive {
        return true;
    }
    let Some(factory) = respawn else {
        return false;
    };
    if *respawns_since_progress >= MAX_RESPAWNS_WITHOUT_PROGRESS {
        tracing::error!(
            attempts = *respawns_since_progress,
            "codex engine crashed repeatedly with no progress; giving up on respawn"
        );
        return false;
    }
    *respawns_since_progress += 1;
    tokio::time::sleep(RESPAWN_BACKOFF).await;

    let new_bridge = match factory().await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("codex engine respawn failed: {e:#}");
            return false;
        }
    };
    *bridge = new_bridge;

    // Point the shared observability at the replacement process so lanes that
    // re-read pid / exit-watch / stderr after this see the new process.
    {
        let mut o = obs.lock().expect("engine observability mutex poisoned");
        o.process_handle = bridge.server.process_handle();
        o.stderr_ring = bridge.server.stderr_ring();
    }

    // Resume every session that was live before the crash, from its stored
    // thread id, so a conversation continues on the next turn. A session that
    // won't resume is dropped from the map — its next warm/run starts fresh
    // rather than erroring forever.
    let mut unresumable = Vec::new();
    for (session, (thread_id, cwd)) in resume_map.iter() {
        if let Err(e) = bridge.resume_session(session, thread_id, cwd).await {
            tracing::warn!(
                session = %session,
                thread_id = %thread_id,
                "codex thread resume failed after respawn: {e:#}"
            );
            unresumable.push(session.clone());
        }
    }
    for session in unresumable {
        resume_map.remove(&session);
    }

    *alive = true;
    tracing::info!(
        resumed = resume_map.len(),
        "codex engine respawned and resumed live threads"
    );
    true
}
