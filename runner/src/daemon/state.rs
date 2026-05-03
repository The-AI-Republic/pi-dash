use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::{Mutex, watch};
use uuid::Uuid;

use crate::cloud::protocol::RunnerStatus;
use crate::config::schema::Config;
use crate::daemon::observability::TokenUsage;
use crate::ipc::protocol::{CurrentRunSummary, RunnerStatusSnapshot};

/// Volatile observability fields that ride on `PollStatus`. Doubles as
/// the in-memory storage shape (held under one `Mutex` inside `Inner`)
/// AND the wire-snapshot returned by `StateHandle::observability_snapshot()`.
/// Keeping them in one struct means `reset_run_snapshot()` is a single
/// `Default::default()` assignment and a new field added here is automatically
/// included in reset, snapshot read, and rid-change wipe — no enumeration to
/// keep in sync.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObservabilitySnapshot {
    pub last_event_at: Option<DateTime<Utc>>,
    pub last_event_kind: Option<String>,
    pub last_event_summary: Option<String>,
    pub agent_pid: Option<u32>,
    pub agent_subprocess_alive: Option<bool>,
    pub tokens: Option<TokenUsage>,
    pub turn_count: Option<u32>,
    /// Last shell command the agent kicked off (for failure-detail enrichment).
    /// Reset on rid change like the other per-run scalars; not serialised
    /// onto the wire snapshot — only consumed locally to enrich
    /// `RunFailed.detail` when the watchdog or stdout-close path fires.
    pub last_exec_command: Option<ExecCommandSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecCommandSnapshot {
    pub command: String,
    pub cwd: Option<String>,
    pub started_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct StateHandle {
    inner: Arc<Inner>,
    tx_tick: watch::Sender<u64>,
    pub rx_tick: watch::Receiver<u64>,
    pub tx_status: watch::Sender<RunnerStatus>,
    pub rx_status: watch::Receiver<RunnerStatus>,
    pub tx_in_flight: watch::Sender<Option<Uuid>>,
    pub rx_in_flight: watch::Receiver<Option<Uuid>>,
    pub tx_heartbeat_secs: watch::Sender<u64>,
    pub rx_heartbeat_secs: watch::Receiver<u64>,
    reconnect: Arc<tokio::sync::Notify>,
    shutdown: Arc<tokio::sync::Notify>,
}

struct Inner {
    cfg: Mutex<Config>,
    /// Cached from the first runner at construction time so
    /// ``runner_snapshot()`` doesn't lock ``cfg`` on every poll. Empty
    /// string when the connection has no runners yet.
    name: String,
    project_slug: Option<String>,
    pod_id: Option<Uuid>,
    /// Cached at construction; the daemon doesn't reload `agent_observability_v1`
    /// at runtime in v1, so reading from `Inner` saves a `Mutex<Config>` lock
    /// on every poll.
    agent_observability_v1: bool,
    cloud_url: Mutex<String>,
    started_at: DateTime<Utc>,
    connected: Mutex<bool>,
    last_heartbeat: Mutex<Option<DateTime<Utc>>>,
    current_run: Mutex<Option<CurrentRunSummary>>,
    approvals_pending: Mutex<usize>,
    runner_id: Mutex<Option<Uuid>>,
    /// Per-active-run observability snapshot. One `Mutex` over the whole
    /// struct (rather than seven `Mutex<Option<T>>` fields) so that:
    ///   1. `reset_run_snapshot()` is a single assignment — adding a
    ///      future field cannot silently leak across run boundaries.
    ///   2. `note_agent_event` stamps `last_event_at` / `last_event_kind`
    ///      / `last_event_summary` atomically — no concurrent reader can
    ///      see a partial update.
    ///   3. `observability_snapshot()` is one lock + one clone instead
    ///      of seven independent locks.
    /// Collectively wiped on rid change in `set_current_run` so a freshly
    /// assigned run never inherits stale metrics.
    run_snapshot: Mutex<ObservabilitySnapshot>,
}

impl StateHandle {
    pub fn new(cfg: Config) -> Self {
        let (tx_tick, rx_tick) = watch::channel(0u64);
        let (tx_status, rx_status) = watch::channel(RunnerStatus::Idle);
        let (tx_in_flight, rx_in_flight) = watch::channel(None);
        let (tx_heartbeat_secs, rx_heartbeat_secs) = watch::channel(25u64);
        let (name, project_slug, pod_id) = match cfg.primary_runner() {
            Some(r) => (r.name.clone(), r.project_slug.clone(), r.pod_id),
            None => (String::new(), None, None),
        };
        let cloud_url = cfg.daemon.cloud_url.clone();
        let agent_observability_v1 = cfg.daemon.agent_observability_v1;
        Self {
            inner: Arc::new(Inner {
                cfg: Mutex::new(cfg),
                name,
                project_slug,
                pod_id,
                agent_observability_v1,
                cloud_url: Mutex::new(cloud_url),
                started_at: Utc::now(),
                connected: Mutex::new(false),
                last_heartbeat: Mutex::new(None),
                current_run: Mutex::new(None),
                approvals_pending: Mutex::new(0),
                runner_id: Mutex::new(None),
                run_snapshot: Mutex::new(ObservabilitySnapshot::default()),
            }),
            tx_tick,
            rx_tick,
            tx_status,
            rx_status,
            tx_in_flight,
            rx_in_flight,
            tx_heartbeat_secs,
            rx_heartbeat_secs,
            reconnect: Arc::new(tokio::sync::Notify::new()),
            shutdown: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.rx_tick.clone()
    }

    /// Whether the runner should serialise the per-active-run observability
    /// snapshot on its `PollStatus`. Cached at construction.
    pub fn agent_observability_v1(&self) -> bool {
        self.inner.agent_observability_v1
    }

    pub fn force_reconnect(&self) {
        self.reconnect.notify_waiters();
    }

    pub fn reconnect_notified(&self) -> Arc<tokio::sync::Notify> {
        self.reconnect.clone()
    }

    pub fn shutdown(&self) {
        self.shutdown.notify_waiters();
    }

    pub fn shutdown_notified(&self) -> Arc<tokio::sync::Notify> {
        self.shutdown.clone()
    }

    fn tick(&self) {
        let prev = *self.tx_tick.borrow();
        let _ = self.tx_tick.send(prev.wrapping_add(1));
    }

    pub async fn set_connected(&self, v: bool) {
        *self.inner.connected.lock().await = v;
        self.tick();
    }

    pub async fn set_heartbeat(&self, ts: DateTime<Utc>) {
        *self.inner.last_heartbeat.lock().await = Some(ts);
        self.tick();
    }

    pub async fn set_current_run(&self, s: Option<CurrentRunSummary>) {
        let next = s.as_ref().map(|r| r.run_id);
        let prev = *self.tx_in_flight.borrow();
        // Clear the per-run observability snapshot only on a run-id
        // *change*. Re-stamping the same run id during startup
        // (supervisor early-stamp + worker re-stamp at supervisor.rs:803)
        // must not erase live values.
        // `set_current_run(None)` on a finished run does NOT reset; the
        // last-known values remain on the next poll until a new run starts.
        let must_reset = match (prev, next) {
            (Some(p), Some(n)) => p != n,
            (None, Some(_)) => true,
            _ => false,
        };
        if must_reset {
            self.reset_run_snapshot().await;
        }
        *self.inner.current_run.lock().await = s;
        let _ = self.tx_in_flight.send(next);
        let status = if next.is_some() {
            RunnerStatus::Busy
        } else {
            RunnerStatus::Idle
        };
        let _ = self.tx_status.send(status);
        self.tick();
    }

    /// Wipe the per-run observability snapshot. Called by
    /// `set_current_run` when the in-flight run id changes, and is safe
    /// to call directly from tests.
    pub async fn reset_run_snapshot(&self) {
        *self.inner.run_snapshot.lock().await = ObservabilitySnapshot::default();
        self.tick();
    }

    /// Bump `last_event_at` and stamp `kind` / `summary` atomically.
    /// Called from `pump_events` on every `BridgeEvent` regardless of variant.
    pub async fn note_agent_event(&self, ts: DateTime<Utc>, kind: String, summary: Option<String>) {
        let mut snap = self.inner.run_snapshot.lock().await;
        snap.last_event_at = Some(ts);
        snap.last_event_kind = Some(kind);
        if let Some(s) = summary {
            snap.last_event_summary = Some(s);
        }
        drop(snap);
        self.tick();
    }

    pub async fn set_agent_pid(&self, pid: Option<u32>) {
        self.inner.run_snapshot.lock().await.agent_pid = pid;
        self.tick();
    }

    pub async fn set_agent_alive(&self, alive: bool) {
        self.inner.run_snapshot.lock().await.agent_subprocess_alive = Some(alive);
        self.tick();
    }

    pub async fn set_tokens(&self, usage: TokenUsage) {
        self.inner.run_snapshot.lock().await.tokens = Some(usage);
        self.tick();
    }

    pub async fn incr_turn(&self) {
        let mut snap = self.inner.run_snapshot.lock().await;
        snap.turn_count = Some(snap.turn_count.unwrap_or(0).saturating_add(1));
        drop(snap);
        self.tick();
    }

    /// Stash the most recent shell command the agent kicked off, for use
    /// by the `RunFailed` enrichment helper. Called from
    /// `supervisor.handle_bridge_event` on `item/started` Raw frames whose
    /// item is a `commandExecution`.
    pub async fn note_exec_command(&self, snapshot: ExecCommandSnapshot) {
        self.inner.run_snapshot.lock().await.last_exec_command = Some(snapshot);
        self.tick();
    }

    /// Snapshot the volatile observability fields for one poll. One lock
    /// + one clone — adding a new field to `ObservabilitySnapshot`
    /// automatically participates without touching this method.
    pub async fn observability_snapshot(&self) -> ObservabilitySnapshot {
        self.inner.run_snapshot.lock().await.clone()
    }

    pub async fn incr_current_run_events(&self) {
        let mut guard = self.inner.current_run.lock().await;
        if let Some(run) = guard.as_mut() {
            run.events = run.events.saturating_add(1);
        }
        drop(guard);
        self.tick();
    }

    pub async fn set_approvals_pending(&self, n: usize) {
        *self.inner.approvals_pending.lock().await = n;
        self.tick();
    }

    /// Read the cached approvals-pending count for the wire snapshot.
    pub async fn approvals_pending_value(&self) -> usize {
        *self.inner.approvals_pending.lock().await
    }

    pub async fn set_config(&self, cfg: Config) {
        *self.inner.cfg.lock().await = cfg;
        self.tick();
    }

    pub async fn set_runner_id(&self, id: Uuid) {
        *self.inner.runner_id.lock().await = Some(id);
        self.tick();
    }

    pub async fn set_status(&self, s: RunnerStatus) {
        let _ = self.tx_status.send(s);
        self.tick();
    }

    /// Per-runner snapshot. The IPC server aggregates these across
    /// every configured `RunnerInstance` into the wire-level
    /// `StatusSnapshot { daemon, runners }`.
    pub async fn runner_snapshot(&self) -> RunnerStatusSnapshot {
        let status = { *self.rx_status.borrow() };
        let runner_id = *self.inner.runner_id.lock().await;
        let last_heartbeat = *self.inner.last_heartbeat.lock().await;
        let current_run = self.inner.current_run.lock().await.clone();
        let approvals_pending = *self.inner.approvals_pending.lock().await;
        RunnerStatusSnapshot {
            runner_id: runner_id.unwrap_or_else(Uuid::nil),
            name: self.inner.name.clone(),
            project_slug: self.inner.project_slug.clone(),
            pod_id: self.inner.pod_id,
            status,
            current_run,
            approvals_pending,
            last_heartbeat,
        }
    }

    /// Connection-level facts shared across every runner the daemon
    /// hosts.
    pub async fn daemon_info(&self) -> crate::ipc::protocol::DaemonInfo {
        let uptime = (Utc::now() - self.inner.started_at).num_seconds().max(0) as u64;
        let cloud_url = self.inner.cloud_url.lock().await.clone();
        let connected = *self.inner.connected.lock().await;
        crate::ipc::protocol::DaemonInfo {
            cloud_url,
            connected,
            uptime_secs: uptime,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_state() -> StateHandle {
        StateHandle::new(Config {
            version: 2,
            daemon: Default::default(),
            runners: vec![],
        })
    }

    fn summary(run_id: Uuid, status: &str) -> CurrentRunSummary {
        CurrentRunSummary {
            run_id,
            thread_id: None,
            status: status.to_string(),
            started_at: Utc::now(),
            events: 0,
        }
    }

    #[tokio::test]
    async fn set_current_run_propagates_to_rx_in_flight() {
        let state = empty_state();
        assert_eq!(*state.rx_in_flight.borrow(), None);

        let rid = Uuid::new_v4();
        state.set_current_run(Some(summary(rid, "preparing"))).await;
        assert_eq!(*state.rx_in_flight.borrow(), Some(rid));

        state.set_current_run(None).await;
        assert_eq!(*state.rx_in_flight.borrow(), None);
    }

    #[tokio::test]
    async fn re_stamping_same_run_id_preserves_observability_snapshot() {
        // Supervisor stamps Some(rid) twice (early + worker re-stamp at
        // supervisor.rs:803). Live observability fields populated between
        // those two calls must survive the re-stamp.
        let state = empty_state();
        let rid = Uuid::new_v4();

        state.set_current_run(Some(summary(rid, "preparing"))).await;
        state.set_agent_pid(Some(4242)).await;
        state.set_agent_alive(true).await;
        state
            .note_agent_event(Utc::now(), "raw".into(), Some("test".into()))
            .await;

        // Same rid → must NOT call reset_run_snapshot.
        state.set_current_run(Some(summary(rid, "starting"))).await;
        let snap = state.observability_snapshot().await;
        assert_eq!(snap.agent_pid, Some(4242));
        assert_eq!(snap.agent_subprocess_alive, Some(true));
        assert!(snap.last_event_at.is_some());
        assert_eq!(snap.last_event_kind.as_deref(), Some("raw"));
    }

    #[tokio::test]
    async fn run_id_change_resets_observability_snapshot() {
        let state = empty_state();
        let rid_a = Uuid::new_v4();
        let rid_b = Uuid::new_v4();

        state.set_current_run(Some(summary(rid_a, "running"))).await;
        state.set_agent_pid(Some(1111)).await;
        state.set_agent_alive(true).await;
        state
            .set_tokens(TokenUsage {
                input: 1,
                output: 2,
                total: 3,
            })
            .await;
        state.incr_turn().await;
        state
            .note_agent_event(Utc::now(), "raw".into(), Some("a".into()))
            .await;

        state.set_current_run(Some(summary(rid_b, "running"))).await;
        let snap = state.observability_snapshot().await;
        // Whole-struct equality with Default. Any *future* field added to
        // ObservabilitySnapshot is automatically covered — a new field
        // can't silently leak across run boundaries without flipping this
        // test red.
        assert_eq!(
            snap,
            ObservabilitySnapshot::default(),
            "snapshot leaked across run boundary"
        );
    }

    #[tokio::test]
    async fn note_exec_command_lands_on_snapshot_and_clears_on_rid_change() {
        let state = empty_state();
        let rid_a = Uuid::new_v4();
        state.set_current_run(Some(summary(rid_a, "running"))).await;
        state
            .note_exec_command(ExecCommandSnapshot {
                command: "git fetch origin".into(),
                cwd: Some("/tmp/x".into()),
                started_at: Utc::now(),
            })
            .await;
        let snap = state.observability_snapshot().await;
        let cmd = snap.last_exec_command.expect("snapshot lost the command");
        assert_eq!(cmd.command, "git fetch origin");
        assert_eq!(cmd.cwd.as_deref(), Some("/tmp/x"));

        // Rid change wipes per-run scalars including the exec command, so
        // a stalled-on-foo failure detail can't bleed into a fresh run.
        let rid_b = Uuid::new_v4();
        state.set_current_run(Some(summary(rid_b, "running"))).await;
        let snap = state.observability_snapshot().await;
        assert!(snap.last_exec_command.is_none());
    }

    #[tokio::test]
    async fn set_current_run_none_preserves_terminal_snapshot() {
        // After a run completes, we keep the last-known scalars on the
        // wire so the cloud can render the terminal state until a new run
        // arrives. Reset happens only on the next idle→busy transition.
        let state = empty_state();
        let rid = Uuid::new_v4();

        state.set_current_run(Some(summary(rid, "running"))).await;
        state.set_agent_pid(Some(7777)).await;

        state.set_current_run(None).await;
        let snap = state.observability_snapshot().await;
        assert_eq!(snap.agent_pid, Some(7777));
    }

    #[tokio::test]
    async fn note_agent_event_atomically_stamps_three_fields() {
        let state = empty_state();
        let now = Utc::now();
        state
            .note_agent_event(now, "tool/exec".into(), Some("running".into()))
            .await;
        let snap = state.observability_snapshot().await;
        assert_eq!(snap.last_event_at, Some(now));
        assert_eq!(snap.last_event_kind.as_deref(), Some("tool/exec"));
        assert_eq!(snap.last_event_summary.as_deref(), Some("running"));
    }

    #[tokio::test]
    async fn incr_turn_increments_from_none() {
        let state = empty_state();
        state.incr_turn().await;
        assert_eq!(state.observability_snapshot().await.turn_count, Some(1));
        state.incr_turn().await;
        assert_eq!(state.observability_snapshot().await.turn_count, Some(2));
    }

    #[tokio::test]
    async fn re_stamping_same_run_id_does_not_flicker_through_none() {
        // Supervisor stamps `Some(rid)` early on Assign; the worker
        // re-stamps with a richer summary ~30s later. The watch must not
        // toggle through None in between, otherwise a session-open in
        // that window would Hello with `in_flight_run=null` and the
        // cloud reaper would kill the run.
        let state = empty_state();
        let rid = Uuid::new_v4();

        state.set_current_run(Some(summary(rid, "preparing"))).await;
        let mut rx = state.rx_in_flight.clone();
        assert_eq!(*rx.borrow(), Some(rid));

        state.set_current_run(Some(summary(rid, "starting"))).await;
        // After the second stamp, the value is still `Some(rid)`. The
        // watch tx doesn't suppress same-value sends — we only assert
        // the visible state never went through None, by checking the
        // borrow is still Some(rid).
        assert_eq!(*rx.borrow(), Some(rid));

        // And rx_status stays Busy across the re-stamp.
        assert!(matches!(*state.rx_status.borrow(), RunnerStatus::Busy));

        // Drain any backlog and confirm no None ever showed up.
        while rx.has_changed().unwrap_or(false) {
            let v = *rx.borrow_and_update();
            assert_eq!(v, Some(rid), "rx_in_flight transitioned to None mid-stream");
        }
    }
}
