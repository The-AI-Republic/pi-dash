use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::approval::router::ApprovalRecord;
use crate::cloud::protocol::{ApprovalDecision, RunnerStatus};
use crate::history::index::RunSummary;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    /// One-shot current-state snapshot.
    StatusGet,
    /// Subscribe to status deltas (stream until the connection closes).
    StatusSubscribe,
    /// Fetch the configuration (redacted of credentials).
    ConfigGet,
    /// Push a new config.
    ConfigUpdate { patch: serde_json::Value },
    /// Paginated runs list from the local index.
    RunsList { limit: Option<usize> },
    /// Single run + events (bounded).
    RunsGet { run_id: Uuid },
    /// Pending approvals snapshot.
    ApprovalsList,
    /// Decide an approval from the local surface.
    ApprovalsDecide {
        approval_id: String,
        decision: ApprovalDecision,
    },
    /// Doctor suite.
    DoctorRun,
    /// Force a WS reconnect.
    RunnerReconnect,
    /// Deregister + stop.
    RunnerDisconnect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", content = "data", rename_all = "snake_case")]
pub enum Response {
    Status(StatusSnapshot),
    Config(serde_json::Value),
    Runs(Vec<RunSummary>),
    Run {
        summary: RunSummary,
        events: Vec<serde_json::Value>,
    },
    Approvals(Vec<ApprovalRecord>),
    Doctor(crate::cli::doctor::Report),
    Ack,
    Error(RpcError),
    StatusDelta(StatusSnapshot),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub runner_name: String,
    pub runner_id: Option<Uuid>,
    pub status: RunnerStatus,
    pub connected: bool,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub current_run: Option<CurrentRunSummary>,
    pub approvals_pending: usize,
    pub cloud_url: String,
    pub uptime_secs: u64,
}

impl StatusSnapshot {
    pub fn print_compact(&self) {
        println!(
            "{} — {} {}",
            self.runner_name,
            if self.connected {
                "connected"
            } else {
                "disconnected"
            },
            self.cloud_url
        );
        if let Some(run) = &self.current_run {
            println!(
                "  current run: {} ({}); events={}",
                run.run_id, run.status, run.events
            );
        } else {
            println!("  idle");
        }
        println!("  approvals pending: {}", self.approvals_pending);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentRunSummary {
    pub run_id: Uuid,
    pub thread_id: Option<String>,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}
