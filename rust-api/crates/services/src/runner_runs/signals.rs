//! Default-pod signal (D-15, stage 5, PIDASHCONV-542).
//!
//! Ports `create_default_pod_for_new_project`
//! (`apps/api/pi_dash/runner/signals.py:32-66`, the `@receiver(post_save,
//! sender=Project)` handler) as an explicit post-commit fn
//! ([`create_default_pod_for_new_project`]): when a project is created,
//! ensure it has a default pod, idempotently. Per the transactions
//! decision, signals become explicit calls — the project-creation owner
//! (D-19/D-25 area) wires this call on its create path; that wiring
//! belongs to that domain's split, not this issue.
//!
//! Body-note fix: the issue text calls this unit
//! `ensure_default_pod_for_project`; the real identifier is
//! `create_default_pod_for_new_project` (split-review correction).
//!
//! Lift note (PIDASHCONV-1268): this module is the single copy of the
//! receiver, moved here from the api crate so the jobs entry point
//! (dummy-data `create_project`) can call it too. The api
//! project-create handlers call this same copy.
//!
//! The fallible core lives in
//! [`pidash_db::runner_runs::pod`](pidash_db::runner_runs::pod)
//! (PIDASHCONV-1297) so the worker seed path — which cannot depend on
//! the api crate — calls the same SQL; this module keeps the
//! swallow-and-log receiver shape and re-exports the shared items.

use sqlx::PgPool;

pub use pidash_db::runner_runs::pod::{
    default_pod_name, ensure_default_pod, insert_default_pod_sql, pod_created_by,
    pod_exists_for_project_sql, NewProject, DEFAULT_POD_DESCRIPTION,
};

/// Ensure a fresh project has a default pod (`signals.py:32-66`):
/// the shared [`ensure_default_pod`] core, with ANY failure
/// swallowed and logged — pod creation must never block project
/// creation (`signals.py:59-66`).
pub async fn create_default_pod_for_new_project(pool: &PgPool, project: &NewProject) {
    if let Err(error) = ensure_default_pod(pool, project).await {
        tracing::error!(
            %error,
            project_id = %project.id,
            "runner.signals: failed to auto-create default pod for project",
        );
    }
}
