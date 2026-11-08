//! The SeaORM-backed creation seam for the domain move path.
//!
//! [`SeaHandoffStore`] is the `&DatabaseTransaction` twin of
//! `pidash_api::project_move_handoff::HandoffStore` (same statements, same
//! order, same mapping — only the executor changes from the sqlx span to
//! the [`WriteTxn`] transaction): it implements [`CreationSeam`] +
//! [`FinalizeAgentRunSeam`] for
//! [`create_project_move_handoff_run`][creation::create_project_move_handoff_run],
//! collecting the dispatch ids, deferred consumes and terminal effects
//! the move chain drains post-commit. The twin helpers below mirror the
//! api `twin_*` functions one for one (same names); the statements are
//! the SeaORM builders in [`creation::orm`] (one per SQL constant the api
//! twin executes), and `twin_map_*` decodes a SeaORM [`QueryResult`] by
//! column name where the api twin decodes a `PgRow`.
//!
//! [`WriteTxn`]: super::txn::WriteTxn
//! [`CreationSeam`]: creation::CreationSeam
//! [`FinalizeAgentRunSeam`]: creation::FinalizeAgentRunSeam
//! [`creation::orm`]: crate::orchestration::creation::orm

use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend, QueryResult, StatementBuilder, TryGetable};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_db::dispatch::status::AgentRunStatus as DbAgentRunStatus;
use pidash_db::tx::Transaction as DbTransaction;
use pidash_types::dispatch::AgentExecutorKind;

use crate::dispatch::execution::{
    execution_fields as resolve_execution_fields, ActorScope, DeferredAdmissionError,
    ExecutionFieldsError, ExecutionInputs, ExecutionSeams, ProjectScope,
};
use crate::dispatch::DeferredConsume;
use crate::extensions::{AssistantModelSeam, CloudAgentToolsetsSeam};
use crate::orchestration::creation::{
    finalize_error_details_patch, orm, user_id_for_run, AdmissionError as ServiceAdmissionError,
    CreationError, CreationSeam, ExecutionError, ExecutionFields as ServiceExecutionFields,
    ExecutionRequest, FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, PodView,
    ProjectView, RenderBundle, RunView, RunnerView, StateView, TickerBudget,
};
use crate::prompting::{composer, context};

use super::issues_move::{CreationDeps, MoveAdmissionCache};
use super::txn::TxnConn;

/// Run one built statement on the transaction, returning at most one
/// row.
async fn query_one(
    conn: &TxnConn<'_>,
    stmt: &impl StatementBuilder,
) -> Result<Option<QueryResult>, CreationError> {
    conn.query_one(DbBackend::Postgres.build(stmt))
        .await
        .map_err(twin_db)
}

/// Run one built statement on the transaction, returning every row.
async fn query_all(
    conn: &TxnConn<'_>,
    stmt: &impl StatementBuilder,
) -> Result<Vec<QueryResult>, CreationError> {
    conn.query_all(DbBackend::Postgres.build(stmt))
        .await
        .map_err(twin_db)
}

/// Run one built write on the transaction.
async fn execute(conn: &TxnConn<'_>, stmt: &impl StatementBuilder) -> Result<(), CreationError> {
    conn.execute(DbBackend::Postgres.build(stmt))
        .await
        .map_err(twin_db)?;
    Ok(())
}

/// One column of a row, by name.
fn get<T: TryGetable>(row: &QueryResult, column: &str) -> Result<T, CreationError> {
    row.try_get("", column).map_err(twin_db)
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

    /// One pod `(id, project_id)` row, or none.
    async fn pod_view(
        &self,
        stmt: &sea_orm::sea_query::SelectStatement,
    ) -> Result<Option<PodView>, CreationError> {
        let row = query_one(&self.conn, stmt).await?;
        row.map(|row| {
            Ok(PodView {
                id: get(&row, "id")?,
                project_id: get(&row, "project_id")?,
            })
        })
        .transpose()
    }
}

