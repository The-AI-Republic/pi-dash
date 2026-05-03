use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::{Mutex, watch};
use uuid::Uuid;

use crate::cloud::protocol::RunnerStatus;
use crate::config::schema::Config;
use crate::ipc::protocol::{CurrentRunSummary, RunnerStatusSnapshot};

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
    cloud_url: Mutex<String>,
    started_at: DateTime<Utc>,
    connected: Mutex<bool>,
    last_heartbeat: Mutex<Option<DateTime<Utc>>>,
    current_run: Mutex<Option<CurrentRunSummary>>,
    approvals_pending: Mutex<usize>,
    runner_id: Mutex<Option<Uuid>>,
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
        Self {
            inner: Arc::new(Inner {
                cfg: Mutex::new(cfg),
                name,
                project_slug,
                pod_id,
                cloud_url: Mutex::new(cloud_url),
                started_at: Utc::now(),
                connected: Mutex::new(false),
                last_heartbeat: Mutex::new(None),
                current_run: Mutex::new(None),
                approvals_pending: Mutex::new(0),
                runner_id: Mutex::new(None),
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
