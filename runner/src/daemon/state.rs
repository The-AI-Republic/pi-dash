use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::{Mutex, watch};
use uuid::Uuid;

use crate::cloud::protocol::RunnerStatus;
use crate::config::schema::Config;
use crate::ipc::protocol::{CurrentRunSummary, StatusSnapshot};

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
    name: String,
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
        let name = cfg.primary_runner().name.clone();
        let cloud_url = cfg.daemon.cloud_url.clone();
        Self {
            inner: Arc::new(Inner {
                cfg: Mutex::new(cfg),
                name,
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

    pub async fn snapshot(&self) -> StatusSnapshot {
        let uptime = (Utc::now() - self.inner.started_at).num_seconds().max(0) as u64;
        let status = { *self.rx_status.borrow() };
        let cloud_url = self.inner.cloud_url.lock().await.clone();
        let runner_id = *self.inner.runner_id.lock().await;
        let connected = *self.inner.connected.lock().await;
        let last_heartbeat = *self.inner.last_heartbeat.lock().await;
        let current_run = self.inner.current_run.lock().await.clone();
        let approvals_pending = *self.inner.approvals_pending.lock().await;
        StatusSnapshot {
            runner_name: self.inner.name.clone(),
            runner_id,
            status,
            connected,
            last_heartbeat,
            current_run,
            approvals_pending,
            cloud_url,
            uptime_secs: uptime,
        }
    }
}
