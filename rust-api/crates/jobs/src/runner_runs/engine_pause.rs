//! Engine-credential expiry sweep (#2910, NEWPIDASH-50).
//!
//! Port of `pause_engines_with_expired_credentials`
//! (`apps/api/pi_dash/runner/tasks.py`, beat schedule
//! `runner-pause-engines-with-expired-credentials` at `crontab(-1)`
//! every five minutes, `apps/api/pi_dash/celery.py`).
//!
//! The driver is the domain function
//! ([`pidash_services::domain::engine_state::pause_engines_with_expired_credentials`]):
//! every new-path, non-revoked machine still `active` whose engine
//! credential ran out gets an `engine_sign_in_required` auto-pause.
//! The beat entry lives in `schedule.rs`; the task and beat names are
//! Django's own (`runner.tasks` / `celery.beat_schedule`).

#![forbid(unsafe_code)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::Utc;
use sqlx::PgPool;

use crate::queue::JobRow;
use crate::worker::{Handler, HandlerError, Registry, Verdict};

/// The beat task name (`runner/tasks.py`).
pub const PAUSE_ENGINES_TASK: &str = "runner.pause_engines_with_expired_credentials";

/// The `celery.py` beat entry name for this task.
pub const PAUSE_ENGINES_BEAT_NAME: &str = "runner-pause-engines-with-expired-credentials";

fn pause_engines_handler(pool: PgPool) -> Handler {
    Arc::new(move |_job: JobRow| {
        let pool = pool.clone();
        let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
            Box::pin(async move {
                let sea = pidash_db::pool::sea_orm_connection(&pool);
                pidash_services::domain::engine_state::pause_engines_with_expired_credentials(
                    &sea,
                    Utc::now(),
                )
                .await
                .map(|_| Verdict::Ack)
                .map_err(|error| error.to_string())
            });
        fut
    })
}

/// Register the expiry-sweep task handler. Like every other port, this
/// only builds the handler table.
pub fn register_engine_pause_tasks(registry: &mut Registry, pool: PgPool) {
    registry.register(PAUSE_ENGINES_TASK, pause_engines_handler(pool));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::{HandlerError, Verdict};

    #[test]
    fn task_and_beat_names_match_django() {
        assert_eq!(
            PAUSE_ENGINES_TASK,
            "runner.pause_engines_with_expired_credentials"
        );
        assert_eq!(
            PAUSE_ENGINES_BEAT_NAME,
            "runner-pause-engines-with-expired-credentials"
        );
        assert_eq!(crate::runner_runs::TASK_NAMES.len(), 16);
        assert!(crate::runner_runs::is_runner_runs_task(PAUSE_ENGINES_TASK));
    }

    #[tokio::test]
    async fn register_owns_the_sweep() {
        let pool =
            sqlx::PgPool::connect_lazy("postgres://localhost:1/unused").expect("lazy pool builds");
        let mut registry = Registry::new();
        assert!(!registry.owns(PAUSE_ENGINES_TASK));
        register_engine_pause_tasks(&mut registry, pool);
        assert!(registry.owns(PAUSE_ENGINES_TASK), "sweep registered");
    }

    #[tokio::test]
    async fn handler_without_database_fails_not_acks() {
        // The pool hosts no database: the handler must surface the
        // failure as an error, never ack work it did not do.
        let pool =
            sqlx::PgPool::connect_lazy("postgres://localhost:1/unused").expect("lazy pool builds");
        let handler = pause_engines_handler(pool);
        let now = chrono::Utc::now();
        let job = JobRow {
            id: 1,
            celery_id: "00000000-0000-0000-0000-000000000000".to_owned(),
            task: PAUSE_ENGINES_TASK.to_owned(),
            args: serde_json::Value::Array(vec![]),
            kwargs: serde_json::Value::Object(Default::default()),
            queue: "default".to_owned(),
            status: "queued".to_owned(),
            attempts: 0,
            max_retries: 3,
            visible_at: now,
            claimed_at: None,
            claimed_by: None,
            created_at: now,
            last_error: None,
        };
        let outcome: Result<Verdict, HandlerError> = handler(job).await;
        assert!(outcome.is_err());
    }
}
