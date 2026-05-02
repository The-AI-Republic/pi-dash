use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::approval::router::ApprovalRecord;
use crate::cloud::protocol::{ApprovalDecision, RunnerStatus};
use crate::history::index::RunSummary;

/// IPC wire version. Bumped on incompatible shape changes between
/// `pidash` (CLI/TUI) and the in-process daemon. Client and daemon
/// ship in the same binary, so this is mostly a guard against running
/// a stale `pidash` binary against a freshly-restarted daemon during
/// dev (out-of-tree mismatch surfaces as a clean error rather than a
/// confusing serde failure).
///
/// v2 reshaped `StatusSnapshot` from a single-runner record into
/// `{ daemon, runners: Vec<RunnerStatusSnapshot> }` and added the
/// `runner` selector to every per-runner request variant.
pub const IPC_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    /// One-shot current-state snapshot.
    StatusGet,
    /// Subscribe to status deltas (stream until the connection closes).
    StatusSubscribe,
    /// Fetch the configuration (redacted of credentials).
    ConfigGet,
    /// Push a new config. The current daemon deep-merges `patch` into
    /// the on-disk config and reloads the whole thing, so `runner` is
    /// **reserved / informational** — it has no effect today. The TUI's
    /// only caller writes the full `config_working`, which is fine
    /// against this implementation. The field is kept on the wire so
    /// the future per-runner live-patch flow can opt-in without a
    /// breaking IPC bump.
    ConfigUpdate {
        patch: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runner: Option<String>,
    },
    /// Paginated runs list from the local index. ``runner`` filters to
    /// one runner's history. When omitted on a multi-runner daemon,
    /// returns the union (callers can disambiguate by `runner_id` on
    /// each `RunSummary`).
    RunsList {
        limit: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runner: Option<String>,
    },
    /// Single run + events (bounded). Run IDs are globally unique so
    /// ``runner`` is optional; when omitted the daemon scans every
    /// instance's history.
    RunsGet {
        run_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runner: Option<String>,
    },
    /// Pending approvals snapshot. ``runner`` filters to one
    /// instance's approvals; omitted = union across all runners.
    ApprovalsList {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runner: Option<String>,
    },
    /// Decide an approval from the local surface. ``runner`` is
    /// required when N>1 because approval IDs are minted per-runner
    /// and the daemon needs to know which mailbox to dispatch the
    /// decision to.
    ApprovalsDecide {
        approval_id: String,
        decision: ApprovalDecision,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runner: Option<String>,
    },
    /// Doctor suite. ``runner`` runs the per-runner checks against one
    /// configured runner; omitted = walk every runner.
    DoctorRun {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runner: Option<String>,
    },
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

/// Connection-level state shared across every runner the daemon hosts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub cloud_url: String,
    pub connected: bool,
    pub uptime_secs: u64,
}

/// Per-runner snapshot. The daemon emits one of these per configured
/// `[[runner]]` block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerStatusSnapshot {
    pub runner_id: Uuid,
    pub name: String,
    /// Project identifier this runner serves. `Option` purely for
    /// back-compat with configs written before the project refactor;
    /// new registrations always populate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_slug: Option<String>,
    /// Pod the cloud assigned at registration. Informational; useful
    /// in `pidash status` to show the routing target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod_id: Option<Uuid>,
    pub status: RunnerStatus,
    pub current_run: Option<CurrentRunSummary>,
    pub approvals_pending: usize,
    pub last_heartbeat: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub daemon: DaemonInfo,
    pub runners: Vec<RunnerStatusSnapshot>,
}

impl StatusSnapshot {
    /// Compact human-readable summary used by `pidash status`.
    pub fn print_compact(&self) {
        println!(
            "daemon — {} {}",
            if self.daemon.connected {
                "connected"
            } else {
                "disconnected"
            },
            self.daemon.cloud_url
        );
        if self.runners.is_empty() {
            println!("  no runners configured");
            return;
        }
        for r in &self.runners {
            r.print_compact();
        }
    }

