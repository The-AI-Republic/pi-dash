//! D-09 cleanup retention + mongo flush handlers.
//!
//! Port of the `@shared_task` entry points of
//! `apps/api/pi_dash/bgtasks/cleanup_task.py` (`:422-:479`). Each task takes
//! no arguments and runs the shared `process_cleanup_task` driver; here each
//! Celery name registers a [`Registry`] handler that runs the same driver
//! through `pidash-services`. Ownership flips to Rust the moment these
//! handlers register (unregistered names still forward to Python).
//!
//! Ack parity: plain `@shared_task` means ack-on-success with no overrides
//! (see the oracle's `test_task_options_parity`). Success returns
//! [`Verdict::Ack`]; any failure returns `Err`, which the worker settles
//! into requeue-with-budget exactly like the F-09 mechanism does for every
//! handler.

use std::sync::Arc;

use pidash_db::tasks_cleanup::cleanup_queries::MongoSink;
use pidash_db::Pools;

use crate::worker::{Handler, Registry, Verdict};

/// Re-export of the five ported task specs (names, collections, labels).
pub use pidash_services::tasks_cleanup::cleanup::TASKS;

/// Register all five cleanup handlers.
pub fn register_cleanup_handlers(registry: &mut Registry, pools: Pools, mongo: Option<MongoSink>) {
    for spec in TASKS {
        let name = spec.task.to_owned();
        let pools = pools.clone();
        let mongo = mongo.clone();
        let handler: Handler = Arc::new(move |job| {
            let pools = pools.clone();
            let mongo = mongo.clone();
            let name = name.clone();
            Box::pin(async move {
                if !job.args.is_array() || !job.kwargs.is_object() {
                    return Err(format!("{name}: unexpected job payload shape"));
                }
                if job.args.as_array().is_some_and(|args| !args.is_empty())
                    || job
                        .kwargs
                        .as_object()
                        .is_some_and(|kwargs| !kwargs.is_empty())
                {
                    // The Python tasks take no parameters; unexpected
                    // arguments raise TypeError there. Fail here so the
                    // worker settles (requeue-with-budget) instead of
                    // silently running the wrong work.
                    return Err(format!("{name} takes no arguments"));
                }
                match pidash_services::tasks_cleanup::cleanup::run_named_task(
                    &pools,
                    mongo.as_ref(),
                    &name,
                )
                .await
                {
                    Ok(outcome) => {
                        tracing::info!(
                            task = name.as_str(),
                            total_processed = outcome.total_processed,
                            total_batches = outcome.total_batches,
                            mongo_available = outcome.mongo_available,
                            "cleanup handler done"
                        );
                        Ok(Verdict::Ack)
                    }
                    Err(error) => Err(error.to_string()),
                }
            })
        });
        registry.register(spec.task, handler);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_five_tasks_registered() {
        let registry = Registry::new();
        // Pools cannot be built without a database; registration takes
        // Pools, so this test asserts the spec table the registration
        // iterates: names, count, and uniqueness.
        let mut names: Vec<&str> = TASKS.iter().map(|spec| spec.task).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 5);
        for name in &names {
            assert!(
                name.starts_with("pi_dash.bgtasks.cleanup_task."),
                "unexpected task name {name}"
            );
            assert!(registry.get(name).is_none());
        }
    }

    #[test]
    fn registry_routes_registered_task_locally() {
        // Ownership routing is pure: emulate what register_cleanup_handlers
        // does (insert the same five names) without needing a pool.
        let mut registry = Registry::new();
        for spec in TASKS {
            let handler: Handler = Arc::new(|_| Box::pin(async { Ok(Verdict::Ack) }));
            registry.register(spec.task, handler);
        }
        for spec in TASKS {
            assert_eq!(
                crate::worker::route_for(&registry, spec.task),
                crate::worker::Route::Local
            );
        }
        assert_eq!(
            crate::worker::route_for(&registry, "pi_dash.bgtasks.cleanup_task.nope"),
            crate::worker::Route::PythonOwned
        );
    }
}
