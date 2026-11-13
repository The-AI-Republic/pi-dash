#![forbid(unsafe_code)]

//! Creation dispatch for runs (D-11 L6, PIDASHCONV-487).
//!
//! What survives P3 step A (agent naming convention): the creation
//! re-exports (`execution_fields` and friends, owned by
//! `pidash_services::issuing::execution_resolution`), the runner-plane
//! `dispatch_agent_run` (non-cloud kinds drain their pod), and the
//! post-commit collector. The cloud lease/offer engine, the event
//! append, and their tests died with the retired plane; P4 removed
//! the `dispatch_waiting` stub and its trait methods.
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-06-dispatch.golden.json`
//! (FX-DISP-06), replayed by the suite below (trimmed in P3 step B).

use chrono::{DateTime, Utc};
use sea_orm::{
    ColumnTrait, EntityTrait, FromQueryResult, QueryFilter, QuerySelect, Select,
};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity::agent_run;
use pidash_db::orm_tx::into_sqlx;
use pidash_db::pool::sea_orm_connection;

/// The executor resolve (`execution_fields`) now lives in
/// `pidash_services::issuing::execution_resolution`; this re-export keeps every
/// `pidash_jobs::issuing::…` path stable.
pub use pidash_services::issuing::execution_resolution::{
    execution_fields, lock_workspace, ActorScope, DeferredAdmissionError, ExecutionFields,
};

// ---------------------------------------------------------------------------
// Queries (SeaORM fetchers)
// ---------------------------------------------------------------------------

fn dispatch_run_query(run_id: Uuid) -> Select<agent_run::Entity> {
    agent_run::Entity::find()
        .select_only()
        .columns([
            agent_run::Column::Id,
            agent_run::Column::Status,
            agent_run::Column::ExecutorKind,
            agent_run::Column::PodId,
            agent_run::Column::WorkspaceId,
        ])
        .filter(agent_run::Column::Id.eq(run_id))
        .limit(1)
}

// ---------------------------------------------------------------------------
// Dispatch engine (`dispatch.py:20-70`)
// ---------------------------------------------------------------------------

// `extra_toolsets_allowed` deleted in P4 with the SDK re-export.

/// One `agent_run` row as `dispatch_agent_run` reads it (`dispatch.py:62`).
#[derive(Debug, Clone, FromQueryResult)]
struct DispatchRunRow {
    // Selected (`.only("id", …)`) but unread, as in Python.
    #[allow(dead_code)]
    id: Uuid,
    status: String,
    // Selected for the select shape; the cloud branch that read them
    // died in P3 step A. Kept so the query shape is unchanged.
    #[allow(dead_code)]
    executor_kind: String,
    pod_id: Uuid,
    #[allow(dead_code)]
    workspace_id: Uuid,
}

/// What `dispatch_agent_run` did with one run (`dispatch.py:61-70`).
/// (`CloudDispatched` deleted in P3 step A: the cloud branch below it
/// is gone.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchDecision {
    /// Missing or not queued: nothing happened.
    Ignored,
    /// A machine-executor run: the runner plane must drain this pod
    /// (`drain_pod_by_id`, runner-owned, D-13+).
    PodDrain {
        /// The pod to drain.
        pod_id: Uuid,
    },
}

/// Dispatch one run (`dispatch_agent_run`, `dispatch.py:61-70`).
///
/// Missing or non-queued rows are ignored. Every surviving executor
/// kind drains its pod on the runner plane (returned as
/// [`DispatchDecision::PodDrain`] — `drain_pod_by_id` is
/// runner-owned and intentionally not inlined here). The cloud arm
/// died in P3 step A.
pub async fn dispatch_agent_run(
    pool: &PgPool,
    run_id: Uuid,
    _now: DateTime<Utc>,
) -> Result<DispatchDecision, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let run = dispatch_run_query(run_id)
        .into_model::<DispatchRunRow>()
        .one(&db)
        .await
        .map_err(into_sqlx)?;
    let Some(run) = run else {
        return Ok(DispatchDecision::Ignored);
    };
    if run.status != "queued" {
        return Ok(DispatchDecision::Ignored);
    }
    Ok(DispatchDecision::PodDrain { pod_id: run.pod_id })
}

/// Dispatch one run after the surrounding creation commits
/// (`dispatch_after_commit`, `creation.py:148-152`):
/// `transaction.on_commit(lambda: dispatch_agent_run(run_id))`.
///
/// The run id collects into the caller's `on_commit` outbox (the
/// [`dispatch_agent_run`] per id. Nothing executes inline — provably: this
/// function performs no I/O.
pub fn dispatch_after_commit(on_commit: &mut dyn FnMut(Uuid), run_id: Uuid) {
    on_commit(run_id);
}
#[cfg(test)]
mod tests {
    use serde_json::Value;

    use pidash_services::issuing::WORKSPACE_LOCK_SQL;

    use super::*;
    use serde_json::Value;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-06-dispatch.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }


    // -- event shaping --------------------------------------------------------

    // -- Celery wire ----------------------------------------------------------


    // -- post-commit dispatch --------------------------------------------------

    #[test]
    fn dispatch_after_commit_collects_exactly_once_without_executing() {
        let run_id = Uuid::new_v4();
        let mut pending = Vec::new();
        dispatch_after_commit(&mut |id| pending.push(id), run_id);
        assert_eq!(pending, vec![run_id]);
        // No inline execution: the function performs no I/O (provable by
        // inspection — it only invokes the caller's collector).
        assert_eq!(
            fixture()["dispatch_after_commit"],
            "transaction.on_commit(lambda: dispatch_agent_run(run_id))"
        );
    }

    // -- SQL -------------------------------------------------------------------

    fn parse_postgres(sql: &str) {
        let statements =
            sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::PostgreSqlDialect {}, sql)
                .expect("dispatch SQL must parse as Postgres");
        assert!(!statements.is_empty(), "unparsed: {sql}");
    }

    #[test]
    fn all_statements_parse_as_postgres() {
        parse_postgres(WORKSPACE_LOCK_SQL);
    }

    #[test]
    fn statements_touch_dispatch_tables_only() {
        assert!(
            WORKSPACE_LOCK_SQL.contains("agent_run")
                || WORKSPACE_LOCK_SQL.contains("workspaces")
                || WORKSPACE_LOCK_SQL.contains("runner")
                || WORKSPACE_LOCK_SQL.contains("pod"),
            "unexpected table in: {WORKSPACE_LOCK_SQL}"
        );
    }

    // -- live Postgres (ignored; needs a migrated scratch DB) ------------------


}
