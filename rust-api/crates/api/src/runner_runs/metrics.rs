//! Runner Prometheus metrics (D-15, stage 5, PIDASHCONV-542; MR-11
//! managed-runner series, PDASHOSS01-119).
//!
//! Ports `apps/api/pi_dash/runner/views/metrics.py`
//! (`GET /api/v1/runner/metrics/`, [`metrics`]): point-in-time series
//! rendered by hand in Prometheus text format — five gauges, the
//! managed-runner online gauge, the `pi_dash_runs_total` counter (one
//! sample per executor kind, zero-filled) and the managed queued-wait
//! histogram (design §15.1). AllowAny with no auth classes — the handler
//! never touches the session — and the response bypasses JSON rendering
//! (`text/plain; version=0.0.4`). Fully owned.
//!
//! Translation notes:
//!
//! * The histogram `_sum` is Python `sum(observations)`: `0` (an `int`)
//!   for no observations, otherwise a float in `repr()` form summed with
//!   CPython 3.12's compensated (Neumaier) float `sum` ([`py_sum`]) — a
//!   naive fold prints a different last digit.
//! * The ages are `(timezone.now() - created_at).total_seconds()` in the
//!   queryset's `Meta.ordering` (`-created_at`), so the summation order
//!   matches too.
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-08-handlers-web.golden.json`
//! (`metrics`, recorded from the MR-11 Python at `d6676db8`).

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::Response;

use chrono::{DateTime, Utc};
use pidash_db::runner_enroll::columns::enums::{
    RUNNER_PROVISIONING_DESKTOP_BUNDLED, RUNNER_STATUS_ONLINE,
};
use pidash_services::app_analytics::export_format::py_float_repr;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::AgentRunStatus;

use super::{json_response, pool_of, view_response, ALLOW_RUNNER_METRICS, SERVER_ERROR_BODY};
use crate::state::AppState;

/// Exact content type (`metrics.py:105`): scrapers require it.
pub const METRICS_CONTENT_TYPE: &str = "text/plain; version=0.0.4";

/// In-flight run statuses (`ACTIVE_RUN_STATUSES`, `metrics.py:40-46`).
/// Note `queued` is NOT active here.
pub const ACTIVE_RUN_STATUSES: [&str; 5] = [
    "assigned",
    "running",
    "cancel_requested",
    "awaiting_approval",
    "awaiting_reauth",
];

/// Upper edges (seconds) of the managed queued-wait histogram
/// (`QUEUED_WAIT_BUCKETS_SECONDS`): 1m, 5m, 15m, 1h, 3h, 6h, 12h. The last
/// finite edge is the default `MANAGED_RUNNER_QUEUED_MAX_AGE_SECS`.
pub const QUEUED_WAIT_BUCKETS_SECONDS: [i64; 7] = [60, 300, 900, 3600, 10800, 21600, 43200];

/// `AgentExecutorKind.values`, in declaration order: the zero-filled
/// sample order of `pi_dash_runs_total`.
pub const EXECUTOR_KINDS: [AgentExecutorKind; 3] = [
    AgentExecutorKind::LocalRunner,
    AgentExecutorKind::CloudAgent,
    AgentExecutorKind::ManagedRunner,
];

/// Runner status counts (`values_list("status").annotate(Count)`).
pub fn runner_status_counts_sql() -> String {
    r#"SELECT "runner"."status", COUNT("runner"."id") FROM "runner" GROUP BY "runner"."status""#
        .to_owned()
}

/// Online desktop-bundled runner count
/// (`Runner.objects.filter(provisioning=DESKTOP_BUNDLED, status=ONLINE)`).
/// Params `$1` `desktop_bundled`, `$2` `online`.
pub fn managed_online_count_sql() -> String {
    r#"SELECT COUNT(*) AS "__count" FROM "runner" WHERE ("runner"."provisioning" = $1 AND "runner"."status" = $2)"#
        .to_owned()
}

/// Runs per executor kind
/// (`values_list("executor_kind").annotate(c=Count("id"))`).
pub fn runs_by_kind_sql() -> String {
    r#"SELECT "agent_run"."executor_kind", COUNT("agent_run"."id") AS "c" FROM "agent_run" GROUP BY "agent_run"."executor_kind""#
        .to_owned()
}

/// `created_at` of every queued managed run, in `Meta.ordering`
/// (`-created_at`). Params `$1` `managed_runner`, `$2` `queued`.
pub fn queued_managed_created_sql() -> String {
    r#"SELECT "agent_run"."created_at" FROM "agent_run" WHERE ("agent_run"."executor_kind" = $1 AND "agent_run"."status" = $2) ORDER BY "agent_run"."created_at" DESC"#
        .to_owned()
}

/// In-flight run count. Params `$1..=$5` the [`ACTIVE_RUN_STATUSES`].
pub fn active_runs_count_sql() -> String {
    r#"SELECT COUNT(*) FROM "agent_run" WHERE "agent_run"."status" IN ($1, $2, $3, $4, $5)"#
        .to_owned()
}

/// Pending-approval count. Param `$1` `pending`.
pub fn pending_approvals_count_sql() -> String {
    r#"SELECT COUNT(*) FROM "agent_run_approval" WHERE "agent_run_approval"."status" = $1"#
        .to_owned()
}

/// One gauge block (`_gauge`, `metrics.py:49-54`).
pub fn gauge(name: &str, help: &str, value: i64) -> String {
    format!("# HELP {name} {help}\n# TYPE {name} gauge\n{name} {value}\n")
}

/// One counter block with a single `executor_kind` label per sample
/// (`_counter`).
pub fn counter(name: &str, help: &str, samples: &[(&str, i64)]) -> String {
    let mut out = format!("# HELP {name} {help}\n# TYPE {name} counter\n");
    for (label, value) in samples {
        out.push_str(&format!("{name}{{executor_kind=\"{label}\"}} {value}\n"));
    }
    out
}

/// CPython 3.12 `sum()` over floats, started from the `int` `0`: the
/// first item is taken exactly (`0 + x`), the rest accumulate with
/// Neumaier compensation, and a finite non-zero compensation is added
/// back once at the end (`Python/bltinmodule.c`, `builtin_sum_impl`).
/// `None` for no items — Python keeps the `int` `0`.
pub fn py_sum(values: &[f64]) -> Option<f64> {
    let (first, rest) = values.split_first()?;
    let mut total = *first;
    let mut compensation = 0.0_f64;
    for &x in rest {
        let t = total + x;
        if total.abs() >= x.abs() {
            compensation += (total - t) + x;
        } else {
            compensation += (x - t) + total;
        }
        total = t;
    }
    if compensation != 0.0 && compensation.is_finite() {
        total += compensation;
    }
    Some(total)
}

/// One cumulative-bucket histogram block (`_histogram`).
pub fn histogram(name: &str, help: &str, buckets: &[i64], observations: &[f64]) -> String {
    let mut out = format!("# HELP {name} {help}\n# TYPE {name} histogram\n");
    for edge in buckets {
        let count = observations
            .iter()
            .filter(|obs| **obs <= *edge as f64)
            .count();
        out.push_str(&format!("{name}_bucket{{le=\"{edge}\"}} {count}\n"));
    }
    let total = match py_sum(observations) {
        Some(total) => py_float_repr(total),
        None => "0".to_owned(),
    };
    out.push_str(&format!(
        "{name}_bucket{{le=\"+Inf\"}} {}\n",
        observations.len()
    ));
    out.push_str(&format!("{name}_sum {total}\n"));
    out.push_str(&format!("{name}_count {}\n", observations.len()));
    out
}

/// The DB-derived numbers one scrape renders.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MetricsSnapshot {
    pub online: i64,
    pub busy: i64,
    pub offline: i64,
    pub managed_online: i64,
    pub active_runs: i64,
    /// One count per [`EXECUTOR_KINDS`] entry, same order.
    pub runs_by_kind: [i64; 3],
    /// Ages (seconds) of queued managed runs, newest first.
    pub queued_wait_seconds: Vec<f64>,
    pub pending_approvals: i64,
}

/// The full body (`metrics.py` `get`), in source order.
pub fn metrics_body(snapshot: &MetricsSnapshot) -> String {
    let kind_samples: Vec<(&str, i64)> = EXECUTOR_KINDS
        .iter()
        .zip(snapshot.runs_by_kind)
        .map(|(kind, count)| (kind.value(), count))
        .collect();
    [
        gauge(
            "pi_dash_runner_online",
            "Runners currently online.",
            snapshot.online,
        ),
        gauge(
            "pi_dash_runner_busy",
            "Runners currently executing a run.",
            snapshot.busy,
        ),
        gauge(
            "pi_dash_runner_offline",
            "Runners that have dropped their heartbeat (excludes revoked).",
            snapshot.offline,
        ),
        gauge(
            "pi_dash_managed_runner_online",
            "Desktop-bundled (managed) runners currently online.",
            snapshot.managed_online,
        ),
        gauge(
            "pi_dash_runs_active",
            "AgentRuns in an in-flight status.",
            snapshot.active_runs,
        ),
        counter(
            "pi_dash_runs_total",
            "AgentRuns created, by executor kind.",
            &kind_samples,
        ),
        histogram(
            "pi_dash_managed_runner_queued_wait_seconds",
            "Age of managed runs currently queued waiting for a desktop.",
            &QUEUED_WAIT_BUCKETS_SECONDS,
            &snapshot.queued_wait_seconds,
        ),
        gauge(
            "pi_dash_approvals_pending",
            "ApprovalRequests waiting for a decision.",
            snapshot.pending_approvals,
        ),
    ]
    .concat()
}

/// `timedelta.total_seconds()`: whole microseconds over 10**6, one
/// correctly rounded division (exact below 2**53 µs).
pub fn total_seconds(now: DateTime<Utc>, created_at: DateTime<Utc>) -> f64 {
    let micros = (now - created_at).num_microseconds().unwrap_or(i64::MAX);
    micros as f64 / 1_000_000.0
}

/// `GET /api/v1/runner/metrics/`: every series as `text/plain`.
/// AllowAny — no session lookup at all (`authentication_classes = []`).
pub async fn metrics(State(state): State<AppState>) -> Response {
    view_response(metrics_inner(State(state)).await, ALLOW_RUNNER_METRICS)
}

async fn metrics_inner(State(state): State<AppState>) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let status_rows: Vec<(String, i64)> = match sqlx::query_as(&runner_status_counts_sql())
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(_) => return server_error(),
    };
    let mut online = 0;
    let mut busy = 0;
    let mut offline = 0;
    for (status, count) in status_rows {
        // Revoked runners are retired, not a health signal: counted in
        // their own group and ignored here (`metrics.py:14-15`).
        match status.as_str() {
            "online" => online = count,
            "busy" => busy = count,
            "offline" => offline = count,
            _ => {}
        }
    }
    let active_sql = active_runs_count_sql();
    let mut active_query = sqlx::query_scalar::<_, i64>(&active_sql);
    for status in ACTIVE_RUN_STATUSES {
        active_query = active_query.bind(status);
    }
    let active_runs: i64 = match active_query.fetch_one(pool).await {
        Ok(count) => count,
        Err(_) => return server_error(),
    };
    let pending_approvals: i64 = match sqlx::query_scalar::<_, i64>(&pending_approvals_count_sql())
        .bind(pidash_types::runner_runs::ApprovalStatus::Pending.value())
        .fetch_one(pool)
        .await
    {
        Ok(count) => count,
        Err(_) => return server_error(),
    };
    let managed_online: i64 = match sqlx::query_scalar::<_, i64>(&managed_online_count_sql())
        .bind(RUNNER_PROVISIONING_DESKTOP_BUNDLED)
        .bind(RUNNER_STATUS_ONLINE)
        .fetch_one(pool)
        .await
    {
        Ok(count) => count,
        Err(_) => return server_error(),
    };
    let kind_rows: Vec<(String, i64)> =
        match sqlx::query_as(&runs_by_kind_sql()).fetch_all(pool).await {
            Ok(rows) => rows,
            Err(_) => return server_error(),
        };
    let mut runs_by_kind = [0_i64; 3];
    for (kind, count) in kind_rows {
        // Unknown stored kinds are not emitted (`AgentExecutorKind.values`).
        if let Some(index) = EXECUTOR_KINDS.iter().position(|k| k.value() == kind) {
            runs_by_kind[index] = count;
        }
    }
    let now = Utc::now();
    let created: Vec<DateTime<Utc>> = match sqlx::query_scalar(&queued_managed_created_sql())
        .bind(AgentExecutorKind::ManagedRunner.value())
        .bind(AgentRunStatus::Queued.value())
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(_) => return server_error(),
    };
    let snapshot = MetricsSnapshot {
        online,
        busy,
        offline,
        managed_online,
        active_runs,
        runs_by_kind,
        queued_wait_seconds: created
            .into_iter()
            .map(|created_at| total_seconds(now, created_at))
            .collect(),
        pending_approvals,
    };
    let body = metrics_body(&snapshot);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, METRICS_CONTENT_TYPE)
        .body(axum::body::Body::from(body))
        .expect("metrics response")
}

/// 500 for transport failures (unreachable in `serve`).
fn server_error() -> Response {
    json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        SERVER_ERROR_BODY.to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_types::runner_runs::AgentRunStatus;
    use serde_json::Value;

    fn fx08() -> Value {
        let raw =
            include_str!("../../../../fixtures/runner_runs/fx-run-08-handlers-web.golden.json");
        serde_json::from_str(raw).expect("valid fixture")
    }

    #[test]
    fn body_matches_fx08_byte_for_byte() {
        let fx = fx08();
        // Fixture scenario: runners 2 online (1 manual + 1 desktop-bundled),
        // 1 busy, 2 offline (1 manual + 1 desktop-bundled); runs
        // local_runner running + queued, managed_runner running; no
        // approvals, so no queued managed run and an empty histogram.
        let body = metrics_body(&MetricsSnapshot {
            online: 2,
            busy: 1,
            offline: 2,
            managed_online: 1,
            active_runs: 2,
            runs_by_kind: [2, 0, 1],
            queued_wait_seconds: vec![],
            pending_approvals: 0,
        });
        assert_eq!(body, fx["metrics"]["body"].as_str().expect("body"));
        assert_eq!(
            METRICS_CONTENT_TYPE,
            fx["metrics"]["content_type"].as_str().expect("ct")
        );
        assert!(fx["metrics"]["auth_classes"]
            .as_array()
            .expect("auth")
            .is_empty());
        assert_eq!(
            fx["metrics"]["perm_classes"],
            serde_json::json!(["AllowAny"])
        );
    }

    #[test]
    fn histogram_matches_fx08_cases_byte_for_byte() {
        let fx = fx08();
        let cases = fx["metrics"]["histogram_cases"]
            .as_array()
            .expect("histogram cases");
        assert_eq!(cases.len(), 5);
        for case in cases {
            let observations: Vec<f64> = case["observations"]
                .as_array()
                .expect("observations")
                .iter()
                .map(|v| v.as_f64().expect("float"))
                .collect();
            assert_eq!(
                histogram(
                    "pi_dash_managed_runner_queued_wait_seconds",
                    "Age of managed runs currently queued waiting for a desktop.",
                    &QUEUED_WAIT_BUCKETS_SECONDS,
                    &observations,
                ),
                case["rendered"].as_str().expect("rendered"),
                "observations {observations:?}"
            );
        }
    }

    #[test]
    fn py_sum_is_compensated_like_cpython_312() {
        assert_eq!(py_sum(&[]), None);
        // A naive fold gives 0.9999999999999999 and 0.0 respectively.
        assert_eq!(py_sum(&[0.1; 10]), Some(1.0));
        assert_eq!(py_sum(&[1e16, 1.0, -1e16]), Some(1.0));
    }

    #[test]
    fn total_seconds_is_microsecond_exact() {
        let created = DateTime::parse_from_rfc3339("2026-10-21T00:00:00Z")
            .expect("ts")
            .with_timezone(&Utc);
        let now = created + chrono::Duration::microseconds(1_891_912_345);
        assert_eq!(total_seconds(now, created), 1891.912345);
    }

    #[test]
    fn executor_kinds_follow_text_choices_order() {
        let values: Vec<&str> = EXECUTOR_KINDS.iter().map(|k| k.value()).collect();
        assert_eq!(values, ["local_runner", "cloud_agent", "managed_runner"]);
    }

    #[test]
    fn active_statuses_match_source_tuple() {
        use AgentRunStatus::{
            Assigned, AwaitingApproval, AwaitingReauth, CancelRequested, Running,
        };
        assert_eq!(
            ACTIVE_RUN_STATUSES,
            [
                Assigned.value(),
                Running.value(),
                CancelRequested.value(),
                AwaitingApproval.value(),
                AwaitingReauth.value()
            ]
        );
    }
}
