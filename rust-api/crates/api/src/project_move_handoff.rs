//! Shared api-side D-12 project-move handoff driver (PIDASHCONV-785).
//!
//! Python fires `complete_project_move_handoff(run_id)` from several
//! post-commit positions: `Runner.revoke` (`models.py:653-674`), the
//! session reaper's `_drain_after_commit` (`session_service.py:277-290`)
//! and the run-lifecycle endpoints. The api crate has one executor for
//! it: the pool-backed twin of `jobs::LiveCreationStore` below, moved
//! here verbatim from `runner_enroll/teardown.rs` so every caller shares
//! one home instead of forking the seam. The D-12 decisions stay in the
//! services driver (`orchestration::creation`); this module only
//! supplies the store and drains the handoff tx's `on_commit` effects.
//!
//! [`complete_project_move_handoff`] is the one callable entry.
//! [`drain_one_publish`] is shared with teardown's revoke fire plan.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_db::dispatch::status::AgentRunStatus as DbAgentRunStatus;
use pidash_db::tx::Transaction as DbTransaction;
use pidash_jobs::dispatch::{
    dispatch_after_commit as collect_dispatch, dispatch_agent_run,
    execution_fields as resolve_execution_fields, lock_cloud_creation_capacity as lock_capacity,
    ActorScope, DeferredAdmissionError, ExecutionFieldsError, ExecutionInputs, ExecutionSeams,
    ProjectScope,
};
use pidash_services::assistant::seams as assistant_seams;
use pidash_services::dispatch::tools as dispatch_tools;
use pidash_services::dispatch::{
    consume_admission_token, AdmissionCache, DeferredConsume, LlmProfile, UserFlags as SvcUserFlags,
};
use pidash_services::extensions::{CloudAgentToolsetsSeam, NoExtraToolsets};
use pidash_services::orchestration::creation::{
    self as creation_kernel, active_run_sql, latest_prior_run_sql, run_insert_returning_sql,
    run_lock_sql, run_select_sql, user_id_for_run, ASSIGNED_POD_SELECT_SQL,
    BUNDLE_ANCESTOR_HOP_SQL, BUNDLE_ASSIGNEES_SQL, BUNDLE_CHILDREN_SQL, BUNDLE_CODE_REVIEWS_SQL,
    BUNDLE_COMMENTS_SQL, BUNDLE_DONE_PAYLOAD_SQL, BUNDLE_LABELS_SQL, BUNDLE_OVERRIDES_SQL,
    BUNDLE_PARENT_COLS_SQL, BUNDLE_PARENT_DESCRIPTION_SQL, BUNDLE_PRIOR_RUN_COUNT_SQL,
    BUNDLE_PROJECT_IDENTIFIER_SQL, BUNDLE_PROJECT_STATES_SQL, BUNDLE_RELATIONS_SQL,
    BUNDLE_RELATION_TARGETS_SQL, BUNDLE_REMOTE_SQL, BUNDLE_SEQUENCE_SQL, BUNDLE_TICKER_SQL,
    BUNDLE_WORKSPACE_SQL, DEFAULT_POD_SELECT_SQL, FINALIZE_UPDATE_SQL, ISSUE_LOCK_SQL,
    ISSUE_SELECT_SQL, PROJECT_SELECT_SQL, PROMPT_UPDATE_SQL, RUNNER_SELECT_SQL,
    RUN_CONFIG_UPDATE_SQL, STATE_SELECT_SQL, TERMINAL_EVENT_EXISTS_SQL, TERMINAL_EVENT_INSERT_SQL,
    TERMINAL_EVENT_SEQ_SQL, TICKER_RESUME_SELECT_SQL, USER_FLAGS_SELECT_SQL,
    WORK_ITEM_ID_SELECT_SQL,
};
use pidash_services::orchestration::creation::{
    finalize_lock_sql, AdmissionError as ServiceAdmissionError, CreationError, CreationSeam,
    ExecutionError, ExecutionFields as ServiceExecutionFields, ExecutionRequest,
    FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, PodView, ProjectView, RenderBundle,
    RunView, RunnerView, StateView, TickerBudget,
};
use pidash_services::prompting::{composer, context};
use pidash_services::runner_runs::finalization as finalize_kernel;
use pidash_services::runner_runs::LifecycleEffect;
use pidash_types::dispatch::AgentExecutorKind;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// Clocks + Redis (the `teardown.rs` positions, restated per module)
// ---------------------------------------------------------------------------

/// `timezone.now()` truncated to microseconds (Django datetimes are
/// microsecond-exact; Postgres would round stored nanos).
fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).expect("micros in range")
}

/// `int(time.time())` for the admission bucket clock.
fn unix_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// Api-crate-owned Redis client: `None` when `REDIS_URL` is unset,
/// empty, or unparsable, mirroring `redis_instance()` returning `None`.
fn redis_client(state: &AppState) -> Option<redis::Client> {
    state
        .settings()
        .redis
        .url
        .as_deref()
        .filter(|url| !url.is_empty())
        .and_then(|url| redis::Client::open(url).ok())
}

// ---------------------------------------------------------------------------
// Publish drains (shared with teardown's revoke fire plan)
// ---------------------------------------------------------------------------

/// Drain one publish effect, isolated with its failure log
/// (`agent_run_finalization.py` `_publish_effects`: the emit and the
/// inline apply sit in separate `try` blocks).
pub(crate) async fn drain_one_publish(
    pool: &PgPool,
    ports: &crate::runner_runs::LivePorts,
    effect: LifecycleEffect,
) {
    let label = match &effect {
        LifecycleEffect::PublishTerminalEffects { run_id } => {
            format!("failed to publish terminal effects for run {run_id}")
        }
        LifecycleEffect::ApplyTerminalEffectsInline { run_id } => {
            format!("failed to apply terminal effects for run {run_id}")
        }
        unexpected => format!("failed to drain unexpected terminal effect: {unexpected:?}"),
    };
    if crate::runner_runs::run_endpoints::drain_lifecycle_effects(pool, ports, vec![effect])
        .await
        .is_err()
    {
        tracing::error!("{label}");
    }
}

