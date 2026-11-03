//! D-09 cleanup retention + mongo flush task logic.
//!
//! Port of the task wiring of `apps/api/pi_dash/bgtasks/cleanup_task.py`
//! (`:92-:163`, `:422-:479`): the five `@shared_task` delete tasks, their
//! Celery names, Mongo collections, log labels, and the
//! `HARD_DELETE_AFTER_DAYS` retention cutoff.
//!
//! The queryset SQL lives in
//! [`pidash_db::tasks_cleanup::cleanup_queries`], the Mongo document shapes
//! in [`pidash_types::tasks_cleanup::cleanup_dto`]; this module ties them
//! together per task. Beat entries are owned by F-09 (already transcribed
//! in `pidash_jobs::schedule`); the scheduler loop is untouched.

use pidash_db::tasks_cleanup::cleanup_queries::{
    self, CleanupError, CleanupTask, MongoSink, TaskOutcome,
};

/// One ported `@shared_task`: its Celery name, Mongo collection, log label,
/// and queryset.
pub struct TaskSpec {
    /// Full Celery task name, e.g.
    /// `pi_dash.bgtasks.cleanup_task.delete_api_logs`.
    pub task: &'static str,
    /// Mongo collection the batch is archived to.
    pub collection: &'static str,
    /// `task_name` log label.
    pub label: &'static str,
    pub kind: CleanupTask,
}

pub const DELETE_API_LOGS: TaskSpec = TaskSpec {
    task: "pi_dash.bgtasks.cleanup_task.delete_api_logs",
    collection: "api_activity_logs",
    label: "API Activity Log",
    kind: CleanupTask::ApiLogs,
};

pub const DELETE_EMAIL_NOTIFICATION_LOGS: TaskSpec = TaskSpec {
    task: "pi_dash.bgtasks.cleanup_task.delete_email_notification_logs",
    collection: "email_notification_logs",
    label: "Email Notification Log",
    kind: CleanupTask::EmailLogs,
};

pub const DELETE_PAGE_VERSIONS: TaskSpec = TaskSpec {
    task: "pi_dash.bgtasks.cleanup_task.delete_page_versions",
    collection: "page_versions",
    label: "Page Version",
    kind: CleanupTask::PageVersions,
};

pub const DELETE_ISSUE_DESCRIPTION_VERSIONS: TaskSpec = TaskSpec {
    task: "pi_dash.bgtasks.cleanup_task.delete_issue_description_versions",
    collection: "issue_description_versions",
    label: "Issue Description Version",
    kind: CleanupTask::IssueDescriptionVersions,
};

pub const DELETE_WEBHOOK_LOGS: TaskSpec = TaskSpec {
    task: "pi_dash.bgtasks.cleanup_task.delete_webhook_logs",
    collection: "webhook_logs",
    label: "Webhook Log",
    kind: CleanupTask::WebhookLogs,
};

/// All five tasks in `@shared_task` definition order (`:422-:479`).
pub const TASKS: [&TaskSpec; 5] = [
    &DELETE_API_LOGS,
    &DELETE_EMAIL_NOTIFICATION_LOGS,
    &DELETE_PAGE_VERSIONS,
    &DELETE_ISSUE_DESCRIPTION_VERSIONS,
    &DELETE_WEBHOOK_LOGS,
];

/// Look up a task by its Celery name (worker dispatch).
pub fn spec_for(task_name: &str) -> Option<&'static TaskSpec> {
    TASKS.iter().find(|spec| spec.task == task_name).copied()
}

/// Every failure the cleanup tasks report.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("bad HARD_DELETE_AFTER_DAYS: {0}")]
    Cutoff(String),
    #[error("cleanup task failed: {0}")]
    Task(#[from] CleanupError),
    #[error("unknown cleanup task: {0}")]
    UnknownTask(String),
}

