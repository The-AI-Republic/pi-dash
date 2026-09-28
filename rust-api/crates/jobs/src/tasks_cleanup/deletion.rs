//! Celery wiring for soft/hard deletion.
//!
//! Task names and payloads mirror the Python registration byte for byte
//! (plain `@shared_task`: default ack-on-success, no `autoretry_for`,
//! no `acks_late` — see the oracle's `test_task_options_parity`):
//!
//! - `pi_dash.bgtasks.deletion_task.soft_delete_related_objects` with
//!   `(app_label, model_name, instance_pk[, using])`.
//! - `pi_dash.bgtasks.deletion_task.hard_delete` with no arguments; the
//!   retention comes from `HARD_DELETE_AFTER_DAYS` (Django default 60).
//!
//! `restore_related_objects` ships with its decorator commented out, so
//! there is deliberately NO handler for [`RESTORE_TASK_NAME`]; the
//! registry test pins that absence (ported bug BUG-DEL-2).
//!
//! Verdict mapping: success (including the silent no-op when the row is
//! already gone) acknowledges — Python has no autoretry on these tasks.
//! A task-level failure parks the row (`Fail`); broker-forward retries
//! stay the worker's business, not the handler's.

use pidash_db::tasks_cleanup::cutoff_for;
use pidash_db::{Pools, RequestContext};
use pidash_services::tasks_cleanup::{
    hard_delete as run_hard_delete, parse_soft_delete_call,
    soft_delete_related_objects as run_soft_delete, DeletionError, SoftDeleteOutcome,
};
use pidash_types::WorkspaceId;

use crate::worker::{Handler, Registry, Verdict};

/// `pi_dash.bgtasks.deletion_task.soft_delete_related_objects`.
pub const SOFT_DELETE_TASK: &str = "pi_dash.bgtasks.deletion_task.soft_delete_related_objects";
/// `pi_dash.bgtasks.deletion_task.hard_delete`.
pub const HARD_DELETE_TASK: &str = "pi_dash.bgtasks.deletion_task.hard_delete";
/// The would-be name of `restore_related_objects`: never registered
/// (its `@shared_task` decorator is commented out upstream).
pub const RESTORE_TASK_NAME: &str = "pi_dash.bgtasks.deletion_task.restore_related_objects";

/// Django `settings.HARD_DELETE_AFTER_DAYS` default (`common.py:647`).
pub const DEFAULT_HARD_DELETE_AFTER_DAYS: i64 = 60;

/// Resolve the retention from the environment, mirroring
/// `int(get_config("HARD_DELETE_AFTER_DAYS", 60))`: unset/empty is the
/// default; a non-integer fails the task (Python's `int()` raises).
pub fn hard_delete_days(raw: Option<String>) -> Result<i64, DeletionError> {
    match raw {
        None => Ok(DEFAULT_HARD_DELETE_AFTER_DAYS),
        Some(text) if text.trim().is_empty() => Ok(DEFAULT_HARD_DELETE_AFTER_DAYS),
        Some(text) => text.trim().parse::<i64>().map_err(|_| {
            DeletionError::BadPayload(format!("bad HARD_DELETE_AFTER_DAYS: {text:?}"))
        }),
    }
}

/// System context for cross-workspace maintenance: there is no tenant
/// (Celery tasks run outside any request), and `actor_id` is `None` so
/// audit FK columns stay NULL, mirroring `null=True` on
/// `created_by`/`updated_by`. The context still scopes every write to
/// the primary pool — no unscoped handle exists on this path.
pub fn system_context() -> RequestContext {
    RequestContext::new(WorkspaceId::from("system"), None)
}

/// Pure verdict core for the soft-delete handler (no pool needed):
/// warnings go back for logging, `Ok` always acknowledges.
pub fn settle_soft(result: Result<SoftDeleteOutcome, DeletionError>) -> (Verdict, Vec<String>) {
    match result {
        Ok(outcome) => (Verdict::Ack, outcome.warnings.clone()),
        Err(error) => (
            Verdict::Fail {
                error: error.to_string(),
            },
            Vec::new(),
        ),
    }
}