/// Drain finalized runs' publish pairs in order.
async fn drain_publish_effects(pool: &PgPool, state: &AppState, effects: Vec<LifecycleEffect>) {
    if effects.is_empty() {
        return;
    }
    let ports = crate::runner_runs::LivePorts::new(pool.clone(), state);
    for effect in effects {
        drain_one_publish(pool, &ports, effect).await;
    }
}

// ---------------------------------------------------------------------------
// The D-12 handoff twin seam (the driver is called, never inlined)
// ---------------------------------------------------------------------------

/// Redis-backed [`AdmissionCache`]: the first production implementation
/// of the sync trait. Each call opens a short sync connection (the
/// trait is sync; only cloud admission paths touch it). An unset or
/// unusable `REDIS_URL` fails closed like Python's backend failure.
#[derive(Clone)]
struct RedisAdmissionCache {
    client: Option<redis::Client>,
}

fn cache_unavailable<T>(what: &'static str) -> Result<T, redis::RedisError> {
    Err(redis::RedisError::from((
        redis::ErrorKind::Io,
        what,
        "redis unavailable".to_owned(),
    )))
}

impl AdmissionCache for RedisAdmissionCache {
    type Error = redis::RedisError;

    fn bucket_count(&self, key: &str) -> Result<Option<i64>, Self::Error> {
        let Some(client) = &self.client else {
            return cache_unavailable("bucket_count");
        };
        let mut connection = client.get_connection()?;
        let value: Option<i64> = redis::cmd("GET").arg(key).query(&mut connection)?;
        Ok(value)
    }

    fn add_or_incr(&self, key: &str, timeout_secs: i64) -> Result<(), Self::Error> {
        let Some(client) = &self.client else {
            return cache_unavailable("add_or_incr");
        };
        let mut connection = client.get_connection()?;
        // `cache.add(key, 1, timeout) or cache.incr(key)`: set-if-absent
        // with TTL, else increment.
        let set: Option<String> = redis::cmd("SET")
            .arg(key)
            .arg(1)
            .arg("EX")
            .arg(timeout_secs)
            .arg("NX")
            .query(&mut connection)?;
        if set.is_none() {
            let _: i64 = redis::cmd("INCR").arg(key).query(&mut connection)?;
        }
        Ok(())
    }
}

/// Pool-backed [`CreationSeam`] + [`FinalizeAgentRunSeam`]: the api-side
/// twin of `jobs::LiveCreationStore` (same builders, same order, same
/// decodes — cited per method). The jobs store cannot serve api
/// requests (its `StoreDeps` closures are sync and have no production
/// factory), and the quarantine keeps the twin in this file; the
/// D-12 decisions stay in the services driver in both.
struct HandoffStore<'t, 'p> {
    tx: DbTransaction<'t>,
    pool: &'p PgPool,
    cloud: CloudAgentSettings,
    managed: ManagedRunnerSettings,
    cache: RedisAdmissionCache,
    now_unix_secs: i64,
    dispatches: Vec<Uuid>,
    deferred: Vec<DeferredConsume>,
    terminal_effects: Vec<Uuid>,
}

impl<'t, 'p> HandoffStore<'t, 'p> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        tx: DbTransaction<'t>,
        pool: &'p PgPool,
        cloud: CloudAgentSettings,
        managed: ManagedRunnerSettings,
        cache: RedisAdmissionCache,
        now_unix_secs: i64,
    ) -> Self {
        Self {
            tx,
            pool,
            cloud,
            managed,
            cache,
            now_unix_secs,
            dispatches: Vec::new(),
            deferred: Vec::new(),
            terminal_effects: Vec::new(),
        }
    }

    fn db(error: sqlx::Error) -> CreationError {
        CreationError::Db(error.to_string())
    }

    fn into_parts(
        self,
    ) -> (
        DbTransaction<'t>,
        Vec<Uuid>,
        Vec<DeferredConsume>,
        Vec<Uuid>,
        RedisAdmissionCache,
    ) {
        (
            self.tx,
            self.dispatches,
            self.deferred,
            self.terminal_effects,
            self.cache,
        )
    }
}

fn twin_decode_error(column: &str, value: &str) -> sqlx::Error {
    sqlx::Error::Decode(format!("unknown agent_run.{column} {value:?}").into())
}

fn twin_map_status(value: String) -> Result<DbAgentRunStatus, sqlx::Error> {
    DbAgentRunStatus::from_value(&value).ok_or_else(|| twin_decode_error("status", &value))
}

fn twin_map_executor(value: String) -> Result<AgentExecutorKind, sqlx::Error> {
    // Strictness is safe here (unlike the trigger): the
    // `agent_run_cloud_has_no_local_assignment` check constraint
    // (`runner/models.py:1065-1075`, migration 0024) admits only the
    // three members, so an unknown stored value is unreachable.
    AgentExecutorKind::from_value(&value).ok_or_else(|| twin_decode_error("executor_kind", &value))
}

fn twin_map_run_view(row: &sqlx::postgres::PgRow) -> Result<RunView, sqlx::Error> {
    let status: String = row.try_get("status")?;
    let trigger: String = row.try_get("trigger")?;
    let executor_kind: String = row.try_get("executor_kind")?;
    Ok(RunView {
        id: row.try_get("id")?,
        workspace_id: row.try_get("workspace_id")?,
        created_by_id: row.try_get("created_by_id")?,
        pod_id: row.try_get("pod_id")?,
        runner_id: row.try_get("runner_id")?,
        pinned_runner_id: row.try_get("pinned_runner_id")?,
        parent_run_id: row.try_get("parent_run_id")?,
        work_item_id: row.try_get("work_item_id")?,
        status: twin_map_status(status)?,
        trigger,
        executor_kind: twin_map_executor(executor_kind)?,
        phase_kind: row.try_get("phase_kind")?,
        run_config: row.try_get("run_config")?,
        tool_plan: row.try_get("tool_plan")?,
        error_code: row.try_get("error_code")?,
        error: row.try_get("error")?,
        prompt: row.try_get("prompt")?,
        prompt_manifest: row.try_get("prompt_manifest")?,
        ended_at: row.try_get("ended_at")?,
    })
}

