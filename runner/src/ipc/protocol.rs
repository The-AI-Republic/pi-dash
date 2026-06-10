use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::approval::router::ApprovalRecord;
use crate::cloud::protocol::{ApprovalDecision, RunnerStatus};
use crate::daemon::state::ObservabilitySnapshot;
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
///
/// v3 added the optional `observability` field to
/// `RunnerStatusSnapshot` so the TUI can surface per-active-run
/// telemetry (turn count, tokens, agent pid, last event) that the
/// daemon already collects under `agent_observability_v1`. The same
/// bump also added `RunnerRemoveLocal`, the CLI's "tell the daemon to
/// clean up this runner without going through the cloud" verb. Older
/// daemons reject the variant with an `unknown method` error; the CLI
/// catches that and falls back to direct config mutation, so mixed
/// daemon/CLI pairs during dev stay safe.
pub const IPC_VERSION: u32 = 3;

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
    /// Local-only runner removal (cloud is **not** contacted).
    ///
    /// Used by ``pidash runner remove`` when the user passed
    /// ``--local-only`` or when the cloud is unreachable. The daemon
    /// runs the same teardown the cloud's ``remove_runner`` wire frame
    /// would trigger: cancel any in-flight run, drop the per-runner
    /// data dir, strip the matching ``[[runner]]`` block from
    /// ``config.toml`` under the host-wide config lock, and exit just
    /// this runner's RunnerLoop. The shared ``pidash.service`` systemd
    /// unit is deliberately not touched.
    ///
    /// Without this verb, the CLI's offline / local-only paths edited
    /// ``config.toml`` directly while the daemon kept polling against
    /// its in-memory copy and re-creating the data dir, until the
    /// operator manually restarted the service.
    RunnerRemoveLocal { runner: String },
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
    /// Update advisory. Populated once a runner's welcome frame carries
    /// a `latest_runner_version` and/or `min_runner_version` from the
    /// cloud. `None` means either the daemon hasn't completed its first
    /// session bootstrap, or the cloud isn't announcing any advisory.
    /// `#[serde(default)]` keeps old `pidash status` clients parsing a
    /// newer daemon's Status without serde failures during dev.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<UpdateAdvisory>,
}

/// Version advisory surfaced via `pidash status` and the TUI. Pure data
/// — the daemon's update orchestration (auto-swap on disk, restart-to-
/// apply hints) consumes the same fields but is implemented separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAdvisory {
    /// Version this daemon process is running (compile-time
    /// `CARGO_PKG_VERSION`). After an auto-swap the on-disk binary may
    /// be ahead — that's what `on_disk_version` is for.
    pub running_version: String,
    /// Version of `~/.local/bin/pidash` on disk. `None` until the
    /// daemon has reason to believe it differs from `running_version`
    /// (e.g. a successful auto-swap completed). Equal to
    /// `running_version` for fresh processes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_disk_version: Option<String>,
    /// Latest version the cloud has announced in the welcome frame.
    /// `None` if the cloud isn't announcing one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_announced: Option<String>,
    /// Minimum acceptable version the cloud is advertising. Advisory
    /// only today — the daemon does not refuse work below this floor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_required: Option<String>,
    /// Whether the user has the auto-update toggle enabled. Surfaced
    /// here so the TUI's banner can phrase itself appropriately
    /// ("restart to apply" vs. "update available — run pidash update --restart").
    /// `#[serde(default)]` defaults to `false` for older daemons that
    /// didn't populate it.
    #[serde(default)]
    pub auto_update_enabled: bool,
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
    /// Per-active-run telemetry. `None` when the daemon isn't running
    /// with `agent_observability_v1` enabled, or when the runner has
    /// never seen a run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observability: Option<ObservabilitySnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub daemon: DaemonInfo,
    pub runners: Vec<RunnerStatusSnapshot>,
    /// Worktree pools, one per configured `[[workdir]]`. Empty for daemons
    /// with no work dirs (legacy single-dir runners). `#[serde(default)]` so
    /// an older client/daemon pairing still decodes the snapshot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pools: Vec<crate::workspace::pool::PoolSnapshot>,
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
        if let Some(advisory) = &self.daemon.update {
            advisory.print_compact();
        }
        if self.runners.is_empty() {
            println!("  no runners configured");
            return;
        }
        for r in &self.runners {
            r.print_compact();
        }
        for p in &self.pools {
            let health = if p.healthy {
                String::new()
            } else {
                format!(
                    " UNHEALTHY: {}",
                    p.unhealthy_reason.as_deref().unwrap_or("(unknown)")
                )
            };
            println!(
                "  workdir {} — {}/{} desks busy, {} queued{}",
                p.workdir_name,
                p.busy,
                p.pool_size,
                p.queue.len(),
                health
            );
        }
    }

    /// Locate a runner snapshot by name. Returns `None` if no runner
    /// with that name is configured.
    pub fn runner_by_name(&self, name: &str) -> Option<&RunnerStatusSnapshot> {
        self.runners.iter().find(|r| r.name == name)
    }
}