    /// Locate a runner snapshot by name. Returns `None` if no runner
    /// with that name is configured.
    pub fn runner_by_name(&self, name: &str) -> Option<&RunnerStatusSnapshot> {
        self.runners.iter().find(|r| r.name == name)
    }
}

impl RunnerStatusSnapshot {
    pub fn print_compact(&self) {
        let project = self.project_slug.as_deref().unwrap_or("(no project)");
        println!("  {} — {:?} project={}", self.name, self.status, project);
        if let Some(run) = &self.current_run {
            println!(
                "    current run: {} ({}); events={}",
                run.run_id, run.status, run.events
            );
        } else {
            println!("    idle");
        }
        if self.approvals_pending > 0 {
            println!("    approvals pending: {}", self.approvals_pending);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_snapshot_roundtrips() {
        let snap = StatusSnapshot {
            daemon: DaemonInfo {
                cloud_url: "https://x".into(),
                connected: true,
                uptime_secs: 42,
            },
            runners: vec![RunnerStatusSnapshot {
                runner_id: Uuid::new_v4(),
                name: "laptop-main".into(),
                project_slug: Some("WEB".into()),
                pod_id: Some(Uuid::new_v4()),
                status: RunnerStatus::Idle,
                current_run: None,
                approvals_pending: 0,
                last_heartbeat: Some(Utc::now()),
            }],
        };
        let s = serde_json::to_string(&snap).unwrap();
        let back: StatusSnapshot = serde_json::from_str(&s).unwrap();
        assert_eq!(back.runners.len(), 1);
        assert_eq!(back.runners[0].name, "laptop-main");
        assert_eq!(back.runners[0].project_slug.as_deref(), Some("WEB"));
        assert!(back.daemon.connected);
    }

    #[test]
    fn runs_list_request_with_runner_selector_roundtrips() {
        let req = Request::RunsList {
            limit: Some(50),
            runner: Some("laptop-side".into()),
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: Request = serde_json::from_str(&s).unwrap();
        match back {
            Request::RunsList { limit, runner } => {
                assert_eq!(limit, Some(50));
                assert_eq!(runner.as_deref(), Some("laptop-side"));
            }
            other => panic!("expected RunsList, got {other:?}"),
        }
    }

    #[test]
    fn runs_list_request_without_runner_serializes_without_field() {
        let req = Request::RunsList {
            limit: None,
            runner: None,
        };
        let s = serde_json::to_string(&req).unwrap();
        // Field is `skip_serializing_if = Option::is_none`, so absent
        // selectors don't bloat the wire (also forward-compatible with
        // older daemons that didn't know about it).
        assert!(!s.contains("\"runner\""), "unexpected runner field: {s}");
    }

    #[test]
    fn runner_by_name_finds_match() {
        let snap = StatusSnapshot {
            daemon: DaemonInfo {
                cloud_url: "https://x".into(),
                connected: false,
                uptime_secs: 0,
            },
            runners: vec![
                RunnerStatusSnapshot {
                    runner_id: Uuid::new_v4(),
                    name: "a".into(),
                    project_slug: None,
                    pod_id: None,
                    status: RunnerStatus::Idle,
                    current_run: None,
                    approvals_pending: 0,
                    last_heartbeat: None,
                },
                RunnerStatusSnapshot {
                    runner_id: Uuid::new_v4(),
                    name: "b".into(),
                    project_slug: None,
                    pod_id: None,
                    status: RunnerStatus::Idle,
                    current_run: None,
                    approvals_pending: 0,
                    last_heartbeat: None,
                },
            ],
        };
        assert!(snap.runner_by_name("a").is_some());
        assert!(snap.runner_by_name("b").is_some());
        assert!(snap.runner_by_name("c").is_none());
    }
}
