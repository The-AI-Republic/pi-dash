//! Runner Prometheus metrics (D-15, stage 5, PIDASHCONV-542).
//!
//! Ports `apps/api/pi_dash/runner/views/metrics.py:1-105`
//! (`GET /api/v1/runner/metrics/`, [`metrics`]): six point-in-time
//! gauges rendered by hand in Prometheus text format. AllowAny with no
//! auth classes — the handler never touches the session — and the
//! response bypasses JSON rendering (`text/plain; version=0.0.4`).
//! Fully owned.

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::Response;

use super::{pool_of, view_response, ALLOW_RUNNER_METRICS};
use crate::state::AppState;
use pidash_compat::response::server_error;

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

/// Runner status counts with the per-status paused count
/// (`values("status").annotate(c=Count("id"), paused=Count("id",
/// filter=Q(paused_at__isnull=False, revoked_at__isnull=True)))`,
/// `09738d222` adds the `paused` arm).
pub fn runner_status_counts_sql() -> String {
    r#"SELECT "runner"."status", COUNT("runner"."id") AS "c", COUNT("runner"."id") FILTER (WHERE ("runner"."paused_at" IS NOT NULL AND "runner"."revoked_at" IS NULL)) AS "paused" FROM "runner" GROUP BY "runner"."status""#
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

/// The six-gauge body (`metrics.py:78-104`), in source order.
/// `online` is already net of paused runners (`09738d222`); `paused`
/// counts non-revoked paused runners in any connectivity state.
pub fn metrics_body(
    online: i64,
    busy: i64,
    offline: i64,
    paused: i64,
    active_runs: i64,
    pending_approvals: i64,
) -> String {
    [
        gauge(
            "pi_dash_runner_online",
            "Runners online and accepting work (excludes paused).",
            online,
        ),
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
            "pi_dash_runner_paused",
            "Non-revoked runners paused by an operator, in any connectivity state.",
            paused,
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

/// `GET /api/v1/runner/metrics/`: the six gauges as `text/plain`.
/// AllowAny — no session lookup at all (`authentication_classes = []`).
pub async fn metrics(State(state): State<AppState>) -> Response {
    view_response(metrics_inner(State(state)).await, ALLOW_RUNNER_METRICS)
}

async fn metrics_inner(State(state): State<AppState>) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let status_rows: Vec<(String, i64, i64)> = match sqlx::query_as(&runner_status_counts_sql())
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(_) => return server_error(),
    };
    let mut online = 0;
    let mut busy = 0;
    let mut offline = 0;
    let mut paused = 0;
    for (status, count, paused_count) in status_rows {
        // The paused gauge sums every status group (`metrics.py:78`); the
        // COUNT filter already excludes revoked runners.
        paused += paused_count;
        // Revoked runners are retired, not a health signal: counted in
        // their own group and ignored here (`metrics.py:14-15`).
        match status.as_str() {
            // The online gauge nets out paused-ONLINE runners
            // (`metrics.py:79-80`, `09738d222`); busy/offline keep
            // their raw counts, as in Python.
            "online" => online = count - paused_count,
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
    let body = metrics_body(
        online,
        busy,
        offline,
        paused,
        active_runs,
        pending_approvals,
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, METRICS_CONTENT_TYPE)
        .body(axum::body::Body::from(body))
        .expect("metrics response")
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
        // Fixture rows: 1 online, 1 busy, 2 offline, 0 paused, 8 active,
        // 2 pending.
        let body = metrics_body(1, 1, 2, 0, 8, 2);
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
