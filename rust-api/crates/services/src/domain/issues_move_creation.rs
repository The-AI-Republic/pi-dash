//! The SeaORM-backed creation seam for the domain move path.
//!
//! [`SeaHandoffStore`] is the `&DatabaseTransaction` twin of
//! `pidash_api::project_move_handoff::HandoffStore` (same builders, same
//! order, same mapping — only the executor changes from the sqlx span to
//! the [`WriteTxn`] transaction): it implements [`CreationSeam`] +
//! [`FinalizeAgentRunSeam`] for
//! [`create_project_move_handoff_run`][creation::create_project_move_handoff_run],
//! collecting the dispatch ids, deferred consumes and terminal effects
//! the move chain drains post-commit. The twin helpers below mirror the
//! api `twin_*` functions one for one (same names, same SQL texts);
//! `twin_map_*` takes a SeaORM [`QueryResult`] where the api twin takes
//! a `PgRow`.
//!
//! [`WriteTxn`]: super::txn::WriteTxn
//! [`CreationSeam`]: creation::CreationSeam
//! [`FinalizeAgentRunSeam`]: creation::FinalizeAgentRunSeam

use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_db::dispatch::status::AgentRunStatus as DbAgentRunStatus;
use pidash_db::tx::Transaction as DbTransaction;
use pidash_types::dispatch::AgentExecutorKind;

use crate::assistant::seams as assistant_seams;
use crate::dispatch::execution::{
    execution_fields as resolve_execution_fields, ActorScope, DeferredAdmissionError,
    ExecutionFieldsError, ExecutionInputs, ExecutionSeams, ProjectScope,
};
use crate::dispatch::{DeferredConsume, LlmProfile};
use crate::extensions::{AssistantModelSeam, CloudAgentToolsetsSeam};
use crate::orchestration::creation::{
    active_run_sql, finalize_lock_sql, latest_prior_run_sql, run_insert_returning_sql,
    run_lock_sql, run_select_sql, user_id_for_run, AdmissionError as ServiceAdmissionError,
    CreationError, CreationSeam, ExecutionError, ExecutionFields as ServiceExecutionFields,
    ExecutionRequest, FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, PodView,
    ProjectView, RenderBundle, RunView, RunnerSelectRow, RunnerView, StateView, TickerBudget,
    ASSIGNED_POD_SELECT_SQL, BUNDLE_ANCESTOR_HOP_SQL, BUNDLE_ASSIGNEES_SQL, BUNDLE_CHILDREN_SQL,
    BUNDLE_CODE_REVIEWS_SQL, BUNDLE_COMMENTS_SQL, BUNDLE_DONE_PAYLOAD_SQL, BUNDLE_LABELS_SQL,
    BUNDLE_OVERRIDES_SQL, BUNDLE_PARENT_COLS_SQL, BUNDLE_PARENT_DESCRIPTION_SQL,
    BUNDLE_PRIOR_RUN_COUNT_SQL, BUNDLE_PROJECT_IDENTIFIER_SQL, BUNDLE_PROJECT_STATES_SQL,
    BUNDLE_RELATIONS_SQL, BUNDLE_RELATION_TARGETS_SQL, BUNDLE_REMOTE_SQL, BUNDLE_SEQUENCE_SQL,
    BUNDLE_TICKER_SQL, BUNDLE_WORKSPACE_SQL, DEFAULT_POD_SELECT_SQL, FINALIZE_UPDATE_SQL,
    ISSUE_LOCK_SQL, ISSUE_SELECT_SQL, PROJECT_SELECT_SQL, PROMPT_UPDATE_SQL, RUNNER_SELECT_SQL,
    RUN_CONFIG_UPDATE_SQL, STATE_SELECT_SQL, TERMINAL_EVENT_EXISTS_SQL, TERMINAL_EVENT_INSERT_SQL,
    TERMINAL_EVENT_SEQ_SQL, TICKER_RESUME_SELECT_SQL, USER_FLAGS_SELECT_SQL,
    WORK_ITEM_ID_SELECT_SQL,
};
use crate::prompting::{composer, context};

use super::issues_move::{CreationDeps, MoveAdmissionCache};
use super::txn::TxnConn;

/// Run raw SQL with bound values on the transaction, returning at most
/// one row.
async fn query_one(
    conn: &TxnConn<'_>,
    sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Option<QueryResult>, CreationError> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Postgres,
        sql,
        values,
    ))
    .await
    .map_err(twin_db)
}

/// Run raw SQL with bound values on the transaction, returning every row.
async fn query_all(
    conn: &TxnConn<'_>,
    sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Vec<QueryResult>, CreationError> {
    conn.query_all(Statement::from_sql_and_values(
        DbBackend::Postgres,
        sql,
        values,
    ))
    .await
    .map_err(twin_db)
}

/// Run a write on the transaction.
async fn execute(
    conn: &TxnConn<'_>,
    sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<(), CreationError> {
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        sql,
        values,
    ))
    .await
    .map_err(twin_db)?;
    Ok(())
}

/// A `fetch_one` miss: the twin's `RowNotFound` answers
/// `CreationError::Db`, so the miss stays `Db` here too (same 500, only
/// the log line differs).
fn expected_row(what: &str) -> CreationError {
    CreationError::Db(format!("sea creation seam: expected one row ({what})"))
}