#[allow(async_fn_in_trait)]
impl CreationSeam for SeaHandoffStore<'_> {
    async fn issue(&mut self, issue_id: Uuid) -> Result<IssueView, CreationError> {
        let row = query_one(&self.conn, &orm::issue_select(issue_id))
            .await?
            .ok_or_else(|| expected_row("issue"))?;
        twin_map_issue(&row)
    }

    async fn project(&mut self, project_id: Uuid) -> Result<ProjectView, CreationError> {
        let row = query_one(&self.conn, &orm::project_select(project_id))
            .await?
            .ok_or_else(|| expected_row("project"))?;
        twin_map_project(&row)
    }

    async fn state(&mut self, state_id: Option<Uuid>) -> Result<Option<StateView>, CreationError> {
        let Some(state_id) = state_id else {
            return Ok(None);
        };
        let row = query_one(&self.conn, &orm::state_select(state_id)).await?;
        match row {
            Some(row) => Ok(Some(twin_map_state(&row)?)),
            None => Err(CreationError::MissingRow(format!("no state {state_id}"))),
        }
    }

    async fn latest_prior_run(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = query_one(&self.conn, &orm::latest_prior_run_select(issue_id)).await?;
        row.map(|row| twin_map_run_view(&row)).transpose()
    }

    async fn active_run_for(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = query_one(&self.conn, &orm::active_run_select(issue_id)).await?;
        row.map(|row| twin_map_run_view(&row)).transpose()
    }

    async fn run(&mut self, run_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = query_one(&self.conn, &orm::run_select(run_id)).await?;
        row.map(|row| twin_map_run_view(&row)).transpose()
    }

    async fn runner(&mut self, runner_id: Uuid) -> Result<Option<RunnerView>, CreationError> {
        let row = query_one(&self.conn, &orm::runner_select(runner_id)).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let machine_id: Option<Uuid> = get(&row, "dev_machine_id")?;
        let engine_paused = match machine_id {
            Some(machine_id) => {
                let mrow = query_one(&self.conn, &orm::dev_machine_select(machine_id)).await?;
                match mrow {
                    Some(mrow) => {
                        let engine_paused_by_owner: bool = get(&mrow, "engine_paused_by_owner")?;
                        let engine_auto_pause_reason: String =
                            get(&mrow, "engine_auto_pause_reason")?;
                        engine_paused_by_owner || !engine_auto_pause_reason.is_empty()
                    }
                    None => false,
                }
            }
            None => false,
        };
        Ok(Some(RunnerView {
            id: get(&row, "id")?,
            pod_id: get(&row, "pod_id")?,
            status: get(&row, "status")?,
            last_heartbeat_at: get(&row, "last_heartbeat_at")?,
            paused_at: get(&row, "paused_at")?,
            provisioning: get(&row, "provisioning")?,
            pod_dispatch: get(&row, "pod_dispatch")?,
            engine_paused,
        }))
    }

    async fn assigned_pod(&mut self, pod_id: Uuid) -> Result<Option<PodView>, CreationError> {
        self.pod_view(&orm::assigned_pod_select(pod_id)).await
    }

    async fn default_pod_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<PodView>, CreationError> {
        self.pod_view(&orm::default_pod_select(project_id)).await
    }

    async fn resume_parent_run_id(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<Uuid>, CreationError> {
        let row = query_one(&self.conn, &orm::ticker_resume_select(issue_id)).await?;
        match row {
            Some(row) => get(&row, "resume_parent_run_id"),
            None => Ok(None),
        }
    }

    async fn work_item_id_for_run(&mut self, run_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        let row = query_one(&self.conn, &orm::work_item_id_select(run_id)).await?;
        match row {
            Some(row) => get(&row, "work_item_id"),
            None => Ok(None),
        }
    }

    async fn lock_issue_for_handoff(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<LockedIssue>, CreationError> {
        let locked = query_one(&self.conn, &orm::issue_lock_select(issue_id)).await?;
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
        let row = query_one(&self.conn, &orm::run_lock_select(run_id)).await?;
        row.map(|row| twin_map_run_view(&row)).transpose()
    }

    async fn user_flags(
        &mut self,
        user_id: Uuid,
    ) -> Result<crate::dispatch::UserFlags, CreationError> {
        let row = query_one(&self.conn, &orm::user_flags_select(user_id))
            .await?
            .ok_or_else(|| expected_row("user flags"))?;
        Ok(crate::dispatch::UserFlags {
            is_active: get(&row, "is_active")?,
            is_bot: get(&row, "is_bot")?,
        })
    }

    async fn insert_run(&mut self, row: &NewAgentRun) -> Result<RunView, CreationError> {
        let inserted = query_one(&self.conn, &orm::run_insert(row))
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
        execute(&self.conn, &orm::prompt_update(run_id, prompt, manifest)).await
    }

    async fn save_run_config(&mut self, run_id: Uuid, config: &Value) -> Result<(), CreationError> {
        execute(&self.conn, &orm::run_config_update(run_id, config)).await
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
            queued_cloud_count_select, quota_verdict, workspace_lock_select,
        };
        if executor_kind != AgentExecutorKind::CloudAgent {
            return Ok(None);
        }
        query_one(&self.conn, &workspace_lock_select(workspace_id))
            .await?
            .ok_or_else(|| expected_row("workspace lock"))?;
        let row = query_one(&self.conn, &queued_cloud_count_select(workspace_id))
            .await?
            .ok_or_else(|| expected_row("queued cloud count"))?;
        let queued: i64 = get(&row, "n")?;
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
        let locked = query_one(&self.conn, &orm::finalize_lock_select(run_id)).await?;
        if locked.is_none() {
            let row = query_one(&self.conn, &orm::run_select(run_id))
                .await?
                .ok_or_else(|| expected_row("unlocked run"))?;
            return twin_map_run_view(&row);
        }
        execute(
            &self.conn,
            &orm::finalize_update(
                run_id,
                now,
                &finalize_error_details_patch(error_code, error),
            ),
        )
        .await?;
        let locked = locked.map(|row| twin_map_run_view(&row)).transpose()?;
        if locked.is_some_and(|run| run.executor_kind == AgentExecutorKind::CloudAgent) {
            let row = query_one(&self.conn, &orm::terminal_event_exists_select(run_id))
                .await?
                .ok_or_else(|| expected_row("terminal event probe"))?;
            let exists: bool = get(&row, "exists")?;
            if !exists {
                let row = query_one(&self.conn, &orm::terminal_event_seq_select(run_id))
                    .await?
                    .ok_or_else(|| expected_row("terminal event seq"))?;
                let seq: i32 = get(&row, "seq")?;
                let payload = serde_json::json!({"status": "failed", "error_code": error_code});
                execute(
                    &self.conn,
                    &orm::terminal_event_insert(run_id, seq, &payload, now),
                )
                .await?;
            }
        }
        // The `on_commit` lambda (`_publish_effects`): captured for the
        // handoff drain below (the jobs twin returns it pending for D-15;
        // the api twin can drain it directly).
        self.terminal_effects.push(run_id);
        let row = query_one(&self.conn, &orm::run_select(run_id))
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

/// A [`orm::RUN_VIEW`] row, by name.
fn twin_map_run_view(row: &QueryResult) -> Result<RunView, CreationError> {
    let status: String = get(row, "status")?;
    let executor_kind: String = get(row, "executor_kind")?;
    Ok(RunView {
        id: get(row, "id")?,
        workspace_id: get(row, "workspace_id")?,
        created_by_id: get(row, "created_by_id")?,
        pod_id: get(row, "pod_id")?,
        runner_id: get(row, "runner_id")?,
        pinned_runner_id: get(row, "pinned_runner_id")?,
        parent_run_id: get(row, "parent_run_id")?,
        work_item_id: get(row, "work_item_id")?,
        status: twin_map_status(status)?,
        trigger: get(row, "trigger")?,
        executor_kind: twin_map_executor(executor_kind)?,
        phase_kind: get(row, "phase_kind")?,
        run_config: get(row, "run_config")?,
        tool_plan: get(row, "tool_plan")?,
        error_details: get(row, "error_details")?,
        prompt: get(row, "prompt")?,
        prompt_manifest: get(row, "prompt_manifest")?,
        ended_at: get(row, "ended_at")?,
    })
}

/// An [`orm::issue_select`] row, by name.
fn twin_map_issue(row: &QueryResult) -> Result<IssueView, CreationError> {
    Ok(IssueView {
        id: get(row, "id")?,
        workspace_id: get(row, "workspace_id")?,
        project_id: get(row, "project_id")?,
        state_id: get(row, "state_id")?,
        parent_id: get(row, "parent_id")?,
        created_by_id: get(row, "created_by_id")?,
        assigned_pod_id: get(row, "assigned_pod_id")?,
        agent_executor: get(row, "agent_executor")?,
        git_work_branch: get(row, "git_work_branch")?,
        workpad: get(row, "workpad")?,
        name: get(row, "name")?,
        description_stripped: get(row, "description_stripped")?,
        priority: get(row, "priority")?,
        sequence_id: get(row, "sequence_id")?,
        target_date: get(row, "target_date")?,
    })
}

/// An [`orm::project_select`] row, by name: the int4 tick columns
/// widen to i64.
fn twin_map_project(row: &QueryResult) -> Result<ProjectView, CreationError> {
    let pool: i32 = get(row, "agent_default_max_ticks")?;
    let interval_impl: i32 = get(row, "agent_default_interval_seconds")?;
    let interval_review: i32 = get(row, "agent_review_default_interval_seconds")?;
    let interval_test: i32 = get(row, "agent_test_default_interval_seconds")?;
    Ok(ProjectView {
        id: get(row, "id")?,
        workspace_id: get(row, "workspace_id")?,
        identifier: get(row, "identifier")?,
        name: get(row, "name")?,
        description: get(row, "description")?,
        repo_url: get(row, "repo_url")?,
        base_branch: get(row, "base_branch")?,
        default_agent_executor: get(row, "default_agent_executor")?,
        project_lead_id: get(row, "project_lead_id")?,
        default_assignee_id: get(row, "default_assignee_id")?,
        pool: i64::from(pool),
        interval_impl: i64::from(interval_impl),
        interval_review: i64::from(interval_review),
        interval_test: i64::from(interval_test),
    })
}

/// An [`orm::state_select`] row, by name.
fn twin_map_state(row: &QueryResult) -> Result<StateView, CreationError> {
    Ok(StateView {
        id: get(row, "id")?,
        name: get(row, "name")?,
        group: get(row, "group")?,
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

/// `get_config(user)` + `has_api_key` (`llm.py:67-68`,
/// `models.py:260-262`): `UserLLMConfig.objects.filter(user).first()`
/// (unordered, `LIMIT 1`), key present and non-empty. Runs on the pool
/// (outside the span), as the twin does.
async fn twin_has_api_key(pool: &PgPool, user_id: Uuid) -> Result<bool, String> {
    use pidash_db::entity::assistant_user_llm_config as config;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
    let key: Option<Option<Vec<u8>>> = config::Entity::find()
        .select_only()
        .column(config::Column::ApiKeyEncrypted)
        .filter(config::Column::UserId.eq(user_id))
        .into_tuple()
        .one(&pidash_db::pool::sea_orm_connection(pool))
        .await
        .map_err(|error| error.to_string())?;
    Ok(key.flatten().is_some_and(|key| !key.is_empty()))
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

async fn twin_bundle_labels(
    conn: &TxnConn<'_>,
    issue_id: Uuid,
) -> Result<Vec<String>, CreationError> {
    let rows = query_all(conn, &orm::bundle_labels_select(issue_id)).await?;
    rows.iter().map(|row| get(row, "name")).collect()
}

async fn twin_bundle_assignees(
    conn: &TxnConn<'_>,
    issue_id: Uuid,
) -> Result<Vec<String>, CreationError> {
    let rows = query_all(conn, &orm::bundle_assignees_select(issue_id)).await?;
    rows.iter()
        .map(|row| {
            let display: Option<String> = get(row, "display_name")?;
            let email: Option<String> = get(row, "email")?;
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
    let rows = query_all(conn, &orm::bundle_project_states_select(project_id)).await?;
    rows.iter()
        .map(|row| {
            Ok(context::ProjectStateView {
                name: get(row, "name")?,
                group: get(row, "group")?,
                description: get(row, "description")?,
            })
        })
        .collect()
}

async fn twin_bundle_children(
    conn: &TxnConn<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::IssueRef>, CreationError> {
    let rows = query_all(conn, &orm::bundle_children_select(issue_id)).await?;
    rows.iter()
        .map(|row| {
            let name: Option<String> = get(row, "name")?;
            let sequence: i32 = get(row, "sequence_id")?;
            let project_identifier: String = get(row, "identifier")?;
            let state: Option<String> = get(row, "state_name")?;
            Ok(context::IssueRef {
                identifier: context::issue_identifier(&project_identifier, i64::from(sequence)),
                title: name.unwrap_or_default(),
                state: state.unwrap_or_default(),
            })
        })
        .collect()
}

/// One [`orm::bundle_relation_targets_select`] row.
struct TwinTarget {
    id: Uuid,
    name: Option<String>,
    sequence: i32,
    project_identifier: String,
    state: Option<String>,
    group: Option<String>,
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
    let rows = query_all(conn, &orm::bundle_relations_select(issue_id)).await?;
    let rows: Vec<(Uuid, Uuid, String)> = rows
        .iter()
        .map(|row| {
            Ok((
                get(row, "issue_id")?,
                get(row, "related_issue_id")?,
                get(row, "relation_type")?,
            ))
        })
        .collect::<Result<_, CreationError>>()?;
    let mut other_ids: Vec<Uuid> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for (left, right, _) in &rows {
        let other = if left == &issue_id { *right } else { *left };
        if other != issue_id && seen_ids.insert(other) {
            other_ids.push(other);
        }
    }
    let targets: Vec<TwinTarget> = if other_ids.is_empty() {
        Vec::new()
    } else {
        query_all(conn, &orm::bundle_relation_targets_select(&other_ids))
            .await?
            .iter()
            .map(|row| {
                Ok(TwinTarget {
                    id: get(row, "id")?,
                    name: get(row, "name")?,
                    sequence: get(row, "sequence_id")?,
                    project_identifier: get(row, "identifier")?,
                    state: get(row, "state_name")?,
                    group: get(row, "state_group")?,
                })
            })
            .collect::<Result<_, CreationError>>()?
    };
    let mut refs = std::collections::HashMap::new();
    let mut directional_refs = std::collections::HashMap::new();
    for target in targets {
        let key = target.id.to_string();
        let title = target.name.unwrap_or_default();
        let state_name = target.state.unwrap_or_default();
        let identifier =
            context::issue_identifier(&target.project_identifier, i64::from(target.sequence));
        refs.insert(
            key.clone(),
            context::IssueRef {
                identifier: identifier.clone(),
                title: title.clone(),
                state: state_name.clone(),
            },
        );
        directional_refs.insert(
            key,
            context::DirectionalRef {
                identifier,
                title,
                state: state_name,
                state_group: target.group.unwrap_or_default(),
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
    let rows = query_all(conn, &orm::bundle_comments_select(issue_id)).await?;
    rows.iter()
        .map(|row| {
            let stripped: Option<String> = get(row, "comment_stripped")?;
            let speaker_type: Option<String> = get(row, "speaker_type")?;
            let speaker_label: Option<String> = get(row, "speaker_label")?;
            let run_id: Option<Uuid> = get(row, "speaker_agent_run_id")?;
            let created_at: Option<DateTime<Utc>> = get(row, "created_at")?;
            let display: Option<String> = get(row, "display_name")?;
            let email: Option<String> = get(row, "email")?;
            let username: Option<String> = get(row, "username")?;
            let is_bot: Option<bool> = get(row, "is_bot")?;
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
    let rows = query_all(conn, &orm::bundle_code_reviews_select(issue_id)).await?;
    rows.iter()
        .map(|row| {
            Ok(context::CodeReviewView {
                url: get(row, "url")?,
                title: get(row, "title")?,
                state: get(row, "state")?,
                merged: get(row, "merged")?,
                draft: get(row, "draft")?,
                provider: get(row, "provider")?,
                external_iid: get(row, "external_iid")?,
            })
        })
        .collect()
}

async fn twin_bundle_remote(
    conn: &TxnConn<'_>,
    project_id: Uuid,
) -> Result<(Option<context::RemoteView>, Option<context::AdapterNames>), CreationError> {
    let row = query_one(conn, &orm::bundle_remote_select(project_id)).await?;
    let Some(row) = row else {
        return Ok((None, None));
    };
    let provider: String = get(&row, "provider")?;
    let host_url: String = get(&row, "host_url")?;
    let full_name: String = get(&row, "full_name")?;
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
        let row = query_one(conn, &orm::bundle_ancestor_hop_select(id)).await?;
        let Some(row) = row else {
            break;
        };
        let name: Option<String> = get(&row, "name")?;
        let project_id: Option<Uuid> = get(&row, "project_id")?;
        let parent_id: Option<Uuid> = get(&row, "parent_id")?;
        let project_identifier = match project_id {
            Some(pid) => {
                let row = query_one(conn, &orm::bundle_project_identifier_select(pid)).await?;
                match row {
                    Some(row) => get(&row, "identifier")?,
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

/// [`orm::bundle_sequence_select`]: the node's `sequence_id`, `0` when
/// the row is gone.
async fn twin_bundle_sequence(conn: &TxnConn<'_>, issue_id: Uuid) -> Result<i64, CreationError> {
    let row = query_one(conn, &orm::bundle_sequence_select(issue_id)).await?;
    match row {
        Some(row) => {
            let sequence: i32 = get(&row, "sequence_id")?;
            Ok(i64::from(sequence))
        }
        None => Ok(0),
    }
}

async fn twin_bundle_parent_and_lineage(
    conn: &TxnConn<'_>,
    ancestors: &[TwinChainNode],
) -> Result<(Option<context::ParentView>, Vec<context::LineageNode>), CreationError> {
    let parent = match ancestors.get(1) {
        None => None,
        Some(node) => {
            let row = query_one(conn, &orm::bundle_parent_cols_select(node.id)).await?;
            match row {
                None => None,
                Some(row) => {
                    let name: Option<String> = get(&row, "name")?;
                    let state_id: Option<Uuid> = get(&row, "state_id")?;
                    let work_branch: Option<String> = get(&row, "git_work_branch")?;
                    let comments_count: i64 = get(&row, "comments_count")?;
                    let state_name = match state_id {
                        Some(id) => {
                            let row = query_one(conn, &orm::state_select(id)).await?;
                            match row {
                                Some(row) => Some(get::<String>(&row, "name")?),
                                None => None,
                            }
                        }
                        None => None,
                    };
                    let row =
                        query_one(conn, &orm::bundle_parent_description_select(node.id)).await?;
                    let description: Option<String> = match row {
                        Some(row) => get(&row, "description_stripped")?,
                        None => None,
                    };
                    let sequence = twin_bundle_sequence(conn, node.id).await?;
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
            let sequence = twin_bundle_sequence(conn, node.id).await?;
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
    let row = query_one(conn, &orm::bundle_done_payload_select(run_id)).await?;
    match row {
        Some(row) => get(&row, "done_payload"),
        None => Ok(None),
    }
}

async fn twin_bundle_overrides(
    conn: &TxnConn<'_>,
    workspace_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<Vec<composer::OverrideRow>, CreationError> {
    let rows = query_all(conn, &orm::bundle_overrides_select(workspace_id, user_id)).await?;
    rows.iter()
        .map(|row| {
            let version: i32 = get(row, "version")?;
            let user_id: Option<Uuid> = get(row, "user_id")?;
            Ok(composer::OverrideRow {
                workspace_id: workspace_id.to_string(),
                section_key: get(row, "section_key")?,
                body: get(row, "body")?,
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
    let row = query_one(conn, &orm::issue_select(issue_id))
        .await?
        .ok_or_else(|| expected_row("bundle issue"))?;
    let issue = twin_map_issue(&row)?;
    let project_id = issue
        .project_id
        .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
    let row = query_one(conn, &orm::project_select(project_id))
        .await?
        .ok_or_else(|| expected_row("bundle project"))?;
    let project = twin_map_project(&row)?;
    let state = match issue.state_id {
        Some(id) => {
            let row = query_one(conn, &orm::state_select(id)).await?;
            row.map(|row| twin_map_state(&row)).transpose()?
        }
        None => None,
    };
    let row = query_one(conn, &orm::bundle_workspace_select(issue.workspace_id))
        .await?
        .ok_or_else(|| expected_row("bundle workspace"))?;
    let workspace_slug: String = get(&row, "slug")?;
    let workspace_name: String = get(&row, "name")?;
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
    let row = query_one(conn, &orm::bundle_prior_run_count_select(issue_id, run_id))
        .await?
        .ok_or_else(|| expected_row("prior run count"))?;
    let prior_run_count: i64 = get(&row, "n")?;
    let row = query_one(conn, &orm::bundle_ticker_select(issue_id)).await?;
    let ticker = match row {
        Some(row) => {
            let used: i32 = get(&row, "used")?;
            let waited: i32 = get(&row, "waited")?;
            let granted: i32 = get(&row, "granted")?;
            let enabled: bool = get(&row, "enabled")?;
            Some(TickerBudget {
                used: i64::from(used),
                waited: i64::from(waited),
                granted: i64::from(granted),
                enabled,
            })
        }
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
        let row = query_one(conn, &orm::ticker_resume_select(issue_id)).await?;
        let resume: Option<Uuid> = match row {
            Some(row) => get(&row, "resume_parent_run_id")?,
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
        ticker,
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

#[cfg(test)]
mod tests {
    //! Live coverage of the [`orm`] statements this seam runs: each
    //! builder executes against temp copies of the production tables
    //! inside one rolled-back sqlx transaction (`TxConn`), and the rows
    //! decode through the same `twin_map_*` / by-name reads the seam
    //! uses. Skips without `DATABASE_URL`.

    use super::*;
    use pidash_db::orm_tx::TxConn;
    use pidash_db::runner_runs::agent_run::{ERROR_DETAILS_CODE, ERROR_DETAILS_MESSAGE};
    use serde_json::json;

    async fn scratch_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::postgres::PgPoolOptions::new()
                    .max_connections(1)
                    .connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    /// Temp copies of the named tables (columns and types only: no
    /// constraints, so a seed binds just the columns a statement
    /// reads), dropped with the transaction.
    async fn temp_copies(tx: &mut sqlx::PgConnection, tables: &[&str]) {
        for table in tables {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} ON COMMIT DROP AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&mut *tx)
            .await
            .unwrap_or_else(|error| panic!("temp {table}: {error}"));
        }
    }

    async fn one(conn: &TxConn<'_>, stmt: &impl StatementBuilder) -> Option<QueryResult> {
        conn.query_one(DbBackend::Postgres.build(stmt))
            .await
            .expect("query_one")
    }

    async fn all(conn: &TxConn<'_>, stmt: &impl StatementBuilder) -> Vec<QueryResult> {
        conn.query_all(DbBackend::Postgres.build(stmt))
            .await
            .expect("query_all")
    }

    async fn exec(conn: &TxConn<'_>, stmt: &impl StatementBuilder) {
        conn.execute(DbBackend::Postgres.build(stmt))
            .await
            .expect("execute");
    }

    fn col<T: TryGetable>(row: &QueryResult, name: &str) -> T {
        row.try_get("", name)
            .unwrap_or_else(|error| panic!("column {name}: {error}"))
    }

    fn t0() -> DateTime<Utc> {
        "2026-06-01T12:00:00+00:00".parse().expect("T0")
    }

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn new_run(id: Uuid, issue: Uuid, now: DateTime<Utc>) -> NewAgentRun {
        NewAgentRun {
            id,
            workspace_id: uid(0x10),
            created_by_id: uid(0x40),
            pod_id: uid(0x50),
            pinned_runner_id: None,
            work_item_id: issue,
            parent_run_id: None,
            executor_kind: AgentExecutorKind::CloudAgent,
            error_details: json!({"other": 1, "code": "old"}),
            tool_plan: json!({"tools": ["a"]}),
            trigger: "state_transition".to_owned(),
            phase_kind: "implement".to_owned(),
            run_config: json!({"k": "v"}),
            now,
        }
    }

    /// The run lifecycle statements: insert + RETURNING, the four run
    /// reads, the lock forms, the prompt/config saves, the finalize
    /// lock + merge, and the terminal event guard/seq/insert.
    #[tokio::test]
    async fn live_run_lifecycle_statements() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin");
        temp_copies(&mut tx, &["agent_run", "agent_run_event", "workspaces"]).await;
        let now = t0();
        let issue = uid(0x01);
        let run_id = uid(0xA1);
        sqlx::query("INSERT INTO workspaces (id, deleted_at) VALUES ($1, NULL), ($2, $3)")
            .bind(uid(0x10))
            .bind(uid(0x11))
            .bind(now)
            .execute(&mut *tx)
            .await
            .expect("seed workspaces");
        let conn = TxConn::new(&mut tx);

        // INSERT ... RETURNING the run view, decoded by name.
        let inserted = one(&conn, &orm::run_insert(&new_run(run_id, issue, now)))
            .await
            .expect("returning row");
        let view = twin_map_run_view(&inserted).expect("map run view");
        assert_eq!(view.id, run_id);
        assert_eq!(view.status, DbAgentRunStatus::Queued);
        assert_eq!(view.executor_kind, AgentExecutorKind::CloudAgent);
        assert_eq!(view.work_item_id, Some(issue));
        assert_eq!(view.phase_kind, "implement");
        assert_eq!(view.trigger, "state_transition");
        assert_eq!(view.prompt, "");
        assert_eq!(view.prompt_manifest, None);
        assert_eq!(view.ended_at, None);
        assert_eq!(view.run_config, json!({"k": "v"}));

        // The literals the INSERT fixes (`cancel_reason`, `thread_id`,
        // `required_capabilities`, `agent_metadata`, `usage`, `llm_model`).
        let fixed = one(
            &conn,
            &sea_orm::sea_query::Query::select()
                .columns([
                    pidash_db::entity::agent_run::Column::CancelReason,
                    pidash_db::entity::agent_run::Column::ThreadId,
                    pidash_db::entity::agent_run::Column::RequiredCapabilities,
                    pidash_db::entity::agent_run::Column::AgentMetadata,
                    pidash_db::entity::agent_run::Column::Usage,
                    pidash_db::entity::agent_run::Column::LlmModel,
                    pidash_db::entity::agent_run::Column::DispatchAttempts,
                    pidash_db::entity::agent_run::Column::OwnerId,
                ])
                .from(pidash_db::entity::agent_run::Entity)
                .to_owned(),
        )
        .await
        .expect("fixed literals");
        assert_eq!(col::<String>(&fixed, "cancel_reason"), "");
        assert_eq!(col::<String>(&fixed, "thread_id"), "");
        assert_eq!(col::<Value>(&fixed, "required_capabilities"), json!([]));
        assert_eq!(col::<Value>(&fixed, "agent_metadata"), json!({}));
        assert_eq!(col::<Value>(&fixed, "usage"), json!({}));
        assert_eq!(col::<String>(&fixed, "llm_model"), "");
        assert_eq!(col::<i32>(&fixed, "dispatch_attempts"), 0);
        assert_eq!(col::<Option<Uuid>>(&fixed, "owner_id"), None);

        // A second, older run on the item: the newest-first reads pick
        // the new one; the active read skips a terminal row.
        let older = uid(0xA0);
        let mut old_row = new_run(older, issue, now - chrono::Duration::hours(1));
        old_row.executor_kind = AgentExecutorKind::LocalRunner;
        exec(&conn, &orm::run_insert(&old_row)).await;
        exec(
            &conn,
            &orm::finalize_update(older, now, &finalize_error_details_patch("x", "y")),
        )
        .await;
        for stmt in [
            orm::latest_prior_run_select(issue),
            orm::active_run_select(issue),
            orm::run_select(run_id),
            orm::run_lock_select(run_id),
            orm::finalize_lock_select(run_id),
        ] {
            let row = one(&conn, &stmt).await.expect("run row");
            assert_eq!(col::<Uuid>(&row, "id"), run_id);
        }
        assert!(one(&conn, &orm::finalize_lock_select(older))
            .await
            .is_none());
        assert_eq!(
            col::<Option<Uuid>>(
                &one(&conn, &orm::work_item_id_select(run_id))
                    .await
                    .expect("row"),
                "work_item_id"
            ),
            Some(issue)
        );
        assert_eq!(
            col::<i64>(
                &one(&conn, &orm::bundle_prior_run_count_select(issue, run_id))
                    .await
                    .expect("row"),
                "n"
            ),
            1
        );

        // Prompt + config saves.
        exec(
            &conn,
            &orm::prompt_update(run_id, "hello", &json!({"sections": 2})),
        )
        .await;
        exec(&conn, &orm::run_config_update(run_id, &json!({"k": "w"}))).await;
        let view = twin_map_run_view(&one(&conn, &orm::run_select(run_id)).await.expect("row"))
            .expect("map");
        assert_eq!(view.prompt, "hello");
        assert_eq!(view.prompt_manifest, Some(json!({"sections": 2})));
        assert_eq!(view.run_config, json!({"k": "w"}));

        // Finalize: the patch's keys replace the stored ones, other keys
        // survive, a null (empty) patch value removes its key.
        let patch = finalize_error_details_patch("prompt_build_failed", "");
        exec(&conn, &orm::finalize_update(run_id, now, &patch)).await;
        let view = twin_map_run_view(&one(&conn, &orm::run_select(run_id)).await.expect("row"))
            .expect("map");
        assert_eq!(view.status, DbAgentRunStatus::Failed);
        assert_eq!(view.ended_at, Some(now));
        assert_eq!(
            view.error_details,
            json!({"other": 1, ERROR_DETAILS_CODE: "prompt_build_failed"})
        );
        assert!(one(&conn, &orm::finalize_lock_select(run_id))
            .await
            .is_none());
        assert!(one(&conn, &orm::active_run_select(issue)).await.is_none());
        // A non-object bag resets to `{}` before the merge.
        exec(
            &conn,
            &sea_orm::sea_query::Query::update()
                .table(pidash_db::entity::agent_run::Entity)
                .value(
                    pidash_db::entity::agent_run::Column::ErrorDetails,
                    json!("not an object"),
                )
                .and_where(
                    sea_orm::sea_query::Expr::col(pidash_db::entity::agent_run::Column::Id)
                        .eq(older),
                )
                .to_owned(),
        )
        .await;
        exec(
            &conn,
            &orm::finalize_update(older, now, &finalize_error_details_patch("c", "m")),
        )
        .await;
        let view = twin_map_run_view(&one(&conn, &orm::run_select(older)).await.expect("row"))
            .expect("map");
        assert_eq!(
            view.error_details,
            json!({ERROR_DETAILS_CODE: "c", ERROR_DETAILS_MESSAGE: "m"})
        );

        // Terminal event: absent -> seq 1 -> present -> seq 2.
        assert!(!terminal_event_exists(&conn, run_id).await);
        assert_eq!(terminal_event_seq(&conn, run_id).await, 1);
        exec(
            &conn,
            &orm::terminal_event_insert(run_id, 1, &json!({"status": "failed"}), now),
        )
        .await;
        assert!(terminal_event_exists(&conn, run_id).await);
        assert_eq!(terminal_event_seq(&conn, run_id).await, 2);
        let event = one(
            &conn,
            &sea_orm::sea_query::Query::select()
                .columns([
                    pidash_db::entity::agent_run_event::Column::Kind,
                    pidash_db::entity::agent_run_event::Column::Payload,
                    pidash_db::entity::agent_run_event::Column::AgentRunId,
                ])
                .from(pidash_db::entity::agent_run_event::Entity)
                .to_owned(),
        )
        .await
        .expect("event row");
        assert_eq!(col::<String>(&event, "kind"), "terminal");
        assert_eq!(col::<Value>(&event, "payload"), json!({"status": "failed"}));
        assert_eq!(col::<Uuid>(&event, "agent_run_id"), run_id);

        // The workspace mutex: live row locks, soft-deleted row misses;
        // the queued-cloud count sees the one queued cloud run left.
        use crate::dispatch::execution::{queued_cloud_count_select, workspace_lock_select};
        assert!(one(&conn, &workspace_lock_select(uid(0x10)))
            .await
            .is_some());
        assert!(one(&conn, &workspace_lock_select(uid(0x11)))
            .await
            .is_none());
        exec(&conn, &orm::run_insert(&new_run(uid(0xA2), issue, now))).await;
        assert_eq!(
            col::<i64>(
                &one(&conn, &queued_cloud_count_select(uid(0x10)))
                    .await
                    .expect("count row"),
                "n"
            ),
            1
        );
        tx.rollback().await.expect("rollback");
    }

    /// The render-bundle reads over a small seeded graph: scopes
    /// (soft-delete, triage, archived, draft, fold), joins (INNER vs
    /// LEFT), orderings and the by-name decodes.
    #[tokio::test]
    async fn live_render_bundle_statements() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin");
        temp_copies(
            &mut tx,
            &[
                "issues",
                "projects",
                "states",
                "workspaces",
                "users",
                "labels",
                "issue_labels",
                "issue_assignees",
                "issue_relations",
                "issue_comments",
                "git_code_review_links",
                "git_repository_bindings",
                "git_repositories",
                "issue_agent_ticker",
                "prompt_section_override",
                "pod",
                "runner",
                "agent_run",
            ],
        )
        .await;
        let now = t0();
        let ws = uid(0x10);
        let project = uid(0x20);
        let other_project = uid(0x21);
        let (todo, triage, done) = (uid(0x30), uid(0x31), uid(0x32));
        let (issue, parent, grand, child_ok, child_triage, child_draft, related) = (
            uid(0x01),
            uid(0x02),
            uid(0x03),
            uid(0x04),
            uid(0x05),
            uid(0x06),
            uid(0x07),
        );
        let (alice, bob) = (uid(0x40), uid(0x41));
        let seed = |sql: &'static str| sql;
        sqlx::query(seed(
            "INSERT INTO workspaces (id, slug, name) VALUES ($1, 'acme', 'Acme')",
        ))
        .bind(ws)
        .execute(&mut *tx)
        .await
        .expect("workspaces");
        sqlx::query(seed(
            "INSERT INTO projects (id, workspace_id, identifier, name, description, repo_url, base_branch, \
             default_agent_executor, project_lead_id, default_assignee_id, agent_default_max_ticks, \
             agent_default_interval_seconds, agent_review_default_interval_seconds, \
             agent_test_default_interval_seconds, archived_at) VALUES \
             ($1, $3, 'ACME', 'Acme', 'd', 'https://r', 'main', 'cloud_agent', NULL, NULL, 5, 60, 70, 80, NULL), \
             ($2, $3, 'OTHR', 'Other', 'd', '', 'main', '', NULL, NULL, 1, 1, 1, 1, NULL)",
        ))
        .bind(project)
        .bind(other_project)
        .bind(ws)
        .execute(&mut *tx)
        .await
        .expect("projects");
        sqlx::query(seed(
            "INSERT INTO states (id, project_id, name, \"group\", description, sequence, deleted_at) VALUES \
             ($1, $4, 'Todo', 'unstarted', 'first', 20000, NULL), \
             ($2, $4, 'Triage', 'triage', NULL, 10000, NULL), \
             ($3, $4, 'Done', 'completed', NULL, 30000, NULL), \
             ($5, $4, 'Gone', 'started', NULL, 25000, $6)",
        ))
        .bind(todo)
        .bind(triage)
        .bind(done)
        .bind(project)
        .bind(uid(0x33))
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("states");
        // issue (parent -> grand chain), four children (one live, one
        // triage, one draft, one soft-deleted) and one relation target.
        sqlx::query(seed(
            "INSERT INTO issues (id, workspace_id, project_id, state_id, parent_id, created_by_id, \
             assigned_pod_id, agent_executor, git_work_branch, workpad, name, description_stripped, \
             priority, sequence_id, target_date, deleted_at, archived_at, is_draft, created_at) VALUES \
             ($1, $9, $10, $11, $2, $13, NULL, 'cloud_agent', 'feat/x', 'pad', 'Issue', 'desc', 'high', 7, '2026-07-01', NULL, NULL, FALSE, $14), \
             ($2, $9, $10, $12, $3, NULL, NULL, NULL, 'feat/p', NULL, 'Parent', 'pdesc', 'none', 6, NULL, NULL, NULL, FALSE, $14), \
             ($3, $9, $10, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 'Grand', NULL, 'none', 5, NULL, NULL, NULL, FALSE, $14), \
             ($4, $9, $10, $11, $1, NULL, NULL, NULL, NULL, NULL, 'Child', NULL, 'none', 8, NULL, NULL, NULL, FALSE, $14 + interval '1 hour'), \
             ($5, $9, $10, $12, $1, NULL, NULL, NULL, NULL, NULL, 'Triaged', NULL, 'none', 9, NULL, NULL, NULL, FALSE, $14), \
             ($6, $9, $10, $11, $1, NULL, NULL, NULL, NULL, NULL, 'Draft', NULL, 'none', 10, NULL, NULL, NULL, TRUE, $14), \
             ($7, $9, $10, $11, $1, NULL, NULL, NULL, NULL, NULL, 'Deleted', NULL, 'none', 11, NULL, $14, NULL, FALSE, $14), \
             ($8, $9, $10, $11, NULL, NULL, NULL, NULL, NULL, NULL, 'Related', NULL, 'none', 12, NULL, NULL, NULL, FALSE, $14)",
        ))
        .bind(issue)
        .bind(parent)
        .bind(grand)
        .bind(child_ok)
        .bind(child_triage)
        .bind(child_draft)
        .bind(uid(0x08))
        .bind(related)
        .bind(ws)
        .bind(project)
        .bind(todo)
        .bind(triage)
        .bind(alice)
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("issues");
        sqlx::query(seed(
            "INSERT INTO users (id, display_name, email, username, is_bot, is_active, created_at) VALUES \
             ($1, 'Alice', 'alice@x', 'alice', FALSE, TRUE, $3), \
             ($2, '', 'bob@x', 'bob', TRUE, FALSE, $3 + interval '1 hour')",
        ))
        .bind(alice)
        .bind(bob)
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("users");
        // Labels: a live one, a soft-deleted one, and a soft-deleted
        // link row to a live label (the link table is unfiltered).
        sqlx::query(seed(
            "INSERT INTO labels (id, name, deleted_at, created_at) VALUES \
             ($1, 'bug', NULL, $4), ($2, 'gone', $4, $4), ($3, 'perf', NULL, $4 + interval '1 hour')",
        ))
        .bind(uid(0x60))
        .bind(uid(0x61))
        .bind(uid(0x62))
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("labels");
        sqlx::query(seed(
            "INSERT INTO issue_labels (issue_id, label_id, deleted_at) VALUES \
             ($1, $2, NULL), ($1, $3, NULL), ($1, $4, $5)",
        ))
        .bind(issue)
        .bind(uid(0x60))
        .bind(uid(0x61))
        .bind(uid(0x62))
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("issue_labels");
        sqlx::query(seed(
            "INSERT INTO issue_assignees (issue_id, assignee_id, deleted_at) VALUES ($1, $2, NULL), ($1, $3, $4)",
        ))
        .bind(issue)
        .bind(alice)
        .bind(bob)
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("issue_assignees");
        sqlx::query(seed(
            "INSERT INTO issue_relations (issue_id, related_issue_id, relation_type, deleted_at, created_at) VALUES \
             ($1, $2, 'blocked_by', NULL, $3), ($2, $1, 'relates_to', NULL, $3 + interval '1 hour'), \
             ($1, $2, 'duplicate', $3, $3)",
        ))
        .bind(issue)
        .bind(related)
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("issue_relations");
        sqlx::query(seed(
            "INSERT INTO issue_comments (id, issue_id, comment_stripped, speaker_type, speaker_label, \
             speaker_agent_run_id, actor_id, labels, deleted_at, created_at) VALUES \
             ($1, $5, 'second', 'human', '', NULL, $6, '{}'::varchar[], NULL, $7 + interval '1 hour'), \
             ($2, $5, 'first', 'agent', 'Bot', $8, NULL, '{}'::varchar[], NULL, $7), \
             ($3, $5, 'folded', 'human', '', NULL, $6, ARRAY['fold']::varchar[], NULL, $7), \
             ($4, $5, 'deleted', 'human', '', NULL, $6, '{}'::varchar[], $7, $7), \
             ($9, $10, 'on parent', 'human', '', NULL, $6, '{}'::varchar[], NULL, $7), \
             ($11, $10, 'gone on parent', 'human', '', NULL, $6, '{}'::varchar[], $7, $7)",
        ))
        .bind(uid(0x70))
        .bind(uid(0x71))
        .bind(uid(0x72))
        .bind(uid(0x73))
        .bind(issue)
        .bind(alice)
        .bind(now)
        .bind(uid(0xA1))
        .bind(uid(0x74))
        .bind(parent)
        .bind(uid(0x75))
        .execute(&mut *tx)
        .await
        .expect("issue_comments");
        sqlx::query(seed(
            "INSERT INTO git_code_review_links (issue_id, url, title, state, merged, draft, provider, external_iid, deleted_at, created_at) VALUES \
             ($1, 'https://pr/1', 'PR one', 'open', FALSE, TRUE, 'github', '1', NULL, $2), \
             ($1, 'https://pr/2', NULL, 'merged', TRUE, FALSE, 'github', '2', NULL, $2 + interval '1 hour'), \
             ($1, 'https://pr/3', 'gone', 'closed', FALSE, FALSE, 'github', '3', $2, $2)",
        ))
        .bind(issue)
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("review links");
        sqlx::query(seed(
            "INSERT INTO git_repositories (id, provider, host_url, full_name, deleted_at) VALUES \
             ($1, 'gitlab', 'https://gl', 'g/one', NULL), ($2, 'github', 'https://gh', 'g/two', NULL), \
             ($3, 'github', 'https://gh', 'g/gone', $4)",
        ))
        .bind(uid(0x80))
        .bind(uid(0x81))
        .bind(uid(0x82))
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("repos");
        // Oldest live binding wins: the soft-deleted-repo binding and
        // the soft-deleted binding are both older but skipped.
        sqlx::query(seed(
            "INSERT INTO git_repository_bindings (project_id, repository_id, deleted_at, created_at) VALUES \
             ($1, $4, NULL, $5 - interval '2 hour'), ($1, $3, $5, $5 - interval '3 hour'), \
             ($1, $2, NULL, $5 - interval '1 hour')",
        ))
        .bind(project)
        .bind(uid(0x80))
        .bind(uid(0x81))
        .bind(uid(0x82))
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("bindings");
        sqlx::query(seed(
            "INSERT INTO issue_agent_ticker (issue_id, used, waited, granted, enabled, resume_parent_run_id) \
             VALUES ($1, 3, 1, 10, TRUE, $2)",
        ))
        .bind(issue)
        .bind(uid(0xA1))
        .execute(&mut *tx)
        .await
        .expect("ticker");
        sqlx::query(seed(
            "INSERT INTO agent_run (id, done_payload) VALUES ($1, $2), ($3, NULL)",
        ))
        .bind(uid(0xA1))
        .bind(json!({"summary": "done"}))
        .bind(uid(0xA2))
        .execute(&mut *tx)
        .await
        .expect("runs");
        sqlx::query(seed(
            "INSERT INTO prompt_section_override (section_key, body, version, user_id, is_active, workspace_id) VALUES \
             ('a', 'ws body', 1, NULL, TRUE, $1), ('b', 'alice body', 2, $2, TRUE, $1), \
             ('c', 'bob body', 1, $3, TRUE, $1), ('d', 'inactive', 1, NULL, FALSE, $1)",
        ))
        .bind(ws)
        .bind(alice)
        .bind(bob)
        .execute(&mut *tx)
        .await
        .expect("overrides");
        sqlx::query(seed(
            "INSERT INTO pod (id, project_id, is_default, deleted_at) VALUES \
             ($1, $4, TRUE, $5), ($2, $4, TRUE, NULL), ($3, $4, FALSE, NULL)",
        ))
        .bind(uid(0x50))
        .bind(uid(0x51))
        .bind(uid(0x52))
        .bind(project)
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("pods");
        sqlx::query(seed(
            "INSERT INTO runner (id, pod_id, status, last_heartbeat_at, paused_at) VALUES ($1, $2, 'online', $3, NULL)",
        ))
        .bind(uid(0x90))
        .bind(uid(0x51))
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("runner");
        let conn = TxConn::new(&mut tx);

        // Row projections.
        let view = twin_map_issue(&one(&conn, &orm::issue_select(issue)).await.expect("issue"))
            .expect("map issue");
        assert_eq!(view.name.as_deref(), Some("Issue"));
        assert_eq!(view.parent_id, Some(parent));
        assert_eq!(view.sequence_id, 7);
        assert_eq!(view.priority.as_deref(), Some("high"));
        let project_view = twin_map_project(
            &one(&conn, &orm::project_select(project))
                .await
                .expect("project"),
        )
        .expect("map project");
        assert_eq!(project_view.identifier, "ACME");
        assert_eq!(project_view.pool, 5);
        assert_eq!(project_view.interval_test, 80);
        let state = twin_map_state(&one(&conn, &orm::state_select(todo)).await.expect("state"))
            .expect("map state");
        assert_eq!(
            (state.name.as_str(), state.group.as_str()),
            ("Todo", "unstarted")
        );
        let flags = one(&conn, &orm::user_flags_select(bob))
            .await
            .expect("flags");
        assert_eq!(
            (
                col::<bool>(&flags, "is_active"),
                col::<bool>(&flags, "is_bot")
            ),
            (false, true)
        );
        let runner = one(&conn, &orm::runner_select(uid(0x90)))
            .await
            .expect("runner");
        assert_eq!(col::<Uuid>(&runner, "pod_id"), uid(0x51));
        assert_eq!(col::<String>(&runner, "status"), "online");
        assert_eq!(col::<Option<DateTime<Utc>>>(&runner, "paused_at"), None);
        // Pods: the soft-deleted default misses; the live default wins
        // the project pick.
        assert!(one(&conn, &orm::assigned_pod_select(uid(0x50)))
            .await
            .is_none());
        assert_eq!(
            col::<Uuid>(
                &one(&conn, &orm::assigned_pod_select(uid(0x52)))
                    .await
                    .expect("pod"),
                "id"
            ),
            uid(0x52)
        );
        assert_eq!(
            col::<Uuid>(
                &one(&conn, &orm::default_pod_select(project))
                    .await
                    .expect("default pod"),
                "id"
            ),
            uid(0x51)
        );
        assert!(one(&conn, &orm::issue_lock_select(issue)).await.is_some());
        assert!(one(&conn, &orm::issue_lock_select(uid(0xFF)))
            .await
            .is_none());

        // Bundle reads.
        let ws_row = one(&conn, &orm::bundle_workspace_select(ws))
            .await
            .expect("workspace");
        assert_eq!(col::<String>(&ws_row, "slug"), "acme");
        assert_eq!(col::<String>(&ws_row, "name"), "Acme");
        assert_eq!(
            twin_bundle_labels_live(&conn, issue).await,
            vec!["perf", "bug"],
            "live labels newest first; the soft-deleted link still renders"
        );
        let assignees: Vec<(Option<String>, Option<String>)> =
            all(&conn, &orm::bundle_assignees_select(issue))
                .await
                .iter()
                .map(|row| (col(row, "display_name"), col(row, "email")))
                .collect();
        assert_eq!(
            assignees,
            vec![
                (Some("".to_owned()), Some("bob@x".to_owned())),
                (Some("Alice".to_owned()), Some("alice@x".to_owned())),
            ],
            "link rows unfiltered, newest user first"
        );
        let states: Vec<(String, String, Option<String>)> =
            all(&conn, &orm::bundle_project_states_select(project))
                .await
                .iter()
                .map(|row| (col(row, "name"), col(row, "group"), col(row, "description")))
                .collect();
        assert_eq!(
            states,
            vec![
                (
                    "Todo".to_owned(),
                    "unstarted".to_owned(),
                    Some("first".to_owned())
                ),
                ("Done".to_owned(), "completed".to_owned(), None),
            ]
        );
        let children: Vec<(String, i32, String, Option<String>)> =
            all(&conn, &orm::bundle_children_select(issue))
                .await
                .iter()
                .map(|row| {
                    (
                        col(row, "name"),
                        col(row, "sequence_id"),
                        col(row, "identifier"),
                        col(row, "state_name"),
                    )
                })
                .collect();
        assert_eq!(
            children,
            vec![(
                "Child".to_owned(),
                8,
                "ACME".to_owned(),
                Some("Todo".to_owned())
            )]
        );
        let relations: Vec<(Uuid, Uuid, String)> = all(&conn, &orm::bundle_relations_select(issue))
            .await
            .iter()
            .map(|row| {
                (
                    col(row, "issue_id"),
                    col(row, "related_issue_id"),
                    col(row, "relation_type"),
                )
            })
            .collect();
        assert_eq!(
            relations,
            vec![
                (related, issue, "relates_to".to_owned()),
                (issue, related, "blocked_by".to_owned()),
            ]
        );
        let targets = all(
            &conn,
            &orm::bundle_relation_targets_select(&[related, uid(0x08), grand]),
        )
        .await;
        let mut target_rows: Vec<(Uuid, Option<String>, Option<String>)> = targets
            .iter()
            .map(|row| {
                (
                    col(row, "id"),
                    col(row, "state_name"),
                    col(row, "state_group"),
                )
            })
            .collect();
        target_rows.sort_by_key(|row| row.0);
        assert_eq!(
            target_rows,
            vec![
                (grand, None, None),
                (
                    related,
                    Some("Todo".to_owned()),
                    Some("unstarted".to_owned())
                ),
            ],
            "the soft-deleted target drops; a stateless one keeps NULLs"
        );
        let comment_rows = all(&conn, &orm::bundle_comments_select(issue)).await;
        let bodies: Vec<String> = comment_rows
            .iter()
            .map(|row| col(row, "comment_stripped"))
            .collect();
        assert_eq!(
            bodies,
            vec!["first", "second"],
            "chronological, fold/deleted out"
        );
        assert_eq!(
            col::<Option<String>>(&comment_rows[0], "display_name"),
            None,
            "LEFT join: a NULL actor keeps the row"
        );
        assert_eq!(
            col::<Option<Uuid>>(&comment_rows[0], "speaker_agent_run_id"),
            Some(uid(0xA1))
        );
        assert_eq!(col::<Option<bool>>(&comment_rows[1], "is_bot"), Some(false));
        let reviews: Vec<(String, Option<String>, bool)> =
            all(&conn, &orm::bundle_code_reviews_select(issue))
                .await
                .iter()
                .map(|row| (col(row, "url"), col(row, "title"), col(row, "merged")))
                .collect();
        assert_eq!(
            reviews,
            vec![
                ("https://pr/2".to_owned(), None, true),
                ("https://pr/1".to_owned(), Some("PR one".to_owned()), false),
            ]
        );
        let remote = one(&conn, &orm::bundle_remote_select(project))
            .await
            .expect("remote");
        assert_eq!(col::<String>(&remote, "provider"), "gitlab");
        assert_eq!(col::<String>(&remote, "full_name"), "g/one");
        assert!(one(&conn, &orm::bundle_remote_select(other_project))
            .await
            .is_none());
        let hop = one(&conn, &orm::bundle_ancestor_hop_select(parent))
            .await
            .expect("hop");
        assert_eq!(
            col::<Option<String>>(&hop, "name"),
            Some("Parent".to_owned())
        );
        assert_eq!(col::<Option<Uuid>>(&hop, "parent_id"), Some(grand));
        assert_eq!(
            col::<String>(
                &one(&conn, &orm::bundle_project_identifier_select(project))
                    .await
                    .expect("identifier"),
                "identifier"
            ),
            "ACME"
        );
        assert_eq!(
            col::<i32>(
                &one(&conn, &orm::bundle_sequence_select(grand))
                    .await
                    .expect("sequence"),
                "sequence_id"
            ),
            5
        );
        let parent_cols = one(&conn, &orm::bundle_parent_cols_select(parent))
            .await
            .expect("parent cols");
        assert_eq!(
            col::<Option<String>>(&parent_cols, "name"),
            Some("Parent".to_owned())
        );
        assert_eq!(col::<Option<Uuid>>(&parent_cols, "state_id"), Some(triage));
        assert_eq!(
            col::<Option<String>>(&parent_cols, "git_work_branch"),
            Some("feat/p".to_owned())
        );
        assert_eq!(
            col::<i64>(&parent_cols, "comments_count"),
            1,
            "live comments only"
        );
        assert_eq!(
            col::<Option<String>>(
                &one(&conn, &orm::bundle_parent_description_select(parent))
                    .await
                    .expect("description"),
                "description_stripped"
            ),
            Some("pdesc".to_owned())
        );
        let ticker = one(&conn, &orm::bundle_ticker_select(issue))
            .await
            .expect("ticker");
        assert_eq!(
            (
                col::<i32>(&ticker, "used"),
                col::<i32>(&ticker, "waited"),
                col::<i32>(&ticker, "granted"),
                col::<bool>(&ticker, "enabled"),
            ),
            (3, 1, 10, true)
        );
        assert_eq!(
            col::<Option<Uuid>>(
                &one(&conn, &orm::ticker_resume_select(issue))
                    .await
                    .expect("resume"),
                "resume_parent_run_id"
            ),
            Some(uid(0xA1))
        );
        assert!(one(&conn, &orm::ticker_resume_select(parent))
            .await
            .is_none());
        assert_eq!(
            col::<Option<Value>>(
                &one(&conn, &orm::bundle_done_payload_select(uid(0xA1)))
                    .await
                    .expect("payload"),
                "done_payload"
            ),
            Some(json!({"summary": "done"}))
        );
        assert_eq!(
            col::<Option<Value>>(
                &one(&conn, &orm::bundle_done_payload_select(uid(0xA2)))
                    .await
                    .expect("payload"),
                "done_payload"
            ),
            None
        );
        let keys = |rows: Vec<QueryResult>| {
            let mut keys: Vec<String> = rows.iter().map(|row| col(row, "section_key")).collect();
            keys.sort();
            keys
        };
        assert_eq!(
            keys(all(&conn, &orm::bundle_overrides_select(ws, Some(alice))).await),
            vec!["a", "b"],
            "active workspace rows plus the user's"
        );
        assert_eq!(
            keys(all(&conn, &orm::bundle_overrides_select(ws, None)).await),
            vec!["a"],
            "`user_id = NULL` never matches"
        );
        tx.rollback().await.expect("rollback");
    }

    /// [`orm::terminal_event_exists_select`]'s `exists` bit.
    async fn terminal_event_exists(conn: &TxConn<'_>, run_id: Uuid) -> bool {
        col(
            &one(conn, &orm::terminal_event_exists_select(run_id))
                .await
                .expect("probe row"),
            "exists",
        )
    }

    /// [`orm::terminal_event_seq_select`]'s next `seq`.
    async fn terminal_event_seq(conn: &TxConn<'_>, run_id: Uuid) -> i32 {
        col(
            &one(conn, &orm::terminal_event_seq_select(run_id))
                .await
                .expect("seq row"),
            "seq",
        )
    }

    /// [`orm::bundle_labels_select`] names, in order.
    async fn twin_bundle_labels_live(conn: &TxConn<'_>, issue_id: Uuid) -> Vec<String> {
        all(conn, &orm::bundle_labels_select(issue_id))
            .await
            .iter()
            .map(|row| col(row, "name"))
            .collect()
    }
}
