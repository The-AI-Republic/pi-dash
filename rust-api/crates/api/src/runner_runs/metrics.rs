//! Runner Prometheus metrics (D-15, stage 5, PIDASHCONV-542).
//!
//! Ports `apps/api/pi_dash/runner/views/metrics.py:1-105`
//! (`GET /api/v1/runner/metrics/`, [`metrics`]): five point-in-time
//! gauges plus the `pi_dash_runs_failed_by_reason{reason}` family
//! (PDASHOSS01-183) rendered by hand in Prometheus text format. AllowAny with no
//! auth classes — the handler never touches the session — and the
//! response bypasses JSON rendering (`text/plain; version=0.0.4`).
//! Fully owned.

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::Response;

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

/// FAILED-run counts per canonical `failure_reason` (`metrics.py`,
/// PDASHOSS01-183): `filter(status=FAILED).exclude(failure_reason="")
/// .values_list("failure_reason").annotate(Count("id"))` — rows that
/// predate the taxonomy backfill (blank reason) are omitted. Param `$1`
/// `failed`. Sorted by the caller (Python `sorted()` over the tuples).
pub fn failed_by_reason_counts_sql() -> String {
    r#"SELECT "agent_run"."failure_reason", COUNT("agent_run"."id") FROM "agent_run" WHERE ("agent_run"."status" = $1 AND NOT ("agent_run"."failure_reason" = '')) GROUP BY "agent_run"."failure_reason""#
        .to_owned()
}

/// One gauge family with a single label (`_labeled_gauge`, `metrics.py`):
/// `rows` are `(label_key, label_value, value)`; label values escaped per
/// the Prometheus text format (`\` then `"`). An empty `rows` still
/// renders the `HELP`/`TYPE` header.
pub fn labeled_gauge(name: &str, help: &str, rows: &[(&str, &str, i64)]) -> String {
    let mut lines = vec![
        format!("# HELP {name} {help}"),
        format!("# TYPE {name} gauge"),
    ];
    for (key, value, count) in rows {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        lines.push(format!("{name}{{{key}=\"{escaped}\"}} {count}"));
    }
    lines.join("\n") + "\n"
}

/// One gauge block (`_gauge`, `metrics.py:49-54`).
pub fn gauge(name: &str, help: &str, value: i64) -> String {
    format!("# HELP {name} {help}\n# TYPE {name} gauge\n{name} {value}\n")
}

/// The metrics body (`metrics.py:78-104`), in source order:
/// `failed_by_reason` is the sorted `(reason, count)` list.
pub fn metrics_body(
    online: i64,
    busy: i64,
    offline: i64,
    active_runs: i64,
    pending_approvals: i64,
    failed_by_reason: &[(String, i64)],
) -> String {
    let reason_rows: Vec<(&str, &str, i64)> = failed_by_reason
        .iter()
        .map(|(reason, count)| ("reason", reason.as_str(), *count))
        .collect();
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
        labeled_gauge(
            "pi_dash_runs_failed_by_reason",
            "FAILED AgentRuns per canonical failure_reason.",
            &reason_rows,
        ),
    ]
    .concat()
}

/// `GET /api/v1/runner/metrics/`: the gauges as `text/plain`.
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
    let mut failed_by_reason: Vec<(String, i64)> =
        match sqlx::query_as(&failed_by_reason_counts_sql())
            .bind(pidash_types::runner_runs::AgentRunStatus::Failed.value())
            .fetch_all(pool)
            .await
        {
            Ok(rows) => rows,
            Err(_) => return server_error(),
        };
    // Python `sorted()` over `(reason, count)` tuples.
    failed_by_reason.sort();
    let body = metrics_body(
        online,
        busy,
        offline,
        active_runs,
        pending_approvals,
        &failed_by_reason,
    );
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
        // Fixture rows: 1 online, 1 busy, 2 offline, 8 active, 2 pending,
        // no FAILED run with a stored reason (header-only family).
        let body = metrics_body(1, 1, 2, 8, 2, &[]);
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
    fn failed_by_reason_family_renders_and_escapes() {
        let rows = vec![
            ("agent_error.provider_network".to_owned(), 4),
            ("timeout".to_owned(), 1),
        ];
        let body = metrics_body(0, 0, 0, 0, 0, &rows);
        assert!(body.ends_with(
            "# HELP pi_dash_runs_failed_by_reason FAILED AgentRuns per canonical failure_reason.\n\
             # TYPE pi_dash_runs_failed_by_reason gauge\n\
             pi_dash_runs_failed_by_reason{reason=\"agent_error.provider_network\"} 4\n\
             pi_dash_runs_failed_by_reason{reason=\"timeout\"} 1\n"
        ));
        assert_eq!(
            labeled_gauge("g", "h", &[("reason", "a\"b\\c", 1)]),
            "# HELP g h\n# TYPE g gauge\ng{reason=\"a\\\"b\\\\c\"} 1\n"
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
