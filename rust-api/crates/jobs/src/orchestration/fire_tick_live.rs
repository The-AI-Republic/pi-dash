#![forbid(unsafe_code)]

//! Live [`FireTickSeam`] for the worker-owned `fire_tick` (PIDASHCONV-861).
//!
//! [`FireTickShim`](super::fire_tick_seam::FireTickShim) cannot run live:
//! its `new_deps: Fn() -> StoreDeps` is sync, but presence
//! (`has_usable_llm_config`, `managed_llm_profile`) is only knowable
//! per fire. [`LiveFireTickSeam`] instead prefetches the creator
//! universe asynchronously on every dispatch (the HTTP path's
//! `dispatch_deps` shape), builds fresh [`StoreDeps`] over a
//! per-call cache, and runs the jobs continuation driver. The sync
//! state methods take the state row the caller resolved live in the
//! claim transaction (PIDASHCONV-1218 — a boot-time cache cannot see
//! a project/state created after start); `has_active_run` /
//! `has_prior_run` read live SQL.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::Utc;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings, RunnerSettings};
use pidash_db::entity::agent_run;
use pidash_db::orchestration::runs::ACTIVE_STATUSES;
use pidash_db::orm_tx::into_sqlx;
use pidash_db::pool::sea_orm_connection;
use pidash_services::extensions::{AssistantModelSeam, CloudAgentToolsetsSeam};
use pidash_services::runner_sessions::live_drains::{drain_pod_by_id, DrainError};

use super::creation_jobs::StoreDeps;
use super::fire_tick_seam::{
    dispatch_continuation_run, emit_logs, shim_is_ticking_state, shim_tick_interval_seconds,
    DispatchJobsError, StateView,
};
use super::live_inputs::{
    creator_universe, fetch_presence, gate_fn, gate_verdicts, profile_fn, JobsMatcher,
};
use crate::admission::RedisAdmissionCache;
use crate::dispatch::DispatchDecision;
use crate::tasks_ticker::fire_tick::{FireTickSeam, ProjectTickCols};

/// Live [`FireTickSeam`]: pool + dispatch settings + the injected
/// toolsets and assistant seams, plus the shared Redis client and
/// runner settings the continuation drain needs. One value serves
/// every fire; per-fire inputs (presence, llm-gate verdicts, cache,
/// `now`) are built inside the async methods, and the ticking state
/// is passed in from the claim read (live, never cached).
pub struct LiveFireTickSeam {
    pool: PgPool,
    cloud: CloudAgentSettings,
    managed: ManagedRunnerSettings,
    redis: Option<redis::Client>,
    runner: RunnerSettings,
    toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    assistant_seam: Arc<dyn AssistantModelSeam>,
}

impl LiveFireTickSeam {
    pub fn new(
        pool: PgPool,
        cloud: CloudAgentSettings,
        managed: ManagedRunnerSettings,
        redis: Option<redis::Client>,
        runner: RunnerSettings,
        toolsets: Arc<dyn CloudAgentToolsetsSeam>,
        assistant_seam: Arc<dyn AssistantModelSeam>,
    ) -> Self {
        Self {
            pool,
            cloud,
            managed,
            redis,
            runner,
            toolsets,
            assistant_seam,
        }
    }
}

impl FireTickSeam for LiveFireTickSeam {
    fn is_ticking_state(&self, state: Option<&StateView>) -> bool {
        shim_is_ticking_state(state)
    }

    fn tick_interval_seconds(&self, state: Option<&StateView>, project: &ProjectTickCols) -> i64 {
        shim_tick_interval_seconds(state, project)
    }

    fn has_active_run(
        &self,
        issue_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<bool, sqlx::Error>> + Send + '_>> {
        let pool = self.pool.clone();
        Box::pin(async move {
            let conn = sea_orm_connection(&pool);
            let statuses: Vec<String> = ACTIVE_STATUSES
                .iter()
                .map(|s| s.value().to_string())
                .collect();
            let found = agent_run::Entity::find()
                .filter(agent_run::Column::WorkItemId.eq(Some(issue_id)))
                .filter(agent_run::Column::Status.is_in(statuses))
                .select_only()
                .column(agent_run::Column::Id)
                .limit(1)
                .into_tuple::<Uuid>()
                .one(&conn)
                .await
                .map_err(into_sqlx)?;
            Ok(found.is_some())
        })
    }

    fn has_prior_run(
        &self,
        issue_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<bool, sqlx::Error>> + Send + '_>> {
        let pool = self.pool.clone();
        Box::pin(async move {
            let conn = sea_orm_connection(&pool);
            let found = agent_run::Entity::find()
                .filter(agent_run::Column::WorkItemId.eq(Some(issue_id)))
                .select_only()
                .column(agent_run::Column::Id)
                .limit(1)
                .into_tuple::<Uuid>()
                .one(&conn)
                .await
                .map_err(into_sqlx)?;
            Ok(found.is_some())
        })
    }

