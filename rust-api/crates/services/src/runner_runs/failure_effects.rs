#![forbid(unsafe_code)]

//! Live reads/writes behind the failure comment (PDASHOSS01-183).
//!
//! Port of the database half of `_post_failure_comment` /
//! `_notify_clock_stopped` (`runner/services/run_lifecycle.py`) and
//! `consecutive_failure_streak` (`runner/failure.py`). The pure decisions
//! live in [`super::lifecycle`] ([`failure_comment_gate`],
//! [`FailureCommentPlan::render`], [`clock_stopped_receivers`], …); both
//! terminal-effects executors (the run endpoints' inline path in
//! `pidash-api` and the `apply_agent_run_terminal_effects` task in
//! `pidash-jobs`) call these on their hooks transaction so the two paths
//! cannot drift.
//!
//! [`failure_comment_gate`]: super::lifecycle::failure_comment_gate
//! [`FailureCommentPlan::render`]: super::lifecycle::FailureCommentPlan::render
//! [`clock_stopped_receivers`]: super::lifecycle::clock_stopped_receivers

use chrono::Utc;
use pidash_types::runner_runs::{failure_streak_from_rows, FAILURE_STREAK_WINDOW};
use sqlx::PgConnection;
use uuid::Uuid;

use super::lifecycle::{
    clock_stopped_data, clock_stopped_message_html, clock_stopped_notification_insert_sql,
    clock_stopped_receivers, ClockStoppedIssue, ReceiverCandidate, CLOCK_STOPPED_ENTITY_NAME,
    CLOCK_STOPPED_SENDER,
};

/// `consecutive_failure_streak` read (`failure.py`):
/// `AgentRun.objects.filter(work_item_id=…).order_by("-created_at")
/// .values_list("status", "failure_reason")[:window]`. `$1` issue id.
pub fn failure_streak_sql() -> String {
    format!(
        "SELECT \"agent_run\".\"status\", \"agent_run\".\"failure_reason\" FROM \"agent_run\" \
         WHERE \"agent_run\".\"work_item_id\" = $1 ORDER BY \"agent_run\".\"created_at\" DESC \
         LIMIT {FAILURE_STREAK_WINDOW}"
    )
}

/// `consecutive_failure_streak(issue_id, reason)`: how many of the issue's
/// newest runs failed with `reason`, stopping at the first that did not.
pub async fn failure_streak(
    conn: &mut PgConnection,
    issue_id: Uuid,
    reason: &str,
) -> Result<u32, sqlx::Error> {
    let rows: Vec<(String, String)> = sqlx::query_as(&failure_streak_sql())
        .bind(issue_id)
        .fetch_all(&mut *conn)
        .await?;
    Ok(failure_streak_from_rows(
        rows.iter()
            .map(|(status, stored)| (status.as_str(), stored.as_str())),
        reason,
    ))
}

/// The issue facts `_notify_clock_stopped` reads through `run.work_item`
/// (FK attribute access — no soft-delete scope) and `issue.project`.
const NOTIFY_ISSUE_SQL: &str = "SELECT \"issues\".\"name\", \"issues\".\"sequence_id\", \
     \"issues\".\"created_by_id\", \"issues\".\"workspace_id\", \"issues\".\"project_id\", \
     \"projects\".\"identifier\" FROM \"issues\" LEFT OUTER JOIN \"projects\" ON \
     (\"issues\".\"project_id\" = \"projects\".\"id\") WHERE \"issues\".\"id\" = $1";

/// `run.runner.owner` (`if run.runner_id and run.runner`).
const NOTIFY_RUNNER_OWNER_SQL: &str = "SELECT \"runner\".\"owner_id\" FROM \"agent_run\" \
     INNER JOIN \"runner\" ON (\"agent_run\".\"runner_id\" = \"runner\".\"id\") \
     WHERE \"agent_run\".\"id\" = $1";

/// `getattr(candidate, "is_active" / "is_bot", …)` for one user.
const NOTIFY_USER_FLAGS_SQL: &str =
    "SELECT \"users\".\"is_active\", \"users\".\"is_bot\" FROM \"users\" WHERE \"users\".\"id\" = $1";

