//! Runner Prometheus metrics (D-15, stage 5, PIDASHCONV-542).
//!
//! Ports `apps/api/pi_dash/runner/views/metrics.py:1-105`
//! (`GET /api/v1/runner/metrics/`, [`metrics`]): five point-in-time
//! gauges rendered by hand in Prometheus text format. AllowAny with no
//! auth classes — the handler never touches the session — and the
//! response bypasses JSON rendering (`text/plain; version=0.0.4`).
//! Fully owned.

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::Response;

use super::{json_response, pool_of, SERVER_ERROR_BODY};
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

/// Runner status counts (`values_list("status").annotate(Count)`).
pub fn runner_status_counts_sql() -> String {
    r#"SELECT "runner"."status", COUNT("runner"."id") FROM "runner" GROUP BY "runner"."status""#
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

/// The five-gauge body (`metrics.py:78-104`), in source order.
pub fn metrics_body(
    online: i64,
    busy: i64,
    offline: i64,
    active_runs: i64,
    pending_approvals: i64,
) -> String {
    [
        gauge("pi_dash_runner_online", "Runners currently online.", online),
        gauge(
            "pi_dash_runner_busy",
            "Runners currently executing a run.",
            busy,
        ),
        gauge(
            "pi_dash_runner_offline",
            "Runners that have dropped their heartbeat (excludes revoked).",
            offline,
        ),
        gauge(
            "pi_dash_runs_active",
            "AgentRuns in an in-flight status.",
            active_runs,
        ),
        gauge(
            "pi_dash_approvals_pending",
            "ApprovalRequests waiting for a decision.",
            pending_approvals,
        ),
    ]
    .concat()
}

/// `GET /api/v1/runner/metrics/`: the five gauges as `text/plain`.
/// AllowAny — no session lookup at all (`authentication_classes = []`).
pub async fn metrics(State(state): State<AppState>) -> Response {
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
    let body = metrics_body(online, busy, offline, active_runs, pending_approvals);
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
        // Fixture rows: 1 online, 1 busy, 2 offline, 8 active, 2 pending.
        let body = metrics_body(1, 1, 2, 8, 2);
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