impl UpdateAdvisory {
    /// One-line print used by `pidash status`. Mirrors the four-state
    /// matrix the TUI's Connection card renders (see `runner/README.md`
    /// → Auto-update). Stays a single line per state so callers like
    /// `pidash status` keep their compact two-line summary.
    pub fn print_compact(&self) {
        let running = self.running_version.as_str();
        if let Some(min) = self.min_required.as_deref()
            && version_lt(running, min)
        {
            let on_disk = self.on_disk_version.as_deref().unwrap_or(running);
            if !version_lt(on_disk, min) {
                println!("  update: REQUIRED — cloud floor v{min}; restart to apply");
            } else if self.auto_update_enabled {
                println!("  update: REQUIRED — cloud floor v{min}; swap pending");
            } else {
                println!("  update: REQUIRED — cloud floor v{min}; run `pidash update --restart`");
            }
            return;
        }
        if let Some(latest) = self.latest_announced.as_deref()
            && version_lt(running, latest)
        {
            let on_disk = self.on_disk_version.as_deref().unwrap_or(running);
            if on_disk == latest {
                println!("  update: restart to apply v{latest} (running v{running})");
            } else if self.auto_update_enabled {
                println!("  update: v{latest} pending swap (running v{running})");
            } else {
                println!("  update: v{latest} available — run `pidash update --restart`");
            }
        }
    }
}