    fn dispatch_continuation_run(
        &self,
        issue_id: Uuid,
        triggered_by: &str,
        actor: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Uuid>, sqlx::Error>> + Send + '_>> {
        let pool = self.pool.clone();
        let cloud = self.cloud.clone();
        let managed = self.managed.clone();
        let redis = self.redis.clone();
        let runner = self.runner.clone();
        let toolsets = self.toolsets.clone();
        let assistant_seam = self.assistant_seam.clone();
        let matcher = JobsMatcher::new(pool.clone());
        let triggered_by = triggered_by.to_owned();
        Box::pin(async move {
            let universe = creator_universe(&pool, actor, issue_id).await?;
            let presence = Arc::new(fetch_presence(&pool, &universe).await?);
            let verdicts = Arc::new(gate_verdicts(&assistant_seam, &presence).await);
            let deps = StoreDeps {
                cloud,
                managed,
                cache: RedisAdmissionCache::from_env(),
                has_usable_llm_config: gate_fn(verdicts),
                toolsets,
                llm_profile: profile_fn(presence),
                now_unix_secs: Utc::now().timestamp(),
            };
            let now = Utc::now();
            // Continuation never retimes a clock (a bounce only
            // disarms), so the jitter draw is unread — 0.0.
            let outcome = dispatch_continuation_run(
                &pool,
                deps,
                &matcher,
                now,
                0.0,
                issue_id,
                &triggered_by,
                actor,
                false,
                false,
            )
            .await
            .map_err(|error| match error {
                DispatchJobsError::Db(db) => db,
                other => sqlx::Error::Protocol(other.to_string()),
            })?;
            // The driver's `dispatch_agent_run` only *returns* the
            // decision (`dispatch.py:61-70` drains the pod inline), so a
            // `PodDrain` must be acted on here or the continuation stays
            // QUEUED (PIDASHCONV-1494).
            for (_, decision) in &outcome.dispatch_decisions {
                if let DispatchDecision::PodDrain { pod_id } = decision {
                    drain_pod_by_id(&pool, redis.as_ref(), &runner, *pod_id)
                        .await
                        .map_err(|error| match error {
                            DrainError::Db(db) => db,
                            other => sqlx::Error::Protocol(other.to_string()),
                        })?;
                }
            }
            emit_logs(&outcome.outcome.logs);
            Ok(outcome.outcome.run_id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::extensions::NoExtraToolsets;

    fn state(id: u128, name: &str, group: &str) -> StateView {
        StateView {
            id: Uuid::from_u128(id),
            name: name.to_owned(),
            group: group.to_owned(),
        }
    }

    /// The trait defaults with a canned title: the CE behaviour lives
    /// in the defaults (the api crate's `CeAssistantSeam` only adds
    /// the title transport).
    #[derive(Debug)]
    struct DefaultSeam;

    impl AssistantModelSeam for DefaultSeam {
        fn generate_title(
            &self,
            request: pidash_services::extensions::TitleRequest,
        ) -> pidash_services::extensions::SeamFuture<
            '_,
            Result<String, pidash_services::extensions::TitleError>,
        > {
            let _ = request;
            Box::pin(async move { Ok("canned".to_owned()) })
        }
    }

    fn seam() -> LiveFireTickSeam {
        let pool = PgPool::connect_lazy("postgres://localhost:1/unused").expect("lazy pool builds");
        let settings = pidash_db::config::Settings::test_defaults();
        LiveFireTickSeam::new(
            pool,
            settings.cloud_agent,
            settings.managed_runner,
            None,
            settings.runner,
            Arc::new(NoExtraToolsets),
            Arc::new(DefaultSeam),
        )
    }

    #[tokio::test]
    async fn sync_methods_resolve_the_passed_state() {
        let seam = seam();
        let started = state(1, "In Progress", "started");
        let review = state(2, "In Review", "review");
        let done = state(3, "Done", "completed");
        let unknown = state(9, "Renamed", "started");
        assert!(seam.is_ticking_state(Some(&started)));
        assert!(seam.is_ticking_state(Some(&review)));
        assert!(!seam.is_ticking_state(Some(&done)));
        assert!(!seam.is_ticking_state(Some(&unknown)));
        assert!(!seam.is_ticking_state(None));
        let project = ProjectTickCols {
            pool: Some(10),
            ticking_enabled: Some(true),
            interval_default: Some(10800),
            interval_review: Some(7200),
            interval_test: Some(3600),
        };
        assert_eq!(seam.tick_interval_seconds(Some(&started), &project), 10800);
        assert_eq!(seam.tick_interval_seconds(Some(&review), &project), 7200);
        assert_eq!(seam.tick_interval_seconds(Some(&unknown), &project), 10800);
    }

    #[test]
    fn run_probe_statements_pin_shape() {
        use sea_orm::{DbBackend, QueryTrait};
        let issue_id = Uuid::from_u128(1);
        let statuses: Vec<String> = ACTIVE_STATUSES
            .iter()
            .map(|s| s.value().to_string())
            .collect();
        let active = agent_run::Entity::find()
            .filter(agent_run::Column::WorkItemId.eq(Some(issue_id)))
            .filter(agent_run::Column::Status.is_in(statuses))
            .select_only()
            .column(agent_run::Column::Id)
            .limit(1)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(active.contains("\"agent_run\""), "{active}");
        assert!(active.contains("\"work_item_id\""), "{active}");
        assert!(active.contains("\"status\" IN"), "{active}");
        assert!(active.contains("LIMIT 1"), "{active}");
        let prior = agent_run::Entity::find()
            .filter(agent_run::Column::WorkItemId.eq(Some(issue_id)))
            .select_only()
            .column(agent_run::Column::Id)
            .limit(1)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(prior.contains("\"agent_run\""), "{prior}");
        assert!(prior.contains("\"work_item_id\""), "{prior}");
        assert!(!prior.contains("\"status\" IN"), "{prior}");
    }

    #[tokio::test]
    async fn live_run_probes_read_seeded_rows() {
        let Some(url) = std::env::var("DATABASE_URL").ok() else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        // One-connection pool keeps temp tables visible to SeaORM
        // queries on the same pool.
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect one-connection pool");
        let mut tx = pool.begin().await.expect("begin");
        sqlx::query(
            "CREATE TEMPORARY TABLE agent_run (
                id UUID PRIMARY KEY, workspace_id UUID NOT NULL, created_by_id UUID NOT NULL,
                pod_id UUID, pinned_runner_id UUID, work_item_id UUID,
                scheduler_binding_id UUID, status TEXT NOT NULL, executor_kind TEXT NOT NULL,
                dispatch_attempts INTEGER NOT NULL, cancel_requested_at TIMESTAMPTZ,
                cancel_reason TEXT NOT NULL, tool_plan JSONB NOT NULL,
                prompt TEXT NOT NULL, trigger TEXT NOT NULL, lease_expires_at TIMESTAMPTZ,
                started_at TIMESTAMPTZ, llm_model TEXT NOT NULL, usage JSONB NOT NULL,
                done_payload JSONB, error_details JSONB NOT NULL, created_at TIMESTAMPTZ NOT NULL,
                phase_kind TEXT NOT NULL DEFAULT 'coding-task', agent_metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
                run_config JSONB NOT NULL DEFAULT '{}'::jsonb, required_capabilities JSONB NOT NULL DEFAULT '{}'::jsonb,
                thread_id TEXT NOT NULL DEFAULT '', owner_id UUID, runner_id UUID, parent_run_id UUID,
                assigned_at TIMESTAMPTZ, ended_at TIMESTAMPTZ, prompt_manifest JSONB, terminal_hooks_applied_at TIMESTAMPTZ,
                terminal_capacity_released_at TIMESTAMPTZ, input_tokens BIGINT, output_tokens BIGINT, total_tokens BIGINT,
                refusal_category TEXT)"
        )
        .execute(&mut *tx)
        .await
        .expect("temp agent_run");
        let issue = Uuid::new_v4();
        let user = Uuid::new_v4();
        let ws = Uuid::new_v4();
        let pod = Uuid::new_v4();
        let now = chrono::Utc::now();
        sqlx::query("INSERT INTO agent_run (id, workspace_id, created_by_id, pod_id, work_item_id, status, executor_kind, dispatch_attempts, cancel_reason, tool_plan, prompt, trigger, llm_model, usage, error_details, created_at) VALUES ($1, $2, $3, $4, $5, 'running', 'local_runner', 0, '', '{}', '', 'direct', '', '{}', '{}', $6)")
            .bind(Uuid::new_v4()).bind(ws).bind(user).bind(pod).bind(issue).bind(now)
            .execute(&mut *tx).await.expect("seed active run");
        // The pool path reads committed state; with a one-connection
        // pool the temp table is visible to the SeaORM exists check
        // on the same connection. Use the transaction's SeaORM bridge.
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        let statuses: Vec<String> = ACTIVE_STATUSES
            .iter()
            .map(|s| s.value().to_string())
            .collect();
        let found = agent_run::Entity::find()
            .filter(agent_run::Column::WorkItemId.eq(Some(issue)))
            .filter(agent_run::Column::Status.is_in(statuses.clone()))
            .select_only()
            .column(agent_run::Column::Id)
            .limit(1)
            .into_tuple::<Uuid>()
            .one(&conn)
            .await
            .expect("active exists");
        assert!(found.is_some());
        let missing = agent_run::Entity::find()
            .filter(agent_run::Column::WorkItemId.eq(Some(Uuid::new_v4())))
            .filter(agent_run::Column::Status.is_in(statuses))
            .select_only()
            .column(agent_run::Column::Id)
            .limit(1)
            .into_tuple::<Uuid>()
            .one(&conn)
            .await
            .expect("missing is none");
        assert!(missing.is_none());
    }
}