/// The creation seam over the move's SeaORM transaction: twin of
/// `pidash_api::project_move_handoff::HandoffStore` (same builders, same
/// order, same mapping). `conn` is the shared transaction handle — no
/// lending dance: `&T` borrows compose. Outbox ids collect here and the
/// move store takes them after the creation kernel returns.
pub(crate) struct SeaHandoffStore<'t> {
    conn: TxnConn<'t>,
    pool: PgPool,
    cloud: CloudAgentSettings,
    managed: ManagedRunnerSettings,
    now_unix_secs: i64,
    toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    assistant_seam: Arc<dyn AssistantModelSeam>,
    admission: Arc<dyn MoveAdmissionCache>,
    dispatches: Vec<Uuid>,
    deferred: Vec<DeferredConsume>,
    terminal_effects: Vec<Uuid>,
}

impl<'t> SeaHandoffStore<'t> {
    pub(crate) fn new(conn: TxnConn<'t>, deps: &CreationDeps) -> Self {
        Self {
            conn,
            pool: deps.pool.clone(),
            cloud: deps.cloud.clone(),
            managed: deps.managed.clone(),
            now_unix_secs: deps.now_unix_secs,
            toolsets: deps.toolsets.clone(),
            assistant_seam: deps.assistant_seam.clone(),
            admission: deps.admission.clone(),
            dispatches: Vec::new(),
            deferred: Vec::new(),
            terminal_effects: Vec::new(),
        }
    }

    pub(crate) fn outboxes(self) -> (Vec<Uuid>, Vec<DeferredConsume>, Vec<Uuid>) {
        (self.dispatches, self.deferred, self.terminal_effects)
    }

    fn db(error: sea_orm::DbErr) -> CreationError {
        CreationError::Db(error.to_string())
    }
}