/// Handler for [`SOFT_DELETE_TASK`]: parse, run, settle.
pub fn soft_delete_handler(pools: Pools) -> Handler {
    std::sync::Arc::new(move |job: crate::queue::JobRow| {
        let pools = pools.clone();
        Box::pin(async move {
            let ctx = system_context();
            let outcome = match parse_soft_delete_call(&job.args, &job.kwargs) {
                Ok(target) => run_soft_delete(&pools, &ctx, target).await,
                Err(error) => Err(error),
            };
            let (verdict, warnings) = settle_soft(outcome);
            for warning in warnings {
                tracing::warn!(task = SOFT_DELETE_TASK, "{warning}");
            }
            Ok(verdict)
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
    })
}

/// Handler for [`HARD_DELETE_TASK`]: retention, cutoff, run, settle.
pub fn hard_delete_handler(pools: Pools) -> Handler {
    std::sync::Arc::new(move |_job: crate::queue::JobRow| {
        let pools = pools.clone();
        Box::pin(async move {
            let ctx = system_context();
            let days = match hard_delete_days(std::env::var("HARD_DELETE_AFTER_DAYS").ok()) {
                Ok(days) => days,
                Err(error) => {
                    return Ok(Verdict::Fail {
                        error: error.to_string(),
                    })
                }
            };
            let cutoff = cutoff_for(days, chrono::Utc::now());
            match run_hard_delete(&pools, &ctx, &cutoff).await {
                Ok(outcome) => {
                    tracing::info!(
                        task = HARD_DELETE_TASK,
                        named_deleted = outcome.named_deleted,
                        sweep_deleted = outcome.sweep_deleted,
                        sweep_tables = outcome.sweep_tables,
                        "hard_delete complete"
                    );
                    Ok(Verdict::Ack)
                }
                Err(error) => Ok(Verdict::Fail {
                    error: error.to_string(),
                }),
            }
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
    })
}

/// Register the two deletion handlers. `restore_related_objects` is
/// deliberately absent (BUG-DEL-2).
pub fn register_deletion_tasks(registry: &mut Registry, pools: Pools) {
    registry.register(SOFT_DELETE_TASK, soft_delete_handler(pools.clone()));
    registry.register(HARD_DELETE_TASK, hard_delete_handler(pools));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::route_for;
    use crate::worker::Route;

    #[test]
    fn task_names_match_python_registration() {
        assert_eq!(
            SOFT_DELETE_TASK,
            "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"
        );
        assert_eq!(
            HARD_DELETE_TASK,
            "pi_dash.bgtasks.deletion_task.hard_delete"
        );
        assert_ne!(RESTORE_TASK_NAME, SOFT_DELETE_TASK);
        assert_ne!(RESTORE_TASK_NAME, HARD_DELETE_TASK);
    }

    fn ack() -> Handler {
        std::sync::Arc::new(|_: crate::queue::JobRow| {
            Box::pin(async { Ok(Verdict::Ack) })
                as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
        })
    }

    #[test]
    fn registered_names_route_local_and_restore_stays_python_owned() {
        let mut registry = Registry::new();
        // Mirrors what register_deletion_tasks does with the real
        // handlers (which need a live pool): the two names route local.
        registry.register(SOFT_DELETE_TASK, ack());
        registry.register(HARD_DELETE_TASK, ack());
        assert_eq!(route_for(&registry, SOFT_DELETE_TASK), Route::Local);
        assert_eq!(route_for(&registry, HARD_DELETE_TASK), Route::Local);
        // BUG-DEL-2 pin: nothing ever registers the restore name.
        assert_eq!(route_for(&registry, RESTORE_TASK_NAME), Route::PythonOwned);
        assert!(!registry.owns(RESTORE_TASK_NAME));
    }

    #[test]
    fn retention_days_default_and_parse() {
        assert_eq!(
            hard_delete_days(None).expect("default"),
            DEFAULT_HARD_DELETE_AFTER_DAYS
        );
        assert_eq!(hard_delete_days(Some(String::new())).expect("empty"), 60);
        assert_eq!(hard_delete_days(Some("30".to_owned())).expect("30"), 30);
        assert!(hard_delete_days(Some("sixty".to_owned())).is_err());
    }

    #[test]
    fn system_context_carries_no_actor() {
        let ctx = system_context();
        assert!(ctx.actor_id().is_none());
        assert!(!ctx.use_read_replica());
    }

    #[test]
    fn settle_soft_acks_and_returns_warnings() {
        let (verdict, warnings) = settle_soft(Ok(SoftDeleteOutcome {
            visited: 2,
            stamped: 2,
            nulled_rows: 0,
            warnings: vec!["Error handling relation x: boom".to_owned()],
        }));
        assert_eq!(verdict, Verdict::Ack);
        assert_eq!(warnings.len(), 1);
        let (verdict, warnings) = settle_soft(Err(DeletionError::UnknownModel {
            app_label: "db".to_owned(),
            model_name: "nope".to_owned(),
        }));
        assert_eq!(
            verdict,
            Verdict::Fail {
                error: "unknown model: db.nope".to_owned()
            }
        );
        assert!(warnings.is_empty());
    }
}
