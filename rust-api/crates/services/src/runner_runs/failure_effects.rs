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
//! Reads and the notification insert go through the SeaORM entities
//! (`pidash_db::entity`), never hand-written SQL: the raw-SQL ratchet
//! (`rust-api/scripts/raw_sql_guard.py`) allows no new `sqlx::query*`
//! sites. The [`failure_streak_sql`] / [`clock_stopped_notification_insert_sql`]
//! texts stay as planner specs (string constants are not counted).
//!
//! [`failure_comment_gate`]: super::lifecycle::failure_comment_gate
//! [`FailureCommentPlan::render`]: super::lifecycle::FailureCommentPlan::render
//! [`clock_stopped_receivers`]: super::lifecycle::clock_stopped_receivers
//! [`clock_stopped_notification_insert_sql`]: super::lifecycle::clock_stopped_notification_insert_sql

use chrono::Utc;
use pidash_db::entity::{agent_run, issues, notifications, projects, runner, users};
use pidash_types::runner_runs::{failure_streak_from_rows, FAILURE_STREAK_WINDOW};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use uuid::Uuid;

use super::lifecycle::{
    clock_stopped_data, clock_stopped_message_html, clock_stopped_receivers, ClockStoppedIssue,
    ReceiverCandidate, CLOCK_STOPPED_ENTITY_NAME, CLOCK_STOPPED_SENDER,
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
    db: &impl ConnectionTrait,
    issue_id: Uuid,
    reason: &str,
) -> Result<u32, sea_orm::DbErr> {
    let rows = agent_run::Entity::find()
        .filter(agent_run::Column::WorkItemId.eq(issue_id))
        .order_by_desc(agent_run::Column::CreatedAt)
        .limit(FAILURE_STREAK_WINDOW as u64)
        .all(db)
        .await?;
    Ok(failure_streak_from_rows(
        rows.iter()
            .map(|run| (run.status.as_str(), run.failure_reason.as_str())),
        reason,
    ))
}

async fn receiver_candidate(
    db: &impl ConnectionTrait,
    user_id: Option<Uuid>,
) -> Result<ReceiverCandidate, sea_orm::DbErr> {
    let Some(user_id) = user_id else {
        return Ok(ReceiverCandidate {
            user_id: None,
            is_active: false,
            is_bot: false,
        });
    };
    let flags = users::Entity::find_by_id(user_id).one(db).await?;
    Ok(match flags {
        Some(user) => ReceiverCandidate {
            user_id: Some(user.id),
            is_active: user.is_active,
            is_bot: user.is_bot,
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
    db: &impl ConnectionTrait,
    run_id: Uuid,
    issue_id: Uuid,
    failure_reason: &str,
    label: &str,
    action: &str,
) -> Result<usize, sea_orm::DbErr> {
    let Some(issue) = issues::Entity::find_by_id(issue_id).one(db).await? else {
        return Ok(0);
    };
    let name = issue.name.clone();
    let sequence_id = issue.sequence_id;
    let created_by_id = issue.created_by_id;
    let workspace_id = issue.workspace_id;
    let project_id = issue.project_id;
    let identifier = projects::Entity::find_by_id(project_id)
        .one(db)
        .await?
        .map(|project| project.identifier)
        .unwrap_or_default();
    let runner_owner = match agent_run::Entity::find_by_id(run_id).one(db).await?.and_then(|run| run.runner_id) {
        Some(runner_id) => runner::Entity::find_by_id(runner_id)
            .one(db)
            .await?
            .map(|row| row.owner_id),
        None => None,
    };
    let creator = receiver_candidate(db, created_by_id).await?;
    let owner = receiver_candidate(db, runner_owner).await?;
    let receivers = clock_stopped_receivers(&[creator, owner]);
    if receivers.is_empty() {
        return Ok(0);
    }
    let data = clock_stopped_data(
        &ClockStoppedIssue {
            id: issue_id,
            name: &name,
            project_identifier: &identifier,
            sequence_id,
        },
        run_id,
        failure_reason,
        action,
    );
    let message_html = clock_stopped_message_html(&name, label, action);
    let now = Utc::now();
    for receiver in &receivers {
        notifications::ActiveModel {
            id: Set(Uuid::new_v4()),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            workspace_id: Set(workspace_id),
            project_id: Set(Some(project_id)),
            data: Set(Some(data.clone())),
            entity_identifier: Set(Some(issue_id)),
            entity_name: Set(CLOCK_STOPPED_ENTITY_NAME.to_owned()),
            title: Set(name.clone()),
            message_html: Set(message_html.clone()),
            sender: Set(CLOCK_STOPPED_SENDER.to_owned()),
            receiver_id: Set(*receiver),
            ..Default::default()
        }
        .insert(db)
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
        let sql = super::super::lifecycle::clock_stopped_notification_insert_sql();
        assert!(sql.contains("$12") && !sql.contains("$13"));
        // 21 columns, 21 values.
        let columns = sql.split(") VALUES (").next().expect("columns");
        assert_eq!(columns.matches(", ").count() + 1, 21);
        let values = sql.split(") VALUES (").nth(1).expect("values");
        assert_eq!(values.matches(", ").count() + 1, 21);
    }
}