#[allow(async_fn_in_trait)]
impl CreationSeam for SeaHandoffStore<'_> {
    async fn issue(&mut self, issue_id: Uuid) -> Result<IssueView, CreationError> {
        let row = query_one(
            &self.conn,
            ISSUE_SELECT_SQL,
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?
        .ok_or_else(|| expected_row("issue"))?;
        twin_map_issue(&row)
    }

    async fn project(&mut self, project_id: Uuid) -> Result<ProjectView, CreationError> {
        let row = query_one(
            &self.conn,
            PROJECT_SELECT_SQL,
            vec![sea_orm::Value::from(project_id)],
        )
        .await?
        .ok_or_else(|| expected_row("project"))?;
        twin_map_project(&row)
    }

    async fn state(&mut self, state_id: Option<Uuid>) -> Result<Option<StateView>, CreationError> {
        let Some(state_id) = state_id else {
            return Ok(None);
        };
        let row = query_one(
            &self.conn,
            STATE_SELECT_SQL,
            vec![sea_orm::Value::from(state_id)],
        )
        .await?;
        match row {
            Some(row) => {
                let (id, name, group): (Uuid, String, String) =
                    row.try_get_many_by_index().map_err(Self::db)?;
                Ok(Some(StateView { id, name, group }))
            }
            None => Err(CreationError::MissingRow(format!("no state {state_id}"))),
        }
    }

    async fn latest_prior_run(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = query_one(
            &self.conn,
            &latest_prior_run_sql(),
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?;
        row.map(|row| twin_map_run_view(&row)).transpose()
    }

    async fn active_run_for(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = query_one(
            &self.conn,
            &active_run_sql(),
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?;
        row.map(|row| twin_map_run_view(&row)).transpose()
    }

    async fn run(&mut self, run_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = query_one(
            &self.conn,
            &run_select_sql(),
            vec![sea_orm::Value::from(run_id)],
        )
        .await?;
        row.map(|row| twin_map_run_view(&row)).transpose()
    }

    async fn runner(&mut self, runner_id: Uuid) -> Result<Option<RunnerView>, CreationError> {
        let row = query_one(
            &self.conn,
            RUNNER_SELECT_SQL,
            vec![sea_orm::Value::from(runner_id)],
        )
        .await?;
        match row {
            Some(row) => {
                let (id, pod_id, status, last_heartbeat_at, paused_at): RunnerSelectRow =
                    row.try_get_many_by_index().map_err(Self::db)?;
                Ok(Some(RunnerView {
                    id,
                    pod_id,
                    status,
                    last_heartbeat_at,
                    paused_at,
                }))
            }
            None => Ok(None),
        }
    }

    async fn assigned_pod(&mut self, pod_id: Uuid) -> Result<Option<PodView>, CreationError> {
        let row = query_one(
            &self.conn,
            ASSIGNED_POD_SELECT_SQL,
            vec![sea_orm::Value::from(pod_id)],
        )
        .await?;
        match row {
            Some(row) => {
                let (id, project_id): (Uuid, Uuid) =
                    row.try_get_many_by_index().map_err(Self::db)?;
                Ok(Some(PodView { id, project_id }))
            }
            None => Ok(None),
        }
    }

    async fn default_pod_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<PodView>, CreationError> {
        let row = query_one(
            &self.conn,
            DEFAULT_POD_SELECT_SQL,
            vec![sea_orm::Value::from(project_id)],
        )
        .await?;
        match row {
            Some(row) => {
                let (id, project_id): (Uuid, Uuid) =
                    row.try_get_many_by_index().map_err(Self::db)?;
                Ok(Some(PodView { id, project_id }))
            }
            None => Ok(None),
        }
    }

    async fn resume_parent_run_id(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<Uuid>, CreationError> {
        let row = query_one(
            &self.conn,
            TICKER_RESUME_SELECT_SQL,
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?;
        match row {
            Some(row) => {
                let (id,): (Option<Uuid>,) = row.try_get_many_by_index().map_err(Self::db)?;
                Ok(id)
            }
            None => Ok(None),
        }
    }

    async fn work_item_id_for_run(&mut self, run_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        let row = query_one(
            &self.conn,
            WORK_ITEM_ID_SELECT_SQL,
            vec![sea_orm::Value::from(run_id)],
        )
        .await?;
        match row {
            Some(row) => {
                let (id,): (Option<Uuid>,) = row.try_get_many_by_index().map_err(Self::db)?;
                Ok(id)
            }
            None => Ok(None),
        }
    }

    async fn lock_issue_for_handoff(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<LockedIssue>, CreationError> {
        let locked = query_one(
            &self.conn,
            ISSUE_LOCK_SQL,
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?;
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
        let row = query_one(
            &self.conn,
            &run_lock_sql(),
            vec![sea_orm::Value::from(run_id)],
        )
        .await?;
        row.map(|row| twin_map_run_view(&row)).transpose()
    }

    async fn user_flags(
        &mut self,
        user_id: Uuid,
    ) -> Result<crate::dispatch::UserFlags, CreationError> {
        let row = query_one(
            &self.conn,
            USER_FLAGS_SELECT_SQL,
            vec![sea_orm::Value::from(user_id)],
        )
        .await?
        .ok_or_else(|| expected_row("user flags"))?;
        let (is_active, is_bot): (bool, bool) = row.try_get_many_by_index().map_err(Self::db)?;
        Ok(crate::dispatch::UserFlags { is_active, is_bot })
    }

    async fn insert_run(&mut self, row: &NewAgentRun) -> Result<RunView, CreationError> {
        let inserted = query_one(
            &self.conn,
            &run_insert_returning_sql(),
            vec![
                sea_orm::Value::from(row.id),
                sea_orm::Value::from(row.workspace_id),
                sea_orm::Value::from(row.created_by_id),
                sea_orm::Value::from(row.pod_id),
                sea_orm::Value::from(row.pinned_runner_id),
                sea_orm::Value::from(row.work_item_id),
                sea_orm::Value::from(row.parent_run_id),
                sea_orm::Value::from(row.executor_kind.value()),
                sea_orm::Value::from(row.error_code.clone()),
                sea_orm::Value::from(row.tool_plan.clone()),
                sea_orm::Value::from(row.trigger.clone()),
                sea_orm::Value::from(row.phase_kind.clone()),
                sea_orm::Value::from(row.run_config.clone()),
                sea_orm::Value::from(row.now),
            ],
        )
        .await?
        .ok_or_else(|| expected_row("inserted run"))?;
        twin_map_run_view(&inserted)
    }

    async fn save_prompt(
        &mut self,
        run_id: Uuid,
        prompt: &str,
        manifest: &Value,
    ) -> Result<(), CreationError> {
        execute(
            &self.conn,
            PROMPT_UPDATE_SQL,
            vec![
                sea_orm::Value::from(prompt),
                sea_orm::Value::from(manifest.clone()),
                sea_orm::Value::from(run_id),
            ],
        )
        .await
    }

    async fn save_run_config(&mut self, run_id: Uuid, config: &Value) -> Result<(), CreationError> {
        execute(
            &self.conn,
            RUN_CONFIG_UPDATE_SQL,
            vec![
                sea_orm::Value::from(config.clone()),
                sea_orm::Value::from(run_id),
            ],
        )
        .await
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
            Some(id) => twin_has_api_key(&self.pool, id)
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
        // The driver short-circuits a missing actor before consulting
        // the closure, so only a present actor consults the seam (and a
        // missing one never pays for the overlay's reads). Clone the seam
        // out: the store is `!Sync`, so the consult must not borrow
        // `self` across the await.
        let assistant_seam = self.assistant_seam.clone();
        let has_llm = match actor_id {
            Some(id) => twin_gate_allows(&assistant_seam, id, has_key).await,
            None => false,
        };
        let mut short = DbTransaction::begin(&self.pool)
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
            now_unix_secs: self.now_unix_secs,
        };
        let cache = DynCache(self.admission.as_ref());
        let deferred = &mut self.deferred;
        let mut collect = |consume: DeferredConsume| deferred.push(consume);
        let fields = resolve_execution_fields(
            &mut short,
            &inputs,
            &self.cloud,
            &self.managed,
            &cache,
            &mut collect,
            ExecutionSeams {
                has_usable_llm_config: move || has_llm,
                toolsets: self.toolsets.clone(),
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
        // The relocated `lock_cloud_creation_capacity` needs the span's
        // sqlx transaction, which the SeaORM transaction cannot lend, so
        // the twin runs its three steps here: the workspace lock, the
        // queued count, and the shared `quota_verdict`.
        use crate::dispatch::execution::{
            quota_verdict, QUEUED_CLOUD_COUNT_SQL, WORKSPACE_LOCK_SQL,
        };
        if executor_kind != AgentExecutorKind::CloudAgent {
            return Ok(None);
        }
        query_one(
            &self.conn,
            WORKSPACE_LOCK_SQL,
            vec![sea_orm::Value::from(workspace_id)],
        )
        .await?
        .ok_or_else(|| expected_row("workspace lock"))?;
        let row = query_one(
            &self.conn,
            QUEUED_CLOUD_COUNT_SQL,
            vec![sea_orm::Value::from(workspace_id)],
        )
        .await?
        .ok_or_else(|| expected_row("queued cloud count"))?;
        let (queued,): (i64,) = row.try_get_many_by_index().map_err(Self::db)?;
        match quota_verdict(
            queued,
            self.cloud.max_queued_per_workspace,
            self.cloud.dispatch_scan_interval_secs,
            automatic,
        ) {
            Ok(deferred) => Ok(deferred.map(twin_map_admission_error)),
            Err(exc) => Err(CreationError::CapacityRefused(exc.detail().to_owned())),
        }
    }

    fn dispatch_after_commit(&mut self, run_id: Uuid) {
        // `jobs::dispatch_after_commit` only invokes the collector.
        self.dispatches.push(run_id);
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
            &self.conn,
            issue_id,
            run_id,
            parent_run_id,
            trigger,
            created_by_id,
        )
        .await
    }

    fn extra_toolsets_schema_tool(&self) -> String {
        self.toolsets.schema_tool_name().to_owned()
    }
}

impl FinalizeAgentRunSeam for SeaHandoffStore<'_> {
    async fn finalize_failed_run(
        &mut self,
        run_id: Uuid,
        error_code: &str,
        error: &str,
        now: DateTime<Utc>,
    ) -> Result<RunView, CreationError> {
        let locked = query_one(
            &self.conn,
            &finalize_lock_sql(),
            vec![sea_orm::Value::from(run_id)],
        )
        .await?;
        if locked.is_none() {
            let row = query_one(
                &self.conn,
                &run_select_sql(),
                vec![sea_orm::Value::from(run_id)],
            )
            .await?
            .ok_or_else(|| expected_row("unlocked run"))?;
            return twin_map_run_view(&row);
        }
        execute(
            &self.conn,
            FINALIZE_UPDATE_SQL,
            vec![
                sea_orm::Value::from(now),
                sea_orm::Value::from(error_code),
                sea_orm::Value::from(error),
                sea_orm::Value::from(run_id),
            ],
        )
        .await?;
        let locked = locked.map(|row| twin_map_run_view(&row)).transpose()?;
        if locked.is_some_and(|run| run.executor_kind == AgentExecutorKind::CloudAgent) {
            let row = query_one(
                &self.conn,
                TERMINAL_EVENT_EXISTS_SQL,
                vec![sea_orm::Value::from(run_id)],
            )
            .await?
            .ok_or_else(|| expected_row("terminal event probe"))?;
            let (exists,): (bool,) = row.try_get_many_by_index().map_err(SeaHandoffStore::db)?;
            if !exists {
                let row = query_one(
                    &self.conn,
                    TERMINAL_EVENT_SEQ_SQL,
                    vec![sea_orm::Value::from(run_id)],
                )
                .await?
                .ok_or_else(|| expected_row("terminal event seq"))?;
                let (seq,): (i32,) = row.try_get_many_by_index().map_err(SeaHandoffStore::db)?;
                let payload = serde_json::json!({"status": "failed", "error_code": error_code});
                execute(
                    &self.conn,
                    TERMINAL_EVENT_INSERT_SQL,
                    vec![
                        sea_orm::Value::from(run_id),
                        sea_orm::Value::from(seq),
                        sea_orm::Value::from(payload),
                        sea_orm::Value::from(now),
                    ],
                )
                .await?;
            }
        }
        // The `on_commit` lambda (`_publish_effects`): captured for the
        // handoff drain below (the jobs twin returns it pending for D-15;
        // the api twin can drain it directly).
        self.terminal_effects.push(run_id);
        let row = query_one(
            &self.conn,
            &run_select_sql(),
            vec![sea_orm::Value::from(run_id)],
        )
        .await?
        .ok_or_else(|| expected_row("finalized run"))?;
        twin_map_run_view(&row)
    }
}

fn twin_db(error: sea_orm::DbErr) -> CreationError {
    CreationError::Db(error.to_string())
}

fn twin_decode_error(column: &str, value: &str) -> CreationError {
    // Same string the twin builds: `sqlx::Error::Decode`'s display over
    // the unknown-value message (same sqlx version, same workspace).
    CreationError::Db(
        sqlx::Error::Decode(format!("unknown agent_run.{column} {value:?}").into()).to_string(),
    )
}

fn twin_map_status(value: String) -> Result<DbAgentRunStatus, CreationError> {
    DbAgentRunStatus::from_value(&value).ok_or_else(|| twin_decode_error("status", &value))
}

fn twin_map_executor(value: String) -> Result<AgentExecutorKind, CreationError> {
    // Strictness is safe here (unlike the trigger): the
    // `agent_run_cloud_has_no_local_assignment` check constraint
    // (`runner/models.py:1065-1075`, migration 0024) admits only the
    // three members, so an unknown stored value is unreachable.
    AgentExecutorKind::from_value(&value).ok_or_else(|| twin_decode_error("executor_kind", &value))
}

fn twin_map_run_view(row: &QueryResult) -> Result<RunView, CreationError> {
    let status: String = row.try_get("", "status").map_err(twin_db)?;
    let trigger: String = row.try_get("", "trigger").map_err(twin_db)?;
    let executor_kind: String = row.try_get("", "executor_kind").map_err(twin_db)?;
    Ok(RunView {
        id: row.try_get("", "id").map_err(twin_db)?,
        workspace_id: row.try_get("", "workspace_id").map_err(twin_db)?,
        created_by_id: row.try_get("", "created_by_id").map_err(twin_db)?,
        pod_id: row.try_get("", "pod_id").map_err(twin_db)?,
        runner_id: row.try_get("", "runner_id").map_err(twin_db)?,
        pinned_runner_id: row.try_get("", "pinned_runner_id").map_err(twin_db)?,
        parent_run_id: row.try_get("", "parent_run_id").map_err(twin_db)?,
        work_item_id: row.try_get("", "work_item_id").map_err(twin_db)?,
        status: twin_map_status(status)?,
        trigger,
        executor_kind: twin_map_executor(executor_kind)?,
        phase_kind: row.try_get("", "phase_kind").map_err(twin_db)?,
        run_config: row.try_get("", "run_config").map_err(twin_db)?,
        tool_plan: row.try_get("", "tool_plan").map_err(twin_db)?,
        error_code: row.try_get("", "error_code").map_err(twin_db)?,
        error: row.try_get("", "error").map_err(twin_db)?,
        prompt: row.try_get("", "prompt").map_err(twin_db)?,
        prompt_manifest: row.try_get("", "prompt_manifest").map_err(twin_db)?,
        ended_at: row.try_get("", "ended_at").map_err(twin_db)?,
    })
}

fn twin_map_issue(row: &QueryResult) -> Result<IssueView, CreationError> {
    Ok(IssueView {
        id: row.try_get("", "id").map_err(twin_db)?,
        workspace_id: row.try_get("", "workspace_id").map_err(twin_db)?,
        project_id: row.try_get("", "project_id").map_err(twin_db)?,
        state_id: row.try_get("", "state_id").map_err(twin_db)?,
        parent_id: row.try_get("", "parent_id").map_err(twin_db)?,
        created_by_id: row.try_get("", "created_by_id").map_err(twin_db)?,
        assigned_pod_id: row.try_get("", "assigned_pod_id").map_err(twin_db)?,
        agent_executor: row.try_get("", "agent_executor").map_err(twin_db)?,
        git_work_branch: row.try_get("", "git_work_branch").map_err(twin_db)?,
        workpad: row.try_get("", "workpad").map_err(twin_db)?,
        name: row.try_get("", "name").map_err(twin_db)?,
        description_stripped: row.try_get("", "description_stripped").map_err(twin_db)?,
        priority: row.try_get("", "priority").map_err(twin_db)?,
        sequence_id: row.try_get("", "sequence_id").map_err(twin_db)?,
        target_date: row.try_get("", "target_date").map_err(twin_db)?,
    })
}

fn twin_map_project(row: &QueryResult) -> Result<ProjectView, CreationError> {
    let pool: i32 = row
        .try_get("", "agent_default_max_ticks")
        .map_err(twin_db)?;
    let interval_impl: i32 = row
        .try_get("", "agent_default_interval_seconds")
        .map_err(twin_db)?;
    let interval_review: i32 = row
        .try_get("", "agent_review_default_interval_seconds")
        .map_err(twin_db)?;
    let interval_test: i32 = row
        .try_get("", "agent_test_default_interval_seconds")
        .map_err(twin_db)?;
    Ok(ProjectView {
        id: row.try_get("", "id").map_err(twin_db)?,
        workspace_id: row.try_get("", "workspace_id").map_err(twin_db)?,
        identifier: row.try_get("", "identifier").map_err(twin_db)?,
        name: row.try_get("", "name").map_err(twin_db)?,
        description: row.try_get("", "description").map_err(twin_db)?,
        repo_url: row.try_get("", "repo_url").map_err(twin_db)?,
        base_branch: row.try_get("", "base_branch").map_err(twin_db)?,
        default_agent_executor: row.try_get("", "default_agent_executor").map_err(twin_db)?,
        project_lead_id: row.try_get("", "project_lead_id").map_err(twin_db)?,
        default_assignee_id: row.try_get("", "default_assignee_id").map_err(twin_db)?,
        pool: i64::from(pool),
        interval_impl: i64::from(interval_impl),
        interval_review: i64::from(interval_review),
        interval_test: i64::from(interval_test),
    })
}

fn twin_map_execution_fields(
    fields: crate::dispatch::execution::ExecutionFields,
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
/// and non-empty. Runs on the pool (outside the span), as the twin does —
/// verbatim sqlx, since the pool read needs no SeaORM handle.
async fn twin_has_api_key(pool: &PgPool, user_id: Uuid) -> Result<bool, String> {
    use sqlx::Row;
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

/// The handoff llm gate through the installed assistant seam
/// (`user_has_llm_config`, `agent_execution.py:69-80`): the CE default
/// answers the BYOK key bit; the overlay routes OpenHub/Codex/BYOK
/// through the same single switch point Pi Dash AI uses (PIDASHCONV-1051).
/// A free function over the cloned seam: the store is `!Sync` (it owns
/// the span transaction), so the consult must not borrow `self` across
/// the await, or the handlers' futures stop being `Send`.
async fn twin_gate_allows(
    assistant_seam: &Arc<dyn AssistantModelSeam>,
    actor_id: Uuid,
    has_key: bool,
) -> bool {
    assistant_seam
        .has_usable_llm_config(actor_id, has_key)
        .await
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

async fn twin_bundle_labels(
    conn: &TxnConn<'_>,
    issue_id: Uuid,
) -> Result<Vec<String>, CreationError> {
    let rows = query_all(
        conn,
        BUNDLE_LABELS_SQL,
        vec![sea_orm::Value::from(issue_id)],
    )
    .await?;
    rows.iter()
        .map(|row| {
            let (label,): (String,) = row.try_get_many_by_index().map_err(twin_db)?;
            Ok(label)
        })
        .collect()
}

async fn twin_bundle_assignees(
    conn: &TxnConn<'_>,
    issue_id: Uuid,
) -> Result<Vec<String>, CreationError> {
    let rows = query_all(
        conn,
        BUNDLE_ASSIGNEES_SQL,
        vec![sea_orm::Value::from(issue_id)],
    )
    .await?;
    rows.iter()
        .map(|row| {
            let (display, email): (Option<String>, Option<String>) =
                row.try_get_many_by_index().map_err(twin_db)?;
            Ok(context::assignee_display(
                display.as_deref(),
                email.as_deref(),
            ))
        })
        .collect()
}

async fn twin_bundle_project_states(
    conn: &TxnConn<'_>,
    project_id: Uuid,
) -> Result<Vec<context::ProjectStateView>, CreationError> {
    let rows = query_all(
        conn,
        BUNDLE_PROJECT_STATES_SQL,
        vec![sea_orm::Value::from(project_id)],
    )
    .await?;
    rows.iter()
        .map(|row| {
            let (name, group, description): (String, String, Option<String>) =
                row.try_get_many_by_index().map_err(twin_db)?;
            Ok(context::ProjectStateView {
                name,
                group,
                description,
            })
        })
        .collect()
}

async fn twin_bundle_children(
    conn: &TxnConn<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::IssueRef>, CreationError> {
    let rows = query_all(
        conn,
        BUNDLE_CHILDREN_SQL,
        vec![sea_orm::Value::from(issue_id)],
    )
    .await?;
    rows.iter()
        .map(|row| {
            let (_, name, sequence, project_identifier, state): TwinChildCols =
                row.try_get_many_by_index().map_err(twin_db)?;
            Ok(context::IssueRef {
                identifier: context::issue_identifier(&project_identifier, i64::from(sequence)),
                title: name.unwrap_or_default(),
                state: state.unwrap_or_default(),
            })
        })
        .collect()
}

async fn twin_bundle_relations(
    conn: &TxnConn<'_>,
    issue_id: Uuid,
) -> Result<
    (
        Vec<context::RelationRow>,
        std::collections::HashMap<String, context::IssueRef>,
        std::collections::HashMap<String, context::DirectionalRef>,
    ),
    CreationError,
> {
    let rows = query_all(
        conn,
        BUNDLE_RELATIONS_SQL,
        vec![sea_orm::Value::from(issue_id)],
    )
    .await?;
    let rows: Vec<(Uuid, Uuid, String)> = rows
        .iter()
        .map(|row| row.try_get_many_by_index().map_err(twin_db))
        .collect::<Result<_, _>>()?;
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
        query_all(
            conn,
            BUNDLE_RELATION_TARGETS_SQL,
            vec![sea_orm::Value::from(other_ids)],
        )
        .await?
        .iter()
        .map(|row| row.try_get_many_by_index().map_err(twin_db))
        .collect::<Result<_, _>>()?
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
    conn: &TxnConn<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::CommentView>, CreationError> {
    let rows = query_all(
        conn,
        BUNDLE_COMMENTS_SQL,
        vec![sea_orm::Value::from(issue_id)],
    )
    .await?;
    rows.iter()
        .map(|row| {
            let (
                stripped,
                speaker_type,
                speaker_label,
                run_id,
                created_at,
                display,
                email,
                username,
                is_bot,
            ): TwinCommentCols = row.try_get_many_by_index().map_err(twin_db)?;
            let actor = match (&display, &email, &username, &is_bot) {
                (None, None, None, None) => None,
                _ => Some(context::ActorView {
                    display_name: display,
                    email,
                    username,
                    is_bot: is_bot.unwrap_or(false),
                }),
            };
            Ok(context::CommentView {
                body: stripped.unwrap_or_default(),
                speaker_type: speaker_type.unwrap_or_default(),
                speaker_label,
                actor,
                created_at_iso: created_at.map(twin_render_isoformat),
                run_id: run_id.map(|id| id.to_string()),
            })
        })
        .collect()
}

async fn twin_bundle_reviews(
    conn: &TxnConn<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::CodeReviewView>, CreationError> {
    let rows = query_all(
        conn,
        BUNDLE_CODE_REVIEWS_SQL,
        vec![sea_orm::Value::from(issue_id)],
    )
    .await?;
    rows.iter()
        .map(|row| {
            let (url, title, state, merged, draft, provider, external_iid): TwinReviewCols =
                row.try_get_many_by_index().map_err(twin_db)?;
            Ok(context::CodeReviewView {
                url,
                title,
                state,
                merged,
                draft,
                provider,
                external_iid,
            })
        })
        .collect()
}

async fn twin_bundle_remote(
    conn: &TxnConn<'_>,
    project_id: Uuid,
) -> Result<(Option<context::RemoteView>, Option<context::AdapterNames>), CreationError> {
    let row = query_one(
        conn,
        BUNDLE_REMOTE_SQL,
        vec![sea_orm::Value::from(project_id)],
    )
    .await?;
    let Some(row) = row else {
        return Ok((None, None));
    };
    let (provider, host_url, full_name): (String, String, String) =
        row.try_get_many_by_index().map_err(twin_db)?;
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
    conn: &TxnConn<'_>,
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
        let row = query_one(
            conn,
            BUNDLE_ANCESTOR_HOP_SQL,
            vec![sea_orm::Value::from(id)],
        )
        .await?;
        let Some(row) = row else {
            break;
        };
        let (name, project_id, parent_id): (Option<String>, Option<Uuid>, Option<Uuid>) =
            row.try_get_many_by_index().map_err(twin_db)?;
        let project_identifier = match project_id {
            Some(pid) => {
                let row = query_one(
                    conn,
                    BUNDLE_PROJECT_IDENTIFIER_SQL,
                    vec![sea_orm::Value::from(pid)],
                )
                .await?;
                match row {
                    Some(row) => {
                        let (identifier,): (String,) =
                            row.try_get_many_by_index().map_err(twin_db)?;
                        identifier
                    }
                    None => String::new(),
                }
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
    conn: &TxnConn<'_>,
    ancestors: &[TwinChainNode],
) -> Result<(Option<context::ParentView>, Vec<context::LineageNode>), CreationError> {
    let parent = match ancestors.get(1) {
        None => None,
        Some(node) => {
            let row = query_one(
                conn,
                BUNDLE_PARENT_COLS_SQL,
                vec![sea_orm::Value::from(node.id)],
            )
            .await?;
            match row {
                None => None,
                Some(row) => {
                    let (name, state_id, work_branch, comments_count): TwinParentCols =
                        row.try_get_many_by_index().map_err(twin_db)?;
                    let state_name = match state_id {
                        Some(id) => {
                            let row =
                                query_one(conn, STATE_SELECT_SQL, vec![sea_orm::Value::from(id)])
                                    .await?;
                            match row {
                                Some(row) => {
                                    let (_, name, _): (Uuid, String, String) =
                                        row.try_get_many_by_index().map_err(twin_db)?;
                                    Some(name)
                                }
                                None => None,
                            }
                        }
                        None => None,
                    };
                    let row = query_one(
                        conn,
                        BUNDLE_PARENT_DESCRIPTION_SQL,
                        vec![sea_orm::Value::from(node.id)],
                    )
                    .await?;
                    let description: Option<String> = match row {
                        Some(row) => {
                            let (text,): (Option<String>,) =
                                row.try_get_many_by_index().map_err(twin_db)?;
                            text
                        }
                        None => None,
                    };
                    let row = query_one(
                        conn,
                        BUNDLE_SEQUENCE_SQL,
                        vec![sea_orm::Value::from(node.id)],
                    )
                    .await?;
                    let sequence: i64 = match row {
                        Some(row) => {
                            let (sequence,): (i32,) =
                                row.try_get_many_by_index().map_err(twin_db)?;
                            i64::from(sequence)
                        }
                        None => 0,
                    };
                    Some(context::ParentView {
                        identifier: context::issue_identifier(&node.project_identifier, sequence),
                        title: name,
                        state_name,
                        work_branch,
                        description_stripped: description,
                        comments_count,
                    })
                }
            }
        }
    };
    let lineage = if ancestors.len() > 2 {
        let mut nodes = Vec::new();
        for node in ancestors {
            let row = query_one(
                conn,
                BUNDLE_SEQUENCE_SQL,
                vec![sea_orm::Value::from(node.id)],
            )
            .await?;
            let sequence: i64 = match row {
                Some(row) => {
                    let (sequence,): (i32,) = row.try_get_many_by_index().map_err(twin_db)?;
                    i64::from(sequence)
                }
                None => 0,
            };
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
    conn: &TxnConn<'_>,
    run_id: Uuid,
) -> Result<Option<Value>, CreationError> {
    let row = query_one(
        conn,
        BUNDLE_DONE_PAYLOAD_SQL,
        vec![sea_orm::Value::from(run_id)],
    )
    .await?;
    match row {
        Some(row) => {
            let (payload,): (Option<Value>,) = row.try_get_many_by_index().map_err(twin_db)?;
            Ok(payload)
        }
        None => Ok(None),
    }
}

async fn twin_bundle_overrides(
    conn: &TxnConn<'_>,
    workspace_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<Vec<composer::OverrideRow>, CreationError> {
    let rows = query_all(
        conn,
        BUNDLE_OVERRIDES_SQL,
        vec![
            sea_orm::Value::from(workspace_id),
            sea_orm::Value::from(user_id),
        ],
    )
    .await?;
    rows.iter()
        .map(|row| {
            let (section_key, body, version, user_id): (String, String, i32, Option<Uuid>) =
                row.try_get_many_by_index().map_err(twin_db)?;
            Ok(composer::OverrideRow {
                workspace_id: workspace_id.to_string(),
                section_key,
                body,
                version: i64::from(version),
                is_active: true,
                user_id: user_id.map(|id| id.to_string()),
            })
        })
        .collect()
}

async fn twin_load_render_bundle(
    conn: &TxnConn<'_>,
    issue_id: Uuid,
    run_id: Uuid,
    parent_run_id: Option<Uuid>,
    trigger: &str,
    created_by_id: Uuid,
) -> Result<RenderBundle, CreationError> {
    let row = query_one(conn, ISSUE_SELECT_SQL, vec![sea_orm::Value::from(issue_id)])
        .await?
        .ok_or_else(|| expected_row("bundle issue"))?;
    let issue = twin_map_issue(&row)?;
    let project_id = issue
        .project_id
        .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
    let row = query_one(
        conn,
        PROJECT_SELECT_SQL,
        vec![sea_orm::Value::from(project_id)],
    )
    .await?
    .ok_or_else(|| expected_row("bundle project"))?;
    let project = twin_map_project(&row)?;
    let state = match issue.state_id {
        Some(id) => {
            let row = query_one(conn, STATE_SELECT_SQL, vec![sea_orm::Value::from(id)]).await?;
            match row {
                Some(row) => {
                    let (id, name, group): (Uuid, String, String) =
                        row.try_get_many_by_index().map_err(twin_db)?;
                    Some(StateView { id, name, group })
                }
                None => None,
            }
        }
        None => None,
    };
    let row = query_one(
        conn,
        BUNDLE_WORKSPACE_SQL,
        vec![sea_orm::Value::from(issue.workspace_id)],
    )
    .await?
    .ok_or_else(|| expected_row("bundle workspace"))?;
    let (workspace_slug, workspace_name): (String, String) =
        row.try_get_many_by_index().map_err(twin_db)?;
    let labels = twin_bundle_labels(conn, issue_id).await?;
    let assignees = twin_bundle_assignees(conn, issue_id).await?;
    let project_states = twin_bundle_project_states(conn, project_id).await?;
    let children = twin_bundle_children(conn, issue_id).await?;
    let (relation_rows, refs, directional_refs) = twin_bundle_relations(conn, issue_id).await?;
    let comments = twin_bundle_comments(conn, issue_id).await?;
    let reviews = twin_bundle_reviews(conn, issue_id).await?;
    let (remote, adapter) = twin_bundle_remote(conn, project_id).await?;
    let ancestors = twin_bundle_ancestors(conn, &issue, &project.identifier).await?;
    let (parent, lineage) = twin_bundle_parent_and_lineage(conn, &ancestors).await?;
    let row = query_one(
        conn,
        BUNDLE_PRIOR_RUN_COUNT_SQL,
        vec![sea_orm::Value::from(issue_id), sea_orm::Value::from(run_id)],
    )
    .await?
    .ok_or_else(|| expected_row("prior run count"))?;
    let (prior_run_count,): (i64,) = row.try_get_many_by_index().map_err(twin_db)?;
    let row = query_one(
        conn,
        BUNDLE_TICKER_SQL,
        vec![sea_orm::Value::from(issue_id)],
    )
    .await?;
    let ticker: Option<(i32, i32, i32, bool)> = match row {
        Some(row) => Some(row.try_get_many_by_index().map_err(twin_db)?),
        None => None,
    };
    let direct_parent_payload = match parent_run_id {
        Some(id) => twin_bundle_done_payload(conn, id).await?,
        None => None,
    };
    // A parent run with a NULL payload must render the fallback, never
    // the ticker payload — so its existence travels separately from the
    // payload itself (PIDASHCONV-1232).
    let has_parent_run = parent_run_id.is_some();
    let ticker_parent_payload = {
        let row = query_one(
            conn,
            TICKER_RESUME_SELECT_SQL,
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?;
        let resume: Option<Uuid> = match row {
            Some(row) => {
                let (id,): (Option<Uuid>,) = row.try_get_many_by_index().map_err(twin_db)?;
                id
            }
            None => None,
        };
        match resume {
            Some(id) => twin_bundle_done_payload(conn, id).await?,
            None => None,
        }
    };
    let user_id = user_id_for_run(trigger, created_by_id)
        .map(|id| id.parse::<Uuid>())
        .transpose()
        .map_err(|_| CreationError::MissingRow("bad user id".to_owned()))?;
    let override_rows = twin_bundle_overrides(conn, issue.workspace_id, user_id).await?;
    Ok(RenderBundle {
        issue,
        project,
        workspace_slug,
        workspace_name,
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
        has_parent_run,
        ticker_parent_payload,
        override_rows,
    })
}

/// [`AdmissionCache`] over the driver-supplied [`MoveAdmissionCache`]:
/// the relocated `execution_fields` resolves against this in its short
/// transaction, exactly as the twin resolves against the api cache.
struct DynCache<'a>(&'a dyn MoveAdmissionCache);

#[derive(Debug, Clone)]
struct CacheError(String);

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CacheError {}

impl crate::dispatch::AdmissionCache for DynCache<'_> {
    type Error = CacheError;

    fn bucket_count(&self, key: &str) -> Result<Option<i64>, Self::Error> {
        self.0.bucket_count(key).map_err(CacheError)
    }

    fn add_or_incr(&self, key: &str, timeout_secs: i64) -> Result<(), Self::Error> {
        self.0.add_or_incr(key, timeout_secs).map_err(CacheError)
    }
}