fn twin_map_issue(row: &sqlx::postgres::PgRow) -> Result<IssueView, sqlx::Error> {
    Ok(IssueView {
        id: row.try_get("id")?,
        workspace_id: row.try_get("workspace_id")?,
        project_id: row.try_get("project_id")?,
        state_id: row.try_get("state_id")?,
        parent_id: row.try_get("parent_id")?,
        created_by_id: row.try_get("created_by_id")?,
        assigned_pod_id: row.try_get("assigned_pod_id")?,
        agent_executor: row.try_get("agent_executor")?,
        git_work_branch: row.try_get("git_work_branch")?,
        workpad: row.try_get("workpad")?,
        name: row.try_get("name")?,
        description_stripped: row.try_get("description_stripped")?,
        priority: row.try_get("priority")?,
        sequence_id: row.try_get("sequence_id")?,
        target_date: row.try_get("target_date")?,
    })
}

fn twin_map_project(row: &sqlx::postgres::PgRow) -> Result<ProjectView, sqlx::Error> {
    let pool: i32 = row.try_get("agent_default_max_ticks")?;
    let interval_impl: i32 = row.try_get("agent_default_interval_seconds")?;
    let interval_review: i32 = row.try_get("agent_review_default_interval_seconds")?;
    let interval_test: i32 = row.try_get("agent_test_default_interval_seconds")?;
    Ok(ProjectView {
        id: row.try_get("id")?,
        workspace_id: row.try_get("workspace_id")?,
        identifier: row.try_get("identifier")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        repo_url: row.try_get("repo_url")?,
        base_branch: row.try_get("base_branch")?,
        default_agent_executor: row.try_get("default_agent_executor")?,
        project_lead_id: row.try_get("project_lead_id")?,
        default_assignee_id: row.try_get("default_assignee_id")?,
        pool: i64::from(pool),
        interval_impl: i64::from(interval_impl),
        interval_review: i64::from(interval_review),
        interval_test: i64::from(interval_test),
    })
}

