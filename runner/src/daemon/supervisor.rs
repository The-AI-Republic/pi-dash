use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc, oneshot, watch};

use crate::agent::{AgentBridge, AgentCursor, BridgeEvent, RunPayload, within_startup_deadline};
use crate::approval::policy::Policy;
use crate::approval::router::{ApprovalRecord, ApprovalRouter, ApprovalStatus, DecisionSource};
use crate::cloud::http::{
    AckEntry, AttachBody, CredentialsHandle, HttpLoop, InboundEnvelope, PollStatus,
    RunnerCloudClient, RunnerCredentials, SharedHttpTransport, TransportError,
};
use crate::cloud::protocol::{
    ClientMsg, FailureReason, RunnerStatus, ServerMsg, TokenUsage as WireTokenUsage, WIRE_VERSION,
    WorkspaceState,
};
use crate::config::schema::{
    AgentKind, Config, Credentials, DaemonConfig, RunnerConfig, canonical_for_compare,
};
use crate::daemon::activity_watchdog::{ActivityWatchdog, WatchdogAction, describe_threshold};
use crate::daemon::drain_journal;
use crate::daemon::run_event_mirror::RunEventMirror;
use crate::daemon::runner_instance::RunnerInstance;
use crate::daemon::runner_out::RunnerOut;
use crate::daemon::state::{AgentHealthReport, ChatTeardown, DaemonShutdownTrigger, StateHandle};
use crate::daemon::workspace_tokens::WorkspaceTokens;
use crate::history::index::{RunSummary, RunsIndex};
use crate::history::jsonl::{HistoryEntry, HistoryWriter};
use crate::ipc::protocol::{
    AuthEventAccountInfo, AuthEventInfo, AuthState, CurrentRunSummary, HelloAccount, HelloInfo,
    HelloWorkspace, IPC_VERSION, Response, RunnerChanges, version_lt,
};
use crate::ipc::server::IpcServer;
use crate::util::chat_unit::ChatUnit;
use crate::util::containment::{self, CleanupOutcome, RunGroup};

/// Empty a run's containment unit and execute its run-end hook, if the
/// run wrote one (parity-stack teardown). Runs on the normal terminal
/// path, before the cloud is told the run ended; the abort path relies on
/// the guard's [`Drop`] (processes only — hooks are left for the sweep).
/// The hook runs once even when processes are left over: it tears down
/// what lives outside the unit and must not wait on what is inside it.
async fn run_cleanup(guard: &mut RunGroup) -> CleanupOutcome {
    let outcome = guard.cleanup().await;
    if outcome == CleanupOutcome::Clean {
        guard.mark_cleaned();
    }
    if let Some(ws) = guard.workspace().map(|p| p.to_path_buf()) {
        containment::run_end_hook(&ws).await;
    }
    outcome
}

/// How often a runner held by unresolved run-end cleanup re-checks it.
const UNRESOLVED_CLEANUP_RECHECK: Duration = Duration::from_secs(5);
use crate::util::fs::create_dir_all_private;
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
        // Credential presence at startup: surface the same warning the
        // pre-pickup gate uses, so an operator sees it without waiting for an
        // Assign to be rejected. Presence-only; never validated.
        for runner_cfg in &config.runners {
            // Managed Codex runners carry Pi Dash-managed auth and skip the
            // presence check, like Engine.
            if crate::agent::auth::is_managed_codex(&runner_cfg.codex) {
                continue;
            }
            if !crate::agent::auth::has_credentials(runner_cfg.agent.kind) {
                tracing::warn!(
                    runner = %runner_cfg.name,
                    agent = %runner_cfg.agent.kind.display_name(),
                    "credentials missing on this runner; runs will be refused at pickup until fixed: {}",
                    crate::agent::auth::fix_hint(runner_cfg.agent.kind)
                );
            }
        }
        // NEWPIDASH-255: reap run containment left by a previous daemon
        // (no run is live at start, so every leftover is orphaned) and
        // execute run-end hooks such a daemon never got to (parity-stack
        // teardown survives a daemon crash through the hook file).
        containment::sweep_orphaned_run_groups().await;
        // PDASHOSS01-417 part 3: process groups a dead daemon left (macOS)
        // are recovered from their records, then this daemon records its own.
        #[cfg(unix)]
        let recovery_hold = crate::util::group_recovery::recover_orphaned_groups(
            &paths.data_dir,
            containment::enabled(),
        )
        .await;
        #[cfg(unix)]
        crate::util::group_recovery::install(&paths.data_dir);
        for runner_cfg in &config.runners {
            containment::run_end_hook(&runner_cfg.workspace.working_dir).await;
        }
        // Daemon-level state has no single runner_id any more — runners
        // come and go under one connection. Per-instance state still
        // holds its own runner_id (set in RunnerInstance::new).

        let transport = if opts.offline {
            None
        } else {
            Some(SharedHttpTransport::new(config.daemon.cloud_url.clone())?)
        };

        // One dev-machine token per workspace (design §4.9): each runner
        // authenticates with its block's workspace token, the machine
        // session with any live one.
        let tokens = WorkspaceTokens::from_cli(config.cli.as_ref()).with_persistence(paths.clone());

        let mut instances: Vec<RunnerInstance> = Vec::new();
        for runner_cfg in &config.runners {
            let inst = build_runner_instance(
                runner_cfg.clone(),
                transport.as_ref(),
                &tokens,
                &paths,
                &config.daemon,
            )
            .await?;
            instances.push(inst);
        }
        // A group whose recovery is unresolved may still have the old run's
        // processes in a runner's working copy: keep that runner reserved
        // until a re-check resolves it (review of #3501 at 1524d8e).
        #[cfg(unix)]
        let recovery = RecoveryReservations::new(recovery_hold);
        #[cfg(not(unix))]
        let recovery = RecoveryReservations::default();
        #[cfg(unix)]
        hold_runners_for_recovery(
            recovery.clone(),
            instances
                .iter()
                .map(|i| (i.runner_id, i.state.clone()))
                .collect(),
        )
        .await;

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

        // Shared registry for IPC routing, including runners activated
        // after startup by local enrollment or machine control.
        let ipc_instances: HashMap<uuid::Uuid, RunnerInstance> = instances
            .iter()
            .cloned()
            .map(|i| (i.runner_id, i))
            .collect();
        let ipc_instances = Arc::new(std::sync::RwLock::new(ipc_instances));

        if opts.offline {
            tracing::info!("offline mode: HTTP transport disabled");
        }

        // Shared context for bringing a runner's tasks up — used by the
        // startup loop below and by the machine control session's
        // cloud-driven `create_runner` hot-add path.
        // The machine's Pi Dash Agent (design §4.3): credential, profile,
        // in-memory model token and machine-level state.
        let engine = crate::pidash_agent::manager::PidashAgentManager::new(
            transport.as_ref().map(|t| t.cloud_url()),
            paths.clone(),
            tokens.clone(),
            crate::pidash_agent::locate::resolved().map(|e| e.binary.clone()),
        );
        let engine_handle = transport.as_ref().map(|_| {
            let engine = engine.clone();
            let shutdown = state.shutdown_notified();
            tokio::spawn(async move { engine.run(shutdown).await })
        });
        // Durable FIFO coordinator (NEWPIDASH-241 T3): one SQLite store +
        // fs2 lock under data_dir. Failure enters storage_blocked: Assigns
        // fail closed (NACK, no execution) until restart with fixed storage.
        let (run_coordinator, run_queue_blocked) = match crate::daemon::run_queue::Coordinator::new(
            paths.data_dir.clone(),
            config.daemon.max_concurrent_agent_runs,
        ) {
            Ok(c) => (Some(Arc::new(c)), false),
            Err(e) => {
                tracing::warn!("run queue storage_blocked: {e}");
                (None, true)
            }
        };
        // Durable receipt-before-ACK (T4): one SQLite store shared by every
        // HttpLoop; a frame commits before any HTTP ACK leaves.
        let run_receipt =
            crate::daemon::receipt::ReceiptStore::open(paths.data_dir.join("run_receipts.sqlite3"))
                .map(Arc::new)
                .map_err(|e| {
                    tracing::warn!("run receipts storage unavailable: {e}");
                })
                .ok();
        // The engine profile's chats run on the shared app-server
        // (NEWPIDASH-333): readiness follows the engine manager through a
        // weak handle, so the profile never keeps the manager alive.
        let engine_weak = std::sync::Arc::downgrade(&engine);
        let chat_availability: crate::pidash_agent::chat_server::AvailabilitySource =
            std::sync::Arc::new(move || {
                engine_weak
                    .upgrade()
                    .map(|engine| engine.chat_availability())
                    .unwrap_or(crate::pidash_agent::chat_server::ChatAvailability {
                        can_run: false,
                        code: crate::pidash_agent::chat_server::ENGINE_START_FAILED.to_string(),
                    })
            });
        let local_chat = Arc::new(crate::ipc::pidash_agent_chat::PidashAgentChatProfile::new(
            config.daemon.dev_machine_id,
            chat_availability,
        ));
        // Lifecycle hooks: sign-out/pause stops the shared child, a model
        // profile change restarts it once idle.
        engine.set_chat_lifecycle(local_chat.server_lifecycle());
        let spawn_ctx = RunnerSpawnCtx {
            engine: engine.clone(),
            local_chat,
            transport: transport.clone(),
            tokens: tokens.clone(),
            paths: paths.clone(),
            daemon: config.daemon.clone(),
            mailboxes: mailboxes.clone(),
            hello_runners: hello_runners.clone(),
            daemon_state: state.clone(),
            instances: ipc_instances.clone(),
            recovery: recovery.clone(),
            tasks: Arc::new(std::sync::Mutex::new(HashMap::new())),
            activation_lock: Arc::new(tokio::sync::Mutex::new(())),
            engine_ops_lock: Arc::new(tokio::sync::Mutex::new(())),
            pending_idle_restarts: Default::default(),
            run_coordinator: run_coordinator.clone(),
            run_queue_blocked,
            run_receipt: run_receipt.clone(),
        };

        let ipc = IpcServer {
            path: paths.ipc_socket_path(),
            primary_state: primary
                .as_ref()
                .map(|p| p.state.clone())
                .unwrap_or_else(|| state.clone()),
            paths: paths.clone(),
            instances: ipc_instances,
            spawn_ctx: spawn_ctx.clone(),
            run_coordinator: run_coordinator.clone(),
        };
        let ipc_handle = tokio::spawn(async move {
            if let Err(e) = ipc.run().await {
                tracing::error!("ipc server exited: {e:#}");
            }
        });

        // Replay RunFailed signals a previous daemon failed to deliver
        // on shutdown, BEFORE any runner loop starts: no session is
        // open yet, so the cloud cannot have redelivered one of these
        // runs and the replay cannot race live work. Leftovers (cloud
        // unreachable) are retried in the background. The previous systemd
        // exit (NEWPIDASH-253) is read once here and carried to both the
        // replay and this daemon's own shutdown drain.
        let previous_exit = if opts.offline {
            None
        } else {
            crate::service::systemd::previous_exit_result().await
        };
        if let Some(prev) = previous_exit.as_deref() {
            tracing::info!(
                previous_exit = prev,
                "previous systemd exit observed at startup"
            );
        }
        let mut replay_retry_handle: Option<tokio::task::JoinHandle<()>> = None;
        if !opts.offline {
            replay_drain_journal_once(&paths, &hello_runners, previous_exit.clone()).await;
            // Legacy journal dispositions first, then SQLite outbox retry:
            // managed terminal signals redeliver their exact payload.
            if let Some(coord) = run_coordinator.as_ref() {
                replay_terminal_outbox_once(coord, &hello_runners, false).await;
                // What that pass could not deliver stays reported in flight
                // when the runner's session opens, until the retry loop
                // below delivers it.
                restore_pending_reports(coord, &hello_runners).await;
            }
            // Always-on retry loop (NEWPIDASH-251): normal RunFailed
            // sends journaled mid-run (not only shutdown drains) must
            // replay without waiting for the next daemon start. The
            // loop is long-lived: it never exits on an empty journal,
            // only on the shutdown abort just before the drain below.
            {
                let replay_paths = paths.clone();
                let replay_runners = hello_runners.clone();
                let replay_previous = previous_exit.clone();
                let replay_outbox = run_coordinator.clone();
                replay_retry_handle = Some(tokio::spawn(async move {
                    replay_drain_journal_retry_loop(
                        replay_paths,
                        replay_runners,
                        replay_previous,
                        replay_outbox,
                        DRAIN_JOURNAL_RETRY_INTERVAL,
                    )
                    .await;
                }));
            }
        }

        // One RunnerLoop per instance. Each consumes from its mailbox.
        // Handles are kept in the spawn context so one runner can be
        // stopped on its own (reconfigure, credential change) and every
        // runner is aborted on shutdown.
        for inst in &instances {
            let tasks = spawn_instance_tasks(inst, &spawn_ctx).await;
            spawn_ctx.register_tasks(inst.runner_id, tasks);
        }
        // Restart orphan sweep (NEWPIDASH-241 T6): waiting entries whose
        // runner no longer exists can never be re-hydrated — durable-cancel
        // them so they cannot block the FIFO forever. The cloud re-queues
        // the run through the reaper.
        if let Some(coord) = run_coordinator.as_ref() {
            let live: std::collections::HashSet<uuid::Uuid> =
                instances.iter().map(|i| i.runner_id).collect();
            for entry in coord.waiting_entries() {
                if !live.contains(&entry.runner_id) {
                    tracing::warn!(run_id=%entry.run_id, "waiting entry for removed runner; durable cancel");
                    let _ = coord.cancel_waiting(entry.run_id);
                }
            }
        }

        // Machine-level control session: lets the cloud push
        // machine-scoped commands (``create_runner``) down to this
        // machine even when it hosts zero runners. One session per
        // machine, opened with any live workspace token and re-opened
        // with another when that one is revoked (design §4.9). Needs a
        // persisted dev_machine_id, written by `pidash auth login`;
        // legacy installs without one simply don't get the channel.
        let machine_handle = match (transport.clone(), config.daemon.dev_machine_id) {
            (Some(shared), Some(dev_machine_id)) => {
                let control = crate::daemon::machine_control::MachineControl::new(
                    shared,
                    dev_machine_id,
                    spawn_ctx.clone(),
                    state.clone(),
                );
                Some(tokio::spawn(async move { control.run().await }))
            }
            _ => {
                if !opts.offline {
                    tracing::info!(
                        "machine control session disabled (no dev_machine_id in config)"
                    );
                }
                None
            }
        };

        // Engine reconcile (design §5): at start and every 10 minutes,
        // drop engine blocks whose server row is revoked and re-add this
        // machine's live engine rows that have no block. The same tick
        // removes engine versions older than the running one once no run
        // can be using them (design §4.5).
        let reconcile_handle = if opts.offline {
            None
        } else {
            let ctx = spawn_ctx.clone();
            let shutdown = state.shutdown_notified();
            Some(tokio::spawn(async move {
                crate::daemon::pidash_agent_reconcile::run(ctx, shutdown).await
            }))
        };

        // Wake from sleep / network change: retry every cloud connection
        // now instead of waiting out a backoff of up to 5 minutes.
        let triggers_handle = if opts.offline {
            None
        } else {
            let kick_ctx = spawn_ctx.clone();
            Some(crate::daemon::reconnect_triggers::spawn(move |trigger| {
                if let Ok(n) = kick_ctx.kick_reconnect(None) {
                    tracing::debug!(trigger, runners = n, "reconnect kicked");
                }
            }))
        };

        let shutdown = state.shutdown_notified();
        let sig = crate::util::signal::shutdown();
        let shutdown_trigger = tokio::select! {
            _ = shutdown.notified() => {
                // The trigger was recorded before notify_waiters by the
                // caller (IPC RunnerDisconnect or invariant restart).
                state.shutdown_reason().unwrap_or(DaemonShutdownTrigger::ControlSocket)
            }
            r = sig => match r {
                Ok(signal) => DaemonShutdownTrigger::from_signal(signal),
                Err(e) => {
                    tracing::warn!("signal watcher failed: {e:#}");
                    DaemonShutdownTrigger::Unknown
                }
            },
        };
        tracing::info!(
            trigger = shutdown_trigger.as_str(),
            "daemon shutdown trigger"
        );

        // Stop the journal replay retry task BEFORE draining: a retry
        // pass running concurrently with the drain could remove() an
        // entry the drain just re-journaled for the same run_id (its
        // "resolved" set was computed from a pre-drain snapshot),
        // silently losing the signal. Await the handle so the task has
        // fully stopped — abort() alone only lands at its next await.
        if let Some(h) = replay_retry_handle {
            h.abort();
            let _ = h.await;
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
            drain_in_flight_runs(
                hello_runners.clone(),
                &paths,
                run_coordinator.clone(),
                shutdown_trigger,
                previous_exit.clone(),
            ),
        )
        .await
        {
            tracing::warn!("drain of in-flight runs timed out at 5s");
        }

        spawn_ctx.abort_all_tasks();
        // Stop the shared chat server: running turns fail with
        // `daemon_stopping` and the child is reaped with the daemon.
        spawn_ctx.local_chat.server().shutdown().await;
        if let Some(h) = machine_handle {
            h.abort();
        }
        if let Some(h) = triggers_handle {
            h.abort();
        }
        if let Some(h) = engine_handle {
            h.abort();
        }
        if let Some(h) = reconcile_handle {
            h.abort();
        }
        ipc_handle.abort();
        Ok(())
    }
}

/// Construct (but don't start) a `RunnerInstance` from its config
/// block. Shared by the startup loop and the machine control session's
/// hot-add path.
///
/// The runner authenticates with the token of its block's
/// `workspace_slug` (design §4.9). When that workspace has no token on
/// this machine and the runner has no legacy per-runner credentials, the
/// instance is built signed out: it runs no cloud loops and reports
/// `workspace_signed_out`, and the other runners are unaffected.
async fn build_runner_instance(
    runner_cfg: RunnerConfig,
    transport: Option<&SharedHttpTransport>,
    tokens: &WorkspaceTokens,
    paths: &Paths,
    daemon: &DaemonConfig,
) -> Result<RunnerInstance> {
    let inst = if let Some(shared) = transport {
        let runner_paths = paths.for_runner(runner_cfg.runner_id);
        let token = tokens.token_for(runner_cfg.workspace_slug.as_deref());
        match load_runner_credentials(&runner_paths, &runner_cfg.name, token.as_deref()).await {
            Ok(creds) => {
                let client = RunnerCloudClient::new(runner_cfg.runner_id, creds, shared.clone());
                let mut inst = RunnerInstance::new_http(runner_cfg, paths, client, daemon.clone());
                inst.machine_token = token;
                inst
            }
            Err(e) if token.is_none() => {
                tracing::warn!(
                    runner = %runner_cfg.name,
                    workspace = runner_cfg.workspace_slug.as_deref().unwrap_or("(default)"),
                    "workspace signed out on this machine; runner stays offline until \
                     `pidash auth login --workspace <slug>` ({e:#})"
                );
                let mut inst = RunnerInstance::new_offline(runner_cfg, paths, daemon.clone());
                inst.workspace_signed_out = true;
                inst
            }
            Err(e) => return Err(e),
        }
    } else {
        RunnerInstance::new_offline(runner_cfg, paths, daemon.clone())
    };
    inst.paths.ensure()?;
    // IPC must report the configured identity even before the first heartbeat.
    inst.state.set_runner_id(inst.runner_id).await;
    Ok(inst)
}

/// Everything a post-startup code path needs to bring a new runner up
/// in-process (cloud `create_runner` → daemon hot-add, no restart).
/// All fields are cheap clones of the supervisor's shared state.
#[derive(Clone)]
pub(crate) struct RunnerSpawnCtx {
    /// The machine's Pi Dash Agent (PIDESKAPP-121).
    pub(crate) engine: Arc<crate::pidash_agent::manager::PidashAgentManager>,
    /// The machine-level engine profile local chat runs on (design §12.1).
    pub(crate) local_chat: Arc<crate::ipc::pidash_agent_chat::PidashAgentChatProfile>,
    transport: Option<SharedHttpTransport>,
    /// Workspace → dev-machine token (design §4.9). Replaces the single
    /// `shared_machine_token`; shared with the machine control session.
    pub(crate) tokens: WorkspaceTokens,
    pub(crate) paths: Paths,
    daemon: DaemonConfig,
    mailboxes: Arc<RwLock<HashMap<uuid::Uuid, mpsc::Sender<InboundEnvelope>>>>,
    hello_runners: Arc<RwLock<HelloRunnerMap>>,
    daemon_state: StateHandle,
    instances: Arc<std::sync::RwLock<HashMap<uuid::Uuid, RunnerInstance>>>,
    /// Task handles of every running runner, so one runner can be stopped
    /// (reconfigure, credential change) while the others keep running,
    /// and every runner is aborted on daemon shutdown.
    tasks: Arc<std::sync::Mutex<HashMap<uuid::Uuid, SpawnedInstanceTasks>>>,
    /// Crash-recovery reservations: a runner added while a crashed
    /// daemon's group that may be its own is unresolved starts reserved.
    recovery: RecoveryReservations,
    /// Serialises every add / stop / restart of a runner.
    activation_lock: Arc<tokio::sync::Mutex<()>>,
    /// Serialises turning the engine on for a project (`EngineRunnerAdd`,
    /// machine control's engine `create_runner`) with the engine reconcile,
    /// so a row created a moment ago is never taken for one with no block.
    pub(crate) engine_ops_lock: Arc<tokio::sync::Mutex<()>>,
    /// Runners waiting to be restarted with new credentials once idle.
    pending_idle_restarts: Arc<std::sync::Mutex<std::collections::HashSet<uuid::Uuid>>>,
    /// Durable FIFO coordinator (NEWPIDASH-241 T3). None only in unit tests
    /// that never touch the queue; production always has one.
    pub(crate) run_coordinator: Option<Arc<crate::daemon::run_queue::Coordinator>>,
    /// True when the queue store failed to open (storage_blocked): Assigns
    /// fail closed (NACK, no execution) until restart with fixed storage.
    pub(crate) run_queue_blocked: bool,
    /// Durable receipt-before-ACK store (NEWPIDASH-241 T4). Shared across
    /// runners; None only in unit tests.
    pub(crate) run_receipt: Option<Arc<crate::daemon::receipt::ReceiptStore>>,
}

/// How often a busy runner whose credentials changed is checked for idle.
const IDLE_RESTART_POLL: Duration = Duration::from_secs(2);

/// How long a stopped runner's HTTP loops get to exit before they are
/// aborted.
const RUNNER_STOP_GRACE: Duration = Duration::from_secs(5);
/// How long `RunnerRemoveLocal` waits for the runner's teardown (block
/// stripped, loop exited) before it answers.
pub(crate) const RUNNER_REMOVE_WAIT: Duration = Duration::from_secs(10);

impl RunnerSpawnCtx {
    /// Bring the runner named `name` up from its block on disk, with no
    /// daemon restart (`pidash runner add`, the TUI, `pidash __managed`).
    ///
    /// The workspace tokens are re-read first, so a runner of a workspace
    /// signed in after the daemon started gets its token. A runner that is
    /// already running is restarted in-process when its block changed, and
    /// left alone otherwise.
    pub(crate) async fn activate_configured(&self, name: &str) -> Result<()> {
        let cfg = crate::config::file::load_config(&self.paths)?;
        cfg.validate()
            .map_err(|e| anyhow::anyhow!("invalid config: {e}"))?;
        if cfg.daemon.cloud_url != self.daemon.cloud_url
            || cfg.daemon.dev_machine_id != self.daemon.dev_machine_id
        {
            anyhow::bail!("daemon identity changed; restart before activating a runner");
        }
        self.tokens.reload(cfg.cli.as_ref());
        let runner = cfg
            .runners
            .iter()
            .find(|r| r.name == name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no configured runner named {name:?}"))?;
        {
            let _activation = self.activation_lock.lock().await;
            match self.instance_by_id(runner.runner_id) {
                None => self.add_runner_locked(runner).await?,
                Some(running) if same_block(&running.config, &runner) => {}
                Some(running) => {
                    ensure_idle(&running)?;
                    self.stop_runner_locked(running.runner_id).await;
                    self.add_runner_locked(runner).await?;
                }
            }
        }
        self.restart_stale_runners().await;
        Ok(())
    }

    /// Re-read the workspace tokens from `config.toml` (`pidash auth
    /// login` / `logout`) and restart, in-process, the idle runners whose
    /// workspace token changed. Returns how many runners were restarted.
    pub(crate) async fn reload_credentials(&self) -> Result<usize> {
        let cfg = crate::config::file::load_config(&self.paths)?;
        if cfg.daemon.cloud_url != self.daemon.cloud_url {
            anyhow::bail!("daemon cloud changed; restart the daemon to apply it");
        }
        self.tokens.reload(cfg.cli.as_ref());
        // A login or logout may have issued or revoked the engine
        // credential: drop the model token and fetch the profile again.
        self.engine.credentials_changed();
        // The server's automatic pause reason (e.g. engine_sign_in_required)
        // may have cleared with this sign-in; re-read it in the background so
        // Hello and `pidash engine status` don't show the old one until the
        // next reconcile tick.
        let ctx = self.clone();
        tokio::spawn(async move { ctx.refresh_engine_pause().await });
        Ok(self.restart_stale_runners().await)
    }

    /// Restart every runner whose workspace now has a different live token
    /// than the one it runs with: a workspace signed in or re-signed in
    /// (new token), or signed out (no token, so the runner comes back up
    /// offline and stops taking work). A busy runner is not interrupted:
    /// it is restarted as soon as it is idle (see
    /// [`Self::schedule_restart_when_idle`]), because a login rotates the
    /// workspace token and the old one stops working on the server.
    async fn restart_stale_runners(&self) -> usize {
        if self.transport.is_none() {
            return 0;
        }
        let _activation = self.activation_lock.lock().await;
        let stale: Vec<RunnerInstance> = self
            .all_instances()
            .into_iter()
            .filter(|inst| {
                let desired = self.tokens.token_for(inst.config.workspace_slug.as_deref());
                desired != inst.machine_token
            })
            .collect();
        let mut restarted = 0;
        for inst in stale {
            if let Err(e) = ensure_idle(&inst) {
                tracing::info!(
                    runner = %inst.name,
                    "credentials changed; restarting once the runner is idle: {e:#}"
                );
                self.schedule_restart_when_idle(inst.runner_id);
                continue;
            }
            let cfg = inst.config.clone();
            self.stop_runner_locked(inst.runner_id).await;
            match self.add_runner_locked(cfg).await {
                Ok(()) => restarted += 1,
                Err(e) => {
                    tracing::error!(runner = %inst.name, "restart with new credentials failed: {e:#}")
                }
            }
        }
        restarted
    }

    /// Restart runner `runner_id` with the current credentials once it has
    /// no run and no local chat turn in flight. One waiter per runner; it
    /// ends when the runner is gone or no longer needs the restart.
    ///
    /// A plain `fn` that spawns a boxed future, so the spawned task can call
    /// back into [`Self::restart_stale_runners`] without a recursive future
    /// type.
    fn schedule_restart_when_idle(&self, runner_id: uuid::Uuid) {
        {
            let mut pending = self
                .pending_idle_restarts
                .lock()
                .expect("idle-restart set poisoned");
            if !pending.insert(runner_id) {
                return;
            }
        }
        let ctx = self.clone();
        let fut: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> =
            Box::pin(async move {
                loop {
                    tokio::time::sleep(IDLE_RESTART_POLL).await;
                    let Some(inst) = ctx.instance_by_id(runner_id) else {
                        break;
                    };
                    let desired = ctx.tokens.token_for(inst.config.workspace_slug.as_deref());
                    if desired == inst.machine_token {
                        break;
                    }
                    if ensure_idle(&inst).is_ok() {
                        ctx.pending_idle_restarts
                            .lock()
                            .expect("idle-restart set poisoned")
                            .remove(&runner_id);
                        ctx.restart_stale_runners().await;
                        return;
                    }
                }
                ctx.pending_idle_restarts
                    .lock()
                    .expect("idle-restart set poisoned")
                    .remove(&runner_id);
            });
        tokio::spawn(fut);
    }

    /// Change one runner's settings in-process (design §5, "a new
    /// per-runner reconfigure"): stop that runner, rewrite its block under
    /// the config lock, validate, start it again. Every other runner keeps
    /// running. Refused while the runner has a run or a local chat turn in
    /// flight. On a failed write the runner is started again unchanged.
    pub(crate) async fn reconfigure_runner(
        &self,
        name: &str,
        changes: RunnerChanges,
    ) -> Result<RunnerConfig> {
        if changes.is_empty() {
            anyhow::bail!("nothing to change");
        }
        if let Some(dir) = &changes.working_dir
            && !dir.is_absolute()
        {
            anyhow::bail!(
                "working dir must be an absolute path, got {}",
                dir.display()
            );
        }
        let _activation = self.activation_lock.lock().await;
        let inst = self
            .all_instances()
            .into_iter()
            .find(|i| i.name == name)
            .ok_or_else(|| anyhow::anyhow!("no running runner named {name:?}"))?;
        self.reconfigure_instance_locked(inst, changes).await
    }

    /// [`Self::reconfigure_runner`] for the runner with id `runner_id`
    /// (the cloud's `reconfigure_runner` machine command addresses runners
    /// by id, since a rename changes the name).
    pub(crate) async fn reconfigure_runner_by_id(
        &self,
        runner_id: uuid::Uuid,
        changes: RunnerChanges,
    ) -> Result<RunnerConfig> {
        if changes.is_empty() {
            anyhow::bail!("nothing to change");
        }
        if let Some(dir) = &changes.working_dir
            && !dir.is_absolute()
        {
            anyhow::bail!(
                "working dir must be an absolute path, got {}",
                dir.display()
            );
        }
        let _activation = self.activation_lock.lock().await;
        let inst = self
            .instance_by_id(runner_id)
            .ok_or_else(|| anyhow::anyhow!("runner {runner_id} is not running on this machine"))?;
        self.reconfigure_instance_locked(inst, changes).await
    }

    /// The body of a reconfigure, under the activation lock.
    async fn reconfigure_instance_locked(
        &self,
        inst: RunnerInstance,
        changes: RunnerChanges,
    ) -> Result<RunnerConfig> {
        let name = inst.name.clone();
        ensure_idle(&inst)?;
        let runner_id = inst.runner_id;

        // Validate against the current file before stopping anything, so
        // a rejected change never takes the runner down.
        let mut preview = crate::config::file::load_config(&self.paths)?;
        apply_runner_changes(&mut preview, runner_id, &changes)?;
        preview
            .validate()
            .map_err(|e| anyhow::anyhow!("invalid change: {e}"))?;
        // A switched agent must be installed here, or the runner would
        // come back up unable to run anything.
        if changes.agent.is_some()
            && let Some(block) = preview.runners.iter().find(|r| r.runner_id == runner_id)
        {
            let binary = agent_binary_for(block).to_string();
            let env = crate::agent::agent_env_for_config(block);
            if crate::util::shell::binary_version(&binary, &env)
                .await
                .is_none()
            {
                anyhow::bail!(
                    "the {} CLI ({binary}) was not found on this machine; install it first",
                    block.agent.kind.display_name()
                );
            }
        }

        self.stop_runner_locked(runner_id).await;
        if let Some(dir) = &changes.working_dir
            && let Err(e) = create_dir_all_private(dir)
        {
            self.add_runner_locked(inst.config.clone()).await?;
            anyhow::bail!("creating working dir {}: {e}", dir.display());
        }
        let paths = self.paths.clone();
        let write_changes = changes.clone();
        let written = tokio::task::spawn_blocking(move || {
            crate::config::file::mutate_config(&paths, |cfg| {
                apply_runner_changes(cfg, runner_id, &write_changes)
            })
        })
        .await
        .map_err(|e| anyhow::anyhow!("config write task failed: {e}"))
        .and_then(|r| r);
        let cfg = match written {
            Ok(cfg) => cfg,
            Err(e) => {
                self.add_runner_locked(inst.config.clone()).await?;
                return Err(e.context("rewriting the runner's block"));
            }
        };
        let block = cfg
            .runners
            .into_iter()
            .find(|r| r.runner_id == runner_id)
            .ok_or_else(|| anyhow::anyhow!("runner {name:?} vanished from config.toml"))?;
        self.add_runner_locked(block.clone()).await?;
        tracing::info!(runner = %name, "reconfigured runner in running daemon");
        Ok(block)
    }

    /// Wait until the runner `runner_id` is fully torn down: gone from the
    /// registry and its runner loop exited (which is after its block was
    /// stripped from `config.toml`). Lets `RunnerRemoveLocal` answer only
    /// once a re-add of the same id is safe.
    pub(crate) async fn wait_runner_removed(
        &self,
        runner_id: uuid::Uuid,
        within: Duration,
    ) -> bool {
        let deadline = Instant::now() + within;
        loop {
            let registered = self.instance_by_id(runner_id).is_some();
            let loop_running = self
                .tasks
                .lock()
                .expect("runner task map poisoned")
                .get(&runner_id)
                .and_then(|t| t.runner_loop.as_ref())
                .is_some_and(|h| !h.is_finished());
            if !registered && !loop_running {
                self.tasks
                    .lock()
                    .expect("runner task map poisoned")
                    .remove(&runner_id);
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// Whether `inst` is signed out: built with no credential, or its
    /// workspace no longer has a live token (signed out, or the cloud
    /// revoked it).
    pub(crate) fn is_workspace_signed_out(&self, inst: &RunnerInstance) -> bool {
        inst.workspace_signed_out
            || (inst.machine_token.is_some()
                && self
                    .tokens
                    .token_for(inst.config.workspace_slug.as_deref())
                    .is_none())
    }

    /// The `Hello` answer (design §4.4).
    pub(crate) fn hello(&self) -> HelloInfo {
        let engine = self.engine.status();
        let instances = self.all_instances();
        let mut slugs: std::collections::BTreeSet<String> =
            self.tokens.known_workspaces().into_iter().collect();
        for inst in &instances {
            if let Some(slug) = inst.config.workspace_slug.as_deref() {
                slugs.insert(slug.to_string());
            }
        }
        let workspaces = slugs
            .into_iter()
            .map(|slug| HelloWorkspace {
                signed_in: self.tokens.token_for(Some(&slug)).is_some(),
                runners: instances
                    .iter()
                    .filter(|i| i.config.workspace_slug.as_deref() == Some(slug.as_str()))
                    .count(),
                slug,
            })
            .collect();
        let account = if self.tokens.session_candidates().is_empty() {
            None
        } else {
            let (user_id, email) = self.tokens.account();
            Some(HelloAccount {
                user_id,
                email,
                cloud_url: self.daemon.cloud_url.clone(),
            })
        };
        // The persisted record, read from disk so a sign-in or sign-out in a
        // terminal since startup is reported at once. A daemon older than
        // this build has no record (design §8): the app starts recording
        // from the first `seq` the updated CLI reports.
        let auth_event = crate::config::file::load_auth_event(&self.paths).map(|e| AuthEventInfo {
            seq: e.seq,
            kind: e.kind.as_str().to_string(),
            reason: e.reason.as_ref().map(|r| r.as_str().to_string()),
            account: e.account.map(|a| AuthEventAccountInfo {
                user_id: a.user_id,
                email: a.email,
                cloud_url: a.cloud_url,
            }),
            at: e.at,
        });
        HelloInfo {
            daemon_version: crate::RUNNER_VERSION.to_string(),
            ipc_version: IPC_VERSION,
            cloud_url: self.daemon.cloud_url.clone(),
            account,
            auth_state: Some(if self.tokens.session_candidates().is_empty() {
                AuthState::SignedOut
            } else {
                AuthState::SignedIn
            }),
            auth_event,
            workspaces,
            machine_id: self.daemon.dev_machine_id,
            engine: engine.state,
            engine_detail: Some(engine),
        }
    }

    /// Desktop session handoff (desktop_unified_signin §5): trade the app's
    /// `challenge` for a single-use code with one workspace daemon
    /// credential. The daemon hands the code to the app over this socket
    /// only; the code is never logged (it is useless without the app's
    /// verifier, which never leaves the app's process).
    pub(crate) async fn session_handoff(&self, challenge: &str) -> Result<Response> {
        let cloud_url = self.daemon.cloud_url.clone();
        if cloud_url.is_empty() {
            anyhow::bail!("this computer is not bound to a Pi Dash cloud");
        }
        let (_, token) = self
            .tokens
            .session_candidates()
            .into_iter()
            .find(|(_, t)| t.starts_with("mt_"))
            .ok_or_else(|| anyhow::anyhow!("this computer is signed out"))?;
        let url = format!("{cloud_url}/api/v1/auth/desktop-handoff/");
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        let resp = client
            .post(&url)
            .header("X-Api-Key", token)
            .json(&serde_json::json!({ "challenge": challenge }))
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            // Status (and the server's error code), never the code or verifier.
            let detail = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string));
            anyhow::bail!(
                "the handoff was refused (HTTP {status}{})",
                detail.map(|d| format!(": {d}")).unwrap_or_default()
            );
        }
        let body: serde_json::Value = resp.json().await.context("parsing the handoff response")?;
        let code = body
            .get("code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("the handoff response carried no code"))?;
        Ok(Response::SessionHandoff {
            code: code.to_string(),
            user_id: body
                .get("user_id")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            email: body
                .get("email")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        })
    }

    fn all_instances(&self) -> Vec<RunnerInstance> {
        self.instances
            .read()
            .expect("runner registry poisoned")
            .values()
            .cloned()
            .collect()
    }

    fn instance_by_id(&self, runner_id: uuid::Uuid) -> Option<RunnerInstance> {
        self.instances
            .read()
            .expect("runner registry poisoned")
            .get(&runner_id)
            .cloned()
    }

    fn register_tasks(&self, runner_id: uuid::Uuid, tasks: SpawnedInstanceTasks) {
        if let Some(old) = self
            .tasks
            .lock()
            .expect("runner task map poisoned")
            .insert(runner_id, tasks)
        {
            old.abort();
        }
    }

    /// Abort every runner's tasks (daemon shutdown).
    fn abort_all_tasks(&self) {
        for (_, tasks) in self.tasks.lock().expect("runner task map poisoned").drain() {
            tasks.abort();
        }
    }

    /// Stop one runner without touching its block or data dir: take it out
    /// of every registry, let its HTTP loops exit (no further cloud
    /// requests), then stop its runner loop. Caller holds
    /// `activation_lock` and has checked the runner is idle.
    async fn stop_runner_locked(&self, runner_id: uuid::Uuid) {
        let inst = self
            .instances
            .write()
            .expect("runner registry poisoned")
            .remove(&runner_id);
        self.mailboxes.write().await.remove(&runner_id);
        self.hello_runners.write().await.remove(&runner_id);
        if let Some(inst) = &inst {
            let _ = inst.remove_tx.send(true);
        }
        let tasks = self
            .tasks
            .lock()
            .expect("runner task map poisoned")
            .remove(&runner_id);
        if let Some(tasks) = tasks {
            for handle in [tasks.http_loop, tasks.refresh].into_iter().flatten() {
                let abort = handle.abort_handle();
                if tokio::time::timeout(RUNNER_STOP_GRACE, handle)
                    .await
                    .is_err()
                {
                    abort.abort();
                }
            }
            if let Some(handle) = tasks.runner_loop {
                handle.abort();
                let _ = handle.await;
            }
        }
        if let Some(inst) = inst {
            inst.state.set_connected(false).await;
            tracing::info!(runner_id = %runner_id, name = %inst.name, "stopped runner");
        }
    }

    pub(crate) fn cloud_url(&self) -> String {
        self.daemon.cloud_url.clone()
    }

    /// The dev machine this daemon is (`[daemon].dev_machine_id`).
    pub(crate) fn dev_machine_id(&self) -> Option<uuid::Uuid> {
        self.daemon.dev_machine_id
    }

    /// Every runner the daemon hosts right now.
    pub(crate) fn running_instances(&self) -> Vec<RunnerInstance> {
        self.all_instances()
    }

    /// Tear `inst` down locally (no cloud call): the same teardown the
    /// cloud's `remove_runner` frame triggers — cancel its run, strip its
    /// block, delete its data dir (its working dir is kept). Returns once
    /// it is out of the registry, so a re-add of the same id cannot race.
    pub(crate) async fn remove_runner_locally(
        &self,
        inst: &RunnerInstance,
        reason: &str,
    ) -> Result<()> {
        use anyhow::Context as _;
        let envelope = crate::cloud::protocol::Envelope::for_runner(
            inst.runner_id,
            ServerMsg::RemoveRunner {
                runner_id: inst.runner_id,
                reason: Some(reason.to_string()),
            },
        );
        inst.mailbox_tx
            .send(InboundEnvelope {
                stream_id: None,
                env: envelope,
            })
            .await
            .with_context(|| {
                format!(
                    "delivering synthesized RemoveRunner to {:?} mailbox (loop may have already exited)",
                    inst.name
                )
            })?;
        if !self
            .wait_runner_removed(inst.runner_id, RUNNER_REMOVE_WAIT)
            .await
        {
            anyhow::bail!("runner {:?} did not finish its teardown in time", inst.name);
        }
        Ok(())
    }

    /// Cut reconnect waits short and restart the backoff schedule
    /// (PDASHOSS01-306): for the runner named `runner`, or for every runner
    /// plus the machine control session when `None`. Returns how many
    /// runners were kicked; errors on an unknown runner name.
    pub(crate) fn kick_reconnect(&self, runner: Option<&str>) -> Result<usize> {
        let instances = self.instances.read().expect("runner registry poisoned");
        match runner {
            Some(name) => {
                let inst = instances
                    .values()
                    .find(|i| i.name == name)
                    .ok_or_else(|| anyhow::anyhow!("no configured runner named {name:?}"))?;
                inst.state.force_reconnect();
                Ok(1)
            }
            None => {
                for inst in instances.values() {
                    inst.state.force_reconnect();
                }
                self.daemon_state.force_reconnect();
                Ok(instances.len())
            }
        }
    }

    /// Hot-add: bring a freshly configured runner up in-process.
    ///
    /// Registers the instance in the shared mailbox / hello maps and
    /// spawns the same three tasks the startup loop spawns. Handles are
    /// deliberately detached: on daemon shutdown the tasks die with the
    /// process, and the graceful drain and IPC registry include hot-added runners.
    pub(crate) async fn add_runner(&self, runner_cfg: RunnerConfig) -> Result<()> {
        let _activation = self.activation_lock.lock().await;
        self.add_runner_locked(runner_cfg).await
    }

    /// [`Self::add_runner`] for a caller that holds `activation_lock`.
    async fn add_runner_locked(&self, runner_cfg: RunnerConfig) -> Result<()> {
        if self
            .instances
            .read()
            .expect("runner registry poisoned")
            .contains_key(&runner_cfg.runner_id)
        {
            return Ok(());
        }
        let inst = build_runner_instance(
            runner_cfg,
            self.transport.as_ref(),
            &self.tokens,
            &self.paths,
            &self.daemon,
        )
        .await?;
        // Before the runner is published or started: its working copy may
        // still hold a crashed daemon's unresolved processes.
        self.recovery
            .reserve_if_held(inst.runner_id, &inst.state)
            .await;
        // A replaced runner's undelivered result stays reported in flight.
        if let Some(coord) = self.run_coordinator.as_ref() {
            restore_pending_report_for(coord, inst.runner_id, &inst.state).await;
        }
        {
            let mut mailboxes = self.mailboxes.write().await;
            mailboxes.insert(inst.runner_id, inst.mailbox_tx.clone());
        }
        {
            let mut hello = self.hello_runners.write().await;
            hello.insert(
                inst.runner_id,
                (
                    inst.out.clone(),
                    inst.state.clone(),
                    inst.config.project_slug.clone(),
                ),
            );
        }
        let tasks = spawn_instance_tasks(&inst, self).await;
        self.register_tasks(inst.runner_id, tasks);
        self.instances
            .write()
            .expect("runner registry poisoned")
            .insert(inst.runner_id, inst.clone());
        tracing::info!(runner_id = %inst.runner_id, name = %inst.name, "activated runner in running daemon");
        Ok(())
    }
}

/// Whether a running runner's block equals the one on disk.
fn same_block(running: &RunnerConfig, on_disk: &RunnerConfig) -> bool {
    match (serde_json::to_value(running), serde_json::to_value(on_disk)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Refuse to stop a runner that is doing work: a run in flight, a local
/// chat turn holding its working copy, or a finished run whose cleanup is
/// unresolved.
fn ensure_idle(inst: &RunnerInstance) -> Result<()> {
    // A finished run kept in flight only until its undelivered result is
    // sent does not block a restart (review of #3502 at 7ba3185): its
    // result is in the outbox, and the replacement reports it in flight
    // again ([`restore_pending_report_for`]) and retries it with the new
    // credentials. Refusing would keep a re-login from ever taking effect.
    let in_flight = *inst.state.rx_in_flight.borrow();
    if let Some(run_id) = in_flight.filter(|id| inst.state.report_pending() != Some(*id)) {
        anyhow::bail!(
            "runner {:?} is mid-run (run {run_id}); try again when it is idle",
            inst.name
        );
    }
    if inst.chat_active.load(std::sync::atomic::Ordering::SeqCst) {
        anyhow::bail!(
            "runner {:?} has a local chat turn in flight; try again when it is idle",
            inst.name
        );
    }
    // A finished run whose cleanup is unresolved still owns the working
    // copy and its machine-queue slot; replacing the runner now would
    // orphan both.
    if inst.state.cleanup_reserved() {
        anyhow::bail!(
            "runner {:?} is still cleaning up its last run's processes; try again when it is idle",
            inst.name
        );
    }
    // A chat runtime whose agents are still stopping (a cloud runtime's
    // first cleanup pass, or a local session being ended or left
    // unresolved) still uses the working copy.
    if inst.state.chat_teardown_pending() || inst.chat_sessions.cleanup_pending() {
        anyhow::bail!(
            "runner {:?} is still stopping a chat session's processes; try again when it is idle",
            inst.name
        );
    }
    Ok(())
}

/// Apply `changes` to runner `runner_id`'s block in `cfg`.
fn apply_runner_changes(
    cfg: &mut Config,
    runner_id: uuid::Uuid,
    changes: &RunnerChanges,
) -> Result<()> {
    let block = cfg
        .runners
        .iter_mut()
        .find(|r| r.runner_id == runner_id)
        .ok_or_else(|| anyhow::anyhow!("runner {runner_id} is not in config.toml"))?;
    if let Some(dir) = &changes.working_dir {
        block.workspace.working_dir = dir.clone();
    }
    if let Some(name) = &changes.name {
        let name = name.trim();
        if name.is_empty() {
            anyhow::bail!("runner name cannot be empty");
        }
        block.name = name.to_string();
    }
    if let Some(agent) = &changes.agent {
        let kind = <AgentKind as clap::ValueEnum>::from_str(agent.trim(), false)
            .map_err(|_| anyhow::anyhow!("unknown agent kind {agent:?}"))?;
        if kind == AgentKind::PidashAgent || block.agent.kind == AgentKind::PidashAgent {
            anyhow::bail!("the Pi Dash Agent cannot be switched to or from another agent");
        }
        block.agent.kind = kind;
    }
    let kind = block.agent.kind;
    if kind == AgentKind::PidashAgent
        && (changes.model.is_some() || changes.reasoning_effort.is_some())
    {
        anyhow::bail!("the Pi Dash Agent's model comes from Pi Dash");
    }
    if let Some(model) = &changes.model {
        let model = model.trim();
        let value = if model.is_empty() {
            None
        } else if crate::cli::runner_ops::model_applies_to_agent(kind, model) {
            Some(model.to_string())
        } else {
            anyhow::bail!(
                "model {model:?} does not apply to the {} agent",
                kind.display_name()
            );
        };
        if let Some(slot) = block.model_slot_mut(kind) {
            *slot = value;
        }
        // Effort is only meaningful alongside an explicit model.
        if block.configured_model().is_none()
            && let Some(slot) = block.effort_slot_mut(kind)
        {
            *slot = None;
        }
    }
    if let Some(effort) = &changes.reasoning_effort {
        let effort = effort.trim().to_ascii_lowercase();
        if effort.is_empty() {
            if let Some(slot) = block.effort_slot_mut(kind) {
                *slot = None;
            }
        } else {
            let Some(tiers) = crate::cli::runner_ops::efforts_for_agent(kind) else {
                anyhow::bail!(
                    "reasoning effort only applies to the Codex, Claude Code and Muse Code agents, not {}",
                    kind.display_name()
                );
            };
            if !tiers.contains(&effort.as_str()) {
                anyhow::bail!(
                    "reasoning effort {effort:?} is not one of {}",
                    tiers.join(", ")
                );
            }
            if block.configured_model().is_none() {
                anyhow::bail!(
                    "reasoning effort needs a model; pick a {} model first",
                    kind.display_name()
                );
            }
            if let Some(slot) = block.effort_slot_mut(kind) {
                *slot = Some(effort);
            }
        }
    }
    Ok(())
}

/// Handles of the tasks that drive one runner. `None` entries mean the
/// corresponding task was not spawned (offline mode has no HTTP/refresh
/// loop; a taken mailbox refuses a duplicate RunnerLoop).
struct SpawnedInstanceTasks {
    runner_loop: Option<tokio::task::JoinHandle<()>>,
    http_loop: Option<tokio::task::JoinHandle<()>>,
    refresh: Option<tokio::task::JoinHandle<()>>,
}

impl SpawnedInstanceTasks {
    fn abort(&self) {
        for handle in [&self.runner_loop, &self.http_loop, &self.refresh]
            .into_iter()
            .flatten()
        {
            handle.abort();
        }
    }
}

/// The binary this runner's configured agent kind will actually launch.
fn agent_binary_for(config: &crate::config::schema::RunnerConfig) -> &str {
    match config.agent.kind {
        crate::config::schema::AgentKind::Codex => &config.codex.binary,
        crate::config::schema::AgentKind::ClaudeCode => &config.claude_code.binary,
        crate::config::schema::AgentKind::CursorAgent => &config.cursor_agent.binary,
        crate::config::schema::AgentKind::OpenClaw => &config.openclaw.binary,
        crate::config::schema::AgentKind::Grok => &config.grok.binary,
        crate::config::schema::AgentKind::Antigravity => &config.antigravity.binary,
        crate::config::schema::AgentKind::MuseCode => &config.muse_code.binary,
        crate::config::schema::AgentKind::PidashAgent => {
            crate::pidash_agent::agent::binary_display()
        }
    }
}

/// Spawn the RunnerLoop + HttpLoop + refresh loop for one instance.
/// Extracted from `Supervisor::run`'s startup loop so the machine
/// control session's hot-add path spawns byte-identical machinery.
async fn spawn_instance_tasks(inst: &RunnerInstance, ctx: &RunnerSpawnCtx) -> SpawnedInstanceTasks {
    // Probe the agent binary once per daemon start, in the background: the
    // answer is only used for reporting, so it must never delay bringing the
    // runner online (or fail startup if the binary is briefly unavailable
    // during a desktop upgrade).
    {
        let slot = inst.engine_version.clone();
        let binary = agent_binary_for(&inst.config).to_string();
        let env = crate::agent::agent_env_for_config(&inst.config);
        tokio::spawn(async move {
            if let Some(version) = crate::util::shell::binary_version(&binary, &env).await {
                *slot.write().await = Some(version);
            }
        });
    }
    let mut tasks = SpawnedInstanceTasks {
        runner_loop: None,
        http_loop: None,
        refresh: None,
    };
    let mailbox_rx = match inst.take_mailbox_rx().await {
        Some(rx) => rx,
        None => {
            tracing::error!(
                %inst.runner_id,
                "mailbox already taken — refusing to spawn a duplicate RunnerLoop"
            );
            return tasks;
        }
    };
    let runner_paths = inst.paths.clone();
    let runner_config = inst.config.clone();
    let inst_state = inst.state.clone();
    let inst_approvals = inst.approvals.clone();
    let inst_out = inst.out.clone();
    let inst_ack_tx = inst.ack_tx.clone();
    let inst_remove_tx = inst.remove_tx.clone();
    let live_mailboxes = ctx.mailboxes.clone();
    let live_hello_runners = ctx.hello_runners.clone();
    let live_instances = ctx.instances.clone();
    let daemon_paths = ctx.paths.clone();
    // The working dir reported to the cloud is this runner's single, exclusive
    // work dir.
    let attach_working_dir = resolve_working_dir(inst);
    let inst_chat_active = inst.chat_active.clone();
    let inst_chat_sessions = inst.chat_sessions.clone();
    let inst_engine = ctx.engine.clone();
    let inst_coordinator = ctx.run_coordinator.clone();
    let inst_queue_blocked = ctx.run_queue_blocked;
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
            live_instances,
            current_run: None,
            current_chat: None,
            stopping_chat: None,
            chat_sessions: inst_chat_sessions,
            local_chat_active: inst_chat_active,
            engine: inst_engine,
            coordinator: inst_coordinator,
            queue_blocked: inst_queue_blocked,
        };
        if let Err(e) = run.run().await {
            tracing::error!("runner loop exited: {e:#}");
        }
    });
    tasks.runner_loop = Some(h);

    if let Some(client) = inst.client.clone() {
        let ack_rx = match inst.take_ack_rx().await {
            Some(rx) => rx,
            None => {
                tracing::error!(
                    %inst.runner_id,
                    "ack receiver already taken — refusing to spawn a duplicate HttpLoop"
                );
                return tasks;
            }
        };
        let http_loop = HttpLoop::new(
            client.clone(),
            inst.mailbox_tx.clone(),
            ack_rx,
            inst.state.rx_status.clone(),
            inst.state.rx_in_flight.clone(),
            inst.state.shutdown_notified(),
            attach_body_for_instance(inst, attach_working_dir),
        )
        .with_state(inst.state.clone())
        .with_teardown_rx(inst.remove_tx.subscribe())
        .with_reconnect_kick(inst.state.reconnect_notified())
        .with_receipt_store(ctx.run_receipt.clone())
        .with_coordinator(ctx.run_coordinator.clone());
        let close_client = client.clone();
        let local_state = inst.state.clone();
        let daemon_state = ctx.daemon_state.clone();
        let purge_mailbox = inst.mailbox_tx.clone();
        let runner_name = inst.name.clone();
        let runner_id = inst.runner_id;
        let tokens = ctx.tokens.clone();
        let machine_token = inst.machine_token.clone();
        let http_handle = tokio::spawn(async move {
            if let Err(e) = http_loop.run().await {
                local_state.set_connected(false).await;
                if matches!(
                    e,
                    TransportError::MachineTokenRevoked
                        | TransportError::MachineTokenExpired
                        | TransportError::MembershipRevoked
                ) && let Some(token) = machine_token.as_deref()
                {
                    // Only this workspace is signed out: its other runners
                    // and the machine session stop offering the token, and
                    // `pidash status` reports `workspace_signed_out`. The
                    // last one going records the daemon's sign-out.
                    let workspace =
                        tokens.mark_fatal(token, crate::daemon::workspace_tokens::fatal_reason(&e));
                    tracing::error!(
                        runner = %runner_name,
                        workspace = workspace.as_deref().unwrap_or("(default)"),
                        "workspace token revoked; this workspace's runners are signed out \
                         until `pidash auth login --workspace <slug>`"
                    );
                    return;
                }
                if matches!(e, TransportError::DevMachineRevoked) {
                    // The computer was removed on the web: no token of it
                    // works any more (design §6.1 `removed`).
                    tokens.mark_machine_removed();
                    tracing::error!(
                        runner = %runner_name,
                        "this computer was removed on the web; signed out"
                    );
                    return;
                }
                if let TransportError::RunnerDeletedInCloud { confirmations } = e {
                    purge_cloud_deleted_runner(
                        &purge_mailbox,
                        &runner_name,
                        runner_id,
                        confirmations,
                    )
                    .await;
                    return;
                }
                if e.is_expected_teardown() {
                    // Removed locally: no further requests for this runner,
                    // not even a courtesy session close.
                    tracing::info!("http loop exited after runner teardown: {e:#}");
                    return;
                }
                tracing::error!("http loop exited: {e:#}");
                let _ = tokio::time::timeout(Duration::from_secs(2), close_client.close_session())
                    .await;
                if e.requires_daemon_restart() {
                    tracing::error!(
                        "http loop hit a local invariant failure; requesting daemon restart"
                    );
                    daemon_state.shutdown_with_reason(DaemonShutdownTrigger::Invariant);
                }
            }
        });
        tasks.http_loop = Some(http_handle);

        let refresh_state = inst.state.clone();
        let refresh_teardown = inst.remove_tx.subscribe();
        let refresh_handle = tokio::spawn(async move {
            refresh_loop(client, refresh_state, refresh_teardown).await;
        });
        tasks.refresh = Some(refresh_handle);
    }
    tasks
}

/// The cloud confirmed (PDASHOSS01-306) that this runner was deleted while
/// the daemon could not receive the RemoveRunner frame. Run the same local
/// teardown the frame would have: hand the runner loop a synthesized
/// `RemoveRunner`, which cancels any run or chat, stops the heartbeat,
/// deletes the runner's data dir and strips its `[[runner]]` block. The
/// HttpLoop has already exited, so nothing contacts the cloud for this id
/// again.
async fn purge_cloud_deleted_runner(
    mailbox: &mpsc::Sender<InboundEnvelope>,
    name: &str,
    runner_id: uuid::Uuid,
    confirmations: u32,
) {
    tracing::warn!(
        "runner {name} ({runner_id}) was deleted in the cloud; removed locally after \
         {confirmations} confirmations"
    );
    let envelope = crate::cloud::protocol::Envelope::for_runner(
        runner_id,
        ServerMsg::RemoveRunner {
            runner_id,
            reason: Some(format!(
                "deleted in the cloud (confirmed {confirmations} times)"
            )),
        },
    );
    if let Err(e) = mailbox
        .send(InboundEnvelope {
            stream_id: None,
            env: envelope,
        })
        .await
    {
        tracing::warn!(
            "could not hand the local removal of {name} ({runner_id}) to its runner \
             loop: {e}. Run `pidash runner remove {name} --local-only --yes` to clean it up."
        );
    }
}

/// The `DaemonRestart` detail for a shutdown (NEWPIDASH-253): keeps the
/// legacy `daemon shutdown requested` prefix (so stores written before
/// trigger carriage still classify as `daemon_restart`) and states the
/// trigger (plus the previous systemd exit when known). Examples:
/// `daemon shutdown requested (SIGTERM; previous exit: oom-kill)`,
/// `daemon shutdown requested (control socket disconnect)`.
pub(crate) fn daemon_shutdown_detail(
    trigger: DaemonShutdownTrigger,
    previous_exit: Option<&str>,
) -> String {
    match previous_exit {
        Some(prev) if !prev.trim().is_empty() => {
            format!(
                "daemon shutdown requested ({}; previous exit: {})",
                trigger.as_str(),
                prev.trim()
            )
        }
        _ => format!("daemon shutdown requested ({})", trigger.as_str()),
    }
}

/// Append the freshly-read previous systemd exit to a journaled detail
/// (NEWPIDASH-253): replay attaches the successor's knowledge of the
/// previous exit to failures the dead daemon journaled. No-op when the
/// detail already carries a previous exit or there is nothing to add.
pub(crate) fn with_previous_exit(detail: &str, previous_exit: Option<&str>) -> String {
    match previous_exit {
        Some(prev) if !prev.trim().is_empty() && !detail.contains("previous exit") => {
            // Insert before the closing paren when the detail carries a
            // trigger parenthetical already, so `… (SIGTERM)` becomes
            // `… (SIGTERM; previous exit: oom-kill)`.
            if let Some(stripped) = detail.strip_suffix(')') {
                format!("{stripped}; previous exit: {})", prev.trim())
            } else {
                format!("{detail} (previous exit: {})", prev.trim())
            }
        }
        _ => detail.to_string(),
    }
}

/// On daemon shutdown, send `RunFailed{DaemonRestart}` for every
/// runner that currently reports an in-flight run. Without this the
/// cloud is left guessing — its heartbeat reaper eventually fails the
/// run with the cryptic ``reaped by heartbeat: ... in_flight_run=
/// (none)`` message after the daemon restarts and the next session
/// Hello legitimately reports null. This sends a clean terminal
/// signal instead. A run that already finished and is only waiting on
/// its run-end cleanup gets its held result instead of the failure.
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
///
/// Every in-flight run is recorded BEFORE the sends are attempted and
/// cleared only on a confirmed send, so a run whose `RunFailed` misses
/// the 2s window (or a SIGKILL mid-drain) is replayed by the next
/// daemon start instead of being silently dropped — a dropped signal
/// leaves the cloud showing the run busy forever, pinned to a runner
/// that no longer knows about it. The record splits by ownership:
/// managed queue attempts persist their exact payload to the SQLite
/// terminal outbox (coordinator-owned, retried verbatim on restart);
/// legacy in-flight runs use the drain journal.
async fn drain_in_flight_runs(
    runners: Arc<RwLock<HelloRunnerMap>>,
    paths: &Paths,
    coord: Option<Arc<crate::daemon::run_queue::Coordinator>>,
    trigger: DaemonShutdownTrigger,
    previous_exit: Option<String>,
) -> usize {
    // Snapshot under the read lock so the lock is dropped before any
    // network I/O; concurrent writers (config reloads, etc.) are not
    // expected during shutdown but this keeps the contract clean.
    let now = Utc::now();
    let detail = daemon_shutdown_detail(trigger, previous_exit.as_deref());
    // One report per in-flight run: the run's own result when it already
    // finished and only its run-end cleanup is pending (held in the
    // runner's state, PDASHOSS01-417), otherwise a DaemonRestart failure.
    // Taking the held reports also stops the worker from sending a second
    // one if it finishes cleanup before the process exits.
    // A managed run's report is persisted to the SQLite terminal outbox in
    // the same step that takes it (`take_drain_report`), so a worker that
    // finishes cleanup meanwhile cannot free the run's queue row first.
    let snapshot: Vec<(uuid::Uuid, uuid::Uuid, ClientMsg, RunnerOut, bool)> = {
        let guard = runners.read().await;
        guard
            .iter()
            .filter_map(|(rid, (out, state, _))| {
                let run_id = (*state.rx_in_flight.borrow())?;
                let (msg, managed) =
                    take_drain_report(state, coord.as_deref(), run_id, || ClientMsg::RunFailed {
                        run_id,
                        reason: FailureReason::DaemonRestart,
                        detail: Some(detail.clone()),
                        ended_at: now,
                        tokens: None,
                        model: None,
                    })?;
                Some((*rid, run_id, msg, out.clone(), managed))
            })
            .collect()
    };
    // Split by ownership: managed queue attempts persist their exact
    // payload to the SQLite terminal outbox before sending; legacy
    // in-flight runs journal a RunFailed (a held RunCompleted or
    // RunCancelled is sent best-effort, as the worker itself would).
    let mut managed: Vec<(uuid::Uuid, uuid::Uuid)> = Vec::new();
    let mut pending: Vec<drain_journal::PendingRunFailure> = Vec::new();
    for (runner_id, run_id, msg, _, is_managed) in &snapshot {
        if *is_managed {
            managed.push((*runner_id, *run_id));
        } else if let ClientMsg::RunFailed {
            reason,
            detail: failed_detail,
            ended_at,
            ..
        } = msg
        {
            let restart = matches!(reason, FailureReason::DaemonRestart);
            pending.push(drain_journal::PendingRunFailure {
                runner_id: *runner_id,
                run_id: *run_id,
                reason: *reason,
                detail: failed_detail.clone().unwrap_or_default(),
                ended_at: *ended_at,
                recorded_at: now,
                trigger: restart.then(|| trigger.as_str().to_string()),
                previous_exit: if restart { previous_exit.clone() } else { None },
            });
        }
    }
    // spawn_blocking keeps the sync fs I/O off the runtime workers and,
    // more importantly, keeps the caller's 5s shutdown deadline honest:
    // sync I/O has no await point for the timeout to fire at, so a
    // wedged disk inlined here would hang shutdown until SIGKILL. As a
    // bonus, a blocking task runs to completion even if the drain
    // future is dropped at the deadline mid-write.
    {
        let journal_paths = paths.clone();
        match tokio::task::spawn_blocking(move || {
            drain_journal::record_all(&journal_paths, &pending)
        })
        .await
        {
            Ok(Ok(())) => {}
            // Journal failure must not abort the drain itself — the
            // sends below are still the primary delivery path.
            Ok(Err(e)) => {
                tracing::warn!("failed to journal in-flight runs before drain: {e:#}");
            }
            Err(e) => tracing::warn!("drain journal task panicked: {e:#}"),
        }
    }
    let drains = snapshot
        .into_iter()
        .map(|(runner_id, run_id, msg, out, _)| {
            async move {
                // Per-attempt timeout so one stuck cloud client can't keep
                // a parallel sibling from completing in time.
                match send_run_failed_bounded(out, msg, Duration::from_secs(2)).await {
                    RunFailedSend::Delivered => {
                        tracing::info!(%runner_id, %run_id, "drained in-flight run on shutdown");
                        Some(run_id)
                    }
                    RunFailedSend::Failed(e) => {
                        tracing::warn!(
                            %runner_id, %run_id,
                            "drain send failed (journaled for replay on next start): {e:#}"
                        );
                        None
                    }
                    RunFailedSend::TimedOut => {
                        tracing::warn!(
                            %runner_id, %run_id,
                            "drain send timed out at 2s (journaled for replay on next start)"
                        );
                        None
                    }
                }
            }
        });
    let delivered: Vec<uuid::Uuid> = futures_util::future::join_all(drains)
        .await
        .into_iter()
        .flatten()
        .collect();
    // Managed delivered terminals clear their SQLite outbox rows
    // (coordinator owns clearing). Legacy rows were never journaled as
    // managed, so clearing them here is a no-op for the journal path.
    for run_id in &delivered {
        if let Some(c) = coord.as_ref() {
            let _ = c.complete_terminal(*run_id);
        }
    }
    // spawn_blocking for the same reasons as record_all above; it also
    // means the cleanup completes even if the caller's 5s deadline
    // drops this future between join_all and here.
    {
        let journal_paths = paths.clone();
        let delivered_ids = delivered.clone();
        match tokio::task::spawn_blocking(move || {
            drain_journal::remove(&journal_paths, &delivered_ids)
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!("failed to clear delivered runs from drain journal: {e:#}");
            }
            Err(e) => tracing::warn!("drain journal cleanup task panicked: {e:#}"),
        }
    }
    delivered.len()
}

/// The run a terminal report (`RunCompleted`/`RunFailed`/`RunCancelled`)
/// is about; `None` for any other message.
fn terminal_run_id(msg: &ClientMsg) -> Option<uuid::Uuid> {
    match msg {
        ClientMsg::RunCompleted { run_id, .. }
        | ClientMsg::RunFailed { run_id, .. }
        | ClientMsg::RunCancelled { run_id, .. } => Some(*run_id),
        _ => None,
    }
}

/// Outcome of one bounded `RunFailed` send attempt. Shared by the
/// shutdown drain and the journal replay so the two paths cannot drift
/// in what counts as a confirmed send.
enum RunFailedSend {
    Delivered,
    Failed(anyhow::Error),
    TimedOut,
}

async fn send_run_failed_bounded(out: RunnerOut, msg: ClientMsg, bound: Duration) -> RunFailedSend {
    match tokio::time::timeout(bound, out.send(msg)).await {
        Ok(Ok(())) => RunFailedSend::Delivered,
        Ok(Err(e)) => RunFailedSend::Failed(e),
        Err(_) => RunFailedSend::TimedOut,
    }
}

/// True when the cloud's reply to a replayed `RunFailed` says the signal
/// can never be delivered: the run row is gone (404 `run_not_found`),
/// owned by another runner after a re-assignment (403), or otherwise
/// permanently gone (410). Retrying such an entry every pass until the
/// TTL would only spam the log and the endpoint.
fn is_permanent_run_failed_rejection(e: &anyhow::Error) -> bool {
    matches!(
        e.downcast_ref::<crate::cloud::http::TransportError>(),
        Some(crate::cloud::http::TransportError::Server { status, .. })
            if matches!(status, 403 | 404 | 410)
    )
}

/// Replay `RunFailed` signals a previous daemon failed to deliver on
/// shutdown (see [`drain_journal`]). One pass over the journal:
///
/// - entry's runner no longer configured → dropped (without that
///   runner's credentials the signal can never be delivered; the
///   cloud's heartbeat reaper remains the fallback);
/// - entry older than [`drain_journal::REPLAY_TTL`] → dropped (the
///   reaper has long since failed the run);
/// - entry whose run is in flight locally again → dropped WITHOUT
///   sending (the cloud redelivered the run to this daemon and it is
///   legitimately running — failing it now would kill live work); the
///   check runs both under the runners lock and again at the last
///   instant before each send, since on retry-loop passes a redelivery
///   can land between the two;
/// - entry the cloud rejects permanently (run row gone, or owned by
///   another runner after re-assignment) → dropped, not retried;
/// - otherwise the signal is sent; confirmed sends are removed and
///   anything else is kept for the next pass.
///
/// The first pass runs from `Supervisor::run` BEFORE the per-runner
/// loops start, so nothing can be redelivered or in flight yet; later
/// passes come from a background retry loop where the in-flight guard
/// does the work. `previous_exit` is the successor's freshly-read
/// previous systemd exit (NEWPIDASH-253): replay attaches it to
/// journaled details that do not carry one yet (see
/// [`with_previous_exit`]).
async fn replay_drain_journal_once(
    paths: &Paths,
    runners: &Arc<RwLock<HelloRunnerMap>>,
    previous_exit: Option<String>,
) -> usize {
    let entries = drain_journal::load(paths);
    if entries.is_empty() {
        return 0;
    }
    let now = Utc::now();
    let mut resolved: Vec<uuid::Uuid> = Vec::new();
    let mut sends = Vec::new();
    {
        let guard = runners.read().await;
        for entry in entries {
            let Some((out, state, _)) = guard.get(&entry.runner_id) else {
                tracing::warn!(
                    runner_id = %entry.runner_id, run_id = %entry.run_id,
                    "dropping journaled RunFailed: runner no longer configured"
                );
                resolved.push(entry.run_id);
                continue;
            };
            if now - entry.recorded_at > drain_journal::REPLAY_TTL {
                tracing::warn!(
                    runner_id = %entry.runner_id, run_id = %entry.run_id,
                    "dropping journaled RunFailed: older than replay TTL"
                );
                resolved.push(entry.run_id);
                continue;
            }
            if *state.rx_in_flight.borrow() == Some(entry.run_id) {
                // The cloud redelivered this run and it is running again
                // — the journaled failure is stale, not undelivered.
                tracing::info!(
                    runner_id = %entry.runner_id, run_id = %entry.run_id,
                    "dropping journaled RunFailed: run is in flight again"
                );
                resolved.push(entry.run_id);
                continue;
            }
            let out = out.clone();
            let in_flight = state.rx_in_flight.clone();
            let previous_exit = previous_exit.clone();
            sends.push(async move {
                // Re-check at the last instant before sending: the guard
                // above ran under the runners lock, but on retry-loop
                // passes the cloud can redeliver this run between that
                // check and now (rx_in_flight is stamped by the runner
                // loop when it processes the Assign).
                if *in_flight.borrow() == Some(entry.run_id) {
                    tracing::info!(
                        runner_id = %entry.runner_id, run_id = %entry.run_id,
                        "dropping journaled RunFailed: run redelivered before send"
                    );
                    return ReplayOutcome::Resolved(entry.run_id);
                }
                let replayed_detail = with_previous_exit(
                    &entry.detail,
                    previous_exit.as_deref().or(entry.previous_exit.as_deref()),
                );
                let msg = ClientMsg::RunFailed {
                    run_id: entry.run_id,
                    reason: entry.reason,
                    detail: Some(replayed_detail),
                    ended_at: entry.ended_at,
                    tokens: None,
                    model: None,
                };
                match send_run_failed_bounded(out, msg, Duration::from_secs(10)).await {
                    RunFailedSend::Delivered => {
                        tracing::info!(
                            runner_id = %entry.runner_id, run_id = %entry.run_id,
                            "replayed journaled RunFailed from previous shutdown"
                        );
                        ReplayOutcome::Delivered(entry.run_id)
                    }
                    RunFailedSend::Failed(e) if is_permanent_run_failed_rejection(&e) => {
                        tracing::warn!(
                            runner_id = %entry.runner_id, run_id = %entry.run_id,
                            "dropping journaled RunFailed: cloud rejected it permanently: {e:#}"
                        );
                        ReplayOutcome::Resolved(entry.run_id)
                    }
                    RunFailedSend::Failed(e) => {
                        tracing::warn!(
                            runner_id = %entry.runner_id, run_id = %entry.run_id,
                            "replaying journaled RunFailed failed, will retry: {e:#}"
                        );
                        ReplayOutcome::Retry
                    }
                    RunFailedSend::TimedOut => {
                        tracing::warn!(
                            runner_id = %entry.runner_id, run_id = %entry.run_id,
                            "replaying journaled RunFailed timed out, will retry"
                        );
                        ReplayOutcome::Retry
                    }
                }
            });
        }
    }
    let mut delivered = 0usize;
    for outcome in futures_util::future::join_all(sends).await {
        match outcome {
            ReplayOutcome::Delivered(run_id) => {
                delivered += 1;
                resolved.push(run_id);
            }
            ReplayOutcome::Resolved(run_id) => resolved.push(run_id),
            ReplayOutcome::Retry => {}
        }
    }
    if let Err(e) = drain_journal::remove(paths, &resolved) {
        tracing::warn!("failed to update drain journal after replay: {e:#}");
    }
    delivered
}

/// Crash-recovery reservations, shared by daemon start and the hot-add
/// path (review of #3501 at 591a610, MUST 6): a runner a held recovery
/// group may belong to stays reserved, whether it was there at start or
/// added while the group is still unresolved. While reserved, a runner
/// refuses assignments and chat ([`StateHandle::cleanup_reserved`]).
#[derive(Clone, Default)]
pub(crate) struct RecoveryReservations(Arc<std::sync::Mutex<RecoveryReservationState>>);

#[derive(Default)]
struct RecoveryReservationState {
    hold: crate::util::group_recovery::RecoveryHold,
    /// The runners this hold reserved, to release once it stops affecting them.
    reserved: Vec<(uuid::Uuid, StateHandle)>,
}

impl RecoveryReservations {
    #[cfg_attr(not(unix), allow(dead_code))]
    fn new(hold: crate::util::group_recovery::RecoveryHold) -> Self {
        Self(Arc::new(std::sync::Mutex::new(RecoveryReservationState {
            hold,
            reserved: Vec::new(),
        })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RecoveryReservationState> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Reserve `runner_id` before it starts taking work when a held group
    /// may belong to it. `true`: reserved.
    async fn reserve_if_held(&self, runner_id: uuid::Uuid, state: &StateHandle) -> bool {
        let held = {
            let mut s = self.lock();
            let held = s.hold.affects(runner_id);
            if held {
                s.reserved.push((runner_id, state.clone()));
            }
            held
        };
        if held {
            state.set_cleanup_reserved(true).await;
        }
        held
    }

    /// Install a re-checked hold and release every reserved runner it no
    /// longer affects. `false` once nothing is held or reserved.
    #[cfg_attr(not(unix), allow(dead_code))]
    async fn update(&self, hold: crate::util::group_recovery::RecoveryHold) -> bool {
        let (release, active) = {
            let mut s = self.lock();
            s.hold = hold;
            let (keep, release): (Vec<_>, Vec<_>) = std::mem::take(&mut s.reserved)
                .into_iter()
                .partition(|(id, _)| s.hold.affects(*id));
            s.reserved = keep;
            (release, !s.hold.is_empty() || !s.reserved.is_empty())
        };
        for (_, state) in release {
            state.set_cleanup_reserved(false).await;
        }
        active
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    fn hold(&self) -> crate::util::group_recovery::RecoveryHold {
        self.lock().hold.clone()
    }
}

/// Reserve every runner `reservations` holds a group for, and re-check the
/// held groups every [`UNRESOLVED_CLEANUP_RECHECK`] in the background,
/// releasing each runner once nothing held affects it.
#[cfg(unix)]
async fn hold_runners_for_recovery(
    reservations: RecoveryReservations,
    runners: Vec<(uuid::Uuid, StateHandle)>,
) {
    if reservations.hold().is_empty() {
        return;
    }
    let mut held = 0;
    for (id, state) in &runners {
        held += usize::from(reservations.reserve_if_held(*id, state).await);
    }
    tracing::warn!(
        runners = held,
        "process groups of a crashed daemon could not be emptied; their runners stay reserved"
    );
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(UNRESOLVED_CLEANUP_RECHECK).await;
            let mut hold = reservations.hold();
            hold.recheck().await;
            if !reservations.update(hold).await {
                break;
            }
        }
        tracing::info!("crashed daemon's process groups resolved; runners released");
    });
}

/// For the shutdown drain: take `run_id`'s held terminal report (or
/// `fallback()` when none is held) and, for a run the machine queue
/// manages, persist it to the terminal outbox. Returns the report and
/// whether the run is managed. Both happen while the runner's terminal
/// hold is locked: a worker finishing cleanup meanwhile sees the hold as
/// drained only afterwards, so it cannot release the slot (and with it
/// the queue row the record lives in) before the result is recorded.
fn take_drain_report(
    state: &StateHandle,
    coord: Option<&crate::daemon::run_queue::Coordinator>,
    run_id: uuid::Uuid,
    fallback: impl FnOnce() -> ClientMsg,
) -> Option<(ClientMsg, bool)> {
    state.take_terminal_for_drain_then(|msgs| {
        // Revalidated under the hold lock: a worker that finished releasing
        // cleared the run first, and it then owns the run's report (and its
        // outbox record). Nothing is sent or written for it here.
        if *state.rx_in_flight.borrow() != Some(run_id) {
            return None;
        }
        // Finished, its report undelivered: the outbox record holds the
        // real result and the next start replays it.
        if coord.is_some() && state.report_pending() == Some(run_id) {
            return None;
        }
        let held = msgs
            .into_iter()
            .find(|m| terminal_run_id(m) == Some(run_id));
        let managed = coord.is_some_and(|c| c.holds_run(run_id));
        let msg = match (held, coord.filter(|_| managed)) {
            (Some(msg), Some(c)) => {
                let payload = serde_json::to_vec(&msg).unwrap_or_default();
                if let Err(e) = c.persist_terminal(run_id, &payload, &format!("drain-{run_id}")) {
                    tracing::warn!(%run_id, "drain: failed to persist terminal report: {e}");
                }
                msg
            }
            (Some(msg), None) => msg,
            // No held result: a terminal payload already in the outbox is
            // the run's real result and is never replaced by the fallback.
            (None, Some(c)) => {
                let msg = fallback();
                let payload = serde_json::to_vec(&msg).unwrap_or_default();
                match c.persist_terminal_if_absent(run_id, &payload, &format!("drain-{run_id}")) {
                    Ok(true) => msg,
                    Ok(false) => c
                        .terminal_pending()
                        .into_iter()
                        .find(|e| e.run_id == run_id)
                        .and_then(|e| e.pending_terminal_payload)
                        .and_then(|p| serde_json::from_slice(&p).ok())
                        .unwrap_or(msg),
                    Err(e) => {
                        tracing::warn!(%run_id, "drain: failed to persist terminal report: {e}");
                        msg
                    }
                }
            }
            (None, None) => fallback(),
        };
        Some((msg, managed))
    })
}

/// Release a finished run's machine-queue slot. The run's terminal outbox
/// record stays when the report may not have reached the cloud: the
/// shutdown drain took it (the drain deletes the record once its send
/// succeeds), or the worker's send failed. The outbox retry then delivers
/// it. Otherwise the worker delivered the report and the record goes with
/// the slot.
fn release_finished_slot(
    coord: &crate::daemon::run_queue::Coordinator,
    state: &StateHandle,
    run_id: uuid::Uuid,
) {
    let undelivered = state.take_terminal_undelivered();
    if state.terminal_drained() || undelivered {
        let _ = coord.release_slot(run_id);
    } else {
        let _ = coord.complete(run_id);
    }
}

/// Retry SQLite terminal-outbox records once at startup: managed queue
/// attempts redeliver their exact persisted payload (same bytes/key),
/// never re-executing. Records for removed runners are cleared (the
/// cloud reaper handles the run). Runs redelivered in flight are
/// resolved without a second signal.
///
/// `skip_held` (the in-process retry): leave a record alone while the queue
/// still holds its run. That run's result is held for run-end cleanup and
/// is the worker's to send.
/// Report every run whose result is still in the outbox as in flight, so
/// the cloud's session-open and heartbeat reapers cannot fail it before the
/// outbox retry delivers its real result (first writer wins, so a failure
/// written first could never be replaced). Called at startup, after the
/// first replay and before any runner opens its session; the retry clears
/// each run once its report is delivered or rejected for good. One run per
/// runner can be kept this way.
async fn restore_pending_reports(
    coord: &crate::daemon::run_queue::Coordinator,
    runners: &Arc<RwLock<HelloRunnerMap>>,
) {
    let guard = runners.read().await;
    for (runner_id, (_, state, _)) in guard.iter() {
        restore_pending_report_for(coord, *runner_id, state).await;
    }
}

/// [`restore_pending_reports`] for one runner's state: also used when a
/// runner is replaced (credential rotation, reconfigure) while a finished
/// run's result is still undelivered.
async fn restore_pending_report_for(
    coord: &crate::daemon::run_queue::Coordinator,
    runner_id: uuid::Uuid,
    state: &StateHandle,
) {
    for entry in coord.terminal_pending() {
        if entry.runner_id != runner_id
            || entry.pending_terminal_payload.is_none()
            || coord.holds_run(entry.run_id)
            || state.rx_in_flight.borrow().is_some()
        {
            continue;
        }
        state
            .set_current_run(Some(CurrentRunSummary {
                run_id: entry.run_id,
                thread_id: None,
                status: "reporting".to_string(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;
        state.set_report_pending(entry.run_id);
        tracing::info!(run_id = %entry.run_id,
            "undelivered run result kept in flight until the outbox retry delivers it");
    }
}

async fn replay_terminal_outbox_once(
    coord: &crate::daemon::run_queue::Coordinator,
    runners: &Arc<RwLock<HelloRunnerMap>>,
    skip_held: bool,
) -> usize {
    let mut pending = coord.terminal_pending();
    if skip_held {
        pending.retain(|entry| !coord.holds_run(entry.run_id));
    }
    if pending.is_empty() {
        return 0;
    }
    let guard = runners.read().await;
    let mut sends = Vec::new();
    for entry in pending {
        let Some(payload) = entry.pending_terminal_payload.clone() else {
            let _ = coord.complete_terminal(entry.run_id);
            continue;
        };
        let Some((out, state, _)) = guard.get(&entry.runner_id) else {
            // In-process, a runner can be missing only for a moment while
            // it is replaced (credential rotation, reconfigure); its record
            // stays for the replacement. Startup drops records of runners
            // that are gone.
            if skip_held {
                continue;
            }
            tracing::warn!(run_id=%entry.run_id, "dropping outbox terminal: runner no longer configured");
            let _ = coord.complete_terminal(entry.run_id);
            continue;
        };
        if *state.rx_in_flight.borrow() == Some(entry.run_id)
            && state.report_pending() != Some(entry.run_id)
        {
            tracing::info!(run_id=%entry.run_id, "dropping outbox terminal: run redelivered");
            let _ = coord.complete_terminal(entry.run_id);
            continue;
        }
        let out = out.clone();
        let state = state.clone();
        sends.push(async move {
            let msg: Result<ClientMsg, _> = serde_json::from_slice(&payload);
            match msg {
                Ok(m) => match send_run_failed_bounded(out, m, Duration::from_secs(10)).await {
                    RunFailedSend::Delivered => {
                        tracing::info!(run_id=%entry.run_id, "redelivered SQLite outbox terminal");
                        let _ = coord.complete_terminal(entry.run_id);
                        state.clear_report_pending(entry.run_id).await;
                        true
                    }
                    RunFailedSend::Failed(e) if is_permanent_run_failed_rejection(&e) => {
                        tracing::warn!(run_id=%entry.run_id, "dropping outbox terminal: cloud rejected permanently: {e:#}");
                        let _ = coord.complete_terminal(entry.run_id);
                        state.clear_report_pending(entry.run_id).await;
                        false
                    }
                    RunFailedSend::Failed(e) => {
                        tracing::warn!(run_id=%entry.run_id, "outbox retry failed, keeps terminal_pending: {e:#}");
                        false
                    }
                    RunFailedSend::TimedOut => {
                        tracing::warn!(run_id=%entry.run_id, "outbox retry timed out, keeps terminal_pending");
                        false
                    }
                },
                Err(e) => {
                    tracing::warn!(run_id=%entry.run_id, "unparsable outbox payload; dropping record: {e:#}");
                    let _ = coord.complete_terminal(entry.run_id);
                    state.clear_report_pending(entry.run_id).await;
                    false
                }
            }
        });
    }
    drop(guard);
    futures_util::future::join_all(sends)
        .await
        .into_iter()
        .filter(|d| *d)
        .count()
}

/// How one replayed journal entry ended up.
enum ReplayOutcome {
    /// The `RunFailed` reached the cloud; the entry is done.
    Delivered(uuid::Uuid),
    /// The entry is finished without a delivery (run redelivered locally,
    /// or the cloud rejected the signal permanently); drop it.
    Resolved(uuid::Uuid),
    /// Transient failure; keep the entry for the next pass.
    Retry,
}

/// Delay between journal replay passes. The retry loop is long-lived:
/// it never exits on an empty journal — only `Supervisor::run`'s
/// shutdown abort stops it — so an entry journaled mid-run
/// (NEWPIDASH-251) still replays on the next tick, not only at the
/// next daemon start.
const DRAIN_JOURNAL_RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// Background retry for journal entries the startup pass could not
/// deliver (cloud unreachable, token mid-refresh), and for entries
/// journaled mid-run (NEWPIDASH-251: normal `RunFailed` sends that
/// exhaust their HTTP retries). Long-lived: an empty journal only
/// skips that tick; entries past the TTL are dropped by
/// [`replay_drain_journal_once`]. Exits only when the owning task is
/// aborted on shutdown (see the abort-before-drain in `Supervisor::run`).
///
/// Each tick also retries the machine queue's terminal outbox (`outbox`):
/// a released run whose report the worker could not send keeps its record
/// there, and `RunCompleted` / `RunCancelled` have no journal entry.
async fn replay_drain_journal_retry_loop(
    paths: Paths,
    runners: Arc<RwLock<HelloRunnerMap>>,
    previous_exit: Option<String>,
    outbox: Option<Arc<crate::daemon::run_queue::Coordinator>>,
    interval: Duration,
) {
    loop {
        tokio::time::sleep(interval).await;
        if let Some(coord) = outbox.as_deref()
            && !coord.terminal_pending().is_empty()
        {
            replay_terminal_outbox_once(coord, &runners, true).await;
        }
        if drain_journal::load(&paths).is_empty() {
            continue;
        }
        replay_drain_journal_once(&paths, &runners, previous_exit.clone()).await;
    }
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
    shared_machine_token: Option<&str>,
) -> Result<CredentialsHandle> {
    if let Some(token) = shared_machine_token {
        return Ok(CredentialsHandle::new(
            runner_paths.credentials_path(),
            RunnerCredentials {
                runner_id: runner_paths.runner_id,
                name: runner_name.to_string(),
                refresh_token: token.to_string(),
                refresh_token_generation: 0,
            },
        ));
    }
    crate::cloud::http::load_runner_credentials_from(runner_paths.credentials_path(), runner_name)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Resolve the local dev-machine working directory the cloud should
/// display for this runner: its single, exclusive `workspace.working_dir`,
/// normalized to an absolute path.
fn resolve_working_dir(inst: &RunnerInstance) -> Option<std::path::PathBuf> {
    Some(canonical_for_compare(&inst.config.workspace.working_dir))
}

fn attach_body_for_instance(
    inst: &RunnerInstance,
    working_dir: Option<std::path::PathBuf>,
) -> AttachBody {
    let mut agent_versions = HashMap::new();
    let kind_key = inst.config.agent.kind.as_str().to_owned();
    // The agent binary's own version when we managed to probe it, falling back
    // to the runner's version so the key is never absent (older cloud builds
    // read it positionally). `try_read` keeps this function sync and lock-free:
    // a probe in flight just means this attach reports the fallback and the
    // next reconnect carries the real value.
    let probed = inst
        .engine_version
        .try_read()
        .ok()
        .and_then(|guard| guard.clone());
    agent_versions.insert(
        kind_key,
        probed.unwrap_or_else(|| crate::RUNNER_VERSION.to_string()),
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
        working_dir: working_dir
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        agent_versions,
        agent_kind: inst.config.agent.kind,
        model: inst
            .config
            .configured_model()
            .unwrap_or_default()
            .to_string(),
        reasoning_effort: inst
            .config
            .configured_reasoning_effort()
            .unwrap_or_default()
            .to_string(),
    }
}

async fn refresh_loop(
    client: RunnerCloudClient,
    state: StateHandle,
    mut teardown: tokio::sync::watch::Receiver<bool>,
) {
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
            // A removed runner refreshes nothing.
            _ = teardown.wait_for(|removed| *removed) => return,
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
    /// The IPC registry. A removed runner is dropped from it so
    /// `pidash status` stops listing it.
    live_instances: Arc<std::sync::RwLock<HashMap<uuid::Uuid, RunnerInstance>>>,
    /// In-flight run, if any. Replaced on each Assign and cleared as soon as
    /// the worker task signals completion via `done_rx` — driven by
    /// `tokio::select!` so a new Assign isn't rejected while we wait for the
    /// next inbound frame.
    current_run: Option<CurrentRun>,
    current_chat: Option<CurrentChat>,
    /// A chat runtime whose processes are not confirmed gone yet (see
    /// [`release_chat_dir`]).
    stopping_chat: Option<StoppingChat>,
    /// This runner's local (IPC) chat sessions: an Assign ends their idle
    /// agents and stops what those left running before taking the working
    /// copy (PDASHOSS01-417).
    chat_sessions: crate::ipc::chat::ChatRegistry,
    /// Shared with this runner's `RunnerInstance` (and therefore the IPC
    /// server): `true` while a *local* chat turn (PDASHOSS01-159, driven over
    /// the daemon IPC socket, not this cloud loop) holds the working copy. The
    /// assign lane consults it so a managed run and a local chat never modify
    /// the same working copy at the same time (issue AC6).
    local_chat_active: Arc<std::sync::atomic::AtomicBool>,
    /// The machine's Pi Dash Agent, for the current profile lane the
    /// free-quota classifier is tied to. Threaded into each `AssignWorker`.
    engine: Arc<crate::pidash_agent::manager::PidashAgentManager>,
    /// Machine FIFO coordinator (NEWPIDASH-241). None only in tests.
    coordinator: Option<Arc<crate::daemon::run_queue::Coordinator>>,
    /// True when the queue store is blocked (fail closed, no execution).
    queue_blocked: bool,
}

struct CurrentRun {
    run_id: uuid::Uuid,
    cancel: tokio_util::sync::CancellationToken,
    done_rx: oneshot::Receiver<()>,
}

/// Run context the cloud attaches to an `Assign` (NEWPIDASH-320 §6.3,
/// §6.1): the issue and project identifiers plus, for `coding-task`
/// runs, the workpad snapshot. All-`None` for old servers; chat turns
/// never carry it.
#[derive(Debug, Clone, Default)]
struct AssignContext {
    issue_identifier: Option<String>,
    project_identifier: Option<String>,
    workpad_body: Option<String>,
    workpad_sha256: Option<String>,
}

impl AssignContext {
    /// The `PIDASH_AGENT_NAME` value for this run:
    /// `<display name> (<model>)`, e.g. `Claude Code (claude-opus-4-8)`.
    /// The model is the assign's expected model, else the runner's
    /// configured default, else omitted (NEWPIDASH-320 §6.3, decision D5).
    fn agent_name(
        &self,
        kind: AgentKind,
        expected_model: Option<&str>,
        configured_model: Option<&str>,
    ) -> String {
        let display = kind.display_name();
        match expected_model.or(configured_model) {
            Some(model) => format!("{display} ({model})"),
            None => display.to_string(),
        }
    }
}

/// Write the run's workpad snapshot to `<working_dir>/.pidash/` before
/// the agent launches (NEWPIDASH-320 §6.1): `workpad.md` (empty when the
/// workpad is empty) plus its `workpad.sha256` guard file. The CLI's
/// `workpad update` sends the hash back as `if_body_sha256` and rewrites
/// it from each response.
fn materialise_workpad_snapshot(
    working_dir: &std::path::Path,
    body: &str,
    sha256: &str,
) -> std::io::Result<()> {
    let dir = working_dir.join(".pidash");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("workpad.md"), body)?;
    std::fs::write(dir.join("workpad.sha256"), format!("{sha256}\n"))?;
    Ok(())
}

/// Remove a stale workpad snapshot from `<working_dir>/.pidash/`
/// (NEWPIDASH-320 review): an assign without a snapshot (a scheduler run,
/// or any run from a server older than the unified workpad) owns no
/// workpad, and the runner reuses one `working_dir` across runs, so a
/// previous run's `workpad.md` + `workpad.sha256` must not leak into it. Best-effort like the write
/// path — missing files are fine, other errors warn.
fn discard_stale_workpad_snapshot(working_dir: &std::path::Path) {
    for name in ["workpad.md", "workpad.sha256"] {
        let path = working_dir.join(".pidash").join(name);
        if let Err(err) = std::fs::remove_file(&path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %path.display(), error = %err, "stale workpad snapshot delete failed; continuing without the file");
        }
    }
}

struct CurrentChat {
    chat_session_id: uuid::Uuid,
    tx: mpsc::Sender<ChatCommand>,
    active_rx: watch::Receiver<bool>,
    /// Fires when the runtime's worker has ended.
    done_rx: oneshot::Receiver<()>,
    /// Fires once the runtime's containment unit is finished clean: nothing
    /// its agents started can still be using the working copy.
    clean_rx: oneshot::Receiver<()>,
}

/// A chat runtime that has ended, or was told to, whose agents' processes
/// are not confirmed gone yet. While set, the working copy is not handed to
/// a run or another chat runtime (see [`release_chat_dir`]).
struct StoppingChat {
    chat_session_id: uuid::Uuid,
    clean_rx: oneshot::Receiver<()>,
    /// How long a handoff waits for `clean_rx` before refusing.
    deadline: tokio::time::Instant,
}

/// How long handing a chat runtime's working copy over waits for the old
/// runtime: its bridge shutdown (5 s), the unit's TERM → KILL grace, and a
/// margin for reaping.
const CHAT_HANDOFF_BOUND: Duration =
    Duration::from_secs(5 + containment::CLEANUP_GRACE.as_secs() + 5);

/// Start a chat runtime: the worker runs with its agents in its own
/// containment unit, which is finished when the worker ends.
fn spawn_chat_runtime(
    mut worker: ChatWorker,
    chat_session_id: uuid::Uuid,
    tx: mpsc::Sender<ChatCommand>,
    active_rx: watch::Receiver<bool>,
) -> CurrentChat {
    let (done_tx, done_rx) = oneshot::channel();
    let (clean_tx, clean_rx) = oneshot::channel();
    tokio::spawn(async move {
        let teardown = match worker.unit.as_ref().map(ChatUnit::env) {
            Some(env) => {
                crate::util::shell::CHAT_ENV
                    .scope(env, worker.run(chat_session_id))
                    .await
            }
            None => worker.run(chat_session_id).await,
        };
        let _ = done_tx.send(());
        if let Some(unit) = worker.unit.take() {
            finish_chat_unit(unit, &worker.state).await;
        }
        drop(teardown);
        let _ = clean_tx.send(());
    });
    CurrentChat {
        chat_session_id,
        tx,
        active_rx,
        done_rx,
        clean_rx,
    }
}

/// Finish an ended chat runtime's unit. While it is unresolved the runner
/// stays reserved (local chat and hot restarts honour the reservation) and
/// the unit is re-checked every [`UNRESOLVED_CLEANUP_RECHECK`]; daemon
/// shutdown ends the wait.
async fn finish_chat_unit(mut unit: ChatUnit, state: &StateHandle) {
    if unit.finish().await == CleanupOutcome::Clean {
        return;
    }
    tracing::warn!(
        "chat runtime's processes are not confirmed gone; runner stays reserved until they are"
    );
    state.set_cleanup_reserved(true).await;
    let shutdown = state.shutdown_notified();
    let notified = shutdown.notified();
    tokio::pin!(notified);
    notified.as_mut().enable();
    loop {
        if state.shutdown_reason().is_some() {
            return;
        }
        tokio::select! {
            _ = &mut notified => return,
            _ = tokio::time::sleep(UNRESOLVED_CLEANUP_RECHECK) => {}
        }
        if unit.finish().await == CleanupOutcome::Clean {
            state.set_cleanup_reserved(false).await;
            tracing::info!("chat runtime's processes are gone; runner released");
            return;
        }
    }
}

/// Free the working copy held by a chat runtime before a run or another chat
/// runtime starts in it. A live runtime in `current` is released (Release,
/// not Shutdown: the cloud session stays open for revival) and moves to
/// `stopping`. Waits, at most until its deadline, for the old runtime's unit
/// to finish clean. `false` while it has not: the caller must not start
/// anything in the working copy, and a later call checks again.
async fn release_chat_dir(
    current: &mut Option<CurrentChat>,
    stopping: &mut Option<StoppingChat>,
) -> bool {
    if let Some(chat) = current.take() {
        let _ = chat.tx.send(ChatCommand::Release).await;
        *stopping = Some(StoppingChat {
            chat_session_id: chat.chat_session_id,
            clean_rx: chat.clean_rx,
            deadline: tokio::time::Instant::now() + CHAT_HANDOFF_BOUND,
        });
    }
    let Some(st) = stopping.as_mut() else {
        return true;
    };
    match tokio::time::timeout_at(st.deadline, &mut st.clean_rx).await {
        // A closed channel means the runtime's task is gone (daemon
        // shutdown): its unit's drop made the last attempt.
        Ok(_) => {
            *stopping = None;
            true
        }
        Err(_) => {
            tracing::warn!(
                chat_session_id = %st.chat_session_id,
                "previous chat runtime's processes are not confirmed gone; not handing over the working copy"
            );
            false
        }
    }
}

async fn acknowledge_cancel_without_active_process(out: &RunnerOut, run_id: uuid::Uuid) {
    if let Err(err) = out
        .send(ClientMsg::RunCancelled {
            run_id,
            cancelled_at: Utc::now(),
            tokens: None,
            model: None,
        })
        .await
    {
        tracing::warn!(
            %run_id,
            error = %err,
            "failed to acknowledge cancel for a run with no active process"
        );
    }
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
    Cancel {
        reason: Option<String>,
    },
    Close {
        reason: Option<String>,
    },
    /// Tear down this runtime to make room for another session (or to free the
    /// runner's working dir for an assign) WITHOUT closing the
    /// cloud session: it stays open and a later warm/message revives it via
    /// `local_thread_id`. Contrast with `Shutdown`, which emits a terminal
    /// `ChatClosed`.
    Release,
    Shutdown,
}

impl RunnerLoop {
    /// Decide whether an `Assign` to this runner should be NACKed before
    /// `Accept` for a credential reason. Returns `Some(reason)` when the
    /// assign must be turned down without starting the agent.
    ///
    /// Pure helper so the Assign-reject decision is unit-testable without
    /// driving the whole loop. The production call site passes
    /// `crate::agent::auth::has_credentials(kind)` and `is_managed_codex`.
    /// Managed Codex runners carry Pi Dash-managed auth and skip the
    /// presence check, like `Engine`.
    fn agent_auth_missing_rejection(
        agent_kind: AgentKind,
        has_credentials: bool,
        managed_codex: bool,
    ) -> Option<FailureReason> {
        match agent_kind {
            AgentKind::OpenClaw | AgentKind::PidashAgent => None,
            AgentKind::Codex if managed_codex => None,
            _ if !has_credentials => Some(FailureReason::AgentAuthMissing),
            _ => None,
        }
    }

    /// Backwards-compatible alias for the Muse-only name.
    #[allow(dead_code)]
    fn muse_assign_rejection(
        agent_kind: AgentKind,
        has_credentials: bool,
    ) -> Option<FailureReason> {
        Self::agent_auth_missing_rejection(agent_kind, has_credentials, false)
    }

    /// The detail string and warning fix hint for an auth-missing NACK.
    /// Names the agent, runner, machine (`hostname`), and exact fix per
    /// the issue example: “Claude Code on runner X (machine Y) has no
    /// credentials: …”.
    fn agent_auth_missing_detail(
        agent_kind: AgentKind,
        runner_name: &str,
        machine: &str,
    ) -> String {
        format!(
            "assign rejected: {} credentials missing on runner {runner_name} (machine {machine}): {}",
            agent_kind.display_name(),
            crate::agent::auth::fix_hint(agent_kind)
        )
    }

    /// Backwards-compatible wrapper for the Muse-only call site.
    #[allow(dead_code)]
    fn muse_auth_missing_detail(runner_name: &str) -> String {
        let machine = hostname().unwrap_or_else(|| "unknown".to_string());
        Self::agent_auth_missing_detail(AgentKind::MuseCode, runner_name, &machine)
    }

    /// NACK an `Assign` this runner cannot take right now. The cloud frees a
    /// runner the moment its previous run goes terminal in the database,
    /// which can precede the local agent actually stopping (most commonly a
    /// user cancel) — so the matcher may dispatch the next run to a runner
    /// that is still busy. Silently dropping that Assign leaves the run
    /// ASSIGNED forever: the runner never starts it and the cloud shows the
    /// runner busy with nothing running. `RunFailed{AssignRejectedBusy}`
    /// lets the cloud re-queue it for a fresh dispatch.
    ///
    /// Sent from a detached task so a slow cloud cannot stall the message
    /// loop. Delivery failure is logged only — the session-open redeliver
    /// path remains the backstop for a lost NACK.
    fn nack_assign(&self, run_id: uuid::Uuid, detail: String) {
        self.nack_assign_with(run_id, detail, FailureReason::AssignRejectedBusy);
    }

    /// Same as [`nack_assign`](Self::nack_assign) but with an explicit
    /// `FailureReason`. Used for `AgentAuthMissing` where the cloud re-queues
    /// identically to `AssignRejectedBusy` but distinguishes the case.
    fn nack_assign_with(&self, run_id: uuid::Uuid, detail: String, reason: FailureReason) {
        let out = self.out.clone();
        tokio::spawn(async move {
            let msg = ClientMsg::RunFailed {
                run_id,
                reason,
                detail: Some(detail),
                ended_at: Utc::now(),
                tokens: None,
                model: None,
            };
            if let Err(e) = out.send(msg).await {
                tracing::warn!(%run_id, "failed to NACK rejected assign: {e:#}");
            }
        });
    }

    /// See [`release_chat_dir`]. Swapping the runtime to another session
    /// releases it rather than shutting it down: the user may switch back to
    /// the old session from the chat history panel, and a warm/message
    /// revives it.
    async fn release_chat_dir(&mut self) -> bool {
        release_chat_dir(&mut self.current_chat, &mut self.stopping_chat).await
    }

    /// Start a runtime for `chat_session_id` once the previous one's
    /// processes are gone; `None` while they are not.
    async fn fresh_chat_runtime(
        &mut self,
        chat_session_id: uuid::Uuid,
    ) -> Option<mpsc::Sender<ChatCommand>> {
        if !self.release_chat_dir().await {
            return None;
        }
        Some(self.start_chat_runtime(chat_session_id).await)
    }

    async fn start_chat_runtime(
        &mut self,
        chat_session_id: uuid::Uuid,
    ) -> mpsc::Sender<ChatCommand> {
        let (tx, rx) = mpsc::channel(8);
        let (active_tx, active_rx) = watch::channel(false);
        let worker = ChatWorker {
            runner_config: self.runner_config.clone(),
            state: self.state.clone(),
            approvals: self.approvals.clone(),
            out: self.out.clone(),
            command_rx: rx,
            active_tx,
            guidance_sent: false,
            unit: Some(ChatUnit::new(chat_session_id)),
        };
        self.current_chat = Some(spawn_chat_runtime(
            worker,
            chat_session_id,
            tx.clone(),
            active_rx,
        ));
        tx
    }

    /// Reserve this runner for a run and spawn its worker. Shared by the
    /// Assign arm and restart re-hydration so outer-loop semantics (early
    /// `in_flight` stamp, single-tenant guard, completion wakeup) cannot
    /// diverge between the two paths.
    async fn reserve_and_spawn(
        &mut self,
        run_id: uuid::Uuid,
        prompt: String,
        repo_url: Option<String>,
        expected_codex_model: Option<String>,
        context: AssignContext,
    ) {
        let cancel = tokio_util::sync::CancellationToken::new();
        let (done_tx, done_rx) = oneshot::channel();
        self.current_run = Some(CurrentRun {
            run_id,
            cancel: cancel.clone(),
            done_rx,
        });
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
        let engine = self.engine.clone();
        let coordinator = self.coordinator.clone();
        let queue_blocked = self.queue_blocked;
        tokio::spawn(async move {
            let mut worker = AssignWorker {
                runner_paths,
                daemon_paths,
                runner_config,
                state,
                approvals,
                out,
                cancel,
                engine,
                coordinator,
                queue_blocked,
                run_end: RunEnd::default(),
            };
            if let Err(e) = worker
                .run(run_id, prompt, repo_url, expected_codex_model, context)
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
                        tokens: None,
                        model: None,
                    })
                    .await;
                worker.state.set_current_run(None).await;
            }
            // The run has been reported; the runner stays reserved (the
            // loop keeps `current_run`) until its cleanup is resolved.
            worker.hold_while_unresolved().await;
            let _ = done_tx.send(());
        });
    }

    async fn run(mut self) -> Result<()> {
        // Restart re-hydration (NEWPIDASH-241 T6): awaiting entries that
        // survived a crash become this loop's current run again — the
        // worker blocks oldest-first and never replays started work
        // (non-waiting phases never load). Malformed payloads fail closed
        // (durable cancel, no execution).
        if let Some(coord) = self.coordinator.as_ref()
            && let Some(entry) = coord.waiting_for(self.runner_paths.runner_id)
        {
            let parsed: Result<serde_json::Value, _> =
                serde_json::from_slice(&entry.assignment_payload);
            match parsed {
                Ok(v) if v.get("prompt").and_then(|p| p.as_str()).is_some() => {
                    let prompt = v
                        .get("prompt")
                        .and_then(|p| p.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let repo_url = v
                        .get("repo_url")
                        .and_then(|r| r.as_str())
                        .map(|s| s.to_string());
                    let model = v
                        .get("expected_codex_model")
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string());
                    // Payloads written before the run-context change lack
                    // these keys — they read back as absent (old servers).
                    let opt =
                        |key: &str| v.get(key).and_then(|s| s.as_str()).map(|s| s.to_string());
                    let context = AssignContext {
                        issue_identifier: opt("issue_identifier"),
                        project_identifier: opt("project_identifier"),
                        workpad_body: opt("workpad_body"),
                        workpad_sha256: opt("workpad_sha256"),
                    };
                    self.reserve_and_spawn(entry.run_id, prompt, repo_url, model, context)
                        .await;
                }
                _ => {
                    tracing::error!(run_id=%entry.run_id, "waiting payload unparsable; durable cancel, no execution");
                    let _ = coord.cancel_waiting(entry.run_id);
                }
            }
        }
        loop {
            let frame = {
                let inbound = self.inbound.recv();
                tokio::pin!(inbound);
                // `done_rx` exists only while a run is in flight; outside of that
                // window we wait on `pending()` so the select arm is inert.
                tokio::select! {
                    biased;
                    () = wait_done(&mut self.current_run) => {
                        // Machine FIFO: release the slot so the oldest
                        // waiting runner can start (coordinator owns
                        // reservation clearing; worker no longer clears).
                        if let Some(finished) = self.current_run.take()
                            && let Some(coord) = self.coordinator.as_ref()
                        {
                            release_finished_slot(coord, &self.state, finished.run_id);
                        }
                        continue;
                    }
                    () = wait_chat_done(&mut self.current_chat) => {
                        // The runtime ended on its own (idle timeout, Close):
                        // its unit is being finished, and the working copy
                        // is held until it is clean.
                        if let Some(chat) = self.current_chat.take() {
                            self.stopping_chat = Some(StoppingChat {
                                chat_session_id: chat.chat_session_id,
                                clean_rx: chat.clean_rx,
                                deadline: tokio::time::Instant::now() + CHAT_HANDOFF_BOUND,
                            });
                        }
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
                        && let Some(swap_guard) = self.state.try_claim_swap()
                    {
                        let st = self.state.clone();
                        let latest_owned = latest.to_string();
                        let paths = self.paths.clone();
                        tokio::spawn(async move {
                            let _swap_guard = swap_guard;
                            tracing::info!(
                                target = %latest_owned,
                                "auto-update: swapping pidash on disk",
                            );
                            match crate::cli::update::check_or_swap(false, &paths).await {
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
                        });
                    }
                }
                ServerMsg::Assign {
                    run_id,
                    prompt,
                    repo_url,
                    expected_codex_model,
                    assignment_token,
                    issue_identifier,
                    project_identifier,
                    workpad_body,
                    workpad_sha256,
                    ..
                } => {
                    // Cross-lane exclusion: a runner has one working dir shared
                    // between chat and issue execution, so the two cannot run
                    // concurrently. A concurrent chat means a second runner, not
                    // a parallel lane inside this one (PDASHOSS01-133). Reject an
                    // assign while a chat turn is active; tear an idle chat
                    // runtime down to free the dir.
                    let cloud_chat_active = self
                        .current_chat
                        .as_ref()
                        .is_some_and(|chat| *chat.active_rx.borrow());
                    // A *local* chat turn (driven over the daemon IPC socket,
                    // PDASHOSS01-159) holds the same working copy but lives in
                    // a different actor, so it is invisible to `current_chat` —
                    // consult the shared flag too (issue AC6).
                    let local_chat_active = self
                        .local_chat_active
                        .load(std::sync::atomic::Ordering::SeqCst);
                    if cloud_chat_active || local_chat_active {
                        tracing::warn!(
                            %run_id,
                            cloud_chat_active,
                            local_chat_active,
                            "assign received while chat is active on this runner; rejecting"
                        );
                        // NACK rather than silently dropping, so the cloud
                        // re-queues the run instead of leaving it stuck
                        // ASSIGNED forever (#246).
                        self.nack_assign(
                            run_id,
                            "assign rejected: a chat turn is active on this runner".to_string(),
                        );
                        continue;
                    }
                    // Intra-assign-lane guard — one issue in flight at a time —
                    // is kept for ALL runners (the assign lane stays
                    // single-tenant; the cloud reaper assumes it, design §3.4).
                    if let Some(current) = &self.current_run {
                        tracing::warn!(
                            %run_id,
                            in_flight = %current.run_id,
                            "assign received while a run is already in flight; rejecting"
                        );
                        self.nack_assign(
                            run_id,
                            format!(
                                "assign rejected: run {} is still in flight on this runner",
                                current.run_id
                            ),
                        );
                        continue;
                    }
                    // Processes of an earlier run (or of a crashed daemon's
                    // run) may still use the working copy.
                    if self.state.cleanup_reserved() {
                        tracing::warn!(
                            %run_id,
                            "assign received while an earlier run's processes are unresolved; rejecting"
                        );
                        self.nack_assign(
                            run_id,
                            "assign rejected: an earlier run's processes on this runner are not yet accounted for"
                                .to_string(),
                        );
                        continue;
                    }
                    // Claim the working copy like a local turn before the
                    // handoff, so no warm or send can start an agent in it
                    // after the idle sessions are snapshotted. Held until the
                    // run is published as in flight below (dropped on every
                    // `continue`).
                    let Some(_handoff) =
                        crate::ipc::chat::claim_working_copy(&self.local_chat_active)
                    else {
                        self.nack_assign(
                            run_id,
                            "assign rejected: a chat turn is active on this runner".to_string(),
                        );
                        continue;
                    };
                    // Hand the working copy over only once no chat agent can
                    // still be using it (PDASHOSS01-417): the cloud chat
                    // runtime and every idle local chat session are ended and
                    // what they started is stopped, ownership proven first.
                    if !self.release_chat_dir().await || !self.chat_sessions.release_idle().await {
                        tracing::warn!(
                            %run_id,
                            "assign received while a chat session's processes are still stopping; rejecting"
                        );
                        self.nack_assign(
                            run_id,
                            "assign rejected: a chat session's processes are still stopping on this runner"
                                .to_string(),
                        );
                        continue;
                    }
                    // Pre-pickup credential check: without the agent's
                    // credentials the agent would fail at
                    // `run.lifecycle.started` before any work, and the failure
                    // would burn the issue's tick budget. Reject the Assign
                    // before `Accept` (and before spawning the agent) so the
                    // cloud re-queues it for another runner.
                    // Managed Codex runners carry Pi Dash-managed auth and
                    // skip the presence check, like Engine.
                    if let Some(reason) = Self::agent_auth_missing_rejection(
                        self.runner_config.agent.kind,
                        crate::agent::auth::has_credentials(self.runner_config.agent.kind),
                        crate::agent::auth::is_managed_codex(&self.runner_config.codex),
                    ) {
                        let machine = hostname().unwrap_or_else(|| "unknown".to_string());
                        let detail = Self::agent_auth_missing_detail(
                            self.runner_config.agent.kind,
                            &self.runner_config.name,
                            &machine,
                        );
                        tracing::warn!(
                            %run_id,
                            runner = %self.runner_config.name,
                            agent = %self.runner_config.agent.kind.display_name(),
                            "assign rejected: {} credentials missing on this runner; refusing pickup before Accept (not starting agent)",
                            self.runner_config.agent.kind.display_name(),
                        );
                        self.nack_assign_with(run_id, detail, reason);
                        continue;
                    }
                    // Durable held check: covers the startup window before
                    // re-hydration reserves the runner (current_run is still
                    // None while the waiting entry already exists).
                    if let Some(coord) = self.coordinator.as_ref()
                        && coord.runner_held(self.runner_paths.runner_id)
                    {
                        tracing::warn!(%run_id, "assign while runner already holds an attempt; rejecting");
                        self.nack_assign(
                            run_id,
                            "assign rejected: runner already holds an attempt".to_string(),
                        );
                        continue;
                    }
                    // Machine FIFO (NEWPIDASH-241): storage failure fails
                    // closed — NACK so the cloud re-queues, never execute.
                    if self.queue_blocked {
                        tracing::warn!(%run_id, "assign while run queue storage_blocked; rejecting");
                        self.nack_assign(
                            run_id,
                            "run queue storage_blocked: durable queue unavailable".to_string(),
                        );
                        continue;
                    }
                    // Durable enqueue before any workspace prep: allocate
                    // the FIFO sequence and persist the payload. The worker
                    // below holds this runner without starting until the
                    // coordinator admits it (oldest-first).
                    // Run context rides the durable queue too, so a restart
                    // between enqueue and admission keeps the env and the
                    // workpad file (old daemons' payloads lack these keys —
                    // the re-hydration reader treats them as absent).
                    let context = AssignContext {
                        issue_identifier,
                        project_identifier,
                        workpad_body,
                        workpad_sha256,
                    };
                    if let Some(coord) = self.coordinator.as_ref() {
                        let payload = serde_json::to_vec(&serde_json::json!({
                            "prompt": prompt,
                            "repo_url": repo_url,
                            "expected_codex_model": expected_codex_model,
                            "issue_identifier": context.issue_identifier,
                            "project_identifier": context.project_identifier,
                            "workpad_body": context.workpad_body,
                            "workpad_sha256": context.workpad_sha256,
                        }))
                        .unwrap_or_default();
                        if let Err(e) = coord.enqueue_assign(
                            self.runner_paths.runner_id,
                            run_id,
                            assignment_token,
                            payload,
                        ) {
                            tracing::error!(%run_id, error = %e, "durable enqueue failed; rejecting");
                            self.nack_assign(
                                run_id,
                                format!("run queue storage_blocked: enqueue failed: {e}"),
                            );
                            continue;
                        }
                    }
                    self.reserve_and_spawn(run_id, prompt, repo_url, expected_codex_model, context)
                        .await;
                }
                ServerMsg::Cancel { run_id, reason } => {
                    tracing::info!(%run_id, ?reason, "cancel received");
                    if let Some(run) = &self.current_run {
                        if run.run_id == run_id {
                            // Durable cancel first (waiting runs must never
                            // start workspace prep or an agent), then signal
                            // the worker (blocked or executing) to stop.
                            if let Some(coord) = self.coordinator.as_ref() {
                                let _ = coord.cancel_waiting(run_id);
                            }
                            run.cancel.cancel();
                        } else {
                            tracing::warn!(
                                "cancel for run {run_id} but active run is {active}; acknowledging stopped target",
                                active = run.run_id,
                            );
                            acknowledge_cancel_without_active_process(&self.out, run_id).await;
                        }
                    } else {
                        // The cloud may cancel an ASSIGNED run before its
                        // Assign frame reaches this daemon, or redeliver a
                        // persisted cancellation after a restart. With no
                        // matching local worker there is no process to stop,
                        // so acknowledge immediately instead of waiting for
                        // heartbeat reconciliation.
                        acknowledge_cancel_without_active_process(&self.out, run_id).await;
                    }
                }
                ServerMsg::Decide {
                    approval_id,
                    decision,
                    ..
                } => {
                    let key = cloud_approval_key(&self.approvals, approval_id).await;
                    self.approvals
                        .decide(&key, decision, DecisionSource::Cloud)
                        .await;
                    self.state.note_approvals_changed();
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
                    // One working dir per runner: chat cannot run during an issue
                    // run (or while an earlier run's processes are unresolved).
                    // A concurrent chat means a second runner.
                    if self.current_run.is_some() || self.state.cleanup_reserved() {
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
                            Some(chat.tx.clone())
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
                            self.fresh_chat_runtime(chat_session_id).await
                        }
                    } else {
                        self.fresh_chat_runtime(chat_session_id).await
                    };
                    let Some(tx) = tx else {
                        let _ = self
                            .out
                            .send(ClientMsg::ChatFailed {
                                chat_session_id,
                                code: "runner_busy".into(),
                                detail: Some(
                                    "the previous chat session's processes are still stopping"
                                        .into(),
                                ),
                                failed_at: Utc::now(),
                            })
                            .await;
                        continue;
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
                    // One working dir per runner: skip warm during an issue run
                    // (or while an earlier run's processes are unresolved).
                    if self.current_run.is_some() || self.state.cleanup_reserved() {
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
                            Some(chat.tx.clone())
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
                            self.fresh_chat_runtime(chat_session_id).await
                        }
                    } else {
                        self.fresh_chat_runtime(chat_session_id).await
                    };
                    let Some(tx) = tx else {
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
                    self.state.note_approvals_changed();
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
                        run.cancel.cancel();
                    }
                    // B4: tear down the chat lane too. RemoveRunner previously
                    // cancelled only the run lane, leaving the chat worker (and
                    // its bridge) to be aborted when the RunnerLoop exits, with
                    // no terminal `ChatClosed` for the client.
                    if let Some(chat) = &self.current_chat {
                        let _ = chat.tx.send(ChatCommand::Shutdown).await;
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
                    self.live_instances
                        .write()
                        .expect("runner registry poisoned")
                        .remove(&self.runner_paths.runner_id);
                    // Best-effort cleanup of this runner's local data
                    // dir. The on-disk state is keyed by runner_id and
                    // is dead weight once the cloud-side row is gone.
                    // The working directory (the user's checkout) is
                    // never deleted — not even when it was configured
                    // inside the data dir.
                    let runner_dir = self.runner_paths.base_dir().to_path_buf();
                    remove_runner_data_dir(&runner_dir, &self.runner_config.workspace.working_dir);
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

/// Delete a removed runner's data dir, keeping its working directory.
/// Normally the two are disjoint (the default working dir lives under
/// `data_dir/workspaces/`); if the working dir sits inside the data dir the
/// data dir is left alone rather than risk the user's checkout.
fn remove_runner_data_dir(runner_dir: &std::path::Path, working_dir: &std::path::Path) {
    if !runner_dir.exists() {
        return;
    }
    let runner_dir_cmp = canonical_for_compare(runner_dir);
    let working_dir_cmp = canonical_for_compare(working_dir);
    if working_dir_cmp.starts_with(&runner_dir_cmp) {
        tracing::warn!(
            "not deleting {:?}: it contains this runner's working directory {:?}",
            runner_dir,
            working_dir,
        );
        return;
    }
    if let Err(e) = std::fs::remove_dir_all(runner_dir) {
        tracing::warn!(
            "failed to delete {:?}: {e:#} (file removal is best-effort)",
            runner_dir,
        );
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

fn chat_resume_id(local_session_id: Option<&str>, local_thread_id: Option<&str>) -> Option<String> {
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
    runner_config: crate::config::schema::RunnerConfig,
    state: StateHandle,
    approvals: ApprovalRouter,
    out: RunnerOut,
    command_rx: mpsc::Receiver<ChatCommand>,
    active_tx: watch::Sender<bool>,
    /// Whether the live bridge already received the CLI guidance
    /// (NEWPIDASH-402). A worker owns one session runtime; a fresh
    /// worker (revival) starts with a fresh subprocess, so this
    /// starts false and is cleared whenever the bridge is dropped.
    guidance_sent: bool,
    /// The runtime's containment unit (PDASHOSS01-417), finished when the
    /// worker ends (see [`spawn_chat_runtime`]). `None` in tests.
    unit: Option<ChatUnit>,
}

impl ChatWorker {
    /// Record a bridge just spawned in the runtime's unit (cgroup backend:
    /// move its agent into the unit's cgroup).
    fn adopt(&mut self, bridge: Option<&AgentBridge>) {
        if let (Some(unit), Some(bridge)) = (self.unit.as_mut(), bridge) {
            unit.adopt(bridge.process_handle().pid);
        }
    }

    /// Returns the runtime's teardown count when it has a unit: held until
    /// the unit's first cleanup pass ends, so local chat stays refused while
    /// the agents are stopping.
    async fn run(&mut self, chat_session_id: uuid::Uuid) -> Option<ChatTeardown> {
        // A session's runtime idles out after 5 minutes of silence and the
        // worker exits. Chat runs in the runner's one working directory and
        // performs no git lifecycle of its own — the agent drives git per the
        // prompt (PDASHOSS01-133). Reviving (a new message for the same session
        // id) spawns a fresh worker in the same directory.
        const CHAT_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

        let mut bridge: Option<AgentBridge> = None;
        let mut workspace_path: Option<std::path::PathBuf> = None;
        let mut bridge_seq = 0u64;
        let mut started_sent = false;

        // B4: the chat lane gets its own graceful-shutdown signal. Before this,
        // only the assign lane observed `shutdown_notified()`, so on daemon
        // shutdown / RemoveRunner the chat loop was aborted with no terminal
        // `ChatClosed`. A turn already in flight finishes first.
        //
        // The `Notified` future is created ONCE and pinned across iterations:
        // `notify_waiters()` does not store a permit, so a fresh future created
        // per loop turn would miss a signal that landed while we were inside a
        // turn (it would then hang until the idle timeout). A pinned future
        // stays registered as a waiter and catches the signal on the next poll.
        let shutdown = self.state.shutdown_notified();
        let notified = shutdown.notified();
        tokio::pin!(notified);
        loop {
            let command = tokio::select! {
                biased;
                _ = &mut notified => {
                    let _ = self
                        .out
                        .send(ClientMsg::ChatClosed {
                            chat_session_id,
                            closed_at: Utc::now(),
                        })
                        .await;
                    break;
                }
                recv = tokio::time::timeout(CHAT_IDLE_TIMEOUT, self.command_rx.recv()) => {
                    match recv {
                        Ok(Some(command)) => command,
                        Ok(None) | Err(_) => break,
                    }
                }
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
                        self.guidance_sent = false;
                    }
                }
                ChatCommand::Message(turn) => {
                    let _ = self.active_tx.send(true);
                    self.state.set_chat_active(true).await;
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
                            self.guidance_sent = false;
                            false
                        }
                    };
                    let _ = self.active_tx.send(false);
                    self.state.set_chat_active(false).await;
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
                ChatCommand::Release => {
                    // Runtime swap (see `stop_idle_chat_runtime`). No terminal
                    // `ChatClosed` — the cloud session stays open for revival.
                    // The post-loop persist below still commits + pushes.
                    break;
                }
                ChatCommand::Shutdown => {
                    // RemoveRunner / teardown. Emit a terminal `ChatClosed` so
                    // the cloud session doesn't linger in "active turn" state.
                    let _ = self
                        .out
                        .send(ClientMsg::ChatClosed {
                            chat_session_id,
                            closed_at: Utc::now(),
                        })
                        .await;
                    break;
                }
            }
        }
        // Counted before the bridge shuts down: from here the agents are
        // stopping, and local chat must not take the working copy.
        let teardown = self
            .unit
            .is_some()
            .then(|| self.state.begin_chat_teardown());
        if let Some(bridge) = bridge.take() {
            bridge.shutdown(Duration::from_secs(5)).await.ok();
        }
        let _ = self.active_tx.send(false);
        self.state.set_chat_active(false).await;
        teardown
    }

    async fn send_timing_event(
        &self,
        chat_session_id: uuid::Uuid,
        bridge_seq: &mut u64,
        stage: &str,
        payload: serde_json::Value,
    ) {
        *bridge_seq = (*bridge_seq).saturating_add(1);
        let out = self.out.clone();
        let event = ClientMsg::ChatEvent {
            chat_session_id,
            bridge_seq: *bridge_seq,
            kind: "chat_timing".into(),
            payload: timing_payload(stage, payload),
        };
        tokio::spawn(async move {
            match tokio::time::timeout(Duration::from_secs(2), out.send(event)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::debug!(error = %e, "chat timing event post failed");
                }
                Err(_) => {
                    tracing::debug!("chat timing event post timed out");
                }
            }
        });
    }

    fn send_chat_started_event(&self, chat_session_id: uuid::Uuid, thread_id: String) {
        let out = self.out.clone();
        tokio::spawn(async move {
            if let Err(e) = out
                .send(ClientMsg::ChatStarted {
                    chat_session_id,
                    local_thread_id: thread_id.clone(),
                    local_session_id: Some(thread_id),
                    started_at: Utc::now(),
                })
                .await
            {
                tracing::debug!(error = %e, "chat started event post failed");
            }
        });
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
            *workspace_path = Some(
                self.resolve_chat_workspace(chat_session_id, warm.cwd.as_deref())
                    .await?,
            );
        }
        let workspace_path = workspace_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("chat workspace missing"))?;

        if bridge.as_ref().is_some_and(bridge_has_exited) {
            if let Some(bridge) = bridge.take() {
                bridge.shutdown(Duration::from_secs(1)).await.ok();
            }
            *started_sent = false;
            self.guidance_sent = false;
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
                    None,
                    // Chat turns have no runner log dir at hand; Antigravity
                    // falls back to its default ACP log location.
                    None,
                )
                .await?,
            );
            self.adopt(bridge.as_ref());
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
        let warmed_thread_id = within_startup_deadline(bridge.warm(workspace_path)).await?;
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
                self.send_chat_started_event(chat_session_id, thread_id.clone());
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
        let bridge_slot = bridge;
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
            *workspace_path = Some(
                self.resolve_chat_workspace(chat_session_id, turn.cwd.as_deref())
                    .await?,
            );
        }
        let workspace_path = workspace_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("chat workspace missing"))?;

        if bridge_slot.as_ref().is_some_and(bridge_has_exited) {
            if let Some(bridge) = bridge_slot.take() {
                bridge.shutdown(Duration::from_secs(1)).await.ok();
            }
            *started_sent = false;
            self.guidance_sent = false;
        }

        let resume_id = chat_resume_id(
            turn.local_session_id.as_deref(),
            turn.local_thread_id.as_deref(),
        );
        if bridge_slot.is_none() {
            let spawn_started = Instant::now();
            *bridge_slot = Some(
                AgentBridge::spawn_from_config_with_resume(
                    &self.runner_config,
                    workspace_path,
                    turn.model.clone(),
                    resume_id.as_deref(),
                    None,
                    // Chat turns have no runner log dir at hand; Antigravity
                    // falls back to its default ACP log location.
                    None,
                )
                .await?,
            );
            self.adopt(bridge_slot.as_ref());
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
        let bridge = bridge_slot
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("chat bridge missing"))?;
        // First turn of a bridge carries the CLI guidance
        // (NEWPIDASH-402); later turns send the user's text verbatim.
        let injected = !self.guidance_sent;
        let prompt = if injected {
            crate::chat_guidance::with_chat_guidance(&turn.content)
        } else {
            turn.content.clone()
        };
        let payload = RunPayload {
            run_id: turn.message_id,
            prompt,
            model: turn.model,
        };
        let turn_started_at = Instant::now();
        // Keep reading commands while the engine starts up so a Cancel/Close
        // is honoured mid-handshake instead of queueing behind it.
        let interrupt = {
            let command_rx = &mut self.command_rx;
            async move {
                loop {
                    match command_rx.recv().await {
                        Some(ChatCommand::Warm(_)) => {
                            tracing::debug!(
                                %chat_session_id,
                                "chat warm ignored while turn is starting"
                            );
                        }
                        Some(ChatCommand::Message(_)) => {
                            tracing::warn!(
                                %chat_session_id,
                                "chat runtime received a second message while a turn is starting"
                            );
                        }
                        other => return other,
                    }
                }
            }
        };
        let started = tokio::select! {
            biased;
            command = interrupt => Err(command),
            r = within_startup_deadline(bridge.run(&payload, workspace_path)) => Ok(r),
        };
        let mut cursor = match started {
            Ok(r) => r?,
            Err(command) => {
                // The handshake was abandoned mid-flight, so the bridge is in
                // an unknown state: discard it and let the next turn respawn.
                let close_after_turn = matches!(command, Some(ChatCommand::Close { .. }));
                tracing::info!(?command, %chat_session_id, "chat turn interrupted during engine startup");
                if let Some(bridge) = bridge_slot.take() {
                    bridge.shutdown(Duration::from_secs(5)).await.ok();
                }
                *started_sent = false;
                self.guidance_sent = false;
                self.out
                    .send(ClientMsg::ChatMessageCompleted {
                        chat_session_id,
                        message_id: turn.message_id,
                        turn_id: None,
                        assistant_message: None,
                        status: "cancelled".into(),
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
                return Ok(close_after_turn);
            }
        };
        // The engine accepted the turn: the bridge is live and guided
        // from here on (or was already). A later drop re-arms via the
        // resets above (NEWPIDASH-402).
        self.guidance_sent = true;
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
            self.send_chat_started_event(chat_session_id, turn_id.clone());
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
                        // Release shouldn't arrive mid-turn (the supervisor
                        // only swaps idle runtimes), but treat it like a
                        // shutdown of this turn if it ever does.
                        Some(ChatCommand::Release | ChatCommand::Shutdown) | None => {
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
                        self.state.set_chat_active(false).await;
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
                                let policy = Policy::new(&self.runner_config.approval_policy, workspace_path, crate::cloud::protocol::ApprovalMode::Ask);
                                let decision = policy.evaluate(kind, &payload);
                                if let Some(auto) = decision.into_cloud() {
                                    if let Err(e) = bridge.send_approval(&approval_id, auto).await {
                                        self.out.send(ClientMsg::ChatFailed {
                                            chat_session_id,
                                            code: "approval_send_failed".into(),
                                            detail: Some(format!("{e:#}")),
                                            failed_at: Utc::now(),
                                        }).await.ok();
                                        self.state.set_chat_active(false).await;
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
                                self.state.note_approvals_changed();
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
                                                self.state.set_chat_active(false).await;
                                                return Ok(true);
                                            }
                                            self.state.note_approvals_changed();
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
                                self.state.set_chat_active(false).await;
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

    async fn resolve_chat_workspace(
        &mut self,
        _chat_session_id: uuid::Uuid,
        cwd: Option<&str>,
    ) -> Result<std::path::PathBuf> {
        // One working dir per runner: chat runs in exactly the same directory as
        // issue runs, `workspace.working_dir`. There is no separate chat
        // workspace and no git lifecycle here — the agent drives git per the
        // prompt. This is the PDASHOSS01-83 fix: a warm-runner chat message does
        // no git operation before the agent spawns.
        let workspace_path = self.runner_config.workspace.working_dir.clone();
        create_dir_all_private(&workspace_path)?;
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

fn format_exec_command_detail(
    cmd: &crate::daemon::state::ExecCommandSnapshot,
    now: DateTime<Utc>,
) -> String {
    let cwd = cmd
        .cwd
        .as_deref()
        .map(|c| format!(" in `{c}`"))
        .unwrap_or_default();
    if let Some(completed_at) = cmd.completed_at {
        let runtime = (completed_at - cmd.started_at).num_seconds().max(0);
        let since_completed = (now - completed_at).num_seconds().max(0);
        // Distinguish a clean completion from a non-success terminal
        // (codex `failed`, Claude `is_error: true`) so the failure
        // detail does not call an errored command a "completion".
        let verb = match cmd.completed_success {
            Some(false) => "exited with error",
            Some(true) | None => "completed",
        };
        format!(
            "command context: last observed command tool `{}`{cwd} \
             ({verb} {since_completed}s before failure; ran {runtime}s)",
            cmd.command
        )
    } else {
        let elapsed = (now - cmd.started_at).num_seconds().max(0);
        format!(
            "command context: last command frame `{}`{cwd} \
             (started {elapsed}s ago; no completion frame observed)",
            cmd.command
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DiagnosticSignalPriority {
    ExitStatus,
    TextError,
    JsonError,
}

fn diagnostic_signal_from_stderr(lines: &[String]) -> Option<String> {
    let mut best: Option<(DiagnosticSignalPriority, String)> = None;
    for line in lines.iter().rev() {
        let Some((priority, signal)) = diagnostic_signal_from_stderr_line(line) else {
            continue;
        };
        if priority == DiagnosticSignalPriority::JsonError {
            return Some(signal);
        }
        if best
            .as_ref()
            .map(|(best_priority, _)| priority > *best_priority)
            .unwrap_or(true)
        {
            best = Some((priority, signal));
        }
    }
    best.map(|(_, signal)| signal)
}

fn diagnostic_signal_from_stderr_line(line: &str) -> Option<(DiagnosticSignalPriority, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed)
        && let Some(message) = json_error_message(&value)
    {
        return Some((
            DiagnosticSignalPriority::JsonError,
            format!("stderr JSON error: {message}"),
        ));
    }

    let lower = trimmed.to_ascii_lowercase();
    let has_exit_code = lower.contains("exit code");
    let generic_exit_status = lower.starts_with("process exited with code")
        || lower.starts_with("exited with code")
        || lower.starts_with("exit code ");
    let looks_relevant = lower.contains("error")
        || lower.contains("failed")
        || lower.contains("failure")
        || lower.contains("denied")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("panic")
        || has_exit_code
        || generic_exit_status
        || lower.contains("not logged in")
        || lower.contains("unauthorized")
        || lower.contains("forbidden");
    if looks_relevant {
        let priority = if generic_exit_status {
            DiagnosticSignalPriority::ExitStatus
        } else {
            DiagnosticSignalPriority::TextError
        };
        Some((priority, format!("stderr: {trimmed}")))
    } else {
        None
    }
}

fn json_error_message(value: &serde_json::Value) -> Option<String> {
    for key in ["error", "detail", "message"] {
        let Some(v) = value.get(key) else {
            continue;
        };
        if let Some(s) = v.as_str()
            && !s.trim().is_empty()
        {
            return Some(s.trim().to_string());
        }
        if let Some(s) = v.get("message").and_then(|m| m.as_str())
            && !s.trim().is_empty()
        {
            return Some(s.trim().to_string());
        }
    }
    None
}

fn core_error_summary(base: &str, diagnostic_signal: Option<&str>) -> String {
    let Some(signal) = diagnostic_signal else {
        return base.to_string();
    };
    signal
        .strip_prefix("stderr JSON error: ")
        .or_else(|| signal.strip_prefix("stderr: "))
        .unwrap_or(signal)
        .to_string()
}

fn assemble_failure_detail(
    base: &str,
    last_cmd: Option<&crate::daemon::state::ExecCommandSnapshot>,
    stderr: &crate::agent::StderrSnapshot,
    now: DateTime<Utc>,
) -> String {
    const DETAIL_BYTES_CAP: usize = 4096;
    const STDERR_TAIL_LINES: usize = 10;

    let diagnostic_signal = diagnostic_signal_from_stderr(&stderr.lines);
    let core = core_error_summary(base, diagnostic_signal.as_deref());

    let mut trace: Vec<String> = vec![format!("- watchdog: {base}")];
    if let Some(signal) = diagnostic_signal {
        trace.push(format!("- diagnostic signal: {signal}"));
    }
    if let Some(cmd) = last_cmd {
        trace.push(format!("- {}", format_exec_command_detail(cmd, now)));
    }
    // The stderr ring has already filtered codex tracing noise;
    // `stderr.dropped` is the running count of lines we rejected so
    // the user has an honest signal of how much was suppressed.
    // Emit the section if either we have content to show OR a
    // non-zero dropped count (the latter alone tells the operator
    // "the agent was emitting noise but no signal").
    if !stderr.lines.is_empty() || stderr.dropped > 0 {
        let shown = stderr.lines.len().min(STDERR_TAIL_LINES);
        let suffix = if stderr.dropped > 0 {
            format!(
                " (plus {} low-signal line(s) filtered from agent stderr)",
                stderr.dropped
            )
        } else {
            String::new()
        };
        if shown > 0 {
            let tail = stderr
                .lines
                .iter()
                .rev()
                .take(STDERR_TAIL_LINES)
                .rev()
                .map(|line| format!("  {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            trace.push(format!(
                "- stderr context ({shown} line(s){suffix}):\n{tail}"
            ));
        } else {
            // Filter dropped everything — surface that fact alone
            // rather than emitting an empty stderr block.
            trace.push(format!("- stderr context: empty after filtering{suffix}"));
        }
    }

    let mut joined = format!("Core error: {core}\n\nError trace:\n{}", trace.join("\n"));
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

/// Readable run-failure base for a gateway refusal. Prefers the gateway's own
/// sentence; falls back to a status-specific line when the engine gave us no
/// body text, so the Activity row is never an empty or raw string.
fn gateway_refusal_detail(status: u64, text: String) -> String {
    if !text.trim().is_empty() {
        return text;
    }
    match status {
        402 => "Model gateway returned 402: the account has no credit left.".to_string(),
        401 => {
            "Model gateway returned 401: the credential is not accepted. Sign in again.".to_string()
        }
        _ => format!("Model gateway returned unexpected status {status}."),
    }
}

/// Clean a terminal failure base that is the engine's raw
/// `unexpected status ...` string. A 402/401 becomes its gateway text (or
/// the status fallback); anything else is returned unchanged.
fn clean_gateway_failure_detail(raw: &str) -> String {
    let Some((status, text)) = crate::codex::gateway::gateway_status_text(raw) else {
        return raw.to_string();
    };
    if !crate::codex::gateway::is_gateway_refusal_status(status) {
        return raw.to_string();
    }
    gateway_refusal_detail(status, text)
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
    cancel: tokio_util::sync::CancellationToken,
    /// The machine's Pi Dash Agent, for the current profile lane the
    /// free-quota classifier is tied to. Read live per event, so a profile
    /// refresh mid-run still gates correctly.
    engine: Arc<crate::pidash_agent::manager::PidashAgentManager>,
    /// Machine FIFO coordinator (NEWPIDASH-241). None only in tests.
    coordinator: Option<Arc<crate::daemon::run_queue::Coordinator>>,
    queue_blocked: bool,
    run_end: RunEnd,
}

/// Run-end state of an [`AssignWorker`] (PDASHOSS01-417). The terminal
/// report itself is held in the runner's [`StateHandle`]
/// (`hold_terminal`), where the shutdown drain can reach it.
#[derive(Default)]
struct RunEnd {
    /// The run's unit when cleanup left processes it could not account
    /// for. While set, the worker does not finish, so the runner loop keeps
    /// the run in flight and rejects new assignments and chat; the state's
    /// cleanup reservation turns away local (desktop) chat.
    unresolved: Option<RunGroup>,
}

struct RunMetadata {
    tokens: Option<WireTokenUsage>,
    model: Option<String>,
}

/// How often `pump_events` re-checks a pending approval. A run waiting on a
/// human is never stalled, but the loop must notice the approval resolving even
/// when no agent event follows it.
const APPROVAL_WATCHDOG_RECHECK: Duration = Duration::from_secs(30);

impl AssignWorker {
    /// The gateway lane of the machine's engine profile, for the free-quota
    /// classifier. Empty when no profile has been fetched yet, which
    /// classifies as nothing and keeps today's behaviour.
    fn engine_lane(&self) -> String {
        self.engine.profile().map(|p| p.lane).unwrap_or_default()
    }

    async fn run_metadata(&self) -> RunMetadata {
        let snapshot = self.state.observability_snapshot().await;
        RunMetadata {
            tokens: snapshot.tokens,
            model: snapshot.model,
        }
    }

    /// Pick the right `FailureReason` when the agent subprocess crashes or
    /// exits abnormally. Codex stays on `CodexCrash` so dashboards that
    /// already filter on `"codex_crash"` keep working; Claude (and any
    /// future non-Codex agent) surfaces as the agent-neutral `AgentCrash`
    /// so the two aren't conflated in telemetry.
    fn crash_reason(&self) -> FailureReason {
        match self.runner_config.agent.kind {
            // The Pi Dash Agent is Codex, driven by the same driver.
            AgentKind::Codex | AgentKind::PidashAgent => FailureReason::CodexCrash,
            AgentKind::ClaudeCode
            | AgentKind::CursorAgent
            | AgentKind::OpenClaw
            | AgentKind::Grok
            | AgentKind::Antigravity
            | AgentKind::MuseCode => FailureReason::AgentCrash,
        }
    }

    async fn run(
        &mut self,
        run_id: uuid::Uuid,
        prompt: String,
        repo_url: Option<String>,
        expected_codex_model: Option<String>,
        context: AssignContext,
    ) -> Result<()> {
        // Machine FIFO gate (NEWPIDASH-241 T6): hold this runner without
        // workspace preparation or agent start until the coordinator
        // admits it (oldest-first). Waiting cancellation never reaches
        // workspace prep or an agent. Storage failure fails closed.
        if self.queue_blocked {
            anyhow::bail!("run queue storage_blocked: durable queue unavailable");
        }
        // A run restored from the durable queue reaches here without the
        // Assign arm's checks: while the runner is reserved (a crashed
        // daemon's unresolved processes, or an earlier cleanup), it waits
        // here, before admission and any workspace preparation.
        if !self.wait_while_reserved().await {
            tracing::info!(%run_id, "run stopped while the runner was reserved; not started");
            // An explicit cancel (not a shutdown, which leaves a restored
            // entry queued for the next start): the loop's cancel handler
            // only dropped the queue entry, so acknowledge it here and stop
            // reporting the run in flight.
            if self.state.shutdown_reason().is_none() && self.cancel.is_cancelled() {
                acknowledge_cancel_without_active_process(&self.out, run_id).await;
                self.state.set_current_run(None).await;
            }
            return Ok(());
        }
        if let Some(coord) = self.coordinator.as_ref() {
            match coord.admit_blocking(run_id, &self.cancel).await {
                Ok(true) => {}
                Ok(false) => {
                    // Cancelled while waiting: no workspace prep, no agent,
                    // no RunFailed (cancel already acked by RunnerLoop).
                    tracing::info!(%run_id, "assign cancelled while waiting for machine capacity");
                    return Ok(());
                }
                Err(e) => anyhow::bail!("queue admission failed: {e}"),
            }
        }
        self.handle_assign(run_id, prompt, repo_url, expected_codex_model, context)
            .await
    }

    /// Wait until the runner's cleanup reservation clears. `false` when the
    /// run is cancelled or the daemon shuts down first: the run must not
    /// start (a restored entry stays queued for the next start).
    async fn wait_while_reserved(&self) -> bool {
        const POLL: Duration = Duration::from_secs(1);
        let mut logged = false;
        while self.state.cleanup_reserved() {
            if !logged {
                tracing::warn!(
                    runner = %self.runner_config.name,
                    "runner reserved by unresolved processes; run waits before preparing the workspace"
                );
                logged = true;
            }
            if self.state.shutdown_reason().is_some() {
                return false;
            }
            tokio::select! {
                _ = self.cancel.cancelled() => return false,
                _ = tokio::time::sleep(POLL) => {}
            }
        }
        !self.cancel.is_cancelled()
    }

    async fn handle_assign(
        &mut self,
        run_id: uuid::Uuid,
        prompt: String,
        repo_url: Option<String>,
        expected_codex_model: Option<String>,
        context: AssignContext,
    ) -> Result<()> {
        // Per-run process containment (NEWPIDASH-255): create the run's
        // cgroup now and carry its backend on RUN_ENV so every agent
        // spawn below is contained. The guard empties the unit on drop,
        // covering every exit path — normal end, failure, cancellation,
        // daemon shutdown and task abort alike.
        let mut guard =
            containment::create_run_group_for(run_id, Some(self.runner_config.runner_id));
        // The run's terminal report is held while the run executes and
        // sent only after cleanup, so the cloud never sees the run finished
        // (and offers the runner new work) while its processes or its
        // teardown hook are still pending.
        self.hold_terminal();
        // Scope the run context over everything this run does so every
        // agent subprocess the bridges spawn (and re-spawn per turn)
        // inherits `PIDASH_RUN_ID` plus the issue/project/agent names —
        // see `util::shell::RUN_ENV`. Chat turns never enter this scope,
        // so they export none of them.
        // Pin every agent run's `pidash` to this daemon binary
        // (NEWPIDASH-363): `RUN_ENV` reaches all bridges (every agent kind,
        // managed or user-enrolled) without per-bridge changes, and the
        // wrapper re-exports it after rc files run. Config/data dirs ride
        // along whenever the daemon itself has them set.
        let agent_name = context.agent_name(
            self.runner_config.agent.kind,
            expected_codex_model.as_deref(),
            self.runner_config.configured_model(),
        );
        let scope = crate::util::shell::RUN_ENV.scope(
            crate::util::shell::RunEnv {
                run_id,
                backend: guard.backend_kind(),
                process_group: guard.agent_pgid(),
                runner_bin_dir: Some(crate::util::shell::ensure_daemon_pidash_link(
                    &self.daemon_paths.data_dir,
                )),
                config_dir: std::env::var_os("PIDASH_CONFIG_DIR").map(PathBuf::from),
                data_dir: std::env::var_os("PIDASH_DATA_DIR").map(PathBuf::from),
                runner_exe: crate::util::shell::current_exe_canonical(),
                issue_identifier: context.issue_identifier.clone(),
                project_identifier: context.project_identifier.clone(),
                agent_name: Some(agent_name),
            },
            self.handle_assign_inner(
                run_id,
                prompt,
                repo_url,
                expected_codex_model,
                context,
                &mut guard,
            ),
        );
        // Cleanup runs even when the run fails: a failed run must not
        // leave its processes (or its parity-stack hook) behind.
        let result = scope.await;
        // An internal failure is known now: stage its report in the hold
        // before cleanup starts, so a shutdown during a slow cleanup or
        // hook reports this error, not a DaemonRestart.
        let result = self.stage_internal_failure(run_id, result).await;
        // Empty the unit, then execute a run-end hook the run opted into
        // (parity-stack teardown), outside the unit. `workspace` is None
        // when workspace resolution never produced one; then there is no
        // hook to run either.
        let outcome = run_cleanup(&mut guard).await;
        if outcome == CleanupOutcome::Unresolved {
            self.reserve_unresolved(guard).await;
        }
        self.release_terminal().await;
        result
    }

    /// Report a run that failed with an internal error as
    /// `RunFailed{Internal}` through the terminal hold, and clear the
    /// in-flight run (deferred while held). The error is consumed, so the
    /// caller does not report the run a second time.
    async fn stage_internal_failure(&self, run_id: uuid::Uuid, result: Result<()>) -> Result<()> {
        let Err(e) = result else {
            return Ok(());
        };
        tracing::error!("run {run_id} failed: {e:#}");
        self.send(ClientMsg::RunFailed {
            run_id,
            reason: FailureReason::Internal,
            detail: Some(format!("{e:#}")),
            ended_at: Utc::now(),
            tokens: None,
            model: None,
        })
        .await;
        self.clear_current_run().await;
        Ok(())
    }

    /// Keep the runner reserved for a run whose cleanup is unresolved. Set
    /// before the run's report goes out, so local chat never sees the
    /// working copy free while the run's processes may still use it.
    async fn reserve_unresolved(&mut self, guard: RunGroup) {
        self.state.set_cleanup_reserved(true).await;
        self.run_end.unresolved = Some(guard);
    }

    /// Start holding terminal reports (see [`StateHandle::hold_terminal`]).
    fn hold_terminal(&self) {
        self.state.hold_terminal();
    }

    /// Stop holding and send what was held, in order. Nothing is sent when
    /// the shutdown drain already reported the run.
    ///
    /// The reports stay in the state until the in-flight run is cleared, so
    /// a shutdown drain that starts mid-send still finds the real result
    /// and takes the send over; the worker then stops sending. A report the
    /// worker had already sent may then be sent once more by the drain:
    /// the same payload, which the cloud's first-writer-wins finalize
    /// acknowledges as a no-op.
    async fn release_terminal(&self) {
        let mut undelivered = None;
        for msg in self.state.begin_release() {
            if !self.state.releasing() {
                break;
            }
            let run_id = terminal_run_id(&msg);
            // Before the send, so a failed send finds the exact record and
            // does not journal a copy (see `deliver`). A delivered report's
            // record goes with the slot release.
            let payload = run_id.and_then(|r| self.persistable_terminal(r, &msg));
            let recorded = run_id.filter(|r| self.ensure_outbox_record(*r, payload.as_deref()));
            if !self.deliver(msg).await {
                self.state.note_terminal_undelivered();
                // Only a run with a retryable outbox record stays pending.
                undelivered = undelivered.or(recorded);
            }
        }
        if self.state.release_clears_current_run() {
            let kept = undelivered.filter(|_| !self.state.terminal_drained());
            match kept {
                // Still reported in flight until the outbox retry delivers
                // the report: a run the runner stops reporting would be
                // failed by the cloud's heartbeat reaper, and the real
                // result could no longer replace that failure.
                Some(run_id) => self.state.set_report_pending(run_id),
                None => self.state.set_current_run(None).await,
            }
        }
        self.state.finish_release();
    }

    /// Clear the in-flight run, or defer it while terminal reports are
    /// held: the shutdown drain reports runs that are still in flight, so a
    /// run whose report is held must stay visible to it.
    async fn clear_current_run(&self) {
        if !self.state.defer_clear_current_run() {
            self.state.set_current_run(None).await;
        }
    }

    /// Keep the runner reserved while the finished run's cleanup is
    /// unresolved: re-check every [`UNRESOLVED_CLEANUP_RECHECK`] (each
    /// check re-proves ownership before any signal) until nothing is left.
    /// Each re-check that finds the group still ours signals it again
    /// (TERM, grace, KILL), so a run's survivors are re-signalled every
    /// round. Daemon shutdown ends the wait, whenever it lands; the unit's
    /// drop then makes one more ownership-proven attempt. The cleanup
    /// reservation stays set then: the daemon is going away.
    async fn hold_while_unresolved(&mut self) {
        let Some(mut guard) = self.run_end.unresolved.take() else {
            return;
        };
        tracing::warn!(
            runner = %self.runner_config.name,
            "run-end cleanup is unresolved; runner stays reserved until the run's processes are gone"
        );
        let shutdown = self.state.shutdown_notified();
        // `notify_waiters` stores no permit, so one waiter is registered up
        // front and kept across rounds, and the recorded reason covers a
        // shutdown signalled before it was registered.
        let notified = shutdown.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        loop {
            if self.state.shutdown_reason().is_some() {
                return;
            }
            tokio::select! {
                _ = &mut notified => return,
                _ = tokio::time::sleep(UNRESOLVED_CLEANUP_RECHECK) => {}
            }
            if guard.cleanup().await == CleanupOutcome::Clean {
                guard.mark_cleaned();
                self.state.set_cleanup_reserved(false).await;
                tracing::info!(
                    runner = %self.runner_config.name,
                    "run-end cleanup resolved; runner released"
                );
                return;
            }
        }
    }

    async fn handle_assign_inner(
        &mut self,
        run_id: uuid::Uuid,
        prompt: String,
        repo_url: Option<String>,
        expected_codex_model: Option<String>,
        context: AssignContext,
        guard: &mut RunGroup,
    ) -> Result<()> {
        // Resolve the directory the agent runs in: this runner's single,
        // exclusive `workspace.working_dir`. Clone-bootstrap it once if a repo
        // url is provided (`resolve` tolerates a non-git task folder too).
        let workspace_path = {
            let wd = self.runner_config.workspace.working_dir.clone();
            match crate::workspace::resolve(&wd, repo_url.as_deref()).await {
                Ok(crate::workspace::Resolution::ExistingRepo(p))
                | Ok(crate::workspace::Resolution::Cloned(p))
                | Ok(crate::workspace::Resolution::Directory(p)) => p,
                Err(e) => {
                    // Git never fails a run: `workspace::resolve` degrades every
                    // repo-related problem to an ordinary task folder and the
                    // agent decides what to do about git. What is left is a
                    // working dir that cannot be used as a directory at all
                    // (a file in its place, permissions), which is not
                    // recoverable — the agent has nowhere to run.
                    let crate::workspace::ResolveError::Io(_) = &e;
                    self.send(ClientMsg::RunFailed {
                        run_id,
                        reason: FailureReason::WorkspaceSetup,
                        detail: Some(e.to_string()),
                        ended_at: Utc::now(),
                        tokens: None,
                        model: None,
                    })
                    .await;
                    // The supervisor stamped ``rx_in_flight = Some(run_id)``
                    // synchronously when the Assign arrived; clear it now so
                    // a session reconnect after the failure doesn't claim a
                    // run that no longer exists.
                    self.clear_current_run().await;
                    return Ok(());
                }
            }
        };
        // Workpad snapshot (NEWPIDASH-320 §6.1): for issue runs (In
        // Progress, review and test; unified workpad U1) the server attached
        // `workpad_body` + `workpad_sha256` to the assign. Materialise them
        // as files before the agent launches so the prompt never needs a
        // re-fetch. An assign without them (scheduler runs, older servers)
        // drops a previous run's files (the runner reuses one
        // `working_dir` across runs) so they can't leak into this run. A
        // stale or missing file is never fatal — the CLI falls back to
        // `workpad get --to-file`.
        if let Some(body) = context.workpad_body.as_deref() {
            let sha = context.workpad_sha256.as_deref().unwrap_or("");
            if let Err(err) = materialise_workpad_snapshot(&workspace_path, body, sha) {
                tracing::warn!(%run_id, error = %err, "workpad snapshot write failed; continuing without the file");
            }
        } else {
            discard_stale_workpad_snapshot(&workspace_path);
        }
        // The `.pidash/` scratch dir must never be committed, whatever the
        // run carries (reuses the CLI context helper).
        if let Err(err) = crate::cli::context::exclude_pidash_from_git(&workspace_path) {
            tracing::warn!(%run_id, error = %err, ".pidash git exclude failed");
        }
        // Fresh run starts clean: drop any hook left by a previous run
        // (the daemon-start sweep is the only other consumer), then
        // record the workspace so containment cleanup can find a hook
        // this run writes.
        containment::discard_stale_hook(&workspace_path);
        guard.set_workspace(workspace_path.clone());
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

        // No platform-side branch checkout: the cloud supplies the work branch
        // in the prompt context and the agent checks it out (or creates one off
        // the base branch) itself. The platform supplies git information; the
        // agent performs git operations (PDASHOSS01-136).

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
        let logs_dir = self.runner_paths.logs_dir();
        let mut bridge = match AgentBridge::spawn_from_config(
            &self.runner_config,
            &workspace_path,
            expected_codex_model.clone(),
            Some(logs_dir.as_path()),
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
                    tokens: None,
                    model: None,
                })
                .await;
                self.clear_current_run().await;
                return Ok(());
            }
        };
        // Capture the bridge-owned process handle now that the subprocess
        // is spawned. Subscribe to its `exit_rx` so the live-state snapshot
        // flips `agent_subprocess_alive=false` the moment the wait task
        // observes termination — independent of stdout-close detection.
        // See `.ai_design/runner_agent_bridge/design.md` §4.4.
        let process_handle = bridge.process_handle();
        // Move the agent into the run's containment unit BEFORE any
        // await: the agent binary starts seconds after spawn (engine
        // startup), so this prompt move contains everything it starts.
        guard.move_pid(process_handle.pid);
        self.state.set_agent_pid(process_handle.pid).await;
        self.state.set_agent_alive(true).await;
        spawn_exit_watch(self.state.clone(), run_id, process_handle.exit_rx.clone());

        let payload = RunPayload {
            run_id,
            prompt,
            model: expected_codex_model,
        };
        // Free-quota retries: a busy/unavailable free lane backs off and
        // restarts the turn instead of failing the run. Each attempt reuses
        // the bridge (and its thread), so a retry is a new turn with the
        // same prompt. One RunStarted is sent, on the first attempt.
        let mut attempt: u32 = 0;
        let status_label: String = loop {
            // Race the startup handshake against cancel and shutdown: a cold
            // engine may take up to `ENGINE_STARTUP_TIMEOUT` to answer, and a
            // Cancel must not sit unobserved for that long. The `Notified` future
            // is created before the race so a `notify_waiters()` that fires while
            // startup is in flight is not missed.
            let shutdown = self.state.shutdown_notified();
            let shutdown_fut = shutdown.notified();
            let started = tokio::select! {
                biased;
                _ = shutdown_fut => None,
                _ = self.cancel.cancelled() => None,
                r = within_startup_deadline(bridge.run_one_shot(&payload, &workspace_path)) => Some(r),
            };
            let Some(started) = started else {
                // The handshake was abandoned mid-flight, so the bridge is
                // discarded. On shutdown the daemon's drain reports the run as
                // `DaemonRestart`; only an explicit cancel is reported here.
                bridge.shutdown(Duration::from_secs(5)).await.ok();
                let cancelled = self.cancel.is_cancelled();
                hist.append(&HistoryEntry::Footer {
                    ts: Utc::now(),
                    final_status: "cancelled".to_string(),
                    done_payload: None,
                    error: None,
                })
                .await
                .ok();
                self.clear_current_run().await;
                if cancelled {
                    self.send(ClientMsg::RunCancelled {
                        run_id,
                        cancelled_at: Utc::now(),
                        tokens: None,
                        model: None,
                    })
                    .await;
                }
                return Ok(());
            };
            let mut cursor = match started {
                Ok(c) => c,
                Err(e) => {
                    // A handshake that fails with the ACP
                    // authentication-required error (code -32000 — e.g. `agy
                    // acp` answering `session/new` when the cached sign-in
                    // expired or was revoked) is a missing sign-in, not a
                    // crash: report `agent_auth_missing` with the agent's fix
                    // hint, mirroring the turn-time mapping in
                    // `BridgeCursor::translate`.
                    let auth = crate::acp::bridge::HandshakeAuthError::find(&e);
                    let reason = if auth.is_some() {
                        FailureReason::AgentAuthMissing
                    } else {
                        self.crash_reason()
                    };
                    let base = auth.map(|a| a.detail()).unwrap_or_else(|| format!("{e:#}"));
                    let detail = self.build_failure_detail(&base, &bridge).await;
                    hist.append(&HistoryEntry::Footer {
                        ts: Utc::now(),
                        final_status: "failed".to_string(),
                        done_payload: None,
                        error: Some(detail.clone()),
                    })
                    .await
                    .ok();
                    self.send(ClientMsg::RunFailed {
                        run_id,
                        reason,
                        detail: Some(detail),
                        ended_at: Utc::now(),
                        tokens: None,
                        model: None,
                    })
                    .await;
                    bridge.shutdown(Duration::from_secs(5)).await.ok();
                    self.clear_current_run().await;
                    return Ok(());
                }
            };
            if attempt == 0 {
                let run_model = cursor.model().map(ToOwned::to_owned);
                self.state.set_model(run_model.clone()).await;
                self.send(ClientMsg::RunStarted {
                    run_id,
                    thread_id: cursor.thread_id().to_string(),
                    // Alias the session id to the thread id, mirroring the chat
                    // bridge's warm path (`local_session_id: Some(thread_id)`): the
                    // run path's only session handle today is the thread id. `agent_kind`
                    // lets the cloud interpret it per-CLI. Both are recorded durably in
                    // `agent_metadata`, which — unlike `thread_id` — survives a retry.
                    local_session_id: cursor.thread_id().to_string(),
                    agent_kind: cursor.agent_kind().to_string(),
                    started_at: Utc::now(),
                    model: run_model,
                })
                .await;
                hist.append(&HistoryEntry::Lifecycle {
                    ts: Utc::now(),
                    state: "started".to_string(),
                    detail: Some(cursor.thread_id().to_string()),
                })
                .await?;
            }

            // Pump events until terminal (or a free-quota retry).
            match self
                .pump_events(&mut bridge, &mut cursor, &mut hist, &workspace_path)
                .await?
            {
                Outcome::Done { status_label } => break status_label,
                Outcome::Retry { code } => {
                    if attempt >= crate::pidash_agent::free_quota::MAX_RETRIES {
                        // The backoff budget is spent: fail with a clear
                        // message instead of failing on the first busy notice.
                        let detail = crate::pidash_agent::free_quota::retry_exhausted_detail(
                            code,
                            crate::pidash_agent::free_quota::MAX_RETRIES,
                        );
                        let enriched = self.build_failure_detail(&detail, &bridge).await;
                        hist.append(&HistoryEntry::Footer {
                            ts: Utc::now(),
                            final_status: "failed".to_string(),
                            done_payload: None,
                            error: Some(enriched.clone()),
                        })
                        .await
                        .ok();
                        self.send(ClientMsg::RunFailed {
                            run_id,
                            reason: FailureReason::Internal,
                            detail: Some(enriched),
                            ended_at: Utc::now(),
                            tokens: None,
                            model: None,
                        })
                        .await;
                        break "failed".to_string();
                    }
                    attempt += 1;
                    let delay = crate::pidash_agent::free_quota::backoff(attempt);
                    tracing::info!(%run_id, %code, attempt, ?delay, "free-quota backoff: retrying the turn");
                    tokio::select! {
                        _ = self.cancel.cancelled() => break "cancelled".to_string(),
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
            }
        };

        bridge.shutdown(Duration::from_secs(5)).await.ok();

        let summary = RunSummary {
            run_id,
            work_item_id: None,
            status: status_label.clone(),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            title: None,
        };
        let mut idx = RunsIndex::load(&self.runner_paths).unwrap_or_default();
        idx.upsert(summary);
        idx.save(&self.runner_paths).ok();

        if status_label == "cancelled" {
            let terminal_metadata = self.run_metadata().await;
            self.clear_current_run().await;
            self.send(ClientMsg::RunCancelled {
                run_id,
                cancelled_at: Utc::now(),
                tokens: terminal_metadata.tokens,
                model: terminal_metadata.model,
            })
            .await;
            return Ok(());
        }

        self.clear_current_run().await;
        Ok(())
    }

    async fn pump_events(
        &mut self,
        bridge: &mut AgentBridge,
        cursor: &mut AgentCursor,
        hist: &mut HistoryWriter,
        workspace_root: &std::path::Path,
    ) -> Result<Outcome> {
        // Passive-kill activity watchdog (see `daemon::activity_watchdog`).
        // A quiet agent is never killed for being quiet a while: it may be
        // inside a long tool call. Only after `unhealthy_after` with no agent
        // event is the run reported unhealthy, and only after `kill_after` is
        // it failed. Provable deaths (stdout closing, terminal agent errors,
        // turn end) are handled in the event branch below, not here.
        let mut watchdog = ActivityWatchdog::new(
            self.state.agent_watchdog(),
            tokio::time::Instant::now(),
            Utc::now(),
        );
        self.publish_agent_health(&watchdog).await;

        let shutdown = self.state.shutdown_notified();
        let cancel = self.cancel.clone();
        let mut cancelled = false;
        let mut run_events = RunEventMirror::new(cursor.run_id());
        let mut token_meter = crate::daemon::observability::RunTokenMeter::default();
        loop {
            // Re-evaluate at every loop entry. A pending approval is a
            // human-wait, which is legitimate silence: it keeps the activity
            // clock fresh, so the clock restarts when the approval resolves.
            // While one is pending the deadline is only a short re-check, so
            // an approval answered with no agent event after it still re-arms
            // the watchdog instead of leaving the loop without a deadline.
            let awaiting_approval = !self.approvals.list_pending().await.is_empty();
            let watchdog_deadline = if !awaiting_approval {
                watchdog.next_deadline().0
            } else {
                if watchdog.on_activity(tokio::time::Instant::now(), Utc::now()) {
                    self.note_agent_health_change(
                        &watchdog,
                        cursor.run_id(),
                        &mut run_events,
                        hist,
                    )
                    .await;
                }
                tokio::time::Instant::now() + APPROVAL_WATCHDOG_RECHECK
            };
            let run_event_flush_deadline = run_events.flush_deadline();
            tokio::select! {
                biased;
                _ = shutdown.notified(), if !cancelled => {
                    cancelled = true;
                    run_events.flush_before_lifecycle(&self.out).await;
                    bridge.interrupt().await.ok();
                }
                _ = cancel.cancelled(), if !cancelled => {
                    run_events.flush_before_lifecycle(&self.out).await;
                    bridge.interrupt().await.ok();
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
                    return Ok(Outcome::Done {
                        status_label: "cancelled".into(),
                    });
                }
                _ = async {
                    match run_event_flush_deadline {
                        Some(d) => tokio::time::sleep_until(d).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    run_events.flush(&self.out).await;
                }
                events = bridge.next_events(cursor) => {
                    let Some(events) = events else {
                        let reason = self.crash_reason();
                        let detail = self
                            .build_failure_detail("agent stdout closed", bridge)
                            .await;
                        run_events.flush_before_lifecycle(&self.out).await;
                        let metadata = self.run_metadata().await;
                        self.send(ClientMsg::RunFailed {
                            run_id: cursor.run_id(),
                            reason,
                            detail: Some(detail.clone()),
                            ended_at: Utc::now(),
                            tokens: metadata.tokens,
                            model: metadata.model,
                        }).await;
                        hist.append(&HistoryEntry::Footer {
                            ts: Utc::now(),
                            final_status: "failed".into(),
                            done_payload: None,
                            error: Some(detail),
                        }).await?;
                        return Ok(Outcome::Done {
                            status_label: "failed".into(),
                        });
                    };
                    if watchdog.on_activity(tokio::time::Instant::now(), Utc::now()) {
                        self.note_agent_health_change(&watchdog, cursor.run_id(), &mut run_events, hist)
                            .await;
                    } else {
                        self.publish_agent_health(&watchdog).await;
                    }
                    for ev in events {
                        if let Some(out) = self
                            .handle_bridge_event(
                                ev,
                                bridge,
                                hist,
                                workspace_root,
                                &mut run_events,
                                &mut token_meter,
                            )
                            .await?
                        {
                            return Ok(out);
                        }
                    }
                }
                _ = tokio::time::sleep_until(watchdog_deadline) => {
                    if awaiting_approval {
                        continue;
                    }
                    if watchdog.on_deadline() == WatchdogAction::MarkUnhealthy {
                        self.note_agent_health_change(&watchdog, cursor.run_id(), &mut run_events, hist)
                            .await;
                        continue;
                    }
                    let idle = describe_threshold(watchdog.kill_after());
                    let base = format!("agent stalled: no agent events for {idle}");
                    let detail = self.build_failure_detail(&base, bridge).await;
                    run_events.flush_before_lifecycle(&self.out).await;
                    let metadata = self.run_metadata().await;
                    self.send(ClientMsg::RunFailed {
                        run_id: cursor.run_id(),
                        reason: FailureReason::Timeout,
                        detail: Some(detail.clone()),
                        ended_at: Utc::now(),
                        tokens: metadata.tokens,
                        model: metadata.model,
                    }).await;
                    hist.append(&HistoryEntry::Footer {
                        ts: Utc::now(),
                        final_status: "failed".into(),
                        done_payload: None,
                        error: Some(detail),
                    }).await?;
                    return Ok(Outcome::Done {
                        status_label: "failed".into(),
                    });
                }
            }
        }
    }

    /// Stamp the watchdog's view of the run on the daemon state, which the
    /// poll status reports to the cloud.
    async fn publish_agent_health(&self, watchdog: &ActivityWatchdog) {
        self.state
            .set_agent_health(AgentHealthReport {
                last_activity_at: Some(watchdog.last_activity_at()),
                unhealthy: watchdog.is_unhealthy(),
            })
            .await;
    }

    /// Report a healthy ↔ unhealthy transition everywhere an operator looks:
    /// the poll status (cloud health field), the run's event timeline, the
    /// local run history and the daemon log. The run keeps going either way.
    async fn note_agent_health_change(
        &self,
        watchdog: &ActivityWatchdog,
        run_id: uuid::Uuid,
        run_events: &mut RunEventMirror,
        hist: &mut HistoryWriter,
    ) {
        self.publish_agent_health(watchdog).await;
        let last_activity_at = watchdog.last_activity_at();
        let (health, detail) = if watchdog.is_unhealthy() {
            let idle = describe_threshold(watchdog.unhealthy_after());
            tracing::warn!(
                %run_id,
                %last_activity_at,
                "agent run unhealthy: no agent events for {idle}; leaving it running"
            );
            ("unhealthy", format!("no agent events for {idle}"))
        } else {
            tracing::info!(%run_id, "agent run healthy again: agent events resumed");
            ("healthy", "agent events resumed".to_string())
        };
        run_events.push_content(
            "runner/agent_health".to_string(),
            serde_json::json!({
                "schema": "runner_agent_health_v1",
                "health": health,
                "last_activity_at": last_activity_at.to_rfc3339(),
            }),
        );
        run_events.flush(&self.out).await;
        hist.append(&HistoryEntry::Lifecycle {
            ts: Utc::now(),
            state: health.into(),
            detail: Some(detail),
        })
        .await
        .ok();
    }

    /// Build a `RunFailed.detail` string that includes whatever local
    /// context might help the cloud / UI explain what went wrong:
    /// - the supervisor's own classifier message,
    /// - the strongest diagnostic signal we can infer from stderr,
    /// - the most recent shell command tool the agent kicked off as context,
    /// - the last few filtered lines of agent stderr.
    ///
    /// All inputs are optional and the assembly degrades gracefully when
    /// they're missing. The first line is the best root-cause summary we
    /// have; supporting context is listed under `Error trace:`. The total
    /// payload is bounded so a runaway stderr dump can't balloon a
    /// `RunFailed` body.
    async fn build_failure_detail(&self, base: &str, bridge: &AgentBridge) -> String {
        let last_cmd = self.state.observability_snapshot().await.last_exec_command;
        let stderr = bridge.recent_stderr().await;
        assemble_failure_detail(base, last_cmd.as_ref(), &stderr, Utc::now())
    }

    async fn handle_bridge_event(
        &mut self,
        ev: BridgeEvent,
        bridge: &mut AgentBridge,
        hist: &mut HistoryWriter,
        workspace_root: &std::path::Path,
        run_events: &mut RunEventMirror,
        token_meter: &mut crate::daemon::observability::RunTokenMeter,
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
            .note_agent_event(Utc::now(), &kind, Some(&summary))
            .await;
        // When a Raw frame carries the agent's human-readable narrative (the
        // "let me first explore…" prose a terminal user sees), mirror it to
        // the cloud verbatim as its own `agent/message` run event so the
        // AgentRun record reads like a live CLI session. Everything else
        // (lifecycle milestones, high-frequency token deltas) keeps the
        // sanitised summary / compaction path so event volume stays bounded.
        let narrative = if let BridgeEvent::Raw { method, params, .. } = &ev {
            crate::daemon::observability::extract_agent_message_text(method, params)
        } else {
            None
        };
        let should_flush = match narrative {
            Some(text) => run_events.push_content(
                "agent/message".to_string(),
                serde_json::json!({
                    "schema": "runner_agent_message_v1",
                    "text": text,
                }),
            ),
            None => run_events.push(kind, summary),
        };
        if should_flush {
            run_events.flush(&self.out).await;
        }
        if let BridgeEvent::Raw { method, params, .. } = &ev {
            match method.as_str() {
                "thread/tokenUsage/updated" | "codex/event/token_count" => {
                    if let Some(frame) =
                        crate::daemon::observability::parse_token_usage(method, params)
                    {
                        self.state.set_tokens(token_meter.observe(frame)).await;
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
                    // by silently absent command-tool context in
                    // production failure details.
                    if let Some(hint) = crate::daemon::observability::extract_exec_command_hint(
                        method.as_str(),
                        params,
                    ) {
                        // PDASHOSS01-228: agent engines silently scrub inline
                        // `VAR=value cmd` assignments for secret-looking
                        // names, so the command runs against default values
                        // (a migrate against the wrong database, in the
                        // incident that motivated this). The runner only
                        // observes the command — it cannot block it — so make
                        // the failure mode loud: a warning in the runner log
                        // and a non-compactable run event on the AgentRun
                        // record. Names only; the values are secrets.
                        let secret_names =
                            crate::daemon::observability::inline_secret_env_names(&hint.command);
                        if !secret_names.is_empty() {
                            tracing::warn!(
                                vars = ?secret_names,
                                "agent command sets secret-like env vars inline \
                                 (`VAR=value cmd`); agent engines may scrub these \
                                 before the command runs, silently falling back to \
                                 defaults — use `export VAR=…` on its own line"
                            );
                            if run_events.push_content(
                                "runner/env_warning".to_string(),
                                serde_json::json!({
                                    "schema": "runner_env_warning_v1",
                                    "vars": secret_names,
                                    "text": format!(
                                        "Command sets secret-like env vars inline ({}); \
                                         the agent engine may scrub inline assignments \
                                         before the command runs, silently falling back \
                                         to default values. Use `export VAR=…` on its \
                                         own line instead.",
                                        secret_names.join(", ")
                                    ),
                                }),
                            ) {
                                run_events.flush(&self.out).await;
                            }
                        }
                        self.state
                            .note_exec_command(crate::daemon::state::ExecCommandSnapshot {
                                command: hint.command,
                                cwd: hint.cwd,
                                tool_call_id: hint.tool_call_id,
                                started_at: Utc::now(),
                                completed_at: None,
                                completed_success: None,
                            })
                            .await;
                    }
                }
                "item/completed" | "user/toolResult" => {
                    for hint in crate::daemon::observability::extract_exec_command_completion_hints(
                        method.as_str(),
                        params,
                    ) {
                        self.state
                            .note_exec_command_completed(
                                &hint.tool_call_id,
                                hint.success,
                                Utc::now(),
                            )
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
                // The engine files a gateway 402/401 under "stream
                // disconnected" and is about to re-send the whole request
                // five times over. Stop it on the first notice instead of
                // logging it as history and waiting out the retries.
                if let Some((status, text)) =
                    crate::codex::gateway::gateway_will_retry_notice(&method, &params)
                {
                    let mut base: Option<String> = None;
                    match crate::pidash_agent::free_quota::classify(
                        &self.engine_lane(),
                        status,
                        &text,
                    ) {
                        // OpenHub free-lane stop codes: the run fails with
                        // the code's clear reason, keeping resets_at.
                        Some(crate::pidash_agent::free_quota::Disposition::Stop(reason)) => {
                            base = Some(reason);
                        }
                        // Free-lane retry codes: stop the engine's futile
                        // rapid retry, back off, and restart the turn. The
                        // run itself never fails on these notices.
                        Some(crate::pidash_agent::free_quota::Disposition::Retry { code }) => {
                            bridge.interrupt().await.ok();
                            run_events.flush_before_lifecycle(&self.out).await;
                            hist.append(&HistoryEntry::Lifecycle {
                                ts: Utc::now(),
                                state: "retrying".into(),
                                detail: Some(format!(
                                    "free-quota response ({code}): retrying the turn with backoff"
                                )),
                            })
                            .await
                            .ok();
                            return Ok(Some(Outcome::Retry { code }));
                        }
                        None => {
                            if crate::codex::gateway::is_gateway_refusal_status(status) {
                                base = Some(gateway_refusal_detail(status, text));
                            }
                        }
                    }
                    if let Some(base) = base {
                        bridge.interrupt().await.ok();
                        let enriched = self.build_failure_detail(&base, bridge).await;
                        run_events.flush_before_lifecycle(&self.out).await;
                        let metadata = self.run_metadata().await;
                        self.send(ClientMsg::RunFailed {
                            run_id,
                            reason: FailureReason::Internal,
                            detail: Some(enriched.clone()),
                            ended_at: Utc::now(),
                            tokens: metadata.tokens,
                            model: metadata.model,
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
                        return Ok(Some(Outcome::Done {
                            status_label: "failed".into(),
                        }));
                    }
                }
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
                run_events.flush_before_lifecycle(&self.out).await;
                let policy = Policy::new(
                    &self.runner_config.approval_policy,
                    workspace_root,
                    crate::cloud::protocol::ApprovalMode::Ask,
                );
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
                self.state.note_approvals_changed();
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
                    let next = tokio::select! {
                        biased;
                        _ = self.cancel.cancelled() => {
                            bridge.interrupt().await.ok();
                            hist.append(&HistoryEntry::Lifecycle {
                                ts: Utc::now(),
                                state: "cancelled".into(),
                                detail: Some("cancelled while awaiting approval".into()),
                            })
                            .await
                            .ok();
                            return Ok(Some(Outcome::Done {
                                status_label: "cancelled".into(),
                            }));
                        }
                        next = rx.recv() => next,
                    };
                    match next {
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
                            self.state.note_approvals_changed();
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
                self.state
                    .set_run_status(RunnerStatus::AwaitingReauth)
                    .await;
                run_events.flush_before_lifecycle(&self.out).await;
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
                run_events.flush_before_lifecycle(&self.out).await;
                let metadata = self.run_metadata().await;
                self.send(ClientMsg::RunCompleted {
                    run_id,
                    done_payload: done_payload.clone(),
                    ended_at: Utc::now(),
                    tokens: metadata.tokens,
                    model: metadata.model,
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
                Ok(Some(Outcome::Done {
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
                // use so the user sees command-tool context and a
                // stderr tail in the issue activity comment, not just
                // the bridge's classifier text.
                let raw_base = detail
                    .clone()
                    .unwrap_or_else(|| "agent reported failure".to_string());
                // OpenHub free-quota codes in the terminal detail get the
                // same treatment as on the willRetry notices: clear stop
                // reasons, backoff retries. Anything else keeps the
                // existing cleaning.
                let disposition = crate::codex::gateway::gateway_status_text(&raw_base).and_then(
                    |(status, text)| {
                        crate::pidash_agent::free_quota::classify(
                            &self.engine_lane(),
                            status,
                            &text,
                        )
                    },
                );
                let base = match disposition {
                    Some(crate::pidash_agent::free_quota::Disposition::Stop(reason)) => reason,
                    Some(crate::pidash_agent::free_quota::Disposition::Retry { code }) => {
                        run_events.flush_before_lifecycle(&self.out).await;
                        hist.append(&HistoryEntry::Lifecycle {
                            ts: Utc::now(),
                            state: "retrying".into(),
                            detail: Some(format!(
                                "free-quota response ({code}): retrying the turn with backoff"
                            )),
                        })
                        .await
                        .ok();
                        return Ok(Some(Outcome::Retry { code }));
                    }
                    None => clean_gateway_failure_detail(&raw_base),
                };
                let enriched = self.build_failure_detail(&base, bridge).await;
                run_events.flush_before_lifecycle(&self.out).await;
                let metadata = self.run_metadata().await;
                self.send(ClientMsg::RunFailed {
                    run_id,
                    reason,
                    detail: Some(enriched.clone()),
                    ended_at: Utc::now(),
                    tokens: metadata.tokens,
                    model: metadata.model,
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
                Ok(Some(Outcome::Done {
                    status_label: "failed".into(),
                }))
            }
        }
    }

    async fn send(&self, msg: ClientMsg) {
        if let Some(run_id) = terminal_run_id(&msg) {
            let payload = self.persistable_terminal(run_id, &msg);
            if let Some(now) = self.state.hold_terminal_msg(msg) {
                self.deliver(now).await;
            } else if let Some(payload) = payload
                && self.state.holding_first_terminal()
            {
                self.persist_held_terminal(run_id, &payload);
            }
        } else {
            self.deliver(msg).await;
        }
    }

    /// The exact payload of a terminal report for a run the machine queue
    /// owns, or `None` when no outbox can hold it (no coordinator, or a
    /// run the queue does not know).
    fn persistable_terminal(&self, run_id: uuid::Uuid, msg: &ClientMsg) -> Option<Vec<u8>> {
        let coord = self.coordinator.as_ref()?;
        if !coord.holds_run(run_id) {
            return None;
        }
        serde_json::to_vec(msg).ok()
    }

    /// Persist a held terminal report to the queue's terminal outbox (review
    /// of #3499 at 78368f6): while run-end cleanup or its hook runs, the
    /// result lives only in memory, and a daemon that is killed (no
    /// graceful drain) would lose it, so the cloud would later reap a
    /// finished run as failed. The next daemon start replays the outbox
    /// verbatim. Only the first held report is persisted: delivery sends it
    /// first, so a later one (say a history-write failure after an agent
    /// failure) must not replace it. Delivery and the slot release still
    /// wait for cleanup; a
    /// normal release ends with `complete`, which deletes the record.
    fn persist_held_terminal(&self, run_id: uuid::Uuid, payload: &[u8]) {
        let Some(coord) = self.coordinator.as_ref() else {
            return;
        };
        match coord.persist_terminal(run_id, payload, &format!("held-{run_id}")) {
            Ok(true) => {}
            Ok(false) => {
                tracing::warn!(%run_id, "held terminal report: no queue record to persist it in")
            }
            Err(e) => tracing::warn!(%run_id, "failed to persist held terminal report: {e}"),
        }
    }

    /// Whether the outbox holds a terminal record for `run_id` the retry can
    /// deliver (review of #3501 at 591a610). A held result whose write
    /// failed is written now from `payload`; when that fails too, `false`:
    /// nothing would ever deliver the result, so the run must not be kept
    /// reported in flight on its account.
    fn ensure_outbox_record(&self, run_id: uuid::Uuid, payload: Option<&[u8]>) -> bool {
        let Some(coord) = self.coordinator.as_ref() else {
            return false;
        };
        if coord.terminal_pending().iter().any(|e| e.run_id == run_id) {
            return true;
        }
        let Some(payload) = payload else {
            return false;
        };
        match coord.persist_terminal_if_absent(run_id, payload, &format!("held-{run_id}")) {
            Ok(_) => coord.terminal_pending().iter().any(|e| e.run_id == run_id),
            Err(e) => {
                tracing::warn!(%run_id, "undelivered terminal report could not be recorded: {e}");
                false
            }
        }
    }

    /// Send `msg` now, bypassing the terminal hold. `false`: the send failed.
    async fn deliver(&self, msg: ClientMsg) -> bool {
        // Clone the journal-relevant fields BEFORE the send consumes
        // `msg`: an undelivered RunFailed (NEWPIDASH-251) would
        // otherwise be discarded with only a warn log, leaving the
        // run non-terminal cloud-side forever. The clone is cheap
        // (RunFailed carries one short detail string) and only made
        // for that variant.
        let failed_signal: Option<(uuid::Uuid, FailureReason, DateTime<Utc>, Option<String>)> =
            match &msg {
                ClientMsg::RunFailed {
                    run_id,
                    reason,
                    detail,
                    ended_at,
                    ..
                } => Some((*run_id, *reason, *ended_at, detail.clone())),
                _ => None,
            };
        if let Err(err) = self.out.send(msg).await {
            tracing::warn!(error = %err, "failed to send runner lifecycle message");
            // A managed run whose exact result is already in the SQLite
            // outbox is retried from there. A journal copy would be
            // replayed first at startup, without tokens or model, and the
            // cloud keeps the first report it accepts.
            let in_outbox = |run_id: uuid::Uuid| {
                self.coordinator
                    .as_ref()
                    .is_some_and(|c| c.terminal_pending().iter().any(|e| e.run_id == run_id))
            };
            if let Some((run_id, reason, ended_at, detail)) =
                failed_signal.filter(|(run_id, ..)| !in_outbox(*run_id))
            {
                // Journal the signal so the (always-on) replay loop
                // redelivers it until the cloud acknowledges; the
                // server's first-writer-wins finalize makes a late
                // redelivery after a sweep reap a no-op ack.
                let entry = drain_journal::PendingRunFailure {
                    runner_id: self.out.runner_id(),
                    run_id,
                    reason,
                    detail: detail.clone().unwrap_or_default(),
                    ended_at,
                    recorded_at: Utc::now(),
                    trigger: None,
                    previous_exit: None,
                };
                let journal_paths = self.daemon_paths.clone();
                match tokio::task::spawn_blocking(move || {
                    drain_journal::record_all(&journal_paths, std::slice::from_ref(&entry))
                })
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        tracing::warn!("failed to journal undelivered RunFailed: {e:#}")
                    }
                    Err(e) => tracing::warn!("drain journal task panicked: {e:#}"),
                }
            }
            return false;
        }
        true
    }
}

enum Outcome {
    /// The run reached a terminal state (completed / failed / cancelled).
    Done { status_label: String },
    /// A free-quota notice wants the turn retried after a backoff, not a
    /// failure. `code` names it (`free_lane_busy` /
    /// `free_quota_unavailable`) for the exhausted-retries message.
    Retry { code: &'static str },
}

/// Map a cloud `Decide` UUID back to the local approval id it was derived
/// from. Engine-issued handles (e.g. `rpc:<uuid>:<json id>`) are not UUIDs, so
/// the cloud only ever sees `uuid_or(handle)`; without this reverse lookup the
/// decision would be parked as an early decision and never reach the engine.
async fn cloud_approval_key(approvals: &ApprovalRouter, cloud_id: uuid::Uuid) -> String {
    approvals
        .list_pending()
        .await
        .into_iter()
        .map(|rec| rec.approval_id)
        .find(|local| uuid_or(local) == cloud_id)
        .unwrap_or_else(|| cloud_id.to_string())
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
        AgentSection, AntigravitySection, ApprovalPolicySection, ClaudeCodeSection, CodexSection,
        CursorAgentSection, GrokSection, MuseCodeSection, OpenClawSection, RunnerConfig,
        WorkspaceSection,
    };
    use crate::daemon::state::ExecCommandSnapshot;
    use chrono::TimeZone;
    use std::path::PathBuf;
    use tokio::sync::Notify;

    /// NEWPIDASH-320 §6.3, decision D5: `PIDASH_AGENT_NAME` is
    /// `<display name> (<model>)`, with the assign's expected model first,
    /// else the runner default, else the bare display name.
    #[test]
    fn agent_name_prefers_expected_model_then_default() {
        let ctx = AssignContext::default();
        assert_eq!(
            ctx.agent_name(
                AgentKind::ClaudeCode,
                Some("claude-opus-4-8"),
                Some("other")
            ),
            "Claude Code (claude-opus-4-8)"
        );
        assert_eq!(
            ctx.agent_name(AgentKind::Codex, None, Some("gpt-5.3-codex")),
            "Codex (gpt-5.3-codex)"
        );
        assert_eq!(ctx.agent_name(AgentKind::MuseCode, None, None), "Muse Code");
    }

    /// NEWPIDASH-320 §6.1: the workpad snapshot lands as two files under
    /// `.pidash/`, byte-verbatim (the CLI sends the file content back
    /// unchanged, so the guard hash must match exactly).
    #[test]
    fn workpad_snapshot_writes_body_and_hash_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let body = "# Plan\n\n- [ ] 1. Parent task\n";
        materialise_workpad_snapshot(dir.path(), body, "deadbeef").expect("snapshot");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".pidash/workpad.md")).expect("body"),
            body
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".pidash/workpad.sha256")).expect("hash"),
            "deadbeef\n"
        );
    }

    #[test]
    fn workpad_snapshot_empty_body_writes_empty_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        materialise_workpad_snapshot(dir.path(), "", "e3b0c4").expect("snapshot");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".pidash/workpad.md")).expect("body"),
            ""
        );
    }

    /// NEWPIDASH-320 review: an assign without a snapshot (scheduler runs)
    /// must not see a previous run's files: they are removed — and missing files are fine.
    #[test]
    fn stale_workpad_snapshot_is_discarded() {
        let dir = tempfile::tempdir().expect("tempdir");
        materialise_workpad_snapshot(dir.path(), "stale", "deadbeef").expect("snapshot");
        discard_stale_workpad_snapshot(dir.path());
        assert!(!dir.path().join(".pidash/workpad.md").exists());
        assert!(!dir.path().join(".pidash/workpad.sha256").exists());
        // Idempotent: nothing there is not an error.
        discard_stale_workpad_snapshot(dir.path());
    }

    #[tokio::test]
    async fn cloud_decide_resolves_engine_rpc_approval_handle() {
        let approvals = ApprovalRouter::new();
        let handle = format!("rpc:{}:7", uuid::Uuid::new_v4());
        approvals
            .open(ApprovalRecord {
                approval_id: handle.clone(),
                runner_id: uuid::Uuid::new_v4(),
                run_id: uuid::Uuid::new_v4(),
                kind: crate::cloud::protocol::ApprovalKind::CommandExecution,
                payload: serde_json::json!({}),
                reason: None,
                requested_at: Utc::now(),
                expires_at: None,
                status: ApprovalStatus::Pending,
            })
            .await;

        // The cloud only ever saw (and echoes back) the derived v5 UUID.
        let key = cloud_approval_key(&approvals, uuid_or(&handle)).await;
        assert_eq!(key, handle);
        let resolved = approvals
            .decide(
                &key,
                crate::cloud::protocol::ApprovalDecision::Accept,
                DecisionSource::Cloud,
            )
            .await;
        assert!(
            resolved.is_some(),
            "cloud decision must resolve the pending engine approval"
        );

        // Unknown ids fall through unchanged (early-decision buffering).
        let other = uuid::Uuid::new_v4();
        assert_eq!(
            cloud_approval_key(&approvals, other).await,
            other.to_string()
        );
    }

    fn paths_for(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
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
            cursor_agent: CursorAgentSection::default(),
            openclaw: OpenClawSection::default(),
            grok: GrokSection::default(),
            antigravity: AntigravitySection::default(),
            muse_code: MuseCodeSection::default(),
            approval_policy: ApprovalPolicySection::default(),
        }
    }

    fn spawn_ctx_for(paths: &Paths, insts: &[&RunnerInstance]) -> RunnerSpawnCtx {
        RunnerSpawnCtx::for_tests(None, WorkspaceTokens::default(), paths, insts)
    }

    impl RunnerSpawnCtx {
        /// A spawn context over `insts` (already built, not registered
        /// with tasks), for unit tests of this module and of machine
        /// control.
        pub(crate) fn for_tests(
            transport: Option<SharedHttpTransport>,
            tokens: WorkspaceTokens,
            paths: &Paths,
            insts: &[&RunnerInstance],
        ) -> RunnerSpawnCtx {
            RunnerSpawnCtx::for_tests_with_daemon(
                transport,
                tokens,
                paths,
                DaemonConfig::default(),
                insts,
            )
        }

        pub(crate) fn for_tests_with_daemon(
            transport: Option<SharedHttpTransport>,
            tokens: WorkspaceTokens,
            paths: &Paths,
            daemon: DaemonConfig,
            insts: &[&RunnerInstance],
        ) -> RunnerSpawnCtx {
            spawn_ctx_inner(transport, tokens, paths, daemon, insts)
        }
    }

    fn spawn_ctx_inner(
        transport: Option<SharedHttpTransport>,
        tokens: WorkspaceTokens,
        paths: &Paths,
        daemon: DaemonConfig,
        insts: &[&RunnerInstance],
    ) -> RunnerSpawnCtx {
        let engine = crate::pidash_agent::manager::PidashAgentManager::new(
            transport.as_ref().map(|t| t.cloud_url()),
            paths.clone(),
            tokens.clone(),
            None,
        );
        // Test contexts read readiness from their manager, as production
        // does; tests that need the engine override the profile afterwards.
        let engine_weak = std::sync::Arc::downgrade(&engine);
        let local_chat = Arc::new(crate::ipc::pidash_agent_chat::PidashAgentChatProfile::new(
            daemon.dev_machine_id,
            std::sync::Arc::new(move || {
                engine_weak
                    .upgrade()
                    .map(|engine| engine.chat_availability())
                    .unwrap_or(crate::pidash_agent::chat_server::ChatAvailability {
                        can_run: false,
                        code: "offline".to_string(),
                    })
            }),
        ));
        RunnerSpawnCtx {
            engine,
            local_chat,
            transport,

            tokens,
            paths: paths.clone(),
            daemon,
            recovery: RecoveryReservations::default(),
            tasks: Arc::new(std::sync::Mutex::new(HashMap::new())),
            mailboxes: Arc::new(RwLock::new(
                insts
                    .iter()
                    .map(|i| (i.runner_id, i.mailbox_tx.clone()))
                    .collect(),
            )),
            hello_runners: Arc::new(RwLock::new(
                insts
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
                    .collect(),
            )),
            daemon_state: StateHandle::new(crate::config::schema::Config {
                engine: Default::default(),
                auth: Default::default(),
                auth_event: None,
                version: 2,
                daemon: Default::default(),
                runners: vec![],
                cli: None,
            }),
            instances: Arc::new(std::sync::RwLock::new(
                insts.iter().map(|i| (i.runner_id, (*i).clone())).collect(),
            )),
            activation_lock: Arc::new(tokio::sync::Mutex::new(())),
            engine_ops_lock: Arc::new(tokio::sync::Mutex::new(())),
            pending_idle_restarts: Default::default(),
            run_coordinator: None,
            run_queue_blocked: false,
            run_receipt: None,
        }
    }

    /// PDASHOSS01-306: a runner the cloud confirmed deleted is torn down
    /// locally exactly like a RemoveRunner frame — config block and data
    /// dir gone, working dir untouched, loop stopped, not listed by IPC.
    #[tokio::test]
    async fn purge_of_cloud_deleted_runner_removes_it_locally() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        paths.ensure().unwrap();
        let wd_gone = tmp.path().join("wd-gone");
        std::fs::create_dir_all(&wd_gone).unwrap();
        std::fs::write(wd_gone.join("user-file.txt"), "keep me").unwrap();
        let gone_cfg = runner_config("gone", "WEB", wd_gone.clone());
        let keep_cfg = runner_config("keep", "API", tmp.path().join("wd-keep"));
        crate::config::file::write_config(
            &paths,
            &Config {
                engine: Default::default(),
                auth: Default::default(),
                auth_event: None,
                version: 2,
                daemon: DaemonConfig::default(),
                runners: vec![gone_cfg.clone(), keep_cfg.clone()],
                cli: None,
            },
        )
        .unwrap();

        let inst = RunnerInstance::new_offline(gone_cfg, &paths, DaemonConfig::default());
        inst.paths.ensure().unwrap();
        let data_dir = inst.paths.base_dir().to_path_buf();
        std::fs::write(data_dir.join("state.json"), "{}").unwrap();
        assert!(data_dir.exists());

        let ctx = spawn_ctx_for(&paths, &[&inst]);
        let mut remove_rx = inst.remove_tx.subscribe();
        let tasks = spawn_instance_tasks(&inst, &ctx).await;
        let runner_loop = tasks.runner_loop.expect("runner loop spawned");

        purge_cloud_deleted_runner(&inst.mailbox_tx, &inst.name, inst.runner_id, 10).await;

        tokio::time::timeout(Duration::from_secs(5), runner_loop)
            .await
            .expect("runner loop must stop")
            .unwrap();
        assert!(*remove_rx.borrow_and_update(), "teardown latch set");
        let cfg = crate::config::file::load_config(&paths).unwrap();
        let names: Vec<_> = cfg.runners.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["keep"],
            "only the deleted runner's block is stripped"
        );
        assert!(!data_dir.exists(), "data dir deleted");
        assert_eq!(
            std::fs::read_to_string(wd_gone.join("user-file.txt")).unwrap(),
            "keep me",
            "working dir untouched"
        );
        assert!(
            !ctx.instances.read().unwrap().contains_key(&inst.runner_id),
            "no longer listed by `pidash status`"
        );
        assert!(!ctx.mailboxes.read().await.contains_key(&inst.runner_id));
    }

    // ---- PIDESKAPP-122: runner changes without a restart ---------------

    fn write_runners(paths: &Paths, runners: Vec<RunnerConfig>) {
        crate::config::file::write_config(
            paths,
            &Config {
                engine: Default::default(),
                auth: Default::default(),
                auth_event: None,
                version: 2,
                daemon: DaemonConfig::default(),
                runners,
                cli: None,
            },
        )
        .unwrap();
    }

    /// Build `cfgs` as offline instances, register them in a fresh spawn
    /// context and start their tasks, like the supervisor's startup loop.
    async fn running_ctx(paths: &Paths, cfgs: &[RunnerConfig]) -> RunnerSpawnCtx {
        let insts: Vec<RunnerInstance> = cfgs
            .iter()
            .map(|c| {
                let inst = RunnerInstance::new_offline(c.clone(), paths, DaemonConfig::default());
                inst.paths.ensure().unwrap();
                inst
            })
            .collect();
        let ctx = spawn_ctx_for(paths, &insts.iter().collect::<Vec<_>>());
        for inst in &insts {
            let tasks = spawn_instance_tasks(inst, &ctx).await;
            ctx.register_tasks(inst.runner_id, tasks);
        }
        ctx
    }

    fn registered_names(ctx: &RunnerSpawnCtx) -> Vec<String> {
        let mut names: Vec<String> = ctx.all_instances().into_iter().map(|i| i.name).collect();
        names.sort();
        names
    }

    fn loop_running(ctx: &RunnerSpawnCtx, runner_id: uuid::Uuid) -> bool {
        ctx.tasks
            .lock()
            .unwrap()
            .get(&runner_id)
            .and_then(|t| t.runner_loop.as_ref())
            .is_some_and(|h| !h.is_finished())
    }

    /// Design §5 "Removal exists": a local removal leaves nothing in the
    /// registry (so `pidash status` no longer lists the runner), and adding
    /// the same id back afterwards brings it up again instead of being a
    /// silent no-op.
    #[tokio::test]
    async fn local_removal_cleans_the_registry_and_the_same_id_can_be_re_added() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        paths.ensure().unwrap();
        let gone = runner_config("gone", "WEB", tmp.path().join("wd-gone"));
        let keep = runner_config("keep", "API", tmp.path().join("wd-keep"));
        write_runners(&paths, vec![gone.clone(), keep.clone()]);
        let ctx = running_ctx(&paths, &[gone.clone(), keep.clone()]).await;

        // What IPC `RunnerRemoveLocal` does: synthesize RemoveRunner, then
        // wait for the teardown.
        let inst = ctx.instance_by_id(gone.runner_id).unwrap();
        inst.mailbox_tx
            .send(InboundEnvelope {
                stream_id: None,
                env: crate::cloud::protocol::Envelope::for_runner(
                    gone.runner_id,
                    ServerMsg::RemoveRunner {
                        runner_id: gone.runner_id,
                        reason: Some("test".into()),
                    },
                ),
            })
            .await
            .unwrap();
        assert!(
            ctx.wait_runner_removed(gone.runner_id, Duration::from_secs(5))
                .await
        );
        assert_eq!(registered_names(&ctx), vec!["keep"]);
        assert!(!ctx.mailboxes.read().await.contains_key(&gone.runner_id));
        assert!(!ctx.hello_runners.read().await.contains_key(&gone.runner_id));
        let on_disk = crate::config::file::load_config(&paths).unwrap();
        assert_eq!(on_disk.runners.len(), 1, "block stripped before the answer");

        // Re-add the same id: block written again, then RunnerActivateLocal.
        write_runners(&paths, vec![gone.clone(), keep.clone()]);
        ctx.activate_configured("gone").await.unwrap();
        assert_eq!(registered_names(&ctx), vec!["gone", "keep"]);
        assert!(loop_running(&ctx, gone.runner_id), "re-added runner runs");
        assert!(loop_running(&ctx, keep.runner_id), "other runner untouched");
    }

    #[tokio::test]
    async fn activating_an_unchanged_running_runner_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        paths.ensure().unwrap();
        let main = runner_config("main", "WEB", tmp.path().join("wd-main"));
        write_runners(&paths, vec![main.clone()]);
        let ctx = running_ctx(&paths, std::slice::from_ref(&main)).await;
        let before = ctx.instance_by_id(main.runner_id).unwrap();
        ctx.activate_configured("main").await.unwrap();
        let after = ctx.instance_by_id(main.runner_id).unwrap();
        assert!(
            Arc::ptr_eq(&before.chat_active, &after.chat_active),
            "same instance: not restarted"
        );
    }

    #[tokio::test]
    async fn reconfigure_moves_one_runner_to_a_new_working_dir_in_process() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        paths.ensure().unwrap();
        let main = runner_config("main", "WEB", tmp.path().join("wd-main"));
        let other = runner_config("other", "API", tmp.path().join("wd-other"));
        write_runners(&paths, vec![main.clone(), other.clone()]);
        let ctx = running_ctx(&paths, &[main.clone(), other.clone()]).await;
        let other_before = ctx.instance_by_id(other.runner_id).unwrap();

        let new_dir = tmp.path().join("moved").join("main");
        let block = ctx
            .reconfigure_runner(
                "main",
                RunnerChanges {
                    working_dir: Some(new_dir.clone()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(block.workspace.working_dir, new_dir);
        assert!(new_dir.is_dir(), "missing working dir is created");
        let on_disk = crate::config::file::load_config(&paths).unwrap();
        let main_on_disk = on_disk.runners.iter().find(|r| r.name == "main").unwrap();
        assert_eq!(main_on_disk.workspace.working_dir, new_dir);
        let running = ctx.instance_by_id(main.runner_id).unwrap();
        assert_eq!(running.config.workspace.working_dir, new_dir);
        assert!(loop_running(&ctx, main.runner_id), "restarted runner runs");

        let other_after = ctx.instance_by_id(other.runner_id).unwrap();
        assert!(
            Arc::ptr_eq(&other_before.chat_active, &other_after.chat_active),
            "the other runner was not restarted"
        );
        assert!(loop_running(&ctx, other.runner_id));
    }

    fn changes_cfg(tmp: &std::path::Path, kind: AgentKind) -> (Config, uuid::Uuid) {
        let paths = paths_for(tmp);
        paths.ensure().unwrap();
        let mut main = runner_config("main", "WEB", tmp.join("wd-main"));
        main.agent.kind = kind;
        let id = main.runner_id;
        write_runners(
            &paths,
            vec![main, runner_config("other", "API", tmp.join("wd-other"))],
        );
        (crate::config::file::load_config(&paths).unwrap(), id)
    }

    fn block(cfg: &Config, id: uuid::Uuid) -> &RunnerConfig {
        cfg.runners.iter().find(|r| r.runner_id == id).unwrap()
    }

    #[test]
    fn apply_changes_sets_and_clears_the_codex_model_and_effort() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut cfg, id) = changes_cfg(tmp.path(), AgentKind::Codex);
        apply_runner_changes(
            &mut cfg,
            id,
            &RunnerChanges {
                model: Some("gpt-5.5".into()),
                reasoning_effort: Some("HIGH".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(block(&cfg, id).configured_model(), Some("gpt-5.5"));
        assert_eq!(block(&cfg, id).configured_reasoning_effort(), Some("high"));

        // Back to the agent default drops the effort with it.
        apply_runner_changes(
            &mut cfg,
            id,
            &RunnerChanges {
                model: Some(String::new()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(block(&cfg, id).configured_model(), None);
        assert_eq!(block(&cfg, id).configured_reasoning_effort(), None);
    }

    #[test]
    fn apply_changes_sets_and_clears_the_muse_code_effort() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut cfg, id) = changes_cfg(tmp.path(), AgentKind::MuseCode);
        // Like Codex, an effort needs a model.
        let err = apply_runner_changes(
            &mut cfg,
            id,
            &RunnerChanges {
                reasoning_effort: Some("max".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("needs a model"), "{err}");

        apply_runner_changes(
            &mut cfg,
            id,
            &RunnerChanges {
                model: Some("muse-spark".into()),
                reasoning_effort: Some("Max".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(block(&cfg, id).configured_reasoning_effort(), Some("max"));

        apply_runner_changes(
            &mut cfg,
            id,
            &RunnerChanges {
                reasoning_effort: Some(String::new()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(block(&cfg, id).configured_reasoning_effort(), None);

        // Back to the agent default drops the effort with it.
        apply_runner_changes(
            &mut cfg,
            id,
            &RunnerChanges {
                reasoning_effort: Some("ultra".into()),
                ..Default::default()
            },
        )
        .unwrap();
        apply_runner_changes(
            &mut cfg,
            id,
            &RunnerChanges {
                model: Some(String::new()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(block(&cfg, id).configured_reasoning_effort(), None);

        apply_runner_changes(
            &mut cfg,
            id,
            &RunnerChanges {
                reasoning_effort: Some(String::new()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(block(&cfg, id).configured_reasoning_effort(), None);

        let err = apply_runner_changes(
            &mut cfg,
            id,
            &RunnerChanges {
                reasoning_effort: Some("turbo".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("is not one of"), "{err}");
    }

    #[test]
    fn apply_changes_switches_agent_and_applies_the_model_to_the_new_one() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut cfg, id) = changes_cfg(tmp.path(), AgentKind::Codex);
        apply_runner_changes(
            &mut cfg,
            id,
            &RunnerChanges {
                agent: Some("claude-code".into()),
                model: Some("claude-opus-5-5".into()),
                name: Some("renamed".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let b = block(&cfg, id);
        assert_eq!(b.agent.kind, AgentKind::ClaudeCode);
        assert_eq!(
            b.claude_code.model_default.as_deref(),
            Some("claude-opus-5-5")
        );
        assert_eq!(b.name, "renamed");
    }

    #[test]
    fn apply_changes_sets_the_claude_code_effort_and_refuses_it_elsewhere() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut cfg, id) = changes_cfg(tmp.path(), AgentKind::ClaudeCode);
        apply_runner_changes(
            &mut cfg,
            id,
            &RunnerChanges {
                model: Some("claude-opus-4-8".into()),
                reasoning_effort: Some("MAX".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(block(&cfg, id).configured_reasoning_effort(), Some("max"));
        // Claude Code has no `minimal` level.
        let err = apply_runner_changes(
            &mut cfg,
            id,
            &RunnerChanges {
                reasoning_effort: Some("minimal".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("is not one of"), "{err}");

        let (mut cfg, id) = changes_cfg(tmp.path(), AgentKind::Grok);
        let err = apply_runner_changes(
            &mut cfg,
            id,
            &RunnerChanges {
                model: Some("grok-4.3".into()),
                reasoning_effort: Some("high".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("only applies to the Codex, Claude Code and Muse Code agents"),
            "{err}"
        );
    }

    #[test]
    fn apply_changes_refuses_mismatched_and_engine_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut cfg, id) = changes_cfg(tmp.path(), AgentKind::ClaudeCode);
        let refused = |cfg: &mut Config, changes: RunnerChanges| {
            format!("{:#}", apply_runner_changes(cfg, id, &changes).unwrap_err())
        };
        let err = refused(
            &mut cfg,
            RunnerChanges {
                model: Some("gpt-5.5".into()),
                ..Default::default()
            },
        );
        assert!(err.contains("does not apply"), "{err}");
        let err = refused(
            &mut cfg,
            RunnerChanges {
                reasoning_effort: Some("high".into()),
                ..Default::default()
            },
        );
        // Claude Code takes an effort, but only alongside a model.
        assert!(err.contains("needs a model"), "{err}");
        let err = refused(
            &mut cfg,
            RunnerChanges {
                agent: Some("engine".into()),
                ..Default::default()
            },
        );
        assert!(err.contains("Pi Dash Agent"), "{err}");
        let err = refused(
            &mut cfg,
            RunnerChanges {
                agent: Some("emacs".into()),
                ..Default::default()
            },
        );
        assert!(err.contains("unknown agent kind"), "{err}");

        let tmp = tempfile::tempdir().unwrap();
        let (mut engine, engine_id) = changes_cfg(tmp.path(), AgentKind::PidashAgent);
        let err = format!(
            "{:#}",
            apply_runner_changes(
                &mut engine,
                engine_id,
                &RunnerChanges {
                    model: Some("m".into()),
                    ..Default::default()
                },
            )
            .unwrap_err()
        );
        assert!(err.contains("comes from Pi Dash"), "{err}");
    }

    #[tokio::test]
    async fn reconfigure_by_id_renames_and_changes_the_model_in_process() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        paths.ensure().unwrap();
        let mut main = runner_config("main", "WEB", tmp.path().join("wd-main"));
        main.agent.kind = AgentKind::Codex;
        write_runners(&paths, vec![main.clone()]);
        let ctx = running_ctx(&paths, std::slice::from_ref(&main)).await;

        let block = ctx
            .reconfigure_runner_by_id(
                main.runner_id,
                RunnerChanges {
                    name: Some("main-renamed".into()),
                    model: Some("gpt-5.5".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(block.name, "main-renamed");
        assert_eq!(block.configured_model(), Some("gpt-5.5"));
        let running = ctx.instance_by_id(main.runner_id).unwrap();
        assert_eq!(running.name, "main-renamed");
        assert_eq!(running.config.configured_model(), Some("gpt-5.5"));
        assert!(loop_running(&ctx, main.runner_id));
        let on_disk = crate::config::file::load_config(&paths).unwrap();
        assert_eq!(on_disk.runners[0].name, "main-renamed");

        let err = ctx
            .reconfigure_runner_by_id(
                uuid::Uuid::new_v4(),
                RunnerChanges {
                    model: Some("gpt-5.5".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("not running on this machine"));
    }

    #[tokio::test]
    async fn reconfigure_is_refused_mid_run_and_for_a_colliding_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        paths.ensure().unwrap();
        let main = runner_config("main", "WEB", tmp.path().join("wd-main"));
        let other = runner_config("other", "API", tmp.path().join("wd-other"));
        write_runners(&paths, vec![main.clone(), other.clone()]);
        let ctx = running_ctx(&paths, &[main.clone(), other.clone()]).await;

        // Nested inside another runner's dir: rejected before anything stops.
        let before = ctx.instance_by_id(main.runner_id).unwrap();
        let err = ctx
            .reconfigure_runner(
                "main",
                RunnerChanges {
                    working_dir: Some(tmp.path().join("wd-other").join("nested")),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("invalid change"), "{err:#}");
        let after = ctx.instance_by_id(main.runner_id).unwrap();
        assert!(Arc::ptr_eq(&before.chat_active, &after.chat_active));
        assert!(loop_running(&ctx, main.runner_id));

        // Mid-run: refused, nothing written.
        let run_id = uuid::Uuid::new_v4();
        before
            .state
            .set_current_run(Some(CurrentRunSummary {
                run_id,
                thread_id: None,
                status: "running".into(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;
        let err = ctx
            .reconfigure_runner(
                "main",
                RunnerChanges {
                    working_dir: Some(tmp.path().join("elsewhere")),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("mid-run"), "{err:#}");
        let on_disk = crate::config::file::load_config(&paths).unwrap();
        assert_eq!(
            on_disk.runners[0].workspace.working_dir,
            main.workspace.working_dir
        );
        assert!(loop_running(&ctx, main.runner_id));
    }

    // ---- PIDESKAPP-122: one daemon, several workspaces -----------------

    fn runner_in(name: &str, workspace: &str, working_dir: PathBuf) -> RunnerConfig {
        let mut cfg = runner_config(name, name, working_dir);
        cfg.workspace_slug = Some(workspace.into());
        cfg
    }

    fn two_workspace_tokens() -> WorkspaceTokens {
        WorkspaceTokens::from_cli(Some(&crate::config::schema::CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            workspaces: vec![crate::config::schema::WorkspaceCredential {
                slug: "beta".into(),
                token: "mt_beta".into(),
            }],
            ..Default::default()
        }))
    }

    #[tokio::test]
    async fn each_runner_authenticates_with_its_workspace_token() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        paths.ensure().unwrap();
        let transport = SharedHttpTransport::new("http://127.0.0.1:1".into()).unwrap();
        let tokens = two_workspace_tokens();
        let daemon = DaemonConfig::default();

        let a = build_runner_instance(
            runner_in("a", "acme", tmp.path().join("wa")),
            Some(&transport),
            &tokens,
            &paths,
            &daemon,
        )
        .await
        .unwrap();
        let b = build_runner_instance(
            runner_in("b", "beta", tmp.path().join("wb")),
            Some(&transport),
            &tokens,
            &paths,
            &daemon,
        )
        .await
        .unwrap();
        let c = build_runner_instance(
            runner_in("c", "gamma", tmp.path().join("wc")),
            Some(&transport),
            &tokens,
            &paths,
            &daemon,
        )
        .await
        .expect("a workspace with no token does not fail the daemon");

        assert_eq!(a.machine_token.as_deref(), Some("mt_acme"));
        assert_eq!(b.machine_token.as_deref(), Some("mt_beta"));
        assert!(c.machine_token.is_none());
        assert!(c.workspace_signed_out && c.client.is_none());

        let ctx = RunnerSpawnCtx::for_tests(Some(transport), tokens.clone(), &paths, &[&a, &b, &c]);
        assert!(!ctx.is_workspace_signed_out(&a));
        assert!(!ctx.is_workspace_signed_out(&b));
        assert!(
            ctx.is_workspace_signed_out(&c),
            "only gamma's runner signed out"
        );

        // The cloud revokes acme's token: only acme's runner is signed out.
        tokens.mark_revoked("mt_acme");
        assert!(ctx.is_workspace_signed_out(&a));
        assert!(!ctx.is_workspace_signed_out(&b));
    }

    /// PIDESKAPP-122 review: a login while a runner is mid-run rotates the
    /// workspace token; the runner is restarted with the new one as soon as
    /// it is idle instead of keeping the revoked token for good. A
    /// sign-out (no token left) takes the runner offline the same way.
    #[tokio::test]
    async fn busy_runner_picks_up_new_credentials_once_idle_and_sign_out_takes_it_offline() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        paths.ensure().unwrap();
        let transport = SharedHttpTransport::new("http://127.0.0.1:1".into()).unwrap();
        let tokens = two_workspace_tokens();
        let a = build_runner_instance(
            runner_in("a", "acme", tmp.path().join("wa")),
            Some(&transport),
            &tokens,
            &paths,
            &DaemonConfig::default(),
        )
        .await
        .unwrap();
        let runner_id = a.runner_id;
        let ctx = RunnerSpawnCtx::for_tests(Some(transport), tokens.clone(), &paths, &[&a]);

        a.state
            .set_current_run(Some(CurrentRunSummary {
                run_id: uuid::Uuid::new_v4(),
                thread_id: None,
                status: "running".into(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;
        let mut relogin = crate::config::schema::CliSection {
            token: Some("mt_acme_2".into()),
            workspace_slug: Some("acme".into()),
            ..Default::default()
        };
        relogin.set_workspace_token("beta", "mt_beta", false);
        tokens.reload(Some(&relogin));
        assert_eq!(
            ctx.restart_stale_runners().await,
            0,
            "busy: not interrupted"
        );
        assert_eq!(
            ctx.instance_by_id(runner_id)
                .unwrap()
                .machine_token
                .as_deref(),
            Some("mt_acme")
        );

        a.state.set_current_run(None).await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let now = ctx.instance_by_id(runner_id).and_then(|i| i.machine_token);
            if now.as_deref() == Some("mt_acme_2") {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "runner was not restarted once idle"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Local-only sign-out of acme: the runner comes back up offline.
        let mut signed_out = crate::config::schema::CliSection::default();
        signed_out.set_workspace_token("beta", "mt_beta", false);
        tokens.reload(Some(&signed_out));
        assert_eq!(ctx.restart_stale_runners().await, 1);
        let after = ctx.instance_by_id(runner_id).unwrap();
        assert!(after.machine_token.is_none());
        assert!(after.workspace_signed_out && after.client.is_none());
    }

    #[tokio::test]
    async fn hello_reports_version_account_workspaces_machine_and_engine() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let mut cli = crate::config::schema::CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            account_email: Some("dev@example.com".into()),
            account_user_id: Some("u-1".into()),
            ..Default::default()
        };
        cli.set_workspace_token("beta", "mt_beta", false);
        let tokens = WorkspaceTokens::from_cli(Some(&cli));
        tokens.mark_revoked("mt_beta");
        let a = RunnerInstance::new_offline(
            runner_in("a", "acme", tmp.path().join("wa")),
            &paths,
            DaemonConfig::default(),
        );
        let g = RunnerInstance::new_offline(
            runner_in("g", "gamma", tmp.path().join("wg")),
            &paths,
            DaemonConfig::default(),
        );
        let machine_id = uuid::Uuid::new_v4();
        let ctx = RunnerSpawnCtx::for_tests_with_daemon(
            None,
            tokens,
            &paths,
            DaemonConfig {
                cloud_url: "https://cloud.example".into(),
                dev_machine_id: Some(machine_id),
                ..DaemonConfig::default()
            },
            &[&a, &g],
        );

        let hello = ctx.hello();
        assert_eq!(hello.daemon_version, crate::RUNNER_VERSION);
        assert_eq!(hello.ipc_version, IPC_VERSION);
        assert_eq!(hello.machine_id, Some(machine_id));
        assert_eq!(hello.engine, crate::ipc::protocol::EngineState::Unavailable);
        assert_eq!(
            hello.account,
            Some(HelloAccount {
                user_id: Some("u-1".into()),
                email: Some("dev@example.com".into()),
                cloud_url: "https://cloud.example".into(),
            })
        );
        assert_eq!(
            hello.workspaces,
            vec![
                HelloWorkspace {
                    slug: "acme".into(),
                    signed_in: true,
                    runners: 1
                },
                HelloWorkspace {
                    slug: "beta".into(),
                    signed_in: false,
                    runners: 0
                },
                HelloWorkspace {
                    slug: "gamma".into(),
                    signed_in: false,
                    runners: 1
                },
            ]
        );

        let wire = serde_json::to_value(crate::ipc::protocol::Response::Hello(hello)).unwrap();
        assert_eq!(wire["result"], "hello");
        assert_eq!(wire["data"]["engine"], "unavailable");
        assert_eq!(wire["data"]["ipc_version"], IPC_VERSION);
        assert_eq!(wire["data"]["account"]["email"], "dev@example.com");
        assert_eq!(wire["data"]["workspaces"][0]["slug"], "acme");
    }

    #[test]
    fn data_dir_removal_spares_a_working_dir_inside_it() {
        let tmp = tempfile::tempdir().unwrap();
        let runner_dir = tmp.path().join("runners").join("r1");
        let inside = runner_dir.join("checkout");
        std::fs::create_dir_all(&inside).unwrap();
        remove_runner_data_dir(&runner_dir, &inside);
        assert!(inside.exists(), "working dir inside the data dir is kept");

        let outside = tmp.path().join("workspaces").join("w1");
        std::fs::create_dir_all(&outside).unwrap();
        remove_runner_data_dir(&runner_dir, &outside);
        assert!(!runner_dir.exists());
        assert!(outside.exists());
    }

    #[tokio::test]
    async fn kick_reconnect_targets_one_runner_or_all() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let a = RunnerInstance::new_offline(
            runner_config("a", "WEB", tmp.path().join("wa")),
            &paths,
            DaemonConfig::default(),
        );
        let b = RunnerInstance::new_offline(
            runner_config("b", "API", tmp.path().join("wb")),
            &paths,
            DaemonConfig::default(),
        );
        let ctx = spawn_ctx_for(&paths, &[&a, &b]);
        async fn fired(inst: &RunnerInstance) -> bool {
            let kick = inst.state.reconnect_notified();
            tokio::time::timeout(Duration::from_millis(50), kick.notified())
                .await
                .is_ok()
        }

        assert!(ctx.kick_reconnect(Some("nope")).is_err());
        assert_eq!(ctx.kick_reconnect(Some("a")).unwrap(), 1);
        assert!(fired(&a).await, "named runner kicked");
        assert!(!fired(&b).await, "other runner left alone");

        assert_eq!(ctx.kick_reconnect(None).unwrap(), 2);
        assert!(fired(&a).await, "all runners kicked");
        assert!(fired(&b).await, "all runners kicked");
    }

    #[test]
    fn resolve_working_dir_absolutizes_runner_path() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let relative = PathBuf::from(format!("relative-wd-{}", uuid::Uuid::new_v4()));
        let expected = std::path::absolute(&relative).unwrap();
        let (out_tx, _out_rx) = mpsc::channel::<Envelope<ClientMsg>>(1);
        let inst = RunnerInstance::new(runner_config("main", "WEB", relative), &paths, out_tx);

        assert_eq!(resolve_working_dir(&inst), Some(expected));
    }

    /// An `AssignWorker` for a Codex run in `wd` whose agent binary is
    /// `binary`, reporting on `tx`. No cloud context, no coordinator.
    #[cfg(unix)]
    fn codex_worker(
        tmp: &std::path::Path,
        wd: std::path::PathBuf,
        binary: &std::path::Path,
        tx: mpsc::Sender<Envelope<ClientMsg>>,
    ) -> AssignWorker {
        let paths = paths_for(tmp);
        paths.ensure().unwrap();
        let mut config = runner_config("general-tasks", "TEST", wd);
        config.project_slug = None;
        config.agent.kind = AgentKind::Codex;
        config.codex.binary = binary.to_string_lossy().into_owned();
        let runner_paths = paths.for_runner(config.runner_id);
        runner_paths.ensure().unwrap();
        let engine_paths = paths.clone();
        AssignWorker {
            runner_paths,
            daemon_paths: paths,
            runner_config: config.clone(),
            engine: crate::pidash_agent::manager::PidashAgentManager::new(
                None,
                engine_paths,
                WorkspaceTokens::default(),
                None,
            ),
            state: StateHandle::new(Config {
                engine: Default::default(),
                auth: Default::default(),
                auth_event: None,
                version: 2,
                daemon: Default::default(),
                runners: vec![config.clone()],
                cli: None,
            }),
            approvals: ApprovalRouter::new(),
            out: RunnerOut::new(config.runner_id, tx),
            cancel: tokio_util::sync::CancellationToken::new(),
            coordinator: None,
            queue_blocked: false,
            run_end: RunEnd::default(),
        }
    }

    /// An idle cloud chat worker over `unit`, with its command channel.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn idle_chat_worker(
        unit: ChatUnit,
    ) -> (ChatWorker, mpsc::Sender<ChatCommand>, watch::Receiver<bool>) {
        let config = runner_config("main", "WEB", PathBuf::from("/nonexistent"));
        let state = StateHandle::new(Config {
            engine: Default::default(),
            auth: Default::default(),
            auth_event: None,
            version: 2,
            daemon: Default::default(),
            runners: vec![config.clone()],
            cli: None,
        });
        idle_chat_worker_on(unit, state)
    }

    /// [`idle_chat_worker`] sharing `state` (a runner instance's).
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn idle_chat_worker_on(
        unit: ChatUnit,
        state: StateHandle,
    ) -> (ChatWorker, mpsc::Sender<ChatCommand>, watch::Receiver<bool>) {
        let config = runner_config("main", "WEB", PathBuf::from("/nonexistent"));
        let (out_tx, _out_rx) = mpsc::channel::<Envelope<ClientMsg>>(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(8);
        let (active_tx, active_rx) = watch::channel(false);
        let worker = ChatWorker {
            runner_config: config.clone(),
            state,
            approvals: ApprovalRouter::new(),
            out: RunnerOut::new(config.runner_id, out_tx),
            command_rx: cmd_rx,
            active_tx,
            guidance_sent: false,
            unit: Some(unit),
        };
        (worker, cmd_tx, active_rx)
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn process_alive(pid: u32) -> bool {
        crate::util::procinfo::info(pid as i32).is_some_and(|p| !p.zombie)
    }

    /// PDASHOSS01-417 (#2952 review item 3): handing the working copy over
    /// from an idle cloud chat runtime stops what its agent left running
    /// there before anything else may start in it.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn chat_handoff_stops_what_the_idle_runtime_left_running() {
        let id = uuid::Uuid::new_v4();
        let mut unit = ChatUnit::new(id);
        // A background process the chat agent started in the working copy.
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
        let reaper = tokio::spawn(async move { leftover.wait().await });
        let (worker, tx, active_rx) = idle_chat_worker(unit);
        let mut current = Some(spawn_chat_runtime(worker, id, tx, active_rx));
        let mut stopping = None;
        assert!(release_chat_dir(&mut current, &mut stopping).await);
        assert!(current.is_none() && stopping.is_none());
        let _ = tokio::time::timeout(Duration::from_secs(5), reaper).await;
        assert!(
            !process_alive(pid),
            "the chat agent's process outlived the handoff"
        );
    }

    /// While a cloud chat runtime's agents are being stopped (its first
    /// cleanup pass, up to the TERM grace), local chat must not take the
    /// working copy: the copy is reserved before teardown begins.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn local_chat_is_refused_while_a_cloud_chat_runtime_is_stopping() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, _out_rx) = mpsc::channel::<Envelope<ClientMsg>>(1);
        let inst = RunnerInstance::new(
            runner_config("main", "WEB", tmp.path().join("wd")),
            &paths,
            out_tx,
        );
        let id = uuid::Uuid::new_v4();
        let mut unit = ChatUnit::new(id);
        // Ignores TERM, so the unit's first pass lasts the whole grace.
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
        let reaper = tokio::spawn(async move { leftover.wait().await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        let (worker, tx, active_rx) = idle_chat_worker_on(unit, inst.state.clone());
        let mut current = Some(spawn_chat_runtime(worker, id, tx, active_rx));
        let mut stopping = None;
        let (handed_over, busy_while_stopping) =
            tokio::join!(release_chat_dir(&mut current, &mut stopping), async {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let alive = process_alive(pid);
                (
                    crate::ipc::chat::try_begin_local_turn(&inst).is_err(),
                    alive,
                )
            });
        let _ = tokio::time::timeout(Duration::from_secs(5), reaper).await;
        assert!(busy_while_stopping.1, "the agent was gone before the check");
        assert!(
            busy_while_stopping.0,
            "local chat claimed the copy while the chat runtime's agents were stopping"
        );
        assert!(handed_over);
        assert!(!process_alive(pid));
        let _turn = crate::ipc::chat::try_begin_local_turn(&inst).expect("free once stopped");
    }

    /// PDASHOSS01-417: a chat runtime whose processes cannot be proven gone
    /// keeps the working copy (and the runner) reserved, signals nothing it
    /// cannot prove its own, and frees both once they are gone.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test(start_paused = true)]
    async fn chat_handoff_waits_while_the_runtimes_processes_are_unresolved() {
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
        let unit = ChatUnit::from_group(RunGroup::claimed_for_test(&recorded));
        let id = uuid::Uuid::new_v4();
        let (worker, tx, active_rx) = idle_chat_worker(unit);
        let state = worker.state.clone();
        let mut current = Some(spawn_chat_runtime(worker, id, tx, active_rx));
        let mut stopping = None;
        let handed_over = release_chat_dir(&mut current, &mut stopping).await;
        let reserved = state.cleanup_reserved();
        let again = release_chat_dir(&mut current, &mut stopping).await;
        let stranger_alive = process_alive(pid);
        let _ = stranger.kill();
        let _ = stranger.wait();
        assert!(!handed_over, "working copy handed over while unresolved");
        assert!(reserved, "runner not reserved while unresolved");
        assert!(!again, "a later handoff ignored the unresolved runtime");
        assert!(stranger_alive, "an unproven group was signalled");
        // Once the processes are gone, the next re-check frees both.
        tokio::time::sleep(UNRESOLVED_CLEANUP_RECHECK * 3).await;
        assert!(release_chat_dir(&mut current, &mut stopping).await);
        assert!(
            !state.cleanup_reserved(),
            "reservation outlived the processes"
        );
    }

    /// PDASHOSS01-417 part 2 (review item 2 on #2952): when run-end cleanup
    /// is unresolved, the worker does not finish, so the runner loop keeps
    /// the run in flight and turns away new work. The group it cannot prove
    /// is never signalled, and the runner is released once it is gone.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test(start_paused = true)]
    async fn unresolved_cleanup_keeps_the_runner_reserved_until_resolved() {
        use std::os::unix::process::CommandExt;
        let tmp = tempfile::tempdir().unwrap();
        let wd = tmp.path().join("wd");
        std::fs::create_dir_all(&wd).unwrap();
        // A stranger now holds the run's recorded pgid: same pid, other
        // start time, so its ownership can never be proven.
        let mut stranger = std::process::Command::new("sleep")
            .arg("300")
            .process_group(0)
            .spawn()
            .unwrap();
        let info = crate::util::procinfo::info(stranger.id() as i32).unwrap();
        let recorded = crate::util::procinfo::ProcInfo {
            start: info.start - 1,
            ..info
        };
        let (tx, _rx) = mpsc::channel(16);
        let mut worker = codex_worker(tmp.path(), wd, &tmp.path().join("unused"), tx);
        let state = worker.state.clone();
        worker
            .reserve_unresolved(RunGroup::claimed_for_test(&recorded))
            .await;
        let held = tokio::spawn(async move { worker.hold_while_unresolved().await });
        tokio::time::sleep(UNRESOLVED_CLEANUP_RECHECK * 4).await;
        let reserved = !held.is_finished();
        // Review item 1 on #3499: local chat sees the reservation too.
        let chat_blocked = state.cleanup_reserved();
        let survived = stranger.try_wait().unwrap().is_none();
        let _ = stranger.kill();
        let _ = stranger.wait();
        assert!(reserved, "runner was released while cleanup was unresolved");
        assert!(
            chat_blocked,
            "local chat could claim the working copy during cleanup"
        );
        assert!(survived, "an unproven process group was signalled");
        tokio::time::timeout(UNRESOLVED_CLEANUP_RECHECK * 3, held)
            .await
            .expect("runner stayed reserved after the group was gone")
            .unwrap();
        assert!(!state.cleanup_reserved(), "reservation outlived cleanup");
    }

    /// The hold ends on daemon shutdown even when the shutdown lands while
    /// a re-check is running, not only during the sleep between checks.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test(start_paused = true)]
    async fn unresolved_hold_ends_on_a_shutdown_signalled_during_a_recheck() {
        use std::os::unix::process::CommandExt;
        let tmp = tempfile::tempdir().unwrap();
        let wd = tmp.path().join("wd");
        std::fs::create_dir_all(&wd).unwrap();
        let mut stranger = std::process::Command::new("sleep")
            .arg("300")
            .process_group(0)
            .spawn()
            .unwrap();
        let info = crate::util::procinfo::info(stranger.id() as i32).unwrap();
        let recorded = crate::util::procinfo::ProcInfo {
            start: info.start - 1,
            ..info
        };
        let (tx, _rx) = mpsc::channel(16);
        let mut worker = codex_worker(tmp.path(), wd, &tmp.path().join("unused"), tx);
        let state = worker.state.clone();
        worker
            .reserve_unresolved(RunGroup::claimed_for_test(&recorded))
            .await;
        // Signalled before the hold starts waiting: no waiter is registered,
        // just as when it lands during a re-check.
        state.shutdown_with_reason(DaemonShutdownTrigger::SigTerm);
        let held = tokio::spawn(async move { worker.hold_while_unresolved().await });
        let ended = tokio::time::timeout(UNRESOLVED_CLEANUP_RECHECK * 4, held).await;
        let _ = stranger.kill();
        let _ = stranger.wait();
        ended
            .expect("a shutdown during a re-check was lost")
            .unwrap();
    }

    /// PDASHOSS01-417 part 2: the run's terminal report goes out only
    /// after run-end cleanup (process stop, then the run-end hook) is
    /// done, so the cloud never sees a run finished while its processes
    /// or teardown are still pending. The test runs on one thread: once
    /// `RunFailed` is received the worker cannot make progress before
    /// the assertion, so a hook still pending at that point is caught.
    #[cfg(unix)]
    #[tokio::test]
    async fn terminal_report_is_sent_after_run_end_cleanup() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let wd = tmp.path().join("task-folder");
        std::fs::create_dir_all(&wd).unwrap();
        // The agent opts into a run-end hook, then crashes on startup.
        let agent = tmp.path().join("fake-codex");
        std::fs::write(
            &agent,
            "#!/bin/sh\nmkdir -p .pidash\necho 'touch hook-ran' > .pidash/run-end-hook\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let mut worker = codex_worker(tmp.path(), wd.clone(), &agent, tx);
        let run = tokio::spawn(async move {
            worker
                .run(
                    uuid::Uuid::new_v4(),
                    "Summarize".into(),
                    None,
                    None,
                    AssignContext::default(),
                )
                .await
        });
        loop {
            let msg = rx.recv().await.expect("worker ended without a report").body;
            if matches!(
                msg,
                ClientMsg::RunFailed { .. }
                    | ClientMsg::RunCompleted { .. }
                    | ClientMsg::RunCancelled { .. }
            ) {
                break;
            }
        }
        assert!(
            wd.join("hook-ran").exists(),
            "terminal report was sent before run-end cleanup finished"
        );
        run.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn repo_free_assignment_reaches_agent_spawn_and_skips_branch_checkout() {
        for kind in [AgentKind::Codex, AgentKind::ClaudeCode] {
            let tmp = tempfile::tempdir().unwrap();
            let paths = paths_for(tmp.path());
            paths.ensure().unwrap();
            let wd = tmp.path().join("task-folder");
            std::fs::create_dir_all(&wd).unwrap();
            std::fs::write(wd.join("notes.txt"), "preserve user content").unwrap();
            let mut config = runner_config("general-tasks", "TEST", wd.clone());
            config.project_slug = None; // no cloud context lookup in this test
            config.agent.kind = kind;
            let missing_binary = tmp
                .path()
                .join("missing-agent")
                .to_string_lossy()
                .into_owned();
            config.codex.binary = missing_binary.clone();
            config.claude_code.binary = missing_binary;
            let runner_paths = paths.for_runner(config.runner_id);
            runner_paths.ensure().unwrap();
            let (tx, mut rx) = mpsc::channel(16);
            let engine_paths = paths.clone();
            let mut worker = AssignWorker {
                runner_paths,
                daemon_paths: paths,
                runner_config: config.clone(),
                engine: crate::pidash_agent::manager::PidashAgentManager::new(
                    None,
                    engine_paths,
                    WorkspaceTokens::default(),
                    None,
                ),
                state: StateHandle::new(Config {
                    engine: Default::default(),
                    auth: Default::default(),
                    auth_event: None,
                    version: 2,
                    daemon: Default::default(),
                    runners: vec![config.clone()],
                    cli: None,
                }),
                approvals: ApprovalRouter::new(),
                out: RunnerOut::new(config.runner_id, tx),
                cancel: tokio_util::sync::CancellationToken::new(),
                coordinator: None,
                queue_blocked: false,
                run_end: RunEnd::default(),
            };
            worker
                .run(
                    uuid::Uuid::new_v4(),
                    "Summarize these notes".into(),
                    None,
                    None,
                    AssignContext::default(),
                )
                .await
                .unwrap();
            assert!(matches!(
                rx.recv().await.unwrap().body,
                ClientMsg::Accept { .. }
            ));
            // Deliberately absent engine proves the assignment got past
            // workspace setup and reached the agent-specific spawn path.
            let failure = rx.recv().await.unwrap().body;
            assert!(matches!(
                failure,
                ClientMsg::RunFailed {
                    reason: FailureReason::CodexCrash | FailureReason::AgentCrash,
                    ..
                }
            ));
            assert_eq!(
                std::fs::read_to_string(wd.join("notes.txt")).unwrap(),
                "preserve user content"
            );
            assert!(!wd.join(".git").exists());
        }
    }

    /// A fake Codex app-server that logs every request line to
    /// `$PROMPT_LOG` and completes two turns (NEWPIDASH-402).
    fn logging_codex_script() -> &'static str {
        r#"
            set -e
            log() { printf '%s\n' "$1" >> "$PROMPT_LOG"; }
            read line; log "$line"
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
            read line; log "$line"
            read thread; log "$thread"
            case "$thread" in *'"method":"thread/start"'*) ;; *) exit 1;; esac
            printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_chat"}}'
            for n in 1 2; do
                read turn; log "$turn"
                case "$turn" in *'"method":"turn/start"'*) ;; *) exit 1;; esac
                printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"text":"hello"}}'
                printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"conclusion":"success","done":{"status":"ok","summary":"done"}}}'
            done
            sleep 0.3
        "#
    }

    #[tokio::test]
    async fn chat_turn_prefixes_guidance_only_on_the_first_turn() {
        // The cloud-relayed lane is the second acceptance hook
        // (NEWPIDASH-402): the first turn of a bridge carries the
        // CLI guidance in its `turn/start` prompt, the second sends
        // the user's text verbatim.
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("prompts.log");
        let wd = tmp.path().join("work");
        std::fs::create_dir_all(&wd).unwrap();
        let config = runner_config("main", "WEB", wd);
        let (tx, mut rx) = mpsc::channel::<Envelope<ClientMsg>>(64);
        let (_cmd_tx, cmd_rx) = mpsc::channel(1);
        let (active_tx, _active_rx) = watch::channel(false);
        let mut chat = ChatWorker {
            runner_config: config.clone(),
            state: StateHandle::new(Config {
                engine: Default::default(),
                auth: Default::default(),
                auth_event: None,
                version: 2,
                daemon: Default::default(),
                runners: vec![config.clone()],
                cli: None,
            }),
            approvals: ApprovalRouter::new(),
            out: RunnerOut::new(config.runner_id, tx),
            command_rx: cmd_rx,
            active_tx,
            guidance_sent: false,
            unit: None,
        };
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c")
            .arg(logging_codex_script())
            .env("PROMPT_LOG", &log);
        let server = crate::codex::app_server::AppServer::spawn_command(cmd)
            .await
            .expect("spawn fake codex");
        let mut bridge = Some(AgentBridge::Codex(
            crate::codex::bridge::Bridge::from_server(server, None),
        ));
        let mut workspace_path: Option<std::path::PathBuf> = None;
        let (mut seq, mut started) = (0u64, false);
        for content in ["list the open issues", "second question"] {
            let turn = ChatTurn {
                message_id: uuid::Uuid::new_v4(),
                content: content.into(),
                cwd: None,
                model: None,
                local_thread_id: None,
                local_session_id: None,
            };
            let close = tokio::time::timeout(
                Duration::from_secs(15),
                chat.handle_turn(
                    uuid::Uuid::new_v4(),
                    turn,
                    &mut bridge,
                    &mut workspace_path,
                    &mut seq,
                    &mut started,
                ),
            )
            .await
            .expect("turn timed out")
            .expect("turn ok");
            assert!(!close, "turn completes without closing");
        }
        let turns: Vec<String> = std::fs::read_to_string(&log)
            .expect("prompt log written")
            .lines()
            .filter(|line| line.contains("\"method\":\"turn/start\""))
            .map(str::to_string)
            .collect();
        assert_eq!(turns.len(), 2, "two turn/start prompts logged");
        assert!(
            turns[0].contains("## Pi Dash CLI") && turns[0].contains("list the open issues"),
            "first turn carries guidance plus user text"
        );
        assert!(
            !turns[1].contains("## Pi Dash CLI") && turns[1].contains("second question"),
            "second turn sends the user's text verbatim"
        );
        // The user-visible history (the cloud chat frames) never shows
        // the injected text: frames carry no user content.
        rx.close();
        let mut leaked = false;
        while let Some(env) = rx.recv().await {
            if format!("{:?}", env.body).contains("## Pi Dash CLI") {
                leaked = true;
            }
        }
        assert!(!leaked, "no cloud chat frame leaks the injected text");
    }

    #[tokio::test]
    async fn chat_and_run_resolve_to_the_same_working_dir() {
        // One exclusive dir per runner: the chat lane resolves to exactly the
        // runner's `workspace.working_dir` — the same directory an issue run
        // resolves — with no separate chat workspace path.
        let tmp = tempfile::tempdir().unwrap();
        let wd = tmp.path().join("the-one-dir");
        let config = runner_config("main", "WEB", wd.clone());
        let (tx, _rx) = mpsc::channel::<Envelope<ClientMsg>>(1);
        let (_cmd_tx, cmd_rx) = mpsc::channel(1);
        let (active_tx, _active_rx) = watch::channel(false);
        let mut chat = ChatWorker {
            runner_config: config.clone(),
            state: StateHandle::new(Config {
                engine: Default::default(),
                auth: Default::default(),
                auth_event: None,
                version: 2,
                daemon: Default::default(),
                runners: vec![config.clone()],
                cli: None,
            }),
            approvals: ApprovalRouter::new(),
            out: RunnerOut::new(config.runner_id, tx),
            command_rx: cmd_rx,
            active_tx,
            guidance_sent: false,
            unit: None,
        };
        let chat_dir = chat
            .resolve_chat_workspace(uuid::Uuid::new_v4(), None)
            .await
            .unwrap();
        assert_eq!(chat_dir, wd);

        // The issue-run path resolves the very same directory (non-git folder).
        let run_dir = match crate::workspace::resolve(&config.workspace.working_dir, None)
            .await
            .unwrap()
        {
            crate::workspace::Resolution::Directory(p)
            | crate::workspace::Resolution::ExistingRepo(p)
            | crate::workspace::Resolution::Cloned(p) => p,
        };
        assert_eq!(run_dir, chat_dir);
    }

    #[tokio::test]
    async fn cancel_without_active_process_is_acknowledged_immediately() {
        let runner_id = uuid::Uuid::new_v4();
        let run_id = uuid::Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(1);
        let out = RunnerOut::new(runner_id, tx);

        acknowledge_cancel_without_active_process(&out, run_id).await;

        let env = rx
            .recv()
            .await
            .expect("expected RunCancelled acknowledgement");
        assert_eq!(env.runner_id, Some(runner_id));
        match env.body {
            ClientMsg::RunCancelled {
                run_id: acknowledged_run_id,
                tokens,
                model,
                ..
            } => {
                assert_eq!(acknowledged_run_id, run_id);
                assert_eq!(tokens, None);
                assert_eq!(model, None);
            }
            other => panic!("expected RunCancelled, got {other:?}"),
        }
    }

    #[test]
    fn format_exec_command_detail_does_not_imply_completed_command_is_running() {
        let started_at = Utc.with_ymd_and_hms(2026, 5, 27, 9, 11, 0).unwrap();
        let completed_at = Utc.with_ymd_and_hms(2026, 5, 27, 9, 11, 4).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 27, 9, 44, 0).unwrap();
        let detail = format_exec_command_detail(
            &ExecCommandSnapshot {
                command: "cat .pidash-workpad.md".into(),
                cwd: None,
                tool_call_id: Some("tool-1".into()),
                started_at,
                completed_at: Some(completed_at),
                completed_success: Some(true),
            },
            now,
        );

        assert!(detail.contains("completed 1976s before failure"));
        assert!(detail.contains("ran 4s"));
        assert!(!detail.contains("no completion event observed"));
    }

    #[test]
    fn format_exec_command_detail_says_when_completion_was_not_seen() {
        let started_at = Utc.with_ymd_and_hms(2026, 5, 27, 9, 11, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 27, 9, 12, 9).unwrap();
        let detail = format_exec_command_detail(
            &ExecCommandSnapshot {
                command: "git fetch origin".into(),
                cwd: Some("/tmp/repo".into()),
                tool_call_id: Some("tool-2".into()),
                started_at,
                completed_at: None,
                completed_success: None,
            },
            now,
        );

        assert_eq!(
            detail,
            "command context: last command frame `git fetch origin` in `/tmp/repo` \
             (started 69s ago; no completion frame observed)"
        );
    }

    #[test]
    fn format_exec_command_detail_distinguishes_errored_completion_from_clean_one() {
        // A command whose completion event reported a non-success
        // terminal status (codex `failed`, Claude `is_error: true`) must
        // not be described as a clean "completion" — that wording reads
        // like success, which can be just as misleading as the original
        // "still running" bug this PR fixes.
        let started_at = Utc.with_ymd_and_hms(2026, 5, 27, 9, 11, 0).unwrap();
        let completed_at = Utc.with_ymd_and_hms(2026, 5, 27, 9, 11, 4).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 27, 9, 12, 0).unwrap();
        let detail = format_exec_command_detail(
            &ExecCommandSnapshot {
                command: "npm test".into(),
                cwd: None,
                tool_call_id: Some("tool-x".into()),
                started_at,
                completed_at: Some(completed_at),
                completed_success: Some(false),
            },
            now,
        );

        assert!(detail.contains("exited with error 56s before failure"));
        assert!(detail.contains("ran 4s"));
        assert!(!detail.contains("completed 56s"));
    }

    #[test]
    fn diagnostic_signal_prefers_json_error_messages() {
        let lines = vec![
            "pidash 0.1.10".to_string(),
            "Process exited with code 1".to_string(),
            r#"{"error":"GET https://pidash.airepublic.com/api/v1/users/me/: error sending request"}"#.to_string(),
        ];
        let signal = diagnostic_signal_from_stderr(&lines).unwrap();
        assert_eq!(
            signal,
            "stderr JSON error: GET https://pidash.airepublic.com/api/v1/users/me/: error sending request"
        );
    }

    #[test]
    fn diagnostic_signal_keeps_non_json_errors() {
        let lines = vec![
            "pidash 0.1.10".to_string(),
            "gh: error: HTTP 401: Bad credentials".to_string(),
        ];
        let signal = diagnostic_signal_from_stderr(&lines).unwrap();
        assert_eq!(signal, "stderr: gh: error: HTTP 401: Bad credentials");
    }

    #[test]
    fn diagnostic_signal_prefers_substantive_error_over_exit_status() {
        let lines = vec![
            "gh: error: HTTP 401: Bad credentials".to_string(),
            "Process exited with code 1".to_string(),
        ];
        let signal = diagnostic_signal_from_stderr(&lines).unwrap();
        assert_eq!(signal, "stderr: gh: error: HTTP 401: Bad credentials");

        let fallback = diagnostic_signal_from_stderr(&["Process exited with code 1".to_string()])
            .expect("exit status should remain useful as a fallback");
        assert_eq!(fallback, "stderr: Process exited with code 1");
    }

    #[test]
    fn assemble_failure_detail_puts_core_error_first_and_trace_second() {
        let started_at = Utc.with_ymd_and_hms(2026, 5, 29, 5, 25, 53).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 29, 5, 30, 57).unwrap();
        let cmd = ExecCommandSnapshot {
            command: "pidash workspace me".into(),
            cwd: Some("/work".into()),
            tool_call_id: Some("call-1".into()),
            started_at,
            completed_at: None,
            completed_success: None,
        };
        let stderr = crate::agent::StderrSnapshot {
            lines: vec![
                "pidash 0.1.10".into(),
                "Process exited with code 1".into(),
                r#"{"error":"GET https://pidash.airepublic.com/api/v1/users/me/: error sending request"}"#.into(),
            ],
            dropped: 42,
        };

        let detail = assemble_failure_detail(
            "agent stalled for 5 minutes without new agent events",
            Some(&cmd),
            &stderr,
            now,
        );

        let mut lines = detail.lines();
        assert_eq!(
            lines.next(),
            Some(
                "Core error: GET https://pidash.airepublic.com/api/v1/users/me/: error sending request"
            )
        );
        assert!(detail.contains("\n\nError trace:\n"));
        assert!(
            detail.contains("- watchdog: agent stalled for 5 minutes without new agent events")
        );
        assert!(detail.contains("- diagnostic signal: stderr JSON error: GET https://pidash.airepublic.com/api/v1/users/me/: error sending request"));
        assert!(
            detail
                .contains("- command context: last command frame `pidash workspace me` in `/work`")
        );
        assert!(detail.contains(
            "- stderr context (3 line(s) (plus 42 low-signal line(s) filtered from agent stderr))"
        ));
    }

    #[test]
    fn assemble_failure_detail_uses_watchdog_when_no_better_signal_exists() {
        let stderr = crate::agent::StderrSnapshot::default();
        let detail = assemble_failure_detail("agent stdout closed", None, &stderr, Utc::now());

        assert!(detail.starts_with("Core error: agent stdout closed\n\nError trace:\n"));
        assert!(detail.contains("- watchdog: agent stdout closed"));
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
            engine: Default::default(),
            auth: Default::default(),
            auth_event: None,
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

    #[test]
    fn daemon_shutdown_detail_states_trigger_and_previous_exit() {
        use crate::daemon::state::DaemonShutdownTrigger;
        assert_eq!(
            daemon_shutdown_detail(DaemonShutdownTrigger::SigTerm, None),
            "daemon shutdown requested (SIGTERM)"
        );
        assert_eq!(
            daemon_shutdown_detail(DaemonShutdownTrigger::SigTerm, Some("oom-kill")),
            "daemon shutdown requested (SIGTERM; previous exit: oom-kill)"
        );
        assert_eq!(
            daemon_shutdown_detail(DaemonShutdownTrigger::SigInt, None),
            "daemon shutdown requested (SIGINT)"
        );
        assert_eq!(
            daemon_shutdown_detail(DaemonShutdownTrigger::ControlSocket, None),
            "daemon shutdown requested (control socket disconnect)"
        );
        assert_eq!(
            daemon_shutdown_detail(DaemonShutdownTrigger::Invariant, None),
            "daemon shutdown requested (local invariant failure)"
        );
        assert_eq!(
            daemon_shutdown_detail(DaemonShutdownTrigger::SigTerm, Some("  ")),
            "daemon shutdown requested (SIGTERM)"
        );
    }

    #[test]
    fn with_previous_exit_appends_without_duplicating() {
        assert_eq!(
            with_previous_exit("daemon shutdown requested (SIGTERM)", Some("oom-kill")),
            "daemon shutdown requested (SIGTERM; previous exit: oom-kill)"
        );
        // Already carrying one: no-op.
        assert_eq!(
            with_previous_exit(
                "daemon shutdown requested (SIGTERM; previous exit: oom-kill)",
                Some("oom-kill")
            ),
            "daemon shutdown requested (SIGTERM; previous exit: oom-kill)"
        );
        assert_eq!(
            with_previous_exit("daemon shutdown requested", Some("oom-kill")),
            "daemon shutdown requested (previous exit: oom-kill)"
        );
        assert_eq!(
            with_previous_exit("daemon shutdown requested (SIGTERM)", None),
            "daemon shutdown requested (SIGTERM)"
        );
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

        let sent =
            drain_in_flight_runs(runners, &paths, None, DaemonShutdownTrigger::SigTerm, None).await;
        assert_eq!(sent, 1, "exactly one runner had an in-flight run");
        assert!(
            drain_journal::load(&paths).is_empty(),
            "a delivered drain must leave no journal entry behind"
        );

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
                assert_eq!(
                    detail.as_deref(),
                    Some("daemon shutdown requested (SIGTERM)")
                );
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

    /// Review item 2 on #3499: a run that finished while its cleanup is
    /// still pending has its real result held. A shutdown in that window
    /// must report that result, not turn the run into a DaemonRestart
    /// failure, and the worker must not report the run a second time.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn drain_reports_the_held_result_of_a_run_still_in_cleanup() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let inst = RunnerInstance::new(
            runner_config("busy", "WEB", tmp.path().join("wd")),
            &paths,
            out_tx,
        );
        let run_id = uuid::Uuid::new_v4();
        inst.state
            .set_current_run(Some(CurrentRunSummary {
                run_id,
                thread_id: None,
                status: "running".into(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;
        inst.state.hold_terminal();
        let held = inst.state.hold_terminal_msg(ClientMsg::RunCompleted {
            run_id,
            done_payload: serde_json::json!({"ok": true}),
            ended_at: Utc::now(),
            tokens: None,
            model: None,
        });
        assert!(held.is_none(), "the result is held while cleanup runs");
        assert!(inst.state.defer_clear_current_run());
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
        let sent =
            drain_in_flight_runs(runners, &paths, None, DaemonShutdownTrigger::SigTerm, None).await;
        assert_eq!(sent, 1);
        let env = tokio::time::timeout(std::time::Duration::from_secs(2), out_rx.recv())
            .await
            .expect("timed out waiting for the drained report")
            .expect("channel closed");
        match env.body {
            ClientMsg::RunCompleted { run_id: got, .. } => assert_eq!(got, run_id),
            other => panic!("expected the held RunCompleted, got {other:?}"),
        }
        // Cleanup then finishes: nothing more is sent for the run.
        let later = inst.state.begin_release();
        assert!(later.is_empty(), "the run would be reported twice");
        assert!(inst.state.release_clears_current_run());
        assert!(
            inst.state
                .hold_terminal_msg(ClientMsg::RunCancelled {
                    run_id,
                    cancelled_at: Utc::now(),
                    tokens: None,
                    model: None,
                })
                .is_none(),
            "a report after the drain must be dropped"
        );
    }

    /// Reopen a queue store the test dropped. A subprocess another test
    /// forks can hold the old lock's file description until it execs, so
    /// the lock may stay taken for a moment after the drop.
    fn reopen_queue(dir: std::path::PathBuf) -> crate::daemon::run_queue::Coordinator {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match crate::daemon::run_queue::Coordinator::new(dir.clone(), 1) {
                Ok(c) => return c,
                Err(e) if std::time::Instant::now() >= deadline => panic!("reopen: {e}"),
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
    }

    fn hello_map(inst: &RunnerInstance) -> Arc<RwLock<HelloRunnerMap>> {
        Arc::new(RwLock::new(
            std::iter::once(inst)
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
        ))
    }

    /// Review item 1 on #3499 at cc5a1a7: the worker has started sending
    /// the held result but has not cleared the in-flight run. A shutdown
    /// drain landing then must still find the real result (and own the
    /// send from there), not report a DaemonRestart failure.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn drain_during_the_release_reports_the_real_result() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let inst = RunnerInstance::new(
            runner_config("busy", "WEB", tmp.path().join("wd")),
            &paths,
            out_tx,
        );
        let run_id = uuid::Uuid::new_v4();
        inst.state
            .set_current_run(Some(CurrentRunSummary {
                run_id,
                thread_id: None,
                status: "running".into(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;
        inst.state.hold_terminal();
        assert!(
            inst.state
                .hold_terminal_msg(ClientMsg::RunCompleted {
                    run_id,
                    done_payload: serde_json::json!({"ok": true}),
                    ended_at: Utc::now(),
                    tokens: None,
                    model: None,
                })
                .is_none()
        );
        assert!(inst.state.defer_clear_current_run());
        // The worker takes the held result to send it...
        let sending = inst.state.begin_release();
        assert_eq!(sending.len(), 1);
        // ...and the shutdown drain lands before the run is cleared.
        let sent = drain_in_flight_runs(
            hello_map(&inst),
            &paths,
            None,
            DaemonShutdownTrigger::SigTerm,
            None,
        )
        .await;
        assert_eq!(sent, 1);
        let env = tokio::time::timeout(std::time::Duration::from_secs(2), out_rx.recv())
            .await
            .expect("timed out waiting for the drained report")
            .expect("channel closed");
        match env.body {
            ClientMsg::RunCompleted { run_id: got, .. } => assert_eq!(got, run_id),
            other => panic!("expected the real RunCompleted, got {other:?}"),
        }
        // The drain now owns the send; the worker stops sending.
        assert!(!inst.state.releasing());
        assert!(inst.state.release_clears_current_run());
    }

    /// The worker side of the release: the held result is sent, the run is
    /// cleared before the hold ends, and nothing is held afterwards.
    #[tokio::test]
    async fn release_sends_the_held_result_then_clears_the_run() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        let worker = codex_worker(tmp.path(), tmp.path().join("wd"), &tmp.path().join("x"), tx);
        let run_id = uuid::Uuid::new_v4();
        worker
            .state
            .set_current_run(Some(CurrentRunSummary {
                run_id,
                thread_id: None,
                status: "running".into(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;
        worker.hold_terminal();
        worker
            .send(ClientMsg::RunCancelled {
                run_id,
                cancelled_at: Utc::now(),
                tokens: None,
                model: None,
            })
            .await;
        worker.clear_current_run().await;
        assert!(rx.try_recv().is_err(), "the result was not held");
        assert!(worker.state.rx_in_flight.borrow().is_some());
        worker.release_terminal().await;
        assert!(matches!(
            rx.try_recv().unwrap().body,
            ClientMsg::RunCancelled { .. }
        ));
        assert!(worker.state.rx_in_flight.borrow().is_none());
        assert!(!worker.state.releasing());
        assert!(worker.state.take_terminal_for_drain().is_empty());
    }

    /// Review item 2 on #3499 at 78368f6: a held result is persisted to the
    /// machine queue's terminal outbox as soon as it is staged, so a daemon
    /// killed during run-end cleanup (no graceful drain) replays the exact
    /// result on its next start. Delivery and the slot release still wait
    /// for cleanup.
    #[tokio::test]
    async fn a_held_result_is_persisted_before_cleanup() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        let mut worker = codex_worker(tmp.path(), tmp.path().join("wd"), &tmp.path().join("x"), tx);
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord =
            Arc::new(crate::daemon::run_queue::Coordinator::new(queue_dir.clone(), 1).unwrap());
        let run_id = uuid::Uuid::new_v4();
        coord
            .enqueue(crate::daemon::run_queue_store::QueueEntry {
                cloud_origin: "test".to_string(),
                runner_id: worker.runner_config.runner_id,
                run_id,
                assignment_token: Some(uuid::Uuid::new_v4()),
                sequence: 1,
                queued_at: 0,
                phase: "waiting".to_string(),
                assignment_payload: vec![],
                report_revision: 0,
                process_scope_id: None,
                pending_terminal_payload: None,
                pending_terminal_message_id: None,
            })
            .unwrap();
        assert!(coord.admit(run_id).unwrap());
        worker.coordinator = Some(coord.clone());
        worker.hold_terminal();
        let result = ClientMsg::RunCompleted {
            run_id,
            done_payload: serde_json::json!({"ok": true}),
            ended_at: Utc::now(),
            tokens: None,
            model: None,
        };
        worker.send(result.clone()).await;
        assert!(rx.try_recv().is_err(), "the result was not held");
        // The daemon dies here, mid-cleanup: a fresh coordinator on the same
        // store finds the exact result to replay.
        drop(worker);
        drop(coord);
        let after_crash = reopen_queue(queue_dir);
        let pending = after_crash.terminal_pending();
        assert_eq!(pending.len(), 1, "the held result was not persisted");
        let replayed: ClientMsg =
            serde_json::from_slice(pending[0].pending_terminal_payload.as_deref().unwrap())
                .unwrap();
        assert_eq!(
            serde_json::to_value(&replayed).unwrap(),
            serde_json::to_value(&result).unwrap()
        );
    }

    /// Review of #3501 at 1524d8e (MUST 5): when a second terminal report is
    /// held for the same run (an agent failure, then a history-write
    /// failure), the outbox keeps the first one, the result live delivery
    /// sends first, so a crash during cleanup replays the real outcome.
    #[tokio::test]
    async fn the_first_held_result_is_the_one_persisted() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, _rx) = mpsc::channel(8);
        let mut worker = codex_worker(tmp.path(), tmp.path().join("wd"), &tmp.path().join("x"), tx);
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord =
            Arc::new(crate::daemon::run_queue::Coordinator::new(queue_dir.clone(), 1).unwrap());
        let run_id = uuid::Uuid::new_v4();
        coord
            .enqueue(crate::daemon::run_queue_store::QueueEntry {
                cloud_origin: "test".to_string(),
                runner_id: worker.runner_config.runner_id,
                run_id,
                assignment_token: Some(uuid::Uuid::new_v4()),
                sequence: 1,
                queued_at: 0,
                phase: "waiting".to_string(),
                assignment_payload: vec![],
                report_revision: 0,
                process_scope_id: None,
                pending_terminal_payload: None,
                pending_terminal_message_id: None,
            })
            .unwrap();
        assert!(coord.admit(run_id).unwrap());
        worker.coordinator = Some(coord.clone());
        worker.hold_terminal();
        let agent_failure = ClientMsg::RunFailed {
            run_id,
            reason: FailureReason::AgentCrash,
            detail: Some("agent exited".into()),
            ended_at: Utc::now(),
            tokens: None,
            model: None,
        };
        worker.send(agent_failure.clone()).await;
        worker
            .stage_internal_failure(run_id, Err(anyhow::anyhow!("history write failed")))
            .await
            .unwrap();
        drop(worker);
        // Read the store the crashed daemon would replay: reopening the
        // queue here could race a sibling test's child that briefly holds
        // the queue lock's inherited descriptor.
        let pending = coord.terminal_pending();
        assert_eq!(pending.len(), 1);
        let replayed: ClientMsg =
            serde_json::from_slice(pending[0].pending_terminal_payload.as_deref().unwrap())
                .unwrap();
        assert_eq!(
            serde_json::to_value(&replayed).unwrap(),
            serde_json::to_value(&agent_failure).unwrap(),
            "a later held report replaced the first in the outbox"
        );
    }

    /// Review of #3501 at 1524d8e (MUST 2): a runner a crashed daemon's
    /// unresolved group may belong to stays reserved (so Assign and chat are
    /// refused) until a re-check resolves the group; other runners are free.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_unresolved_recovery_reserves_its_runner_until_resolved() {
        let cfg = || crate::config::schema::Config {
            engine: Default::default(),
            auth: Default::default(),
            auth_event: None,
            version: 2,
            daemon: Default::default(),
            runners: vec![],
            cli: None,
        };
        let (a, b, c) = (
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
        );
        let (sa, sb, sc) = (
            StateHandle::new(cfg()),
            StateHandle::new(cfg()),
            StateHandle::new(cfg()),
        );
        let reservations =
            RecoveryReservations::new(crate::util::group_recovery::RecoveryHold::for_tests(vec![
                Some(a),
                Some(c),
            ]));
        assert!(reservations.reserve_if_held(a, &sa).await);
        assert!(!reservations.reserve_if_held(b, &sb).await);
        assert!(
            sa.cleanup_reserved(),
            "the affected runner was not reserved"
        );
        assert!(!sb.cleanup_reserved());
        // Review of #3501 at 591a610 (MUST 6): a runner added while its
        // group is still unresolved starts reserved too.
        assert!(reservations.reserve_if_held(c, &sc).await);
        assert!(
            sc.cleanup_reserved(),
            "a runner added during recovery started unreserved"
        );
        let resolved = crate::util::group_recovery::RecoveryHold::for_tests(vec![]);
        assert!(!reservations.update(resolved).await);
        assert!(
            !sa.cleanup_reserved() && !sc.cleanup_reserved(),
            "the reservation outlived recovery"
        );
        // Once resolved, a runner added later is not reserved.
        let d = StateHandle::new(cfg());
        assert!(!reservations.reserve_if_held(a, &d).await);
    }

    /// Review of #3501 at 1524d8e (MUST 4): once the shutdown drain has
    /// taken a run's held result and persisted it for replay, the worker
    /// finishing cleanup releases the queue slot but must not delete that
    /// outbox record: the drain's send may still time out, and the next
    /// daemon start must find the result to replay.
    #[test]
    fn a_drain_owned_terminal_record_survives_the_slot_release() {
        let tmp = tempfile::tempdir().unwrap();
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord = crate::daemon::run_queue::Coordinator::new(queue_dir.clone(), 1).unwrap();
        let run_id = uuid::Uuid::new_v4();
        coord
            .enqueue(crate::daemon::run_queue_store::QueueEntry {
                cloud_origin: "test".to_string(),
                runner_id: uuid::Uuid::new_v4(),
                run_id,
                assignment_token: Some(uuid::Uuid::new_v4()),
                sequence: 1,
                queued_at: 0,
                phase: "waiting".to_string(),
                assignment_payload: vec![],
                report_revision: 0,
                process_scope_id: None,
                pending_terminal_payload: None,
                pending_terminal_message_id: None,
            })
            .unwrap();
        assert!(coord.admit(run_id).unwrap());
        let state = StateHandle::new(crate::config::schema::Config {
            engine: Default::default(),
            auth: Default::default(),
            auth_event: None,
            version: 2,
            daemon: Default::default(),
            runners: vec![],
            cli: None,
        });
        state.hold_terminal();
        // The drain takes the held report and persists it before sending.
        let _ = state.take_terminal_for_drain();
        assert!(coord.persist_terminal(run_id, b"{}", "drain").unwrap());

        release_finished_slot(&coord, &state, run_id);

        assert!(!coord.holds_run(run_id), "the slot was not released");
        drop(coord);
        let after = reopen_queue(queue_dir);
        assert_eq!(
            after.terminal_pending().len(),
            1,
            "the drain's terminal record was deleted"
        );
    }

    /// Review of #3502 at 968b972 (MUST 6): the drain reads the in-flight
    /// run before it locks the terminal hold. A worker whose send failed can
    /// clear the run and end its release in between; its real result is
    /// already in the outbox. The drain must then leave the run alone, and
    /// never write its `DaemonRestart` fallback over a recorded result.
    #[tokio::test]
    async fn the_drain_never_overwrites_a_recorded_result_with_its_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord = crate::daemon::run_queue::Coordinator::new(queue_dir, 1).unwrap();
        let run_id = uuid::Uuid::new_v4();
        coord
            .enqueue(crate::daemon::run_queue_store::QueueEntry {
                cloud_origin: "test".to_string(),
                runner_id: uuid::Uuid::new_v4(),
                run_id,
                assignment_token: Some(uuid::Uuid::new_v4()),
                sequence: 1,
                queued_at: 0,
                phase: "waiting".to_string(),
                assignment_payload: vec![],
                report_revision: 0,
                process_scope_id: None,
                pending_terminal_payload: None,
                pending_terminal_message_id: None,
            })
            .unwrap();
        assert!(coord.admit(run_id).unwrap());
        let completed = ClientMsg::RunCompleted {
            run_id,
            done_payload: serde_json::json!({}),
            ended_at: Utc::now(),
            tokens: None,
            model: None,
        };
        let real = serde_json::to_vec(&completed).unwrap();
        assert!(coord.persist_terminal(run_id, &real, "held").unwrap());
        let state = StateHandle::new(crate::config::schema::Config {
            engine: Default::default(),
            auth: Default::default(),
            auth_event: None,
            version: 2,
            daemon: Default::default(),
            runners: vec![],
            cli: None,
        });
        let restart = || ClientMsg::RunFailed {
            run_id,
            reason: FailureReason::DaemonRestart,
            detail: None,
            ended_at: Utc::now(),
            tokens: None,
            model: None,
        };
        let recorded = |coord: &crate::daemon::run_queue::Coordinator| {
            coord
                .terminal_pending()
                .into_iter()
                .find(|e| e.run_id == run_id)
                .and_then(|e| e.pending_terminal_payload)
        };

        // The worker already cleared the run and ended its release.
        let stale = take_drain_report(&state, Some(&coord), run_id, restart);
        assert!(
            stale.is_none(),
            "the drain reported a run no longer in flight"
        );
        assert_eq!(recorded(&coord).as_deref(), Some(&real[..]));

        // Still in flight with nothing held: the recorded result is reported.
        state
            .set_current_run(Some(CurrentRunSummary {
                run_id,
                thread_id: None,
                status: "running".to_string(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;
        let (msg, managed) =
            take_drain_report(&state, Some(&coord), run_id, restart).expect("the run is in flight");
        assert!(managed);
        assert!(
            matches!(msg, ClientMsg::RunCompleted { .. }),
            "the drain replaced the recorded result: {msg:?}"
        );
        assert_eq!(
            recorded(&coord).as_deref(),
            Some(&real[..]),
            "the fallback overwrote the recorded result"
        );
    }

    /// Review of #3501 at ba9c1c3 (MUST 4): the shutdown drain must record
    /// a held result in the terminal outbox in the same step that takes it.
    /// A worker that finishes cleanup right after the take (it sees the hold
    /// drained, skips its own persist and releases the slot) must not free
    /// the run's queue row before the drain has recorded the result, or the
    /// drain finds the run unmanaged and only tries one best-effort send.
    #[tokio::test]
    async fn the_drain_records_a_held_result_before_the_slot_can_go() {
        let tmp = tempfile::tempdir().unwrap();
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord =
            Arc::new(crate::daemon::run_queue::Coordinator::new(queue_dir.clone(), 1).unwrap());
        let run_id = uuid::Uuid::new_v4();
        coord
            .enqueue(crate::daemon::run_queue_store::QueueEntry {
                cloud_origin: "test".to_string(),
                runner_id: uuid::Uuid::new_v4(),
                run_id,
                assignment_token: Some(uuid::Uuid::new_v4()),
                sequence: 1,
                queued_at: 0,
                phase: "waiting".to_string(),
                assignment_payload: vec![],
                report_revision: 0,
                process_scope_id: None,
                pending_terminal_payload: None,
                pending_terminal_message_id: None,
            })
            .unwrap();
        assert!(coord.admit(run_id).unwrap());
        let state = StateHandle::new(crate::config::schema::Config {
            engine: Default::default(),
            auth: Default::default(),
            auth_event: None,
            version: 2,
            daemon: Default::default(),
            runners: vec![],
            cli: None,
        });
        state
            .set_current_run(Some(CurrentRunSummary {
                run_id,
                thread_id: None,
                status: "running".to_string(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;
        state.hold_terminal();
        let completed = ClientMsg::RunCompleted {
            run_id,
            done_payload: serde_json::json!({}),
            ended_at: Utc::now(),
            tokens: None,
            model: None,
        };
        // Held only in memory: the worker's own persist is what the race skips.
        assert!(state.hold_terminal_msg(completed).is_none());
        // The worker: as soon as it sees the hold drained, it releases the slot.
        let worker = {
            let (state, coord) = (state.clone(), coord.clone());
            std::thread::spawn(move || {
                while !state.terminal_drained() {
                    std::thread::yield_now();
                }
                release_finished_slot(&coord, &state, run_id);
            })
        };

        let (msg, managed) = take_drain_report(&state, Some(&coord), run_id, || unreachable!())
            .expect("the run is in flight");
        worker.join().unwrap();

        assert!(matches!(msg, ClientMsg::RunCompleted { .. }));
        assert!(managed, "the drain lost track of the managed run");
        assert!(
            coord.terminal_pending().iter().any(|e| e.run_id == run_id),
            "the held result never reached the terminal outbox"
        );
    }

    /// Review of #3501 at 5542f04 (MUST 3): when the worker's send of a held
    /// result fails, releasing the slot must keep the run's terminal outbox
    /// record, so the outbox retry still delivers it. Only `RunFailed` has
    /// the drain journal as a fallback; `RunCompleted` would be lost.
    #[tokio::test]
    async fn an_undelivered_result_keeps_its_terminal_record() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel(8);
        let mut worker = codex_worker(tmp.path(), tmp.path().join("wd"), &tmp.path().join("x"), tx);
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord =
            Arc::new(crate::daemon::run_queue::Coordinator::new(queue_dir.clone(), 1).unwrap());
        let run_id = uuid::Uuid::new_v4();
        coord
            .enqueue(crate::daemon::run_queue_store::QueueEntry {
                cloud_origin: "test".to_string(),
                runner_id: worker.runner_config.runner_id,
                run_id,
                assignment_token: Some(uuid::Uuid::new_v4()),
                sequence: 1,
                queued_at: 0,
                phase: "waiting".to_string(),
                assignment_payload: vec![],
                report_revision: 0,
                process_scope_id: None,
                pending_terminal_payload: None,
                pending_terminal_message_id: None,
            })
            .unwrap();
        assert!(coord.admit(run_id).unwrap());
        worker.coordinator = Some(coord.clone());
        worker.hold_terminal();
        worker
            .send(ClientMsg::RunCompleted {
                run_id,
                done_payload: serde_json::json!({"ok": true}),
                ended_at: Utc::now(),
                tokens: None,
                model: None,
            })
            .await;
        // The cloud connection is gone when cleanup ends.
        drop(rx);
        worker.release_terminal().await;

        release_finished_slot(&coord, &worker.state, run_id);

        assert!(!coord.holds_run(run_id), "the slot was not released");
        assert_eq!(
            coord.terminal_pending().len(),
            1,
            "an undelivered RunCompleted lost its terminal record"
        );
    }

    /// Review of #3502 at 968b972 (MUST 3): a run restored from the durable
    /// queue skips the Assign arm's checks, so the worker itself must not
    /// admit it or prepare the workspace while the runner is reserved by a
    /// crashed daemon's unresolved processes; it starts once that clears.
    #[tokio::test(start_paused = true)]
    async fn a_restored_run_waits_while_the_runner_is_reserved() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, _rx) = mpsc::channel(8);
        let mut worker = codex_worker(tmp.path(), tmp.path().join("wd"), &tmp.path().join("x"), tx);
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord = Arc::new(crate::daemon::run_queue::Coordinator::new(queue_dir, 1).unwrap());
        let run_id = uuid::Uuid::new_v4();
        coord
            .enqueue(crate::daemon::run_queue_store::QueueEntry {
                cloud_origin: "test".to_string(),
                runner_id: worker.runner_config.runner_id,
                run_id,
                assignment_token: Some(uuid::Uuid::new_v4()),
                sequence: 1,
                queued_at: 0,
                phase: "waiting".to_string(),
                assignment_payload: vec![],
                report_revision: 0,
                process_scope_id: None,
                pending_terminal_payload: None,
                pending_terminal_message_id: None,
            })
            .unwrap();
        worker.coordinator = Some(coord.clone());
        let state = worker.state.clone();
        let cancel = worker.cancel.clone();
        state.set_cleanup_reserved(true).await;
        let task = tokio::spawn(async move {
            worker
                .run(run_id, "p".into(), None, None, AssignContext::default())
                .await
        });
        tokio::time::sleep(Duration::from_secs(30)).await;
        let admitted_while_reserved = coord.is_admitted(run_id);
        // Cancelled while reserved: the run never starts.
        cancel.cancel();
        let ended = tokio::time::timeout(Duration::from_secs(5), task).await;
        assert!(
            !admitted_while_reserved,
            "a restored run was admitted while the runner was reserved"
        );
        assert!(ended.is_ok(), "the waiting run ignored its cancellation");
        assert!(coord.holds_run(run_id) && !coord.is_admitted(run_id));
    }

    /// Review of #3501 at 591a610 (MUST 5): the cloud cancels a restored
    /// run while it waits on the reservation. The loop's cancel handler
    /// only drops the queue entry and cancels the worker, so the worker must
    /// acknowledge the cancellation and stop reporting the run in flight.
    #[tokio::test(start_paused = true)]
    async fn a_restored_run_cancelled_while_reserved_is_acknowledged() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        let mut worker = codex_worker(tmp.path(), tmp.path().join("wd"), &tmp.path().join("x"), tx);
        let state = worker.state.clone();
        let cancel = worker.cancel.clone();
        let run_id = uuid::Uuid::new_v4();
        state
            .set_current_run(Some(CurrentRunSummary {
                run_id,
                thread_id: None,
                status: "preparing".to_string(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;
        state.set_cleanup_reserved(true).await;
        let task = tokio::spawn(async move {
            worker
                .run(run_id, "p".into(), None, None, AssignContext::default())
                .await
        });
        tokio::time::sleep(Duration::from_secs(3)).await;
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the waiting run ignored its cancellation")
            .unwrap()
            .unwrap();
        let acked = std::iter::from_fn(|| rx.try_recv().ok())
            .any(|e| matches!(e.body, ClientMsg::RunCancelled { run_id: r, .. } if r == run_id));
        assert!(acked, "the cancellation was never acknowledged");
        assert!(
            state.rx_in_flight.borrow().is_none(),
            "the cancelled run is still reported in flight"
        );
    }

    /// Review of #3502 at 968b972 (MUST 2): a released run whose report
    /// could not be sent stays reported in flight, so the cloud's heartbeat
    /// reaper cannot fail it before the outbox retry delivers the real
    /// result; once delivered, the runner stops reporting it.
    #[tokio::test]
    async fn an_undelivered_run_stays_in_flight_until_the_retry_delivers_it() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let inst = RunnerInstance::new(
            runner_config("pending", "WEB", tmp.path().join("wd")),
            &paths,
            out_tx,
        );
        let (tx, rx) = mpsc::channel(8);
        let mut worker = codex_worker(tmp.path(), tmp.path().join("wd"), &tmp.path().join("x"), tx);
        worker.state = inst.state.clone();
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord =
            Arc::new(crate::daemon::run_queue::Coordinator::new(queue_dir.clone(), 1).unwrap());
        let run_id = uuid::Uuid::new_v4();
        coord
            .enqueue(crate::daemon::run_queue_store::QueueEntry {
                cloud_origin: "test".to_string(),
                runner_id: inst.runner_id,
                run_id,
                assignment_token: Some(uuid::Uuid::new_v4()),
                sequence: 1,
                queued_at: 0,
                phase: "waiting".to_string(),
                assignment_payload: vec![],
                report_revision: 0,
                process_scope_id: None,
                pending_terminal_payload: None,
                pending_terminal_message_id: None,
            })
            .unwrap();
        assert!(coord.admit(run_id).unwrap());
        worker.coordinator = Some(coord.clone());
        inst.state
            .set_current_run(Some(CurrentRunSummary {
                run_id,
                thread_id: None,
                status: "running".to_string(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;
        worker.hold_terminal();
        worker
            .send(ClientMsg::RunCompleted {
                run_id,
                done_payload: serde_json::json!({"ok": true}),
                ended_at: Utc::now(),
                tokens: None,
                model: None,
            })
            .await;
        worker.clear_current_run().await;
        // The terminal POST fails when cleanup ends; polling still works.
        drop(rx);
        worker.release_terminal().await;
        release_finished_slot(&coord, &inst.state, run_id);

        assert_eq!(
            *inst.state.rx_in_flight.borrow(),
            Some(run_id),
            "an undelivered run stopped being reported, so the reaper can fail it"
        );

        let delivered = replay_terminal_outbox_once(&coord, &hello_map(&inst), true).await;
        assert_eq!(delivered, 1, "the retry dropped the run's own report");
        assert!(matches!(
            out_rx.try_recv().unwrap().body,
            ClientMsg::RunCompleted { .. }
        ));
        assert!(
            inst.state.rx_in_flight.borrow().is_none(),
            "the delivered run is still reported in flight"
        );
        assert!(coord.terminal_pending().is_empty());
    }

    /// Review of #3502 at bf9a2ba (MUST 1): a result the startup replay
    /// could not deliver stays in the outbox, but a runner whose session
    /// opens without that run in flight lets the cloud's reaper fail the
    /// run first, and the real result can no longer win finalization. The
    /// run is reported in flight until the retry delivers its result.
    #[tokio::test]
    async fn an_undelivered_report_found_at_startup_stays_in_flight() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let inst = RunnerInstance::new(
            runner_config("outbox", "WEB", tmp.path().join("wd")),
            &paths,
            out_tx,
        );
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord = crate::daemon::run_queue::Coordinator::new(queue_dir.clone(), 1).unwrap();
        let run_id = uuid::Uuid::new_v4();
        coord
            .enqueue(crate::daemon::run_queue_store::QueueEntry {
                cloud_origin: "test".to_string(),
                runner_id: inst.runner_id,
                run_id,
                assignment_token: Some(uuid::Uuid::new_v4()),
                sequence: 1,
                queued_at: 0,
                phase: "waiting".to_string(),
                assignment_payload: vec![],
                report_revision: 0,
                process_scope_id: None,
                pending_terminal_payload: None,
                pending_terminal_message_id: None,
            })
            .unwrap();
        assert!(coord.admit(run_id).unwrap());
        let msg = ClientMsg::RunCompleted {
            run_id,
            done_payload: serde_json::json!({}),
            ended_at: Utc::now(),
            tokens: None,
            model: None,
        };
        assert!(
            coord
                .persist_terminal(run_id, &serde_json::to_vec(&msg).unwrap(), "held")
                .unwrap()
        );
        assert!(coord.release_slot(run_id).unwrap());
        let runners = hello_map(&inst);

        // The startup replay fails: the cloud is unreachable.
        drop(out_rx);
        assert_eq!(
            replay_terminal_outbox_once(&coord, &runners, false).await,
            0
        );
        restore_pending_reports(&coord, &runners).await;
        assert_eq!(
            *inst.state.rx_in_flight.borrow(),
            Some(run_id),
            "the session opens without the undelivered run, so the reaper can fail it"
        );

        // The retry delivers it; then the run stops being reported.
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let mut live = inst.clone();
        live.out = RunnerOut::new(inst.runner_id, out_tx);
        let delivered = replay_terminal_outbox_once(&coord, &hello_map(&live), true).await;
        assert_eq!(delivered, 1);
        assert!(matches!(
            out_rx.try_recv().unwrap().body,
            ClientMsg::RunCompleted { .. }
        ));
        assert!(inst.state.rx_in_flight.borrow().is_none());
        assert!(coord.terminal_pending().is_empty());
    }

    /// Review of #3501 at 591a610 (MUST 4): a run is kept reported in
    /// flight only while a retryable outbox record exists. When the held
    /// result could not be written (here: the queue has no row for the
    /// run, as after a failed write) and its send fails, nothing would ever
    /// deliver it, so the run must not stay in flight for good.
    #[tokio::test]
    async fn an_undelivered_run_without_an_outbox_record_is_not_kept_in_flight() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, _out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let inst = RunnerInstance::new(
            runner_config("norecord", "WEB", tmp.path().join("wd")),
            &paths,
            out_tx,
        );
        let (tx, rx) = mpsc::channel(8);
        let mut worker = codex_worker(tmp.path(), tmp.path().join("wd"), &tmp.path().join("x"), tx);
        worker.state = inst.state.clone();
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord =
            Arc::new(crate::daemon::run_queue::Coordinator::new(queue_dir.clone(), 1).unwrap());
        worker.coordinator = Some(coord.clone());
        let run_id = uuid::Uuid::new_v4();
        inst.state
            .set_current_run(Some(CurrentRunSummary {
                run_id,
                thread_id: None,
                status: "running".to_string(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;
        worker.hold_terminal();
        worker
            .send(ClientMsg::RunCompleted {
                run_id,
                done_payload: serde_json::json!({"ok": true}),
                ended_at: Utc::now(),
                tokens: None,
                model: None,
            })
            .await;
        worker.clear_current_run().await;
        drop(rx);
        worker.release_terminal().await;

        assert!(coord.terminal_pending().is_empty());
        assert_eq!(inst.state.report_pending(), None);
        assert!(
            inst.state.rx_in_flight.borrow().is_none(),
            "a run with no retryable record stays reported in flight forever"
        );
    }

    /// The in-process outbox retry delivers a released run's kept record,
    /// and leaves a record whose run the queue still holds (its result is
    /// held for cleanup) untouched and unsent.
    #[tokio::test]
    async fn the_outbox_retry_skips_runs_the_queue_still_holds() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let inst = RunnerInstance::new(
            runner_config("outbox", "WEB", tmp.path().join("wd")),
            &paths,
            out_tx,
        );
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord = crate::daemon::run_queue::Coordinator::new(queue_dir.clone(), 2).unwrap();
        let mut ids = Vec::new();
        for seq in 1..=2 {
            let run_id = uuid::Uuid::new_v4();
            coord
                .enqueue(crate::daemon::run_queue_store::QueueEntry {
                    cloud_origin: "test".to_string(),
                    runner_id: inst.runner_id,
                    run_id,
                    assignment_token: Some(uuid::Uuid::new_v4()),
                    sequence: seq,
                    queued_at: 0,
                    phase: "waiting".to_string(),
                    assignment_payload: vec![],
                    report_revision: 0,
                    process_scope_id: None,
                    pending_terminal_payload: None,
                    pending_terminal_message_id: None,
                })
                .unwrap();
            assert!(coord.admit(run_id).unwrap());
            let msg = ClientMsg::RunCompleted {
                run_id,
                done_payload: serde_json::json!({}),
                ended_at: Utc::now(),
                tokens: None,
                model: None,
            };
            let payload = serde_json::to_vec(&msg).unwrap();
            assert!(coord.persist_terminal(run_id, &payload, "held").unwrap());
            ids.push(run_id);
        }
        let (released, held) = (ids[0], ids[1]);
        assert!(coord.release_slot(released).unwrap());

        let delivered = replay_terminal_outbox_once(&coord, &hello_map(&inst), true).await;

        assert_eq!(delivered, 1);
        match out_rx.try_recv().unwrap().body {
            ClientMsg::RunCompleted { run_id, .. } => assert_eq!(run_id, released),
            other => panic!("unexpected {other:?}"),
        }
        assert!(out_rx.try_recv().is_err(), "a held run's result was sent");
        let pending: Vec<_> = coord
            .terminal_pending()
            .into_iter()
            .map(|e| e.run_id)
            .collect();
        assert_eq!(pending, vec![held]);
    }

    /// A drained runner's later run, which the drain never reported, has no
    /// terminal record: its row goes with the slot.
    #[test]
    fn a_drained_runner_without_a_terminal_record_deletes_its_row() {
        let tmp = tempfile::tempdir().unwrap();
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord = crate::daemon::run_queue::Coordinator::new(queue_dir.clone(), 1).unwrap();
        let run_id = uuid::Uuid::new_v4();
        coord
            .enqueue(crate::daemon::run_queue_store::QueueEntry {
                cloud_origin: "test".to_string(),
                runner_id: uuid::Uuid::new_v4(),
                run_id,
                assignment_token: Some(uuid::Uuid::new_v4()),
                sequence: 1,
                queued_at: 0,
                phase: "waiting".to_string(),
                assignment_payload: vec![],
                report_revision: 0,
                process_scope_id: None,
                pending_terminal_payload: None,
                pending_terminal_message_id: None,
            })
            .unwrap();
        assert!(coord.admit(run_id).unwrap());
        assert!(coord.release_slot(run_id).unwrap());
        assert!(!coord.holds_run(run_id));
        assert!(coord.store_has_row(run_id).is_some_and(|has| !has));
    }

    /// A normal release (no drain) deletes the record as before.
    #[test]
    fn a_normal_slot_release_deletes_the_terminal_record() {
        let tmp = tempfile::tempdir().unwrap();
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord = crate::daemon::run_queue::Coordinator::new(queue_dir.clone(), 1).unwrap();
        let run_id = uuid::Uuid::new_v4();
        coord
            .enqueue(crate::daemon::run_queue_store::QueueEntry {
                cloud_origin: "test".to_string(),
                runner_id: uuid::Uuid::new_v4(),
                run_id,
                assignment_token: Some(uuid::Uuid::new_v4()),
                sequence: 1,
                queued_at: 0,
                phase: "waiting".to_string(),
                assignment_payload: vec![],
                report_revision: 0,
                process_scope_id: None,
                pending_terminal_payload: None,
                pending_terminal_message_id: None,
            })
            .unwrap();
        assert!(coord.admit(run_id).unwrap());
        let state = StateHandle::new(crate::config::schema::Config {
            engine: Default::default(),
            auth: Default::default(),
            auth_event: None,
            version: 2,
            daemon: Default::default(),
            runners: vec![],
            cli: None,
        });
        assert!(coord.persist_terminal(run_id, b"{}", "held").unwrap());

        release_finished_slot(&coord, &state, run_id);

        assert!(!coord.holds_run(run_id));
        drop(coord);
        let after = reopen_queue(queue_dir);
        assert!(after.terminal_pending().is_empty());
    }

    /// Review of #3502 at 7ba3185 (MUST 1): a runner whose only "work" is
    /// a finished run's undelivered result must still be replaceable, so a
    /// re-login can rotate its credentials; the replacement picks the
    /// pending result back up from the outbox. A real run still blocks.
    #[tokio::test]
    async fn a_runner_only_reporting_a_result_can_be_restarted() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, _out_rx) = mpsc::channel::<Envelope<ClientMsg>>(1);
        let cfg = runner_config("reporting", "WEB", tmp.path().join("wd"));
        let runner_id = cfg.runner_id;
        let inst = RunnerInstance::new(cfg.clone(), &paths, out_tx.clone());
        let run_id = uuid::Uuid::new_v4();
        let summary = CurrentRunSummary {
            run_id,
            thread_id: None,
            status: "reporting".to_string(),
            started_at: Utc::now(),
            events: 0,
        };
        inst.state.set_current_run(Some(summary.clone())).await;
        assert!(
            ensure_idle(&inst).is_err(),
            "a live run was treated as idle"
        );
        inst.state.set_report_pending(run_id);
        assert!(
            ensure_idle(&inst).is_ok(),
            "a runner only waiting to report a result blocks credential rotation"
        );

        // The replacement reports the same run in flight until delivery.
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord = crate::daemon::run_queue::Coordinator::new(queue_dir, 1).unwrap();
        coord
            .enqueue(crate::daemon::run_queue_store::QueueEntry {
                cloud_origin: "test".to_string(),
                runner_id,
                run_id,
                assignment_token: Some(uuid::Uuid::new_v4()),
                sequence: 1,
                queued_at: 0,
                phase: "waiting".to_string(),
                assignment_payload: vec![],
                report_revision: 0,
                process_scope_id: None,
                pending_terminal_payload: None,
                pending_terminal_message_id: None,
            })
            .unwrap();
        assert!(coord.admit(run_id).unwrap());
        assert!(coord.persist_terminal(run_id, b"{}", "held").unwrap());
        coord.release_slot(run_id).unwrap();
        assert!(!coord.holds_run(run_id));
        let fresh = RunnerInstance::new(cfg, &paths, out_tx);
        restore_pending_report_for(&coord, runner_id, &fresh.state).await;
        assert_eq!(*fresh.state.rx_in_flight.borrow(), Some(run_id));
        assert_eq!(fresh.state.report_pending(), Some(run_id));
        // While the runner is between stop and start it is absent from the
        // registry; the in-process retry must not drop its record then.
        let empty: Arc<RwLock<HelloRunnerMap>> = Arc::new(RwLock::new(HashMap::new()));
        assert_eq!(replay_terminal_outbox_once(&coord, &empty, true).await, 0);
        assert_eq!(
            coord.terminal_pending().len(),
            1,
            "a restarting runner's undelivered result was dropped"
        );
    }

    /// Review item 2 on #3499 at cc5a1a7: a runner whose last run's cleanup
    /// is unresolved is not idle, so a credential or config change must not
    /// hot-restart it.
    #[tokio::test]
    async fn a_runner_with_unresolved_cleanup_is_not_idle() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, _out_rx) = mpsc::channel::<Envelope<ClientMsg>>(1);
        let inst = RunnerInstance::new(
            runner_config("busy", "WEB", tmp.path().join("wd")),
            &paths,
            out_tx,
        );
        assert!(ensure_idle(&inst).is_ok());
        inst.state.set_cleanup_reserved(true).await;
        assert!(
            ensure_idle(&inst).is_err(),
            "a runner still cleaning up was treated as idle"
        );
        inst.state.set_cleanup_reserved(false).await;
        assert!(ensure_idle(&inst).is_ok());
    }

    /// Review item 3 on #3502 at bf9a2ba: a chat runtime whose agents are
    /// still stopping (a cloud runtime's first cleanup pass, or a local
    /// session being ended) still uses the working copy, so a credential or
    /// config change must not hot-restart the runner.
    #[tokio::test]
    async fn a_runner_with_chat_cleanup_pending_is_not_idle() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, _out_rx) = mpsc::channel::<Envelope<ClientMsg>>(1);
        let inst = RunnerInstance::new(
            runner_config("busy", "WEB", tmp.path().join("wd")),
            &paths,
            out_tx,
        );
        let teardown = inst.state.begin_chat_teardown();
        assert!(
            ensure_idle(&inst).is_err(),
            "a cloud chat runtime still stopping was treated as idle"
        );
        drop(teardown);
        assert!(ensure_idle(&inst).is_ok());

        let ending = inst.chat_sessions.begin_ending_for_test();
        assert!(
            ensure_idle(&inst).is_err(),
            "a local chat session still being ended was treated as idle"
        );
        drop(ending);
        assert!(ensure_idle(&inst).is_ok());
    }

    /// Review item 4 on #3499 at cc5a1a7: an internal error (here the
    /// history write) is known before run-end cleanup starts, so its
    /// `RunFailed{Internal}` goes through the terminal hold, where a
    /// shutdown drain during cleanup finds it, and the run is reported once.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_internal_failure_is_staged_before_run_end_cleanup() {
        let tmp = tempfile::tempdir().unwrap();
        let wd = tmp.path().join("wd");
        std::fs::create_dir_all(&wd).unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let mut worker = codex_worker(tmp.path(), wd, &tmp.path().join("unused"), tx);
        // The run's history file cannot be created.
        let runs = worker.runner_paths.runs_dir();
        let _ = std::fs::remove_dir_all(&runs);
        std::fs::write(&runs, b"not a directory").unwrap();
        let run_id = uuid::Uuid::new_v4();
        let result = worker
            .handle_assign(run_id, "p".into(), None, None, AssignContext::default())
            .await;
        assert!(
            result.is_ok(),
            "the failure would be reported a second time"
        );
        let mut failed = Vec::new();
        while let Ok(env) = rx.try_recv() {
            if let ClientMsg::RunFailed { reason, .. } = env.body {
                failed.push(reason);
            }
        }
        assert!(
            matches!(failed.as_slice(), [FailureReason::Internal]),
            "expected one RunFailed{{Internal}}, got {failed:?}"
        );
        assert!(worker.state.rx_in_flight.borrow().is_none());
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

        let sent =
            drain_in_flight_runs(runners, &paths, None, DaemonShutdownTrigger::Unknown, None).await;
        assert_eq!(sent, 0);
        assert!(drain_journal::load(&paths).is_empty());
        let stray =
            tokio::time::timeout(std::time::Duration::from_millis(100), out_rx.recv()).await;
        assert!(
            stray.is_err(),
            "idle drain produced a stray frame: {stray:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn drain_journals_run_failed_when_send_cannot_be_delivered() {
        // The cloud channel is gone (receiver dropped): the send fails,
        // and the RunFailed must survive in the journal for the next
        // daemon start to replay.
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        drop(out_rx);

        let inst = RunnerInstance::new(
            runner_config("busy", "WEB", tmp.path().join("wd")),
            &paths,
            out_tx,
        );
        let run_id = uuid::Uuid::new_v4();
        inst.state
            .set_current_run(Some(CurrentRunSummary {
                run_id,
                thread_id: None,
                status: "running".into(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;
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

        let sent = drain_in_flight_runs(
            runners,
            &paths,
            None,
            DaemonShutdownTrigger::SigTerm,
            Some("oom-kill".to_string()),
        )
        .await;
        assert_eq!(sent, 0, "send must fail with a dropped receiver");

        let journal = drain_journal::load(&paths);
        assert_eq!(journal.len(), 1);
        assert_eq!(journal[0].runner_id, inst.runner_id);
        assert_eq!(journal[0].run_id, run_id);
        assert!(matches!(journal[0].reason, FailureReason::DaemonRestart));
        // NEWPIDASH-253: trigger + previous exit carried through the journal.
        assert_eq!(journal[0].trigger.as_deref(), Some("SIGTERM"));
        assert_eq!(journal[0].previous_exit.as_deref(), Some("oom-kill"));
        assert_eq!(
            journal[0].detail,
            "daemon shutdown requested (SIGTERM; previous exit: oom-kill)"
        );
    }

    /// Review of #3501 at 591a610 (MUST 3): a managed run's exact result is
    /// already in the SQLite outbox. If its send fails, the legacy journal
    /// must not get a copy too: startup replays the journal first, without
    /// tokens or model, and the cloud keeps the first report it accepts.
    #[tokio::test]
    async fn a_managed_failure_with_an_outbox_record_is_not_journaled() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel(8);
        drop(rx); // every send fails
        let mut worker = codex_worker(tmp.path(), tmp.path().join("wd"), &tmp.path().join("x"), tx);
        worker.daemon_paths.ensure().unwrap();
        let queue_dir = tmp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).unwrap();
        let coord =
            Arc::new(crate::daemon::run_queue::Coordinator::new(queue_dir.clone(), 1).unwrap());
        let run_id = uuid::Uuid::new_v4();
        coord
            .enqueue(crate::daemon::run_queue_store::QueueEntry {
                cloud_origin: "test".to_string(),
                runner_id: worker.runner_config.runner_id,
                run_id,
                assignment_token: Some(uuid::Uuid::new_v4()),
                sequence: 1,
                queued_at: 0,
                phase: "waiting".to_string(),
                assignment_payload: vec![],
                report_revision: 0,
                process_scope_id: None,
                pending_terminal_payload: None,
                pending_terminal_message_id: None,
            })
            .unwrap();
        assert!(coord.admit(run_id).unwrap());
        worker.coordinator = Some(coord.clone());
        let failed = ClientMsg::RunFailed {
            run_id,
            reason: FailureReason::AgentCrash,
            detail: Some("boom".into()),
            ended_at: Utc::now(),
            tokens: None,
            model: None,
        };
        assert!(
            coord
                .persist_terminal(run_id, &serde_json::to_vec(&failed).unwrap(), "held")
                .unwrap()
        );

        assert!(!worker.deliver(failed).await);

        assert!(
            drain_journal::load(&worker.daemon_paths).is_empty(),
            "journaled a failure whose exact payload is already in the outbox"
        );
        // Without an outbox record the journal is still the fallback.
        let other = uuid::Uuid::new_v4();
        assert!(
            !worker
                .deliver(ClientMsg::RunFailed {
                    run_id: other,
                    reason: FailureReason::AgentCrash,
                    detail: None,
                    ended_at: Utc::now(),
                    tokens: None,
                    model: None,
                })
                .await
        );
        let journal = drain_journal::load(&worker.daemon_paths);
        assert_eq!(journal.len(), 1);
        assert_eq!(journal[0].run_id, other);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn undelivered_run_failed_is_journaled_by_assign_worker_send() {
        // NEWPIDASH-251: a normal (non-drain) RunFailed whose send
        // exhausts its retries must land in the journal so the
        // replay loop redelivers it, not be discarded with a warn.
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        paths.ensure().unwrap();
        let config = runner_config("journaltest", "WEB", tmp.path().join("wd"));
        let runner_paths = paths.for_runner(config.runner_id);
        runner_paths.ensure().unwrap();
        let (out_tx, out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        drop(out_rx); // force every send to fail
        let worker = AssignWorker {
            runner_paths,
            daemon_paths: paths.clone(),
            runner_config: config.clone(),
            state: StateHandle::new(Config {
                engine: Default::default(),
                auth: Default::default(),
                version: 2,
                daemon: Default::default(),
                runners: vec![config.clone()],
                cli: None,
                auth_event: None,
            }),
            approvals: ApprovalRouter::new(),
            out: RunnerOut::new(config.runner_id, out_tx),
            cancel: tokio_util::sync::CancellationToken::new(),
            engine: crate::pidash_agent::manager::PidashAgentManager::new(
                None,
                paths.clone(),
                WorkspaceTokens::default(),
                None,
            ),
            coordinator: None,
            queue_blocked: false,
            run_end: RunEnd::default(),
        };
        let run_id = uuid::Uuid::new_v4();
        let ended_at = Utc::now();
        worker
            .send(ClientMsg::RunFailed {
                run_id,
                reason: FailureReason::AgentCrash,
                detail: Some(
                    "timed out waiting for muse exec's run.lifecycle.started record".into(),
                ),
                ended_at,
                tokens: None,
                model: None,
            })
            .await;

        let journal = drain_journal::load(&paths);
        assert_eq!(journal.len(), 1, "undelivered RunFailed must be journaled");
        assert_eq!(journal[0].runner_id, config.runner_id);
        assert_eq!(journal[0].run_id, run_id);
        assert!(matches!(journal[0].reason, FailureReason::AgentCrash));
        assert_eq!(
            journal[0].detail,
            "timed out waiting for muse exec's run.lifecycle.started record"
        );
        assert_eq!(journal[0].ended_at, ended_at);
        assert!(journal[0].trigger.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn delivered_run_failed_is_not_journaled() {
        // The healthy path journals nothing: a delivered RunFailed
        // leaves the journal empty so replay has nothing to do.
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        paths.ensure().unwrap();
        let config = runner_config("healthytest", "WEB", tmp.path().join("wd"));
        let runner_paths = paths.for_runner(config.runner_id);
        runner_paths.ensure().unwrap();
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let worker = AssignWorker {
            runner_paths,
            daemon_paths: paths.clone(),
            runner_config: config.clone(),
            state: StateHandle::new(Config {
                engine: Default::default(),
                auth: Default::default(),
                version: 2,
                daemon: Default::default(),
                runners: vec![config.clone()],
                cli: None,
                auth_event: None,
            }),
            approvals: ApprovalRouter::new(),
            out: RunnerOut::new(config.runner_id, out_tx),
            cancel: tokio_util::sync::CancellationToken::new(),
            engine: crate::pidash_agent::manager::PidashAgentManager::new(
                None,
                paths.clone(),
                WorkspaceTokens::default(),
                None,
            ),
            coordinator: None,
            queue_blocked: false,
            run_end: RunEnd::default(),
        };
        worker
            .send(ClientMsg::RunFailed {
                run_id: uuid::Uuid::new_v4(),
                reason: FailureReason::Timeout,
                detail: Some("agent stalled".into()),
                ended_at: Utc::now(),
                tokens: None,
                model: None,
            })
            .await;
        // The send went out on the channel (delivered).
        let env = tokio::time::timeout(std::time::Duration::from_secs(5), out_rx.recv())
            .await
            .expect("timed out waiting for RunFailed")
            .expect("channel closed before RunFailed arrived");
        assert!(matches!(env.body, ClientMsg::RunFailed { .. }));
        assert!(
            drain_journal::load(&paths).is_empty(),
            "delivered RunFailed must not be journaled"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn replay_delivers_journaled_run_failed_and_clears_journal() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let inst = RunnerInstance::new(
            runner_config("survivor", "WEB", tmp.path().join("wd")),
            &paths,
            out_tx,
        );
        let run_id = uuid::Uuid::new_v4();
        let ended_at = Utc::now();
        drain_journal::record_all(
            &paths,
            &[drain_journal::PendingRunFailure {
                runner_id: inst.runner_id,
                run_id,
                reason: FailureReason::DaemonRestart,
                detail: "daemon shutdown requested (SIGTERM)".into(),
                ended_at,
                recorded_at: ended_at,
                trigger: Some("SIGTERM".into()),
                previous_exit: None,
            }],
        )
        .unwrap();
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

        // NEWPIDASH-253: replay attaches the successor's previous
        // oom-kill to the journaled SIGTERM detail.
        let delivered =
            replay_drain_journal_once(&paths, &runners, Some("oom-kill".to_string())).await;
        assert_eq!(delivered, 1);
        assert!(drain_journal::load(&paths).is_empty());

        let env = tokio::time::timeout(std::time::Duration::from_secs(2), out_rx.recv())
            .await
            .expect("timed out waiting for replayed RunFailed")
            .expect("channel closed");
        assert_eq!(env.runner_id, Some(inst.runner_id));
        match env.body {
            ClientMsg::RunFailed {
                run_id: got,
                reason,
                detail,
                ..
            } => {
                assert_eq!(got, run_id);
                assert!(matches!(reason, FailureReason::DaemonRestart));
                assert_eq!(
                    detail.as_deref(),
                    Some("daemon shutdown requested (SIGTERM; previous exit: oom-kill)")
                );
            }
            other => panic!("expected RunFailed, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn replay_skips_run_that_is_in_flight_again() {
        // The cloud redelivered the journaled run to this daemon and it
        // is running again: replay must drop the stale entry WITHOUT
        // sending RunFailed, or it would kill live work.
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let inst = RunnerInstance::new(
            runner_config("resumed", "WEB", tmp.path().join("wd")),
            &paths,
            out_tx,
        );
        let run_id = uuid::Uuid::new_v4();
        inst.state
            .set_current_run(Some(CurrentRunSummary {
                run_id,
                thread_id: None,
                status: "running".into(),
                started_at: Utc::now(),
                events: 0,
            }))
            .await;
        let now = Utc::now();
        drain_journal::record_all(
            &paths,
            &[drain_journal::PendingRunFailure {
                runner_id: inst.runner_id,
                run_id,
                reason: FailureReason::DaemonRestart,
                detail: "daemon shutdown requested".into(),
                ended_at: now,
                recorded_at: now,
                trigger: None,
                previous_exit: None,
            }],
        )
        .unwrap();
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

        let delivered = replay_drain_journal_once(&paths, &runners, None).await;
        assert_eq!(delivered, 0);
        assert!(
            drain_journal::load(&paths).is_empty(),
            "stale entry must be dropped, not retried forever"
        );
        let stray =
            tokio::time::timeout(std::time::Duration::from_millis(100), out_rx.recv()).await;
        assert!(
            stray.is_err(),
            "no RunFailed may be sent for a run that is in flight again: {stray:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn replay_drops_entry_for_unconfigured_runner() {
        // The runner was removed from config since the entry was
        // journaled: without its credentials the signal can never be
        // delivered, so the entry is dropped instead of retried forever.
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let now = Utc::now();
        drain_journal::record_all(
            &paths,
            &[drain_journal::PendingRunFailure {
                runner_id: uuid::Uuid::new_v4(),
                run_id: uuid::Uuid::new_v4(),
                reason: FailureReason::DaemonRestart,
                detail: "daemon shutdown requested".into(),
                ended_at: now,
                recorded_at: now,
                trigger: None,
                previous_exit: None,
            }],
        )
        .unwrap();
        let runners: Arc<RwLock<HelloRunnerMap>> = Arc::new(RwLock::new(HashMap::new()));

        let delivered = replay_drain_journal_once(&paths, &runners, None).await;
        assert_eq!(delivered, 0);
        assert!(drain_journal::load(&paths).is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn retry_loop_replays_entry_journaled_after_empty_tick() {
        // NEWPIDASH-251 review: the retry loop must stay alive across
        // empty ticks. An entry journaled AFTER the first (empty) tick
        // must still replay without a daemon restart. The old
        // `return` on empty killed the task at the first tick, so this
        // entry would have sat in the journal until the next restart.
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let inst = RunnerInstance::new(
            runner_config("midrun", "WEB", tmp.path().join("wd")),
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
        let handle = tokio::spawn(replay_drain_journal_retry_loop(
            paths.clone(),
            runners.clone(),
            None,
            None,
            Duration::from_millis(20),
        ));
        // Let a few empty ticks elapse. With the buggy `return` the
        // task would already be finished after the first one.
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(
            !handle.is_finished(),
            "retry loop must not exit on an empty journal"
        );
        // Journal a mid-run failure (pre-start style) after startup.
        let run_id = uuid::Uuid::new_v4();
        let now = Utc::now();
        drain_journal::record_all(
            &paths,
            &[drain_journal::PendingRunFailure {
                runner_id: inst.runner_id,
                run_id,
                reason: FailureReason::AgentCrash,
                detail: "timed out waiting for muse exec's run.lifecycle.started record".into(),
                ended_at: now,
                recorded_at: now,
                trigger: None,
                previous_exit: None,
            }],
        )
        .unwrap();
        let env = tokio::time::timeout(Duration::from_secs(2), out_rx.recv())
            .await
            .expect("timed out waiting for replayed mid-run RunFailed")
            .expect("channel closed");
        match env.body {
            ClientMsg::RunFailed {
                run_id: got,
                reason,
                detail,
                ..
            } => {
                assert_eq!(got, run_id);
                assert!(matches!(reason, FailureReason::AgentCrash));
                assert_eq!(
                    detail.as_deref(),
                    Some("timed out waiting for muse exec's run.lifecycle.started record")
                );
            }
            other => panic!("expected RunFailed, got {other:?}"),
        }
        // The loop replays until the journal is empty, then keeps
        // idling (still alive) until aborted on shutdown.
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(
            drain_journal::load(&paths).is_empty(),
            "replayed entry must be cleared from the journal"
        );
        assert!(!handle.is_finished(), "loop must keep idling after replay");
        handle.abort();
        let _ = handle.await;
    }

    /// The rig for driving bridge events through `handle_bridge_event`:
    /// a worker, a fake codex process, history and the outbound channel.
    /// `lane` seeds the engine profile's gateway lane (`None` = no profile
    /// fetched yet, i.e. no lane, so nothing classifies).
    async fn notice_rig(lane: Option<&str>) -> NoticeRig {
        use crate::codex::app_server::AppServer;
        use crate::codex::bridge::Bridge;
        use crate::config::schema::CliSection;
        use crate::pidash_agent::test_server;
        use tokio::process::Command;

        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        paths.ensure().unwrap();
        let wd = tmp.path().join("wd");
        std::fs::create_dir_all(&wd).unwrap();
        let config = runner_config("main", "WEB", wd.clone());
        let runner_paths = paths.for_runner(config.runner_id);
        runner_paths.ensure().unwrap();

        let engine = match lane {
            None => crate::pidash_agent::manager::PidashAgentManager::new(
                None,
                paths_for(tmp.path()),
                WorkspaceTokens::default(),
                None,
            ),
            Some(lane) => {
                let lane = lane.to_string();
                let handler: test_server::Handler = Arc::new(move |req| {
                    if req.path == crate::pidash_agent::api::PROFILE_PATH {
                        (
                            200,
                            serde_json::json!({
                                "available": true, "lane": lane,
                                "base_url": "https://gw.example/v1", "model": "gpt-5.5",
                                "reason_code": "", "engine_sign_in_state": "active"
                            })
                            .to_string(),
                        )
                    } else {
                        (404, "{}".to_string())
                    }
                });
                let server = test_server::start(handler).await;
                let cli = CliSection {
                    token: Some("mt_acme".into()),
                    workspace_slug: Some("acme".into()),
                    ..Default::default()
                };
                let manager = crate::pidash_agent::manager::PidashAgentManager::new(
                    Some(&server.url),
                    paths_for(tmp.path()),
                    WorkspaceTokens::from_cli(Some(&cli)),
                    None,
                );
                manager.refresh_profile().await;
                manager
            }
        };

        let (tx, rx) = mpsc::channel(16);
        let worker = AssignWorker {
            runner_paths: runner_paths.clone(),
            daemon_paths: paths.clone(),
            runner_config: config.clone(),
            state: StateHandle::new(Config {
                engine: Default::default(),
                auth: Default::default(),
                auth_event: None,
                version: 2,
                daemon: Default::default(),
                runners: vec![config.clone()],
                cli: None,
            }),
            approvals: ApprovalRouter::new(),
            out: RunnerOut::new(config.runner_id, tx),
            cancel: tokio_util::sync::CancellationToken::new(),
            engine,
            coordinator: None,
            queue_blocked: false,
            run_end: RunEnd::default(),
        };
        let run_id = uuid::Uuid::new_v4();
        let hist = HistoryWriter::open(&runner_paths, run_id).await.unwrap();
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("while read _; do :; done");
        let server = AppServer::spawn_command(cmd)
            .await
            .expect("spawn fake codex");
        let bridge = AgentBridge::Codex(Bridge::from_server(server, None));
        NoticeRig {
            worker,
            bridge,
            hist,
            run_events: RunEventMirror::new(run_id),
            token_meter: crate::daemon::observability::RunTokenMeter::default(),
            rx,
            run_id,
            wd,
            _tmp: tmp,
        }
    }

    struct NoticeRig {
        worker: AssignWorker,
        bridge: AgentBridge,
        hist: HistoryWriter,
        run_events: RunEventMirror,
        token_meter: crate::daemon::observability::RunTokenMeter,
        rx: mpsc::Receiver<Envelope<ClientMsg>>,
        run_id: uuid::Uuid,
        wd: PathBuf,
        _tmp: tempfile::TempDir,
    }

    impl NoticeRig {
        /// Drive one bridge event through the run lane.
        async fn drive(&mut self, ev: BridgeEvent) -> Option<Outcome> {
            self.worker
                .handle_bridge_event(
                    ev,
                    &mut self.bridge,
                    &mut self.hist,
                    &self.wd,
                    &mut self.run_events,
                    &mut self.token_meter,
                )
                .await
                .expect("handle event")
        }

        /// The RunFailed detail sent since the rig was built, if any
        /// (RunEvents mirror frames are skipped).
        async fn failed_detail(&mut self) -> Option<String> {
            let deadline = std::time::Instant::now() + Duration::from_millis(400);
            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return None;
                }
                let Ok(Some(msg)) = tokio::time::timeout(remaining, self.rx.recv()).await else {
                    return None;
                };
                if let ClientMsg::RunFailed { detail, .. } = msg.body {
                    return detail;
                }
            }
        }
    }

    fn outcome_label(outcome: &Option<Outcome>) -> Option<String> {
        match outcome {
            Some(Outcome::Done { status_label }) => Some(status_label.clone()),
            Some(Outcome::Retry { code }) => Some(format!("retry:{code}")),
            None => None,
        }
    }

    /// Drive one Raw error notice through `handle_bridge_event` and return
    /// the outcome plus the RunFailed detail (if any). PIDESKAPP-162: the
    /// run lane must fail on a 402/401 willRetry notice, not log it as
    /// history. `None` lane = no profile, so the generic behaviour holds.
    async fn run_error_notice_through_handler(
        status: u64,
        body_inner: &str,
    ) -> (Option<String>, Option<String>) {
        run_notice(None, status, body_inner).await
    }

    async fn run_notice(
        lane: Option<&str>,
        status: u64,
        body_inner: &str,
    ) -> (Option<String>, Option<String>) {
        let mut rig = notice_rig(lane).await;
        let params = serde_json::json!({
            "willRetry": true,
            "error": {
                "message": "Reconnecting... 1/5",
                "codexErrorInfo": {"responseStreamDisconnected": {"httpStatusCode": status}},
                "additionalDetails": body_inner,
            },
            "threadId": "th_test",
            "turnId": "turn_1",
        });
        let out = rig
            .drive(BridgeEvent::Raw {
                run_id: rig.run_id,
                method: "error".into(),
                params,
            })
            .await;
        let status_label = outcome_label(&out);
        let detail = rig.failed_detail().await;
        rig.bridge.shutdown(Duration::from_secs(1)).await.ok();
        (status_label, detail)
    }

    #[tokio::test]
    async fn run_lane_gateway_402_will_retry_fails_run_with_cleaned_detail() {
        let (status, detail) = run_error_notice_through_handler(
            402,
            "unexpected status 402 Payment Required: Daily free quota used and LLM credit balance is insufficient; add credit or upgrade your plan., url: https://gateway.test/v1/responses, request id: req_1",
        )
        .await;
        assert_eq!(status.as_deref(), Some("failed"));
        let detail = detail.expect("RunFailed detail");
        assert!(
            detail.contains("Daily free quota used"),
            "detail should carry gateway text, got: {detail}"
        );
        assert!(
            !detail.contains("url:")
                && !detail.contains("request id")
                && !detail.contains("Reconnecting"),
            "detail must not leak URL/request id/Reconnecting, got: {detail}"
        );
    }

    #[tokio::test]
    async fn run_lane_gateway_401_will_retry_fails_run_with_cleaned_detail() {
        let (status, detail) = run_error_notice_through_handler(
            401,
            "unexpected status 401 Unauthorized: Invalid JWT, url: https://gateway.test/v1/responses, request id: req_1",
        )
        .await;
        assert_eq!(status.as_deref(), Some("failed"));
        let detail = detail.expect("RunFailed detail");
        assert!(
            detail.contains("Invalid JWT"),
            "detail should carry gateway text, got: {detail}"
        );
        assert!(
            !detail.contains("url:")
                && !detail.contains("request id")
                && !detail.contains("Reconnecting"),
            "detail must not leak URL/request id/Reconnecting, got: {detail}"
        );
    }

    #[tokio::test]
    async fn run_lane_transient_503_will_retry_stays_a_raw_event() {
        let (status, detail) = run_error_notice_through_handler(
            503,
            "unexpected status 503 Service Unavailable: try again later, url: https://gateway.test/v1/responses, request id: req_1",
        )
        .await;
        assert!(
            status.is_none(),
            "503 is worth retrying and must stay Raw, got status {status:?}"
        );
        assert!(
            detail.is_none(),
            "no RunFailed may be sent for a retryable 503, got: {detail:?}"
        );
    }

    #[tokio::test]
    async fn run_lane_free_quota_exhausted_stops_with_resets_at() {
        let body = r#"{"error":{"code":"free_quota_exhausted","limit":2000000,"plan":"free","resets_at":"2026-11-09T00:00:00Z"}}"#;
        let details = format!(
            "unexpected status 402 Payment Required: {body}, url: https://gateway.test/v1/responses, request id: req_1"
        );
        let (status, detail) = run_notice(Some("openhub"), 402, &details).await;
        assert_eq!(status.as_deref(), Some("failed"));
        let detail = detail.expect("RunFailed detail");
        assert!(
            detail.contains("daily free limit reached, resets at 2026-11-09T00:00:00Z"),
            "detail must carry the clear reason with resets_at, got: {detail}"
        );
        assert!(
            !detail.contains("url:") && !detail.contains("Reconnecting"),
            "detail must not leak URL/Reconnecting, got: {detail}"
        );
    }

    #[tokio::test]
    async fn run_lane_free_plan_paid_lane_blocked_needs_credit() {
        let body = r#"{"error":{"code":"free_plan_paid_lane_blocked"}}"#;
        let details = format!(
            "unexpected status 402 Payment Required: {body}, url: https://gateway.test/v1/responses, request id: req_1"
        );
        let (status, detail) = run_notice(Some("openhub"), 402, &details).await;
        assert_eq!(status.as_deref(), Some("failed"));
        let detail = detail.expect("RunFailed detail");
        assert!(
            detail.contains("this model needs credit"),
            "detail must name the credit need, got: {detail}"
        );
    }

    #[tokio::test]
    async fn run_lane_wallet_empty_codes_stop_with_top_up() {
        for code in ["insufficient_balance", "no_credit_account"] {
            let body = format!(r#"{{"error":{{"code":"{code}"}}}}"#);
            let details = format!(
                "unexpected status 402 Payment Required: {body}, url: https://gateway.test/v1/responses, request id: req_1"
            );
            let (status, detail) = run_notice(Some("openhub"), 402, &details).await;
            assert_eq!(status.as_deref(), Some("failed"), "{code}");
            let detail = detail.expect("RunFailed detail");
            assert!(
                detail.contains("top up"),
                "{code} must stop with a top-up reason, got: {detail}"
            );
        }
    }

    #[tokio::test]
    async fn run_lane_free_lane_busy_retries_without_failing() {
        let body = r#"{"error":{"code":"free_lane_busy"}}"#;
        let details = format!(
            "unexpected status 429 Too Many Requests: {body}, url: https://gateway.test/v1/responses, request id: req_1"
        );
        let (status, detail) = run_notice(Some("openhub"), 429, &details).await;
        assert_eq!(status.as_deref(), Some("retry:free_lane_busy"));
        assert!(
            detail.is_none(),
            "no RunFailed may be sent for a busy free lane, got: {detail:?}"
        );
    }

    #[tokio::test]
    async fn run_lane_free_quota_unavailable_retries_without_failing() {
        let body = r#"{"error":{"code":"free_quota_unavailable"}}"#;
        let details = format!(
            "unexpected status 503 Service Unavailable: {body}, url: https://gateway.test/v1/responses, request id: req_1"
        );
        let (status, detail) = run_notice(Some("openhub"), 503, &details).await;
        assert_eq!(status.as_deref(), Some("retry:free_quota_unavailable"));
        assert!(
            detail.is_none(),
            "no RunFailed may be sent for an unavailable free quota, got: {detail:?}"
        );
    }

    #[tokio::test]
    async fn run_lane_free_lane_busy_once_still_finishes() {
        let mut rig = notice_rig(Some("openhub")).await;
        let params = serde_json::json!({
            "willRetry": true,
            "error": {
                "message": "Reconnecting... 1/5",
                "codexErrorInfo": {"responseStreamDisconnected": {"httpStatusCode": 429}},
                "additionalDetails": "unexpected status 429 Too Many Requests: {\"error\":{\"code\":\"free_lane_busy\"}}, url: https://gateway.test/v1/responses, request id: req_1",
            },
            "threadId": "th_test",
            "turnId": "turn_1",
        });
        let out = rig
            .drive(BridgeEvent::Raw {
                run_id: rig.run_id,
                method: "error".into(),
                params,
            })
            .await;
        assert_eq!(
            outcome_label(&out).as_deref(),
            Some("retry:free_lane_busy"),
            "a busy notice must ask for a backoff retry, not a failure"
        );
        // The retried turn succeeds: the run finishes.
        let out = rig
            .drive(BridgeEvent::Completed {
                run_id: rig.run_id,
                done_payload: serde_json::json!({"conclusion": "success"}),
            })
            .await;
        assert_eq!(outcome_label(&out).as_deref(), Some("completed"));
        assert!(
            rig.failed_detail().await.is_none(),
            "a run that hit free_lane_busy once must not send RunFailed"
        );
        rig.bridge.shutdown(Duration::from_secs(1)).await.ok();
    }

    #[tokio::test]
    async fn run_lane_free_codes_on_other_lanes_keep_today_behaviour() {
        let exhausted = r#"{"error":{"code":"free_quota_exhausted","limit":2000000,"plan":"free","resets_at":"2026-11-09T00:00:00Z"}}"#;
        let details = format!(
            "unexpected status 402 Payment Required: {exhausted}, url: https://gateway.test/v1/responses, request id: req_1"
        );
        let (status, detail) = run_notice(Some("byok"), 402, &details).await;
        assert_eq!(status.as_deref(), Some("failed"));
        let detail = detail.expect("RunFailed detail");
        assert!(
            !detail.contains("daily free limit reached"),
            "a non-openhub lane must keep the generic detail, got: {detail}"
        );
        let busy = r#"{"error":{"code":"free_lane_busy"}}"#;
        let details = format!(
            "unexpected status 429 Too Many Requests: {busy}, url: https://gateway.test/v1/responses, request id: req_1"
        );
        let (status, detail) = run_notice(Some("byok"), 429, &details).await;
        assert!(status.is_none(), "other lanes stay Raw, got {status:?}");
        assert!(
            detail.is_none(),
            "no RunFailed on other lanes, got {detail:?}"
        );
    }

    #[tokio::test]
    async fn run_lane_terminal_exhausted_detail_gets_the_clear_reason() {
        let mut rig = notice_rig(Some("openhub")).await;
        let raw = "unexpected status 402 Payment Required: {\"error\":{\"code\":\"free_quota_exhausted\",\"limit\":2000000,\"plan\":\"free\",\"resets_at\":\"2026-11-09T00:00:00Z\"}}, url: https://gateway.test/v1/responses, request id: req_1";
        let out = rig
            .drive(BridgeEvent::Failed {
                run_id: rig.run_id,
                reason: FailureReason::Internal,
                detail: Some(raw.to_string()),
            })
            .await;
        assert_eq!(outcome_label(&out).as_deref(), Some("failed"));
        let detail = rig.failed_detail().await.expect("RunFailed detail");
        assert!(
            detail.contains("daily free limit reached, resets at 2026-11-09T00:00:00Z"),
            "terminal detail must get the clear reason, got: {detail}"
        );
        rig.bridge.shutdown(Duration::from_secs(1)).await.ok();
    }

    #[tokio::test]
    async fn run_lane_terminal_busy_detail_retries() {
        let mut rig = notice_rig(Some("openhub")).await;
        let raw = "unexpected status 429 Too Many Requests: {\"error\":{\"code\":\"free_lane_busy\"}}, url: https://gateway.test/v1/responses, request id: req_1";
        let out = rig
            .drive(BridgeEvent::Failed {
                run_id: rig.run_id,
                reason: FailureReason::Internal,
                detail: Some(raw.to_string()),
            })
            .await;
        assert_eq!(
            outcome_label(&out).as_deref(),
            Some("retry:free_lane_busy"),
            "a terminal busy failure must retry, not fail"
        );
        assert!(
            rig.failed_detail().await.is_none(),
            "no RunFailed may be sent for a busy terminal failure"
        );
        rig.bridge.shutdown(Duration::from_secs(1)).await.ok();
    }

    #[test]
    fn terminal_gateway_402_detail_is_cleaned_not_raw() {
        let raw = "unexpected status 402 Payment Required: Daily free quota used and LLM credit balance is insufficient; add credit or upgrade your plan., url: https://gateway.test/v1/responses, request id: req_1";
        let cleaned = clean_gateway_failure_detail(raw);
        assert_eq!(
            cleaned,
            "Daily free quota used and LLM credit balance is insufficient; add credit or upgrade your plan."
        );
        assert_eq!(
            clean_gateway_failure_detail(
                "unexpected status 503 Service Unavailable: boom, url: https://x/v1/responses, request id: r1"
            ),
            "unexpected status 503 Service Unavailable: boom, url: https://x/v1/responses, request id: r1"
        );
    }

    #[test]
    fn muse_assign_with_missing_credentials_is_rejected_with_agent_auth_missing() {
        // The pre-pickup gate: every supported kind + no creds → reject;
        // with creds → accept; OpenClaw/Engine never blocked.
        assert_eq!(
            RunnerLoop::agent_auth_missing_rejection(AgentKind::MuseCode, false, false),
            Some(FailureReason::AgentAuthMissing)
        );
        assert_eq!(
            RunnerLoop::agent_auth_missing_rejection(AgentKind::MuseCode, true, false),
            None
        );
        assert_eq!(
            RunnerLoop::agent_auth_missing_rejection(AgentKind::Codex, false, false),
            Some(FailureReason::AgentAuthMissing)
        );
        assert_eq!(
            RunnerLoop::agent_auth_missing_rejection(AgentKind::ClaudeCode, false, false),
            Some(FailureReason::AgentAuthMissing)
        );
        assert_eq!(
            RunnerLoop::agent_auth_missing_rejection(AgentKind::CursorAgent, false, false),
            Some(FailureReason::AgentAuthMissing)
        );
        assert_eq!(
            RunnerLoop::agent_auth_missing_rejection(AgentKind::Grok, false, false),
            Some(FailureReason::AgentAuthMissing)
        );
        // Antigravity is gated by the NEWPIDASH-397 presence check (API key,
        // ADC, or OS-keychain item; an unanswered keychain counts as present,
        // so `has_credentials` is only false when a sign-in is provably absent).
        assert_eq!(
            RunnerLoop::agent_auth_missing_rejection(AgentKind::Antigravity, false, false),
            Some(FailureReason::AgentAuthMissing)
        );
        assert_eq!(
            RunnerLoop::agent_auth_missing_rejection(AgentKind::Antigravity, true, false),
            None
        );
        assert_eq!(
            RunnerLoop::agent_auth_missing_rejection(AgentKind::OpenClaw, false, false),
            None
        );
        assert_eq!(
            RunnerLoop::agent_auth_missing_rejection(AgentKind::PidashAgent, false, false),
            None
        );
        // Managed Codex (Pi Dash-managed auth) skips the presence check
        // even without user credentials, like Engine.
        assert_eq!(
            RunnerLoop::agent_auth_missing_rejection(AgentKind::Codex, false, true),
            None
        );
        assert_eq!(
            RunnerLoop::agent_auth_missing_rejection(AgentKind::Codex, true, true),
            None
        );
        // Backwards-compatible alias still works for Muse.
        assert_eq!(
            RunnerLoop::muse_assign_rejection(AgentKind::MuseCode, false),
            Some(FailureReason::AgentAuthMissing)
        );
    }

    #[test]
    fn muse_auth_missing_detail_names_runner_and_fix() {
        let detail = RunnerLoop::muse_auth_missing_detail("build-box");
        assert!(
            detail.contains("build-box"),
            "detail names runner: {detail}"
        );
        assert!(detail.contains("muse login"), "detail has fix: {detail}");
        assert!(detail.contains("META_API_KEY"), "detail has fix: {detail}");
        // Machine is included for the issue example format.
        assert!(detail.contains("machine"), "detail names machine: {detail}");
    }

    #[test]
    fn agent_auth_missing_detail_names_agent_runner_machine_and_fix() {
        let detail =
            RunnerLoop::agent_auth_missing_detail(AgentKind::ClaudeCode, "build-box", "my-machine");
        assert!(detail.contains("Claude Code"), "names agent: {detail}");
        assert!(detail.contains("build-box"), "names runner: {detail}");
        assert!(detail.contains("my-machine"), "names machine: {detail}");
        assert!(detail.contains("ANTHROPIC_API_KEY"), "names fix: {detail}");
    }
}