async fn receiver_candidate(
    conn: &mut PgConnection,
    user_id: Option<Uuid>,
) -> Result<ReceiverCandidate, sqlx::Error> {
    let Some(user_id) = user_id else {
        return Ok(ReceiverCandidate {
            user_id: None,
            is_active: false,
            is_bot: false,
        });
    };
    let flags: Option<(bool, bool)> = sqlx::query_as(NOTIFY_USER_FLAGS_SQL)
        .bind(user_id)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(match flags {
        Some((is_active, is_bot)) => ReceiverCandidate {
            user_id: Some(user_id),
            is_active,
            is_bot,
        },
        // A dangling FK reads as no candidate.
        None => ReceiverCandidate {
            user_id: None,
            is_active: false,
            is_bot: false,
        },
    })
}

/// `_notify_clock_stopped(run, label=…, action=…)`: one in-app
/// notification per surviving receiver (the issue's creator, then the
/// runner's owner — deduped, active, non-bot). Returns the rows written.
pub async fn notify_clock_stopped(
    conn: &mut PgConnection,
    run_id: Uuid,
    issue_id: Uuid,
    failure_reason: &str,
    label: &str,
    action: &str,
) -> Result<usize, sqlx::Error> {
    type IssueRow = (
        String,
        i32,
        Option<Uuid>,
        Uuid,
        Option<Uuid>,
        Option<String>,
    );
    let issue: Option<IssueRow> = sqlx::query_as(NOTIFY_ISSUE_SQL)
        .bind(issue_id)
        .fetch_optional(&mut *conn)
        .await?;
    let Some((name, sequence_id, created_by_id, workspace_id, project_id, identifier)) = issue
    else {
        return Ok(0);
    };
    let runner_owner: Option<Uuid> = sqlx::query_scalar(NOTIFY_RUNNER_OWNER_SQL)
        .bind(run_id)
        .fetch_optional(&mut *conn)
        .await?;
    let creator = receiver_candidate(conn, created_by_id).await?;
    let owner = receiver_candidate(conn, runner_owner).await?;
    let receivers = clock_stopped_receivers(&[creator, owner]);
    if receivers.is_empty() {
        return Ok(0);
    }
    let project_identifier = if project_id.is_some() {
        identifier.unwrap_or_default()
    } else {
        String::new()
    };
    let data = clock_stopped_data(
        &ClockStoppedIssue {
            id: issue_id,
            name: &name,
            project_identifier: &project_identifier,
            sequence_id,
        },
        run_id,
        failure_reason,
        action,
    );
    let message_html = clock_stopped_message_html(&name, label, action);
    let now = Utc::now();
    for receiver in &receivers {
        sqlx::query(clock_stopped_notification_insert_sql())
            .bind(Uuid::new_v4())
            .bind(now)
            .bind(now)
            .bind(workspace_id)
            .bind(project_id)
            .bind(&data)
            .bind(issue_id)
            .bind(CLOCK_STOPPED_ENTITY_NAME)
            .bind(&name)
            .bind(&message_html)
            .bind(CLOCK_STOPPED_SENDER)
            .bind(*receiver)
            .execute(&mut *conn)
            .await?;
    }
    Ok(receivers.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streak_sql_reads_the_window_newest_first() {
        let sql = failure_streak_sql();
        assert!(sql.starts_with(
            "SELECT \"agent_run\".\"status\", \"agent_run\".\"failure_reason\" FROM \"agent_run\""
        ));
        assert!(sql.ends_with("ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 8"));
    }

    #[test]
    fn notification_insert_binds_twelve_params() {
        let sql = clock_stopped_notification_insert_sql();
        assert!(sql.contains("$12") && !sql.contains("$13"));
        // 21 columns, 21 values.
        let columns = sql.split(") VALUES (").next().expect("columns");
        assert_eq!(columns.matches(", ").count() + 1, 21);
        let values = sql.split(") VALUES (").nth(1).expect("values");
        assert_eq!(values.matches(", ").count() + 1, 21);
    }
}