/// Read the retention window in days: `int(os.environ.get(
/// "HARD_DELETE_AFTER_DAYS", 30))` (`cleanup_task.py:269`).
///
/// Python's `int()` strips surrounding whitespace and accepts a leading
/// `+`/`-`; anything else raises, failing the task. Empty string raises on
/// both sides.
pub fn retention_cutoff_days() -> Result<i64, Error> {
    match std::env::var("HARD_DELETE_AFTER_DAYS") {
        Err(_) => Ok(30),
        Ok(raw) => raw.trim().parse::<i64>().map_err(|_| {
            Error::Cutoff(format!("HARD_DELETE_AFTER_DAYS={raw:?} is not an integer"))
        }),
    }
}

/// Run one cleanup task: compute the cutoff, stream its queryset, archive
/// each batch to Mongo (when configured), and hard-delete it from
/// Postgres. Mirrors `process_cleanup_task` end to end.
pub async fn run_cleanup_task(
    pools: &pidash_db::Pools,
    mongo: Option<&MongoSink>,
    spec: &TaskSpec,
) -> Result<TaskOutcome, Error> {
    let days = retention_cutoff_days()?;
    let cutoff = cleanup_queries::render_cutoff(&cleanup_queries::cutoff_time(days));
    cleanup_queries::run_cleanup(
        pools.primary(),
        mongo,
        spec.kind,
        &cutoff,
        spec.label,
        spec.collection,
    )
    .await
    .map_err(Error::Task)
}

/// Run a cleanup task by Celery name (worker dispatch entry point).
pub async fn run_named_task(
    pools: &pidash_db::Pools,
    mongo: Option<&MongoSink>,
    task_name: &str,
) -> Result<TaskOutcome, Error> {
    let spec = spec_for(task_name).ok_or_else(|| Error::UnknownTask(task_name.to_owned()))?;
    run_cleanup_task(pools, mongo, spec).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same committed evidence as the other layers:
    /// `rust-api/fixtures/tasks_cleanup/cleanup.json`.
    static FIXTURE: &str = include_str!("../../../../fixtures/tasks_cleanup/cleanup.json");

    #[test]
    fn task_table_matches_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        let tasks = fixture["tasks"].as_object().expect("tasks table");
        assert_eq!(tasks.len(), TASKS.len());
        for spec in TASKS {
            let short = spec.task.rsplit('.').next().unwrap();
            let entry = &tasks[short];
            assert_eq!(entry["collection"].as_str().unwrap(), spec.collection);
            assert_eq!(entry["task_name"].as_str().unwrap(), spec.label);
        }
    }

    #[test]
    fn spec_for_resolves_all_five_names() {
        for spec in TASKS {
            assert_eq!(spec_for(spec.task).unwrap().collection, spec.collection);
        }
        assert!(spec_for("pi_dash.bgtasks.cleanup_task.nope").is_none());
        assert!(spec_for("").is_none());
    }

    #[test]
    fn cutoff_env_parsing_mirrors_python_int() {
        // Sequential in one test: the process environment is shared.
        let saved = std::env::var("HARD_DELETE_AFTER_DAYS").ok();
        let set = |v: Option<&str>| match v {
            Some(s) => std::env::set_var("HARD_DELETE_AFTER_DAYS", s),
            None => std::env::remove_var("HARD_DELETE_AFTER_DAYS"),
        };
        set(None);
        assert_eq!(retention_cutoff_days().unwrap(), 30);
        set(Some("7"));
        assert_eq!(retention_cutoff_days().unwrap(), 7);
        set(Some("  45  "));
        assert_eq!(retention_cutoff_days().unwrap(), 45);
        set(Some("+3"));
        assert_eq!(retention_cutoff_days().unwrap(), 3);
        set(Some("-1"));
        assert_eq!(retention_cutoff_days().unwrap(), -1);
        // Note: Python int() also accepts underscores ("3_0" == 30); that
        // edge is left unspecified rather than pinned.
        for bad in ["", "  ", "3.0", "thirty"] {
            set(Some(bad));
            assert!(
                retention_cutoff_days().is_err(),
                "{bad:?} must fail like int()"
            );
        }
        set(saved.as_deref());
    }
}
