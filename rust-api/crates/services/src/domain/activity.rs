//! The single copy of the Celery task names the domain writes enqueue.
//!
//! Seventeen modules each defined their own `ISSUE_ACTIVITY_TASK`, nine
//! their own `MODEL_ACTIVITY_TASK`, and the two version-task names lived in
//! two places each. Every one of those is now a re-export of the constants
//! below, so a rename touches one line. The per-view kwarg builders stay
//! with their views: Django passes different kwargs per view (design D6),
//! and consolidating those is each aggregate's migration issue, not this
//! module.

/// `issue_activity` (`bgtasks/issue_activities_task.py:1503-1504`): full
/// Celery name. A plain task (`@shared_task` default), so the name is the
/// module path plus the function name.
pub const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";

/// `model_activity` (`bgtasks/webhook_task.py:463-464`): full Celery name.
pub const MODEL_ACTIVITY_TASK: &str = "pi_dash.bgtasks.webhook_task.model_activity";

/// `page_transaction` (`bgtasks/page_transaction_task.py:84-85`): full
/// Celery name.
pub const PAGE_TRANSACTION_TASK: &str = "pi_dash.bgtasks.page_transaction_task.page_transaction";

/// `track_page_version` (`bgtasks/page_version_task.py:21-22`): full Celery
/// name.
pub const TRACK_PAGE_VERSION_TASK: &str = "pi_dash.bgtasks.page_version_task.track_page_version";

/// The task transport as an object-safe hook (the
/// [`IssueTransitionHook`](super::transition::IssueTransitionHook) precedent:
/// boxed futures, no generics). `pidash-services` cannot depend on
/// `pidash-jobs` (the crate graph is `types` → `db` → `services`, and jobs
/// sits above services), so the binary that owns the queue implements this
/// over `pidash_jobs::queue::enqueue`, the driver registers it on the
/// [`HookRegistry`](super::HookRegistry), and domain chains call it from
/// their after-commit steps. Infallible like the handler-local
/// `enqueue_task` it replaces: failures warn inside the implementation
/// and the response stands.
pub trait TaskEnqueueHook: Send + Sync {
    fn enqueue_task(
        &self,
        task: &str,
        args: Vec<serde_json::Value>,
        kwargs: serde_json::Map<String, serde_json::Value>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_names_are_the_celery_wire_names() {
        assert_eq!(
            ISSUE_ACTIVITY_TASK,
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
        assert_eq!(
            MODEL_ACTIVITY_TASK,
            "pi_dash.bgtasks.webhook_task.model_activity"
        );
        assert_eq!(
            PAGE_TRANSACTION_TASK,
            "pi_dash.bgtasks.page_transaction_task.page_transaction"
        );
        assert_eq!(
            TRACK_PAGE_VERSION_TASK,
            "pi_dash.bgtasks.page_version_task.track_page_version"
        );
    }
}
