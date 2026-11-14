#![forbid(unsafe_code)]

//! Creation dispatch for runs (D-11 L6, PIDASHCONV-487).
//!
//! What survives P3 step A (agent naming convention): the creation
//! re-exports (`execution_fields` and friends, owned by
//! `pidash_services::dispatch::execution`), the runner-plane
//! `dispatch_agent_run` (non-cloud kinds drain their pod), the
//! post-commit collector, and the `dispatch_waiting` stub. The cloud
//! lease/offer engine, the event append, and their tests died with
//! the retired plane; `dispatch_waiting` is removed with its trait
//! methods in P4.
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-06-dispatch.golden.json`
//! (FX-DISP-06), replayed by the suite below (trimmed in P3 step B).

use chrono::{DateTime, Utc};
use sea_orm::{
    ColumnTrait, EntityTrait, FromQueryResult, QueryFilter, QuerySelect, Select,
};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::config::CloudAgentSettings;
use pidash_db::entity::agent_run;
use pidash_db::orm_tx::into_sqlx;
use pidash_db::pool::sea_orm_connection;

/// The executor resolve + creation-capacity closure
/// (`managed_refusal_detail` … `lock_cloud_creation_capacity`) now lives in
/// `pidash_services::dispatch::execution` so the SeaORM-backed creation seam
/// (domain move path) can call it; this re-export keeps every
/// `pidash_jobs::dispatch::…` path stable.
pub use pidash_services::dispatch::execution::{
    defer_admission_error, execution_fields, lock_cloud_creation_capacity, lock_workspace,
    quota_verdict, ActorScope, DeferredAdmissionError, ExecutionFields, ExecutionFieldsError,
    ExecutionInputs, ExecutionSeams, ProjectScope, QUEUED_CLOUD_COUNT_SQL, WORKSPACE_LOCK_SQL,
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

/// The `extra_toolsets` plan flag (`runtime.py:36`): a sibling of the
/// name sets, read with Python truthiness over the plan value.
///
/// Survives from the retired execute plane by spec (P3 step A): the
/// SDK re-exports it and the overlay uses it until P4 removes it.
pub fn extra_toolsets_allowed(tool_plan: &Value) -> bool {
    match tool_plan.get("extra_toolsets") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => {
            number.as_i64().map(|n| n != 0).unwrap_or(false)
                || number.as_u64().map(|n| n != 0).unwrap_or(false)
                || number.as_f64().map(|n| n != 0.0).unwrap_or(false)
        }
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

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

/// Lease and offer the oldest queued rows up to workspace capacity
/// (`dispatch_waiting`, `dispatch.py:20-48`).
///
/// P3 step A: the cloud lease/offer engine is gone — no queue row
/// tail remains to offer into. This stub returns `Ok(0)` (offered
/// nothing) so the runner-plane callers stay compiling; the trait
/// methods and fakes are removed in P4.
pub async fn dispatch_waiting(
    _pool: &PgPool,
    _cloud: &CloudAgentSettings,
    _workspace_id: Uuid,
    _now: DateTime<Utc>,
) -> Result<usize, sqlx::Error> {
    Ok(0)
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
    _cloud: &CloudAgentSettings,
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
/// `DeferredConsume` precedent); the caller drains it after commit by awaiting
/// [`dispatch_agent_run`] per id. Nothing executes inline — provably: this
/// function performs no I/O.
pub fn dispatch_after_commit(on_commit: &mut dyn FnMut(Uuid), run_id: Uuid) {
    on_commit(run_id);
}
#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::dispatch::{build_tool_plan, CloudAgentAdmissionError};
    use serde_json::json;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-06-dispatch.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn django_cloud_settings() -> CloudAgentSettings {
        CloudAgentSettings {
            enabled: false,
            writes_enabled: false,
            github_tools_enabled: true,
            disabled_tools: Vec::new(),
            job_queue: "celery".to_string(),
            reconcile_interval_secs: 30,
            model_request_timeout_secs: 60,
            execution_timeout_secs: 285,
            run_soft_limit_secs: 300,
            run_hard_limit_secs: 330,
            stale_grace_secs: 60,
            dispatch_lease_secs: 60,
            dispatch_backoff_secs: 10,
            dispatch_scan_interval_secs: 10,
            sweep_interval_secs: 30,
            dispatch_scan_batch: 100,
            max_queue_age_secs: 900,
            model_request_limit: 25,
            tool_call_limit: 20,
            write_call_limit: 3,
            input_token_limit: 144_000,
            output_token_limit: 16_000,
            total_token_limit: 160_000,
            max_output_tokens_per_request: 4096,
            max_queued_per_workspace: 20,
            max_running_per_workspace: 2,
            user_creation_rate_per_minute: 6,
            workspace_creation_rate_per_minute: 30,
            tool_timeout_secs: 20,
            max_tool_result_bytes: 65536,
            max_prompt_bytes: 262_144,
            max_final_result_bytes: 65536,
            max_events: 500,
            block_private_urls: true,
        }
    }

    // -- quota verdict (pure core of both capacity gates) --------------------

    #[test]
    fn quota_verdict_matrix() {
        // Under the cap: admitted either way.
        assert_eq!(quota_verdict(19, 20, 10, false), Ok(None));
        assert_eq!(quota_verdict(19, 20, 10, true), Ok(None));
        // At the cap, manual raises with the retry-after (`creation.py:169-176`).
        let err = quota_verdict(20, 20, 10, false).expect_err("manual raises");
        assert_eq!(err.code(), "run_quota_exceeded");
        assert_eq!(
            err.to_string(),
            "Cloud Agent queue is full for this workspace"
        );
        assert_eq!(err.retry_after_seconds(), Some(10));
        // At the cap, automatic captures `{code, detail}` — the retry-after
        // the raised error carries is dropped (`creation.py:70,175`).
        let deferred = quota_verdict(20, 20, 10, true).expect("automatic defers");
        assert_eq!(
            deferred,
            Some(DeferredAdmissionError {
                code: "run_quota_exceeded".to_owned(),
                detail: "Cloud Agent queue is full for this workspace".to_owned(),
            })
        );
    }

    #[test]
    fn admission_capture_drops_retry_after() {
        let exc = CloudAgentAdmissionError::new("run_quota_exceeded", "detail held", Some(42));
        assert_eq!(
            defer_admission_error(&exc),
            DeferredAdmissionError {
                code: "run_quota_exceeded".to_owned(),
                detail: "detail held".to_owned(),
            }
        );
    }

    // -- event shaping --------------------------------------------------------

    // -- Celery wire ----------------------------------------------------------

    // -- tool-plan snapshot shape ---------------------------------------------

    #[test]
    fn tool_plan_snapshot_keeps_key_order_and_limits() {
        // The cloud `tool_plan` half of `execution_fields` (`creation.py:73-81`)
        // through the real L3 builder: key order, Django-default limits, the
        // `extra_toolsets` sibling flag.
        let cloud = CloudAgentSettings {
            enabled: true,
            github_tools_enabled: false,
            ..django_cloud_settings()
        };
        let plan = build_tool_plan::<fn() -> bool, fn() -> bool>(
            "issue",
            true,
            &[],
            None,
            Some(|| true),
            &cloud,
        )
        .expect("empty requirements plan");
        let value = serde_json::to_value(&plan).expect("plan serializes");
        let keys: Vec<&str> = value
            .as_object()
            .expect("plan is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "v",
                "catalog_version",
                "tools",
                "required_tools",
                "limits",
                "unavailable_capabilities",
                "extra_toolsets",
            ],
            "creation.py:135-158 dict order"
        );
        assert_eq!(
            value["limits"],
            json!({
                "model_requests": 25,
                "tool_calls": 20,
                "writes": 3,
                "input_tokens": 144_000,
                "output_tokens": 16_000,
                "total_tokens": 160_000,
                "wall_seconds": 285,
            }),
            "settings/common.py defaults"
        );
        assert_eq!(value["extra_toolsets"], true);
        assert_eq!(
            value["unavailable_capabilities"],
            json!(["filesystem", "shell", "worktree"])
        );
    }

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
        for sql in [WORKSPACE_LOCK_SQL, QUEUED_CLOUD_COUNT_SQL] {
            parse_postgres(sql);
        }
    }

    #[test]
    fn statements_touch_dispatch_tables_only() {
        for sql in [WORKSPACE_LOCK_SQL, QUEUED_CLOUD_COUNT_SQL] {
            assert!(
                sql.contains("agent_run")
                    || sql.contains("workspaces")
                    || sql.contains("runner")
                    || sql.contains("pod"),
                "unexpected table in: {sql}"
            );
        }
    }

    // -- live Postgres (ignored; needs a migrated scratch DB) ------------------


}