impl CreationSeam for HandoffStore<'_, '_> {
    async fn issue(&mut self, issue_id: Uuid) -> Result<IssueView, CreationError> {
        let row = sqlx::query(ISSUE_SELECT_SQL)
            .bind(issue_id)
            .fetch_one(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        twin_map_issue(&row).map_err(Self::db)
    }

    async fn project(&mut self, project_id: Uuid) -> Result<ProjectView, CreationError> {
        let row = sqlx::query(PROJECT_SELECT_SQL)
            .bind(project_id)
            .fetch_one(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        twin_map_project(&row).map_err(Self::db)
    }

    async fn state(&mut self, state_id: Option<Uuid>) -> Result<Option<StateView>, CreationError> {
        let Some(state_id) = state_id else {
            return Ok(None);
        };
        let row: Option<(Uuid, String, String)> = sqlx::query_as(STATE_SELECT_SQL)
            .bind(state_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        match row {
            Some((id, name, group)) => Ok(Some(StateView { id, name, group })),
            None => Err(CreationError::MissingRow(format!("no state {state_id}"))),
        }
    }

    async fn latest_prior_run(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = sqlx::query(&latest_prior_run_sql())
            .bind(issue_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        row.map(|row| twin_map_run_view(&row))
            .transpose()
            .map_err(Self::db)
    }

    async fn active_run_for(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = sqlx::query(&active_run_sql())
            .bind(issue_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        row.map(|row| twin_map_run_view(&row))
            .transpose()
            .map_err(Self::db)
    }

    async fn run(&mut self, run_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = sqlx::query(&run_select_sql())
            .bind(run_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        row.map(|row| twin_map_run_view(&row))
            .transpose()
            .map_err(Self::db)
    }

    async fn runner(&mut self, runner_id: Uuid) -> Result<Option<RunnerView>, CreationError> {
        let row: Option<(Uuid, Uuid, String)> = sqlx::query_as(RUNNER_SELECT_SQL)
            .bind(runner_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(row.map(|(id, pod_id, status)| RunnerView { id, pod_id, status }))
    }

    async fn assigned_pod(&mut self, pod_id: Uuid) -> Result<Option<PodView>, CreationError> {
        let row: Option<(Uuid, Uuid)> = sqlx::query_as(ASSIGNED_POD_SELECT_SQL)
            .bind(pod_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(row.map(|(id, project_id)| PodView { id, project_id }))
    }

    async fn default_pod_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<PodView>, CreationError> {
        let row: Option<(Uuid, Uuid)> = sqlx::query_as(DEFAULT_POD_SELECT_SQL)
            .bind(project_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(row.map(|(id, project_id)| PodView { id, project_id }))
    }

    async fn resume_parent_run_id(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<Uuid>, CreationError> {
        let row: Option<(Option<Uuid>,)> = sqlx::query_as(TICKER_RESUME_SELECT_SQL)
            .bind(issue_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(row.and_then(|row| row.0))
    }

    async fn work_item_id_for_run(&mut self, run_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        let row: Option<(Option<Uuid>,)> = sqlx::query_as(WORK_ITEM_ID_SELECT_SQL)
            .bind(run_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(row.and_then(|row| row.0))
    }

    async fn lock_issue_for_handoff(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<LockedIssue>, CreationError> {
        let locked: Option<(Uuid,)> = sqlx::query_as(ISSUE_LOCK_SQL)
            .bind(issue_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        if locked.is_none() {
            return Ok(None);
        }
        let issue = self.issue(issue_id).await?;
        let project_id = issue
            .project_id
            .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
        let project = self.project(project_id).await?;
        let state = self.state(issue.state_id).await?;
        Ok(Some(LockedIssue {
            issue,
            project,
            state,
        }))
    }

    async fn lock_run_for_handoff(
        &mut self,
        run_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        let row = sqlx::query(&run_lock_sql())
            .bind(run_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        row.map(|row| twin_map_run_view(&row))
            .transpose()
            .map_err(Self::db)
    }

    async fn user_flags(&mut self, user_id: Uuid) -> Result<SvcUserFlags, CreationError> {
        let row: (bool, bool) = sqlx::query_as(USER_FLAGS_SELECT_SQL)
            .bind(user_id)
            .fetch_one(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(SvcUserFlags {
            is_active: row.0,
            is_bot: row.1,
        })
    }

    async fn insert_run(&mut self, row: &NewAgentRun) -> Result<RunView, CreationError> {
        let inserted = sqlx::query(&run_insert_returning_sql())
            .bind(row.id)
            .bind(row.workspace_id)
            .bind(row.created_by_id)
            .bind(row.pod_id)
            .bind(row.pinned_runner_id)
            .bind(row.work_item_id)
            .bind(row.parent_run_id)
            .bind(row.executor_kind.value())
            .bind(&row.error_code)
            .bind(&row.tool_plan)
            .bind(&row.trigger)
            .bind(&row.phase_kind)
            .bind(&row.run_config)
            .bind(row.now)
            .fetch_one(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        twin_map_run_view(&inserted).map_err(Self::db)
    }

    async fn save_prompt(
        &mut self,
        run_id: Uuid,
        prompt: &str,
        manifest: &Value,
    ) -> Result<(), CreationError> {
        sqlx::query(PROMPT_UPDATE_SQL)
            .bind(prompt)
            .bind(manifest)
            .bind(run_id)
            .execute(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(())
    }

    async fn save_run_config(&mut self, run_id: Uuid, config: &Value) -> Result<(), CreationError> {
        sqlx::query(RUN_CONFIG_UPDATE_SQL)
            .bind(config)
            .bind(run_id)
            .execute(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(())
    }

    async fn execution_fields(
        &mut self,
        req: &ExecutionRequest,
    ) -> Result<ServiceExecutionFields, ExecutionError> {
        let project = ProjectScope {
            project_id: req.project_id,
            workspace_id: req.workspace_id,
            default_agent_executor: req.default_agent_executor.clone(),
        };
        let actor = req.actor.map(|actor| ActorScope {
            id: actor.id,
            flags: actor.flags,
        });
        let actor_id = actor.as_ref().map(|actor| actor.id);
        // The jobs store's H/E/P closures have no production factory, so
        // the twin prefetches their facts asynchronously and moves the
        // values into the `FnOnce` seams (identical verdicts).
        let has_key = match actor_id {
            Some(id) => twin_has_api_key(self.pool, id)
                .await
                .map_err(|detail| ExecutionError::Store(CreationError::Db(detail)))?,
            None => false,
        };
        let profile = {
            let mapped = assistant_seams::agent_model_profile_for_user(has_key);
            LlmProfile {
                available: mapped.available,
                reason_code: mapped.reason_code,
            }
        };
        let has_llm = assistant_seams::has_usable_llm_config(has_key);
        let Self {
            pool,
            cache,
            cloud,
            managed,
            now_unix_secs,
            deferred,
            ..
        } = self;
        let mut short = DbTransaction::begin(pool)
            .await
            .map_err(|error| ExecutionError::Store(CreationError::Db(error.to_string())))?;
        let inputs = ExecutionInputs {
            project: &project,
            run_kind: &req.run_kind,
            has_issue: req.has_issue,
            required_capabilities: &[],
            actor: actor.as_ref(),
            automatic: req.automatic,
            requested: req.requested.as_deref(),
            now_unix_secs: *now_unix_secs,
        };
        let mut collect = |consume: DeferredConsume| deferred.push(consume);
        let fields = resolve_execution_fields(
            &mut short,
            &inputs,
            cloud,
            managed,
            cache,
            &mut collect,
            ExecutionSeams {
                has_usable_llm_config: move || has_llm,
                extra_toolsets_enabled: move || dispatch_tools::extra_toolsets_enabled_for(),
                llm_profile: move || profile,
            },
        )
        .await;
        short
            .commit()
            .await
            .map_err(|error| ExecutionError::Store(CreationError::Db(error.to_string())))?;
        fields
            .map(twin_map_execution_fields)
            .map_err(twin_map_execution_error)
    }

    async fn lock_cloud_creation_capacity(
        &mut self,
        workspace_id: Uuid,
        executor_kind: AgentExecutorKind,
        automatic: bool,
    ) -> Result<Option<ServiceAdmissionError>, CreationError> {
        let outcome = lock_capacity(
            &mut self.tx,
            workspace_id,
            &executor_kind,
            automatic,
            &self.cloud,
        )
        .await;
        match outcome {
            Ok(deferred) => Ok(deferred.map(twin_map_admission_error)),
            Err(ExecutionFieldsError::Admission(exc)) => {
                Err(CreationError::CapacityRefused(exc.detail().to_owned()))
            }
            Err(ExecutionFieldsError::Db(error)) => Err(Self::db(error)),
            // Unreachable: the lock path only locks, counts and
            // verdicts (no resolve/LLM/tool gates).
            Err(other) => Err(CreationError::Db(format!("unexpected lock error: {other}"))),
        }
    }

    fn dispatch_after_commit(&mut self, run_id: Uuid) {
        collect_dispatch(&mut |id| self.dispatches.push(id), run_id);
    }

    async fn render_bundle(
        &mut self,
        issue_id: Uuid,
        run_id: Uuid,
        parent_run_id: Option<Uuid>,
        trigger: &str,
        created_by_id: Uuid,
    ) -> Result<RenderBundle, CreationError> {
        twin_load_render_bundle(
            &mut self.tx,
            issue_id,
            run_id,
            parent_run_id,
            trigger,
            created_by_id,
        )
        .await
    }

    fn extra_toolsets_schema_tool(&self) -> String {
        NoExtraToolsets.schema_tool_name().to_owned()
    }
}

fn twin_map_execution_fields(
    fields: pidash_jobs::dispatch::ExecutionFields,
) -> ServiceExecutionFields {
    let pinned_runner_entry = match fields.executor_kind {
        AgentExecutorKind::CloudAgent => Some(None),
        AgentExecutorKind::ManagedRunner => Some(fields.pinned_runner_id),
        AgentExecutorKind::LocalRunner => None,
    };
    ServiceExecutionFields {
        executor_kind: fields.executor_kind,
        tool_plan: fields.tool_plan,
        pinned_runner_entry,
        error_code: fields.error_code,
        cloud_admission_error: fields.cloud_admission_error.map(twin_map_admission_error),
    }
}

fn twin_map_execution_error(error: ExecutionFieldsError) -> ExecutionError {
    match error {
        ExecutionFieldsError::Db(error) => {
            ExecutionError::Store(CreationError::Db(error.to_string()))
        }
        ExecutionFieldsError::Admission(exc) => ExecutionError::Refused(exc.detail().to_owned()),
        other => ExecutionError::Refused(other.to_string()),
    }
}

fn twin_map_admission_error(error: DeferredAdmissionError) -> ServiceAdmissionError {
    ServiceAdmissionError {
        code: error.code,
        detail: error.detail,
    }
}

/// `get_config(user)` (`llm.py:67-68`):
/// `UserLLMConfig.objects.filter(user).first()` — full row, unordered
/// (`LIMIT 1`, no `ORDER BY`). `$1` = user id.
const TWIN_LLM_CONFIG_SQL: &str = r#"SELECT "id", "user_id", "provider_kind", "base_url", "model_name", "api_key_encrypted", "last_verified_at", "created_at", "updated_at" FROM "assistant_user_llm_config" WHERE "assistant_user_llm_config"."user_id" = $1 LIMIT 1"#;

/// `get_config(user)` + `has_api_key` (`models.py:260-262`): key present
/// and non-empty.
async fn twin_has_api_key(pool: &PgPool, user_id: Uuid) -> Result<bool, String> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(TWIN_LLM_CONFIG_SQL)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| error.to_string())?;
    let Some(row) = row else {
        return Ok(false);
    };
    let key: Option<Vec<u8>> = row
        .try_get("api_key_encrypted")
        .map_err(|error| error.to_string())?;
    Ok(key.is_some_and(|key| !key.is_empty()))
}

fn twin_db(error: sqlx::Error) -> CreationError {
    CreationError::Db(error.to_string())
}

/// Python `datetime.isoformat()` for an aware UTC timestamp (`+00:00`,
/// microseconds only when nonzero — `context.py:322`).
fn twin_render_isoformat(dt: DateTime<Utc>) -> String {
    if dt.timestamp_subsec_micros() == 0 {
        dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
    } else {
        dt.to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
    }
}

async fn twin_bundle_labels(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<String>, CreationError> {
    sqlx::query_scalar(BUNDLE_LABELS_SQL)
        .bind(issue_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)
}

async fn twin_bundle_assignees(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<String>, CreationError> {
    let rows: Vec<(Option<String>, Option<String>)> = sqlx::query_as(BUNDLE_ASSIGNEES_SQL)
        .bind(issue_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(|(display, email)| context::assignee_display(display.as_deref(), email.as_deref()))
        .collect())
}

async fn twin_bundle_project_states(
    tx: &mut DbTransaction<'_>,
    project_id: Uuid,
) -> Result<Vec<context::ProjectStateView>, CreationError> {
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(BUNDLE_PROJECT_STATES_SQL)
        .bind(project_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(|(name, group, description)| context::ProjectStateView {
            name,
            group,
            description,
        })
        .collect())
}

type TwinChildCols = (Uuid, Option<String>, i32, String, Option<String>);
type TwinTargetCols = (
    Uuid,
    Option<String>,
    i32,
    String,
    Option<String>,
    Option<String>,
);
type TwinCommentCols = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<Uuid>,
    Option<DateTime<Utc>>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<bool>,
);
type TwinReviewCols = (String, Option<String>, String, bool, bool, String, String);
type TwinParentCols = (Option<String>, Option<Uuid>, Option<String>, i64);

async fn twin_bundle_children(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::IssueRef>, CreationError> {
    let rows: Vec<TwinChildCols> = sqlx::query_as(BUNDLE_CHILDREN_SQL)
        .bind(issue_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(
            |(_, name, sequence, project_identifier, state)| context::IssueRef {
                identifier: context::issue_identifier(&project_identifier, i64::from(sequence)),
                title: name.unwrap_or_default(),
                state: state.unwrap_or_default(),
            },
        )
        .collect())
}

async fn twin_bundle_relations(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<
    (
        Vec<context::RelationRow>,
        std::collections::HashMap<String, context::IssueRef>,
        std::collections::HashMap<String, context::DirectionalRef>,
    ),
    CreationError,
> {
    let rows: Vec<(Uuid, Uuid, String)> = sqlx::query_as(BUNDLE_RELATIONS_SQL)
        .bind(issue_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let mut other_ids: Vec<Uuid> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for (left, right, _) in &rows {
        let other = if left == &issue_id { *right } else { *left };
        if other != issue_id && seen_ids.insert(other) {
            other_ids.push(other);
        }
    }
    let targets: Vec<TwinTargetCols> = if other_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as(BUNDLE_RELATION_TARGETS_SQL)
            .bind(&other_ids)
            .fetch_all(&mut **tx.inner())
            .await
            .map_err(twin_db)?
    };
    let mut refs = std::collections::HashMap::new();
    let mut directional_refs = std::collections::HashMap::new();
    for (id, name, sequence, project_identifier, state, group) in targets {
        let key = id.to_string();
        let title = name.unwrap_or_default();
        let state_name = state.unwrap_or_default();
        refs.insert(
            key.clone(),
            context::IssueRef {
                identifier: context::issue_identifier(&project_identifier, i64::from(sequence)),
                title: title.clone(),
                state: state_name.clone(),
            },
        );
        directional_refs.insert(
            key,
            context::DirectionalRef {
                identifier: context::issue_identifier(&project_identifier, i64::from(sequence)),
                title,
                state: state_name,
                state_group: group.unwrap_or_default(),
            },
        );
    }
    let relation_rows = rows
        .into_iter()
        .map(|(left, right, relation_type)| context::RelationRow {
            issue_id: left.to_string(),
            related_issue_id: right.to_string(),
            relation_type,
        })
        .collect();
    Ok((relation_rows, refs, directional_refs))
}

async fn twin_bundle_comments(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::CommentView>, CreationError> {
    let rows: Vec<TwinCommentCols> = sqlx::query_as(BUNDLE_COMMENTS_SQL)
        .bind(issue_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(
            |(
                stripped,
                speaker_type,
                speaker_label,
                run_id,
                created_at,
                display,
                email,
                username,
                is_bot,
            )| {
                let actor = match (&display, &email, &username, &is_bot) {
                    (None, None, None, None) => None,
                    _ => Some(context::ActorView {
                        display_name: display,
                        email,
                        username,
                        is_bot: is_bot.unwrap_or(false),
                    }),
                };
                context::CommentView {
                    body: stripped.unwrap_or_default(),
                    speaker_type: speaker_type.unwrap_or_default(),
                    speaker_label,
                    actor,
                    created_at_iso: created_at.map(twin_render_isoformat),
                    run_id: run_id.map(|id| id.to_string()),
                }
            },
        )
        .collect())
}

async fn twin_bundle_reviews(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::CodeReviewView>, CreationError> {
    let rows: Vec<TwinReviewCols> = sqlx::query_as(BUNDLE_CODE_REVIEWS_SQL)
        .bind(issue_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(
            |(url, title, state, merged, draft, provider, external_iid)| context::CodeReviewView {
                url,
                title,
                state,
                merged,
                draft,
                provider,
                external_iid,
            },
        )
        .collect())
}

async fn twin_bundle_remote(
    tx: &mut DbTransaction<'_>,
    project_id: Uuid,
) -> Result<(Option<context::RemoteView>, Option<context::AdapterNames>), CreationError> {
    let row: Option<(String, String, String)> = sqlx::query_as(BUNDLE_REMOTE_SQL)
        .bind(project_id)
        .fetch_optional(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let Some((provider, host_url, full_name)) = row else {
        return Ok((None, None));
    };
    let adapter = match provider.to_lowercase().as_str() {
        "github" => Some(context::AdapterNames {
            display_name: "GitHub".to_owned(),
            code_review_term: "pull request".to_owned(),
        }),
        "gitlab" => Some(context::AdapterNames {
            display_name: "GitLab".to_owned(),
            code_review_term: "merge request".to_owned(),
        }),
        _ => None,
    };
    Ok((
        Some(context::RemoteView {
            provider,
            host_url,
            full_name,
        }),
        adapter,
    ))
}

struct TwinChainNode {
    id: Uuid,
    project_identifier: String,
    title: String,
}

async fn twin_bundle_ancestors(
    tx: &mut DbTransaction<'_>,
    issue: &IssueView,
    project_identifier: &str,
) -> Result<Vec<TwinChainNode>, CreationError> {
    let mut chain = vec![TwinChainNode {
        id: issue.id,
        project_identifier: project_identifier.to_owned(),
        title: issue.name.clone().unwrap_or_default(),
    }];
    let mut seen = std::collections::HashSet::from([issue.id]);
    let mut next = issue.parent_id;
    while let Some(id) = next {
        if chain.len() >= 50 || !seen.insert(id) {
            break;
        }
        let row: Option<(Option<String>, Option<Uuid>, Option<Uuid>)> =
            sqlx::query_as(BUNDLE_ANCESTOR_HOP_SQL)
                .bind(id)
                .fetch_optional(&mut **tx.inner())
                .await
                .map_err(twin_db)?;
        let Some((name, project_id, parent_id)) = row else {
            break;
        };
        let project_identifier = match project_id {
            Some(pid) => {
                let row: Option<(String,)> = sqlx::query_as(BUNDLE_PROJECT_IDENTIFIER_SQL)
                    .bind(pid)
                    .fetch_optional(&mut **tx.inner())
                    .await
                    .map_err(twin_db)?;
                row.map(|row| row.0).unwrap_or_default()
            }
            None => String::new(),
        };
        chain.push(TwinChainNode {
            id,
            project_identifier,
            title: name.unwrap_or_default(),
        });
        next = parent_id;
    }
    Ok(chain)
}

async fn twin_bundle_parent_and_lineage(
    tx: &mut DbTransaction<'_>,
    ancestors: &[TwinChainNode],
) -> Result<(Option<context::ParentView>, Vec<context::LineageNode>), CreationError> {
    let parent = match ancestors.get(1) {
        None => None,
        Some(node) => {
            let row: Option<TwinParentCols> = sqlx::query_as(BUNDLE_PARENT_COLS_SQL)
                .bind(node.id)
                .fetch_optional(&mut **tx.inner())
                .await
                .map_err(twin_db)?;
            match row {
                None => None,
                Some((name, state_id, work_branch, comments_count)) => {
                    let state_name = match state_id {
                        Some(id) => {
                            let row: Option<(Uuid, String, String)> =
                                sqlx::query_as(STATE_SELECT_SQL)
                                    .bind(id)
                                    .fetch_optional(&mut **tx.inner())
                                    .await
                                    .map_err(twin_db)?;
                            row.map(|row| row.1)
                        }
                        None => None,
                    };
                    let description: Option<(Option<String>,)> =
                        sqlx::query_as(BUNDLE_PARENT_DESCRIPTION_SQL)
                            .bind(node.id)
                            .fetch_optional(&mut **tx.inner())
                            .await
                            .map_err(twin_db)?;
                    let sequence: Option<(i32,)> = sqlx::query_as(BUNDLE_SEQUENCE_SQL)
                        .bind(node.id)
                        .fetch_optional(&mut **tx.inner())
                        .await
                        .map_err(twin_db)?;
                    let sequence = sequence.map(|row| i64::from(row.0)).unwrap_or_default();
                    Some(context::ParentView {
                        identifier: context::issue_identifier(&node.project_identifier, sequence),
                        title: name,
                        state_name,
                        work_branch,
                        description_stripped: description.and_then(|row| row.0),
                        comments_count,
                    })
                }
            }
        }
    };
    let lineage = if ancestors.len() > 2 {
        let mut nodes = Vec::new();
        for node in ancestors {
            let sequence: Option<(i32,)> = sqlx::query_as(BUNDLE_SEQUENCE_SQL)
                .bind(node.id)
                .fetch_optional(&mut **tx.inner())
                .await
                .map_err(twin_db)?;
            let sequence = sequence.map(|row| i64::from(row.0)).unwrap_or_default();
            nodes.push(context::LineageNode {
                identifier: context::issue_identifier(&node.project_identifier, sequence),
                title: node.title.clone(),
            });
        }
        nodes
    } else {
        Vec::new()
    };
    Ok((parent, lineage))
}

async fn twin_bundle_done_payload(
    tx: &mut DbTransaction<'_>,
    run_id: Uuid,
) -> Result<Option<Value>, CreationError> {
    let row: Option<(Option<Value>,)> = sqlx::query_as(BUNDLE_DONE_PAYLOAD_SQL)
        .bind(run_id)
        .fetch_optional(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(row.and_then(|row| row.0))
}

async fn twin_bundle_overrides(
    tx: &mut DbTransaction<'_>,
    workspace_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<Vec<composer::OverrideRow>, CreationError> {
    let rows: Vec<(String, String, i32, Option<Uuid>)> = sqlx::query_as(BUNDLE_OVERRIDES_SQL)
        .bind(workspace_id)
        .bind(user_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(
            |(section_key, body, version, user_id)| composer::OverrideRow {
                workspace_id: workspace_id.to_string(),
                section_key,
                body,
                version: i64::from(version),
                is_active: true,
                user_id: user_id.map(|id| id.to_string()),
            },
        )
        .collect())
}

async fn twin_load_render_bundle(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
    run_id: Uuid,
    parent_run_id: Option<Uuid>,
    trigger: &str,
    created_by_id: Uuid,
) -> Result<RenderBundle, CreationError> {
    let row = sqlx::query(ISSUE_SELECT_SQL)
        .bind(issue_id)
        .fetch_one(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let issue = twin_map_issue(&row).map_err(twin_db)?;
    let project_id = issue
        .project_id
        .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
    let row = sqlx::query(PROJECT_SELECT_SQL)
        .bind(project_id)
        .fetch_one(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let project = twin_map_project(&row).map_err(twin_db)?;
    let state = match issue.state_id {
        Some(id) => {
            let row: Option<(Uuid, String, String)> = sqlx::query_as(STATE_SELECT_SQL)
                .bind(id)
                .fetch_optional(&mut **tx.inner())
                .await
                .map_err(twin_db)?;
            row.map(|(id, name, group)| StateView { id, name, group })
        }
        None => None,
    };
    let workspace: (String, String) = sqlx::query_as(BUNDLE_WORKSPACE_SQL)
        .bind(issue.workspace_id)
        .fetch_one(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let labels = twin_bundle_labels(tx, issue_id).await?;
    let assignees = twin_bundle_assignees(tx, issue_id).await?;
    let project_states = twin_bundle_project_states(tx, project_id).await?;
    let children = twin_bundle_children(tx, issue_id).await?;
    let (relation_rows, refs, directional_refs) = twin_bundle_relations(tx, issue_id).await?;
    let comments = twin_bundle_comments(tx, issue_id).await?;
    let reviews = twin_bundle_reviews(tx, issue_id).await?;
    let (remote, adapter) = twin_bundle_remote(tx, project_id).await?;
    let ancestors = twin_bundle_ancestors(tx, &issue, &project.identifier).await?;
    let (parent, lineage) = twin_bundle_parent_and_lineage(tx, &ancestors).await?;
    let prior_run_count: i64 = sqlx::query_scalar(BUNDLE_PRIOR_RUN_COUNT_SQL)
        .bind(issue_id)
        .bind(run_id)
        .fetch_one(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let ticker: Option<(i32, i32, i32, bool)> = sqlx::query_as(BUNDLE_TICKER_SQL)
        .bind(issue_id)
        .fetch_optional(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let direct_parent_payload = match parent_run_id {
        Some(id) => twin_bundle_done_payload(tx, id).await?,
        None => None,
    };
    let ticker_parent_payload = {
        let resume: Option<(Option<Uuid>,)> = sqlx::query_as(TICKER_RESUME_SELECT_SQL)
            .bind(issue_id)
            .fetch_optional(&mut **tx.inner())
            .await
            .map_err(twin_db)?;
        match resume.and_then(|row| row.0) {
            Some(id) => twin_bundle_done_payload(tx, id).await?,
            None => None,
        }
    };
    let user_id = user_id_for_run(trigger, created_by_id)
        .map(|id| id.parse::<Uuid>())
        .transpose()
        .map_err(|_| CreationError::MissingRow("bad user id".to_owned()))?;
    let override_rows = twin_bundle_overrides(tx, issue.workspace_id, user_id).await?;
    Ok(RenderBundle {
        issue,
        project,
        workspace_slug: workspace.0,
        workspace_name: workspace.1,
        state,
        labels,
        assignees,
        project_states,
        children,
        relation_rows,
        refs,
        directional_refs,
        comments,
        reviews,
        remote,
        adapter,
        parent,
        lineage,
        prior_run_count,
        ticker: ticker.map(|(used, waited, granted, enabled)| TickerBudget {
            used: i64::from(used),
            waited: i64::from(waited),
            granted: i64::from(granted),
            enabled,
        }),
        direct_parent_payload,
        ticker_parent_payload,
        override_rows,
    })
}

impl FinalizeAgentRunSeam for HandoffStore<'_, '_> {
    async fn finalize_failed_run(
        &mut self,
        run_id: Uuid,
        error_code: &str,
        error: &str,
        now: DateTime<Utc>,
    ) -> Result<RunView, CreationError> {
        let locked = sqlx::query(&finalize_lock_sql())
            .bind(run_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(twin_db)?;
        if locked.is_none() {
            let row = sqlx::query(&run_select_sql())
                .bind(run_id)
                .fetch_one(&mut **self.tx.inner())
                .await
                .map_err(twin_db)?;
            return twin_map_run_view(&row).map_err(twin_db);
        }
        sqlx::query(FINALIZE_UPDATE_SQL)
            .bind(now)
            .bind(error_code)
            .bind(error)
            .bind(run_id)
            .execute(&mut **self.tx.inner())
            .await
            .map_err(twin_db)?;
        let locked = locked
            .map(|row| twin_map_run_view(&row))
            .transpose()
            .map_err(twin_db)?;
        if locked.is_some_and(|run| run.executor_kind == AgentExecutorKind::CloudAgent) {
            let exists: bool = sqlx::query_scalar(TERMINAL_EVENT_EXISTS_SQL)
                .bind(run_id)
                .fetch_one(&mut **self.tx.inner())
                .await
                .map_err(twin_db)?;
            if !exists {
                let seq: i32 = sqlx::query_scalar(TERMINAL_EVENT_SEQ_SQL)
                    .bind(run_id)
                    .fetch_one(&mut **self.tx.inner())
                    .await
                    .map_err(twin_db)?;
                let payload = serde_json::json!({"status": "failed", "error_code": error_code});
                sqlx::query(TERMINAL_EVENT_INSERT_SQL)
                    .bind(run_id)
                    .bind(seq)
                    .bind(&payload)
                    .bind(now)
                    .execute(&mut **self.tx.inner())
                    .await
                    .map_err(twin_db)?;
            }
        }
        // The `on_commit` lambda (`_publish_effects`): captured for the
        // handoff drain below (the jobs twin returns it pending for D-15;
        // the api twin can drain it directly).
        self.terminal_effects.push(run_id);
        let row = sqlx::query(&run_select_sql())
            .bind(run_id)
            .fetch_one(&mut **self.tx.inner())
            .await
            .map_err(twin_db)?;
        twin_map_run_view(&row).map_err(twin_db)
    }
}

/// Complete one post-commit project-move handoff (`models.py:653-674`,
/// `session_service.py:277-290` → D-12 `complete_project_move_handoff`,
/// `service.py:644-732`): own transaction, commit, then the handoff tx's
/// own `on_commit` registrations in order (terminal pairs, dispatches,
/// deferred consumes).
///
/// `Ok(Some(id))` is the replacement run the driver returned (created,
/// recorded, or the active run that won); `Ok(None)` is every early-out
/// (unknown run, non-terminal source, no marker, moved again, no pod).
/// Any failure `Err`s — callers swallow it after their own failure log.
pub(crate) async fn complete_project_move_handoff(
    pool: &PgPool,
    state: &AppState,
    run_id: Uuid,
) -> Result<Option<Uuid>, ()> {
    let settings = state.settings();
    let cloud = settings.cloud_agent.clone();
    let managed = settings.managed_runner.clone();
    let cache = RedisAdmissionCache {
        client: redis_client(state),
    };
    let tx = DbTransaction::begin(pool).await.map_err(|_| ())?;
    let mut store = HandoffStore::new(tx, pool, cloud.clone(), managed, cache, unix_now_secs());
    let outcome = creation_kernel::complete_project_move_handoff(&mut store, run_id, now_micros())
        .await
        .map_err(|_| ())?;
    let (tx, dispatches, deferred, terminal_effects, cache) = store.into_parts();
    tx.commit().await.map_err(|_| ())?;
    let mut pairs = Vec::with_capacity(terminal_effects.len() * 2);
    for id in terminal_effects {
        pairs.extend(finalize_kernel::plan_publish_effects(id));
    }
    drain_publish_effects(pool, state, pairs).await;
    let now = now_micros();
    for id in dispatches {
        dispatch_agent_run(pool, &cloud, id, now)
            .await
            .map_err(|_| ())?;
    }
    for consume in &deferred {
        consume_admission_token(&cache, consume);
    }
    Ok(outcome.map(|run| run.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::EdgeHandle;

    const FIXTURE_HANDOFF: &str =
        include_str!("../../../fixtures/orchestration/fx06_creation/handoff.before_after.json");

    fn cases() -> serde_json::Value {
        let fixture: serde_json::Value =
            serde_json::from_str(FIXTURE_HANDOFF).expect("handoff fixture parses");
        assert_eq!(
            fixture["marker_key"], "_project_move_handoff",
            "the marker key the services driver reads"
        );
        fixture["cases"].clone()
    }

    /// The entry's `Ok(None)` is the fixture's `result: null`: every
    /// early-out case (unknown run, non-terminal source, no marker,
    /// moved again, no target pod) records no replacement.
    #[test]
    fn early_out_cases_record_null_result() {
        let cases = cases();
        for key in [
            "H7a_unknown_run_id",
            "H7b_source_not_terminal",
            "H7c_no_marker",
            "H4_moved_again_suppressed",
            "H6_no_target_pod",
        ] {
            let case = cases
                .get(key)
                .unwrap_or_else(|| panic!("fixture case {key}"));
            assert!(case["result"].is_null(), "{key}: {case}");
        }
        // H7a is the bare shape: nothing but the null result.
        assert_eq!(
            cases["H7a_unknown_run_id"],
            serde_json::json!({"result": null})
        );
    }

    /// The entry's `Ok(Some(id))` is the fixture's non-null `result`: the
    /// created, recorded, or winning active run.
    #[test]
    fn replacement_cases_record_a_run() {
        let cases = cases();
        for key in [
            "H2_happy_path",
            "H3_replacement_id_idempotent",
            "H5_active_run_wins",
            "H9_active_race_returns_existing",
        ] {
            let case = cases
                .get(key)
                .unwrap_or_else(|| panic!("fixture case {key}"));
            assert!(case["result"].is_string(), "{key}: {case}");
        }
    }

    // -- live scratch-DB test (env-gated) --------------------------------
    // Same convention as `runner_runs::runs`: unset DATABASE_URL (plain
    // `cargo test`) skips it.

    /// H7a through the real entry: an unknown run id reads no
    /// `work_item_id`, so the driver returns before any lock and the
    /// entry commits an empty transaction and drains nothing. One
    /// connection, so the temp table is visible to the handoff tx.
    #[tokio::test]
    async fn live_unknown_run_is_the_no_handoff_early_out() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        sqlx::query("CREATE TEMPORARY TABLE agent_run (id UUID PRIMARY KEY, work_item_id UUID)")
            .execute(&pool)
            .await
            .expect("temp agent_run");
        let state = AppState::with_edge("0.1.0", EdgeHandle::for_tests("http://127.0.0.1:1"));
        let outcome = complete_project_move_handoff(&pool, &state, Uuid::new_v4()).await;
        assert_eq!(outcome, Ok(None));
        // A run with no work item is the same early-out (`row.0` is NULL).
        let orphan = Uuid::new_v4();
        sqlx::query("INSERT INTO agent_run (id, work_item_id) VALUES ($1, NULL)")
            .bind(orphan)
            .execute(&pool)
            .await
            .expect("insert orphan run");
        let outcome = complete_project_move_handoff(&pool, &state, orphan).await;
        assert_eq!(outcome, Ok(None));
    }
}