/// Numeric-triple semver compare with one bit of prerelease handling:
/// when the `(major, minor, patch)` triples are equal, a version with
/// a prerelease suffix ranks *below* one without (SemVer §11.4.1). So
/// `0.1.2-rc.1 < 0.1.2` is `true`, matching the SemVer spec.
///
/// We deliberately do *not* compare two prerelease identifiers against
/// each other (`rc.1` vs `beta.2`) — both render as "less than the GA"
/// for the same triple, which is enough to surface "you should upgrade
/// to the GA" but not enough to differentiate prerelease channels. If
/// the cloud announces a prerelease as `LATEST_RUNNER_VERSION`, stable
/// users will still see it as newer (numeric triple `0.1.3 > 0.1.2`);
/// guard that policy on the Django side rather than here.
///
/// Any non-numeric segment or parse error returns `false` so the caller
/// doesn't surface an unhelpful "update required" banner from a version
/// string we don't understand. Pub(crate) because the daemon's
/// auto-swap gate uses the same compare to decide whether to swap.
pub(crate) fn version_lt(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> Option<((u32, u32, u32), bool)> {
        let (core, has_pre) = match v.split_once('-') {
            Some((c, _)) => (c, true),
            None => (v, false),
        };
        let mut it = core.split('.');
        let major = it.next()?.parse().ok()?;
        let minor = it.next()?.parse().ok()?;
        let patch = it.next()?.parse().ok()?;
        Some(((major, minor, patch), has_pre))
    }
    match (parts(a), parts(b)) {
        (Some((ta, pa)), Some((tb, pb))) => {
            if ta != tb {
                return ta < tb;
            }
            // Equal triple: prerelease is "less than" no-prerelease.
            matches!((pa, pb), (true, false))
        }
        _ => false,
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
    fn version_lt_handles_numeric_triples() {
        assert!(version_lt("0.1.2", "0.1.3"));
        assert!(version_lt("0.1.2", "0.2.0"));
        assert!(version_lt("0.1.2", "1.0.0"));
        assert!(!version_lt("0.1.2", "0.1.2"));
        assert!(!version_lt("0.1.3", "0.1.2"));
    }

    #[test]
    fn version_lt_prerelease_ranks_below_ga_at_same_triple() {
        // SemVer §11.4.1: a version with a prerelease suffix ranks
        // below the same triple without one.
        assert!(version_lt("0.1.2-rc.1", "0.1.2"));
        assert!(!version_lt("0.1.2", "0.1.2-rc.1"));
        // Two prereleases on the same triple compare equal in our
        // crude model — neither is "less than" the other. Good enough
        // until we wire in the `semver` crate.
        assert!(!version_lt("0.1.2-rc.1", "0.1.2-rc.2"));
        // Cross-triple comparison still uses the numeric ordering,
        // so a stable user sees a newer-triple prerelease as newer.
        // This is policy: the Django side controls what gets
        // announced as `LATEST_RUNNER_VERSION`.
        assert!(version_lt("0.1.2", "0.1.3-rc.1"));
    }

    #[test]
    fn version_lt_returns_false_on_unparseable_input() {
        assert!(!version_lt("nope", "0.1.2"));
        assert!(!version_lt("0.1.2", "nope"));
        assert!(!version_lt("0.1", "0.1.2"));
    }

    #[test]
    fn status_snapshot_roundtrips() {
        let snap = StatusSnapshot {
            daemon: DaemonInfo {
                cloud_url: "https://x".into(),
                connected: true,
                uptime_secs: 42,
                update: None,
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
                observability: None,
            }],
            pools: vec![],
        };
        let s = serde_json::to_string(&snap).unwrap();
        let back: StatusSnapshot = serde_json::from_str(&s).unwrap();
        assert_eq!(back.runners.len(), 1);
        assert_eq!(back.runners[0].name, "laptop-main");
        assert_eq!(back.runners[0].project_slug.as_deref(), Some("WEB"));
        assert!(back.daemon.connected);
    }

    #[test]
    fn observability_field_roundtrips() {
        use crate::daemon::observability::TokenUsage;
        use crate::daemon::state::ObservabilitySnapshot;
        let snap = StatusSnapshot {
            daemon: DaemonInfo {
                cloud_url: "https://x".into(),
                connected: true,
                uptime_secs: 1,
                update: None,
            },
            runners: vec![RunnerStatusSnapshot {
                runner_id: Uuid::new_v4(),
                name: "obs".into(),
                project_slug: None,
                pod_id: None,
                status: RunnerStatus::Busy,
                current_run: None,
                approvals_pending: 0,
                last_heartbeat: None,
                observability: Some(ObservabilitySnapshot {
                    last_event_at: Some(Utc::now()),
                    last_event_kind: Some("raw".into()),
                    last_event_summary: Some("turn started".into()),
                    agent_pid: Some(12345),
                    agent_subprocess_alive: Some(true),
                    tokens: Some(TokenUsage {
                        input: 10,
                        output: 20,
                        total: 30,
                    }),
                    model: Some("gpt-5.1-codex".into()),
                    turn_count: Some(2),
                    last_exec_command: None,
                }),
            }],
            pools: vec![],
        };
        let s = serde_json::to_string(&snap).unwrap();
        let back: StatusSnapshot = serde_json::from_str(&s).unwrap();
        let obs = back.runners[0]
            .observability
            .as_ref()
            .expect("observability field lost on roundtrip");
        assert_eq!(obs.turn_count, Some(2));
        assert_eq!(obs.agent_pid, Some(12345));
        assert_eq!(obs.tokens.map(|t| t.total), Some(30));
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
                update: None,
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
                    observability: None,
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
                    observability: None,
                },
            ],
            pools: vec![],
        };
        assert!(snap.runner_by_name("a").is_some());
        assert!(snap.runner_by_name("b").is_some());
        assert!(snap.runner_by_name("c").is_none());
    }
}
