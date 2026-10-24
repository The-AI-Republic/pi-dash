#![forbid(unsafe_code)]

//! Live scheduler-run dispatch for the worker-owned
//! `fire_scheduler_binding` (PIDASHCONV-861).
//!
//! Three pieces, all live:
//!
//! * `impl EntriesSeam for LiveCreationStore`: the 16 entries methods
//!   over the insertion transaction. Reads and writes run on the
//!   driver's transaction (the `DispatchSeam` precedent), so the
//!   capacity lock, the INSERT, and the FAILED finalization share one
//!   transaction exactly as the services driver's contract requires.
//!   SQL text comes from the services consts, executed verbatim.
//! * [`dispatch_scheduler_run`]: the jobs transaction driver — open
//!   the transaction, run the services driver, roll back when the
//!   outcome says so, commit, then drain post-commit dispatch. The
//!   `dispatch_driver!` shape, written out because the services call
//!   takes `(seam, req)` instead of the continuation argument shape.
//! * [`live_scheduler_dispatch`]: the [`SchedulerDispatch`] value
//!   `worker()` registers. Each call prefetches presence for the
//!   binding's creator chain (actor, project lead, default assignee),
//!   runs the driver, and maps the outcome onto the
//!   `(run, fail_reason)` contract.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_db::orchestration::querysets::{
    fetch_project_has_allowed_role, fetch_project_is_member, fetch_workspace_admin_by_slug,
};
use pidash_db::orchestration::workpad::{get_agent_system_user, GetAgentUserError};
use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
use pidash_db::tx::Transaction;
use pidash_services::dispatch::admission::AdmissionCache;
use pidash_services::dispatch::LlmProfile;
use pidash_services::extensions::{AssistantModelSeam, CloudAgentToolsetsSeam};
use pidash_services::orchestration::clock::{ClockWrite, ProjectClockPolicy};
use pidash_services::orchestration::creation::{
    CreationError, PodView, RenderedTurn, RunView, BUNDLE_WORKSPACE_SQL,
};
use pidash_services::orchestration::dispatch::DispatchError;
use pidash_services::orchestration::entries::{
    dispatch_scheduler_run as services_dispatch_scheduler_run, queued_follow_up_sql,
    scheduler_run_insert_returning_sql, BindingView, EntriesError, EntriesSeam, NewSchedulerRun,
    ProjectRoleFacts, SchedulerOutcome, SchedulerRequest, SchedulerView, WorkspaceView,
    BINDING_SELECT_SQL, CLOCK_POLICY_SQL, PRIOR_STATE_SELECT_SQL, SCHEDULER_ALLOWED_ROLES,
    SCHEDULER_POD_OVERRIDE_SQL, SCHEDULER_SELECT_SQL,
};
use pidash_services::prompting::{composer, context};

use super::creation_jobs::{bundle_overrides, map_run_view, LiveCreationStore, StoreDeps};
use super::fire_tick_seam::{
    create_ticker_sql, drain_dispatch, lock_ticker_by_issue_sql, map_ticker, save_clock_sql,
    DispatchJobsError, DispatchOutcome,
};
use super::live_inputs::{
    fetch_presence, gate_fn, gate_verdicts, order_universe, profile_fn, project_leads,
};
use crate::admission::RedisAdmissionCache;
use crate::tasks_ticker::fire_binding::{
    BindingForDispatch, DispatchRun, SchedulerDispatch, SchedulerDispatchResult,
};

fn db(error: sqlx::Error) -> CreationError {
    CreationError::Db(error.to_string())
}

/// One [`CLOCK_POLICY_SQL`] row: ticking flag, pool, impl/review/test
/// cadence columns.
type ClockPolicyRow = (
    Option<bool>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
);

/// One [`BINDING_SELECT_SQL`] row: id, workspace, project,
/// scheduler, outcome mode, extra context, actor, pod override.
type BindingRow = (
    Uuid,
    Uuid,
    Option<Uuid>,
    Uuid,
    String,
    String,
    Option<Uuid>,
    Option<Uuid>,
);

impl<C, H, P> EntriesSeam for LiveCreationStore<'_, C, H, P>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    async fn prior_state_id(&mut self, issue_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(PRIOR_STATE_SELECT_SQL)
            .bind(issue_id)
            .fetch_optional(&mut **self.dispatch_tx().inner())
            .await
            .map_err(db)?;
        Ok(row.and_then(|(_, state_id)| state_id))
    }

    async fn queued_follow_up(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row: Option<PgRow> = sqlx::query(&queued_follow_up_sql())
            .bind(issue_id)
            .fetch_optional(&mut **self.dispatch_tx().inner())
            .await
            .map_err(db)?;
        row.map(|row| map_run_view(&row).map_err(db)).transpose()
    }

    async fn lock_ticker(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, CreationError> {
        let row: Option<PgRow> = sqlx::query(&lock_ticker_by_issue_sql())
            .bind(issue_id)
            .fetch_optional(&mut **self.dispatch_tx().inner())
            .await
            .map_err(db)?;
        row.map(|row| map_ticker(&row).map_err(db)).transpose()
    }

    async fn save_ticker(
        &mut self,
        row: &IssueAgentTicker,
        write: ClockWrite,
    ) -> Result<(), CreationError> {
        match write {
            ClockWrite::None => Ok(()),
            // CREATE_TICKER_PARAMS order: now, created_by_id, id,
            // issue_id, next_run_at, enabled, disarm_reason,
            // pending_entry, pending_entry_free,
            // pending_entry_actor_id, pending_entry_trigger,
            // resume_parent_run_id.
            ClockWrite::Insert => {
                sqlx::query(&create_ticker_sql())
                    .bind(row.created_at)
                    .bind(row.created_by_id)
                    .bind(row.id)
                    .bind(row.issue_id)
                    .bind(row.next_run_at)
                    .bind(row.enabled)
                    .bind(row.disarm_reason.as_str())
                    .bind(row.pending_entry)
                    .bind(row.pending_entry_free)
                    .bind(row.pending_entry_actor_id)
                    .bind(row.pending_entry_trigger.as_str())
                    .bind(row.resume_parent_run_id)
                    .execute(&mut **self.dispatch_tx().inner())
                    .await
                    .map_err(db)?;
                Ok(())
            }
            // SAVE_CLOCK_PARAMS order: updated_at, granted,
            // next_run_at, enabled, disarm_reason, pending_entry,
            // pending_entry_free, pending_entry_actor_id,
            // pending_entry_trigger, resume_parent_run_id, id.
            // `updated_at` is now-at-write: reconcile leaves the
            // locked row's stamp untouched, and Python's `save()`
            // stamps `auto_now` at write time.
            ClockWrite::Update => {
                sqlx::query(&save_clock_sql())
                    .bind(Utc::now())
                    .bind(row.granted)
                    .bind(row.next_run_at)
                    .bind(row.enabled)
                    .bind(row.disarm_reason.as_str())
                    .bind(row.pending_entry)
                    .bind(row.pending_entry_free)
                    .bind(row.pending_entry_actor_id)
                    .bind(row.pending_entry_trigger.as_str())
                    .bind(row.resume_parent_run_id)
                    .bind(row.id)
                    .execute(&mut **self.dispatch_tx().inner())
                    .await
                    .map_err(db)?;
                Ok(())
            }
        }
    }

    fn set_rollback(&mut self) {
        self.dispatch_request_rollback();
    }

    async fn clock_policy(
        &mut self,
        project_id: Uuid,
    ) -> Result<ProjectClockPolicy, CreationError> {
        let row: Option<ClockPolicyRow> = sqlx::query_as(CLOCK_POLICY_SQL)
            .bind(project_id)
            .fetch_optional(&mut **self.dispatch_tx().inner())
            .await
            .map_err(db)?;
        let Some((ticking_enabled, max_ticks, interval, interval_review, interval_test)) = row
        else {
            return Err(CreationError::MissingRow(format!(
                "no clock policy {project_id}"
            )));
        };
        Ok(ProjectClockPolicy {
            agent_ticking_enabled: ticking_enabled,
            agent_default_max_ticks: max_ticks,
            agent_default_interval_seconds: interval.map(i64::from),
            agent_review_default_interval_seconds: interval_review.map(i64::from),
            agent_test_default_interval_seconds: interval_test.map(i64::from),
        })
    }

    async fn binding(&mut self, binding_id: Uuid) -> Result<BindingView, CreationError> {
        let row: Option<BindingRow> = sqlx::query_as(BINDING_SELECT_SQL)
            .bind(binding_id)
            .fetch_optional(&mut **self.dispatch_tx().inner())
            .await
            .map_err(db)?;
        let Some((
            id,
            workspace_id,
            project_id,
            scheduler_id,
            outcome_mode,
            extra_context,
            actor_id,
            pod_id,
        )) = row
        else {
            return Err(CreationError::MissingRow(format!(
                "no binding {binding_id}"
            )));
        };
        Ok(BindingView {
            id,
            workspace_id,
            project_id,
            scheduler_id,
            outcome_mode,
            extra_context,
            actor_id,
            pod_id,
        })
    }

    async fn scheduler_override_pod(
        &mut self,
        pod_id: Uuid,
        project_id: Option<Uuid>,
    ) -> Result<Option<PodView>, CreationError> {
        // A `None` project binds NULL, which never matches — mirroring
        // `filter(project_id=None)`, exactly as the const documents.
        let row: Option<(Uuid, Uuid)> = sqlx::query_as(SCHEDULER_POD_OVERRIDE_SQL)
            .bind(pod_id)
            .bind(project_id)
            .fetch_optional(&mut **self.dispatch_tx().inner())
            .await
            .map_err(db)?;
        Ok(row.map(|(id, project_id)| PodView { id, project_id }))
    }

    async fn workspace(&mut self, workspace_id: Uuid) -> Result<WorkspaceView, CreationError> {
        let row: Option<(String, String)> = sqlx::query_as(BUNDLE_WORKSPACE_SQL)
            .bind(workspace_id)
            .fetch_optional(&mut **self.dispatch_tx().inner())
            .await
            .map_err(db)?;
        let Some((slug, name)) = row else {
            return Err(CreationError::MissingRow(format!(
                "no workspace {workspace_id}"
            )));
        };
        Ok(WorkspaceView { slug, name })
    }

    async fn scheduler_row(&mut self, scheduler_id: Uuid) -> Result<SchedulerView, CreationError> {
        let row: Option<(Uuid, String, String, String, String)> =
            sqlx::query_as(SCHEDULER_SELECT_SQL)
                .bind(scheduler_id)
                .fetch_optional(&mut **self.dispatch_tx().inner())
                .await
                .map_err(db)?;
        let Some((id, slug, name, description, prompt)) = row else {
            return Err(CreationError::MissingRow(format!(
                "no scheduler {scheduler_id}"
            )));
        };
        Ok(SchedulerView {
            id,
            slug,
            name,
            description,
            prompt,
        })
    }

    async fn scheduler_override_rows(
        &mut self,
        workspace_id: Uuid,
    ) -> Result<Vec<composer::OverrideRow>, CreationError> {
        // `$2 = NULL`: workspace rows only — scheduler runs are
        // always automatic.
        bundle_overrides(self.dispatch_tx(), workspace_id, None).await
    }

    async fn project_role_facts(
        &mut self,
        user_id: Uuid,
        workspace_slug: &str,
        project_id: Uuid,
    ) -> Result<ProjectRoleFacts, CreationError> {
        let tx = self.dispatch_tx().inner();
        let has_allowed_role = fetch_project_has_allowed_role(
            &mut **tx,
            Some(user_id),
            workspace_slug,
            project_id,
            SCHEDULER_ALLOWED_ROLES,
        )
        .await
        .map_err(db)?;
        let tx = self.dispatch_tx().inner();
        let is_project_member =
            fetch_project_is_member(&mut **tx, Some(user_id), workspace_slug, project_id)
                .await
                .map_err(db)?;
        let tx = self.dispatch_tx().inner();
        let is_workspace_admin =
            fetch_workspace_admin_by_slug(&mut **tx, Some(user_id), workspace_slug)
                .await
                .map_err(db)?;
        Ok(ProjectRoleFacts {
            authenticated: true,
            has_allowed_role,
            is_project_member,
            is_workspace_admin,
        })
    }

    async fn has_usable_llm_config(&mut self, user_id: Uuid) -> Result<bool, CreationError> {
        Ok(self.dispatch_has_usable_llm_config(user_id))
    }

    async fn agent_system_user(
        &mut self,
    ) -> Result<
        Result<Uuid, pidash_db::orchestration::workpad::AgentUserCollisionError>,
        CreationError,
    > {
        match get_agent_system_user(self.dispatch_tx().inner(), Utc::now()).await {
            Ok((user, _)) => Ok(Ok(user.id)),
            Err(GetAgentUserError::Collision(collision)) => Ok(Err(collision)),
            Err(GetAgentUserError::Db(error)) => Err(db(error)),
        }
    }

    async fn insert_scheduler_run(
        &mut self,
        row: &NewSchedulerRun,
    ) -> Result<RunView, CreationError> {
        // `$1..$10` in [`NewSchedulerRun`] order: id, workspace_id,
        // created_by_id, pod_id, pinned_runner_id, binding_id,
        // executor_kind, error_code, tool_plan, now.
        let inserted: PgRow = sqlx::query(&scheduler_run_insert_returning_sql())
            .bind(row.id)
            .bind(row.workspace_id)
            .bind(row.created_by_id)
            .bind(row.pod_id)
            .bind(row.pinned_runner_id)
            .bind(row.binding_id)
            .bind(row.executor_kind.value())
            .bind(row.error_code.as_str())
            .bind(&row.tool_plan)
            .bind(row.now)
            .fetch_one(&mut **self.dispatch_tx().inner())
            .await
            .map_err(db)?;
        map_run_view(&inserted).map_err(db)
    }

    fn compose_scheduler_turn(
        &mut self,
        ctx: &Value,
        index: &composer::OverrideIndex,
        workspace_id: Option<&str>,
        executor_kind: Option<&str>,
        tool_catalog_version: i64,
    ) -> Result<RenderedTurn, String> {
        // The live store delegates to the merged composer and maps
        // the manifest (`to_json`, or `Null` when absent).
        match context::build_scheduler_turn(
            ctx,
            index,
            workspace_id,
            executor_kind,
            tool_catalog_version,
        ) {
            Ok(turn) => Ok(RenderedTurn {
                text: turn.text,
                manifest: turn
                    .manifest
                    .map(|manifest| manifest.to_json())
                    .unwrap_or(Value::Null),
            }),
            Err(error) => Err(error.message().to_owned()),
        }
    }
}

// ---------------------------------------------------------------------------
// Jobs transaction driver
// ---------------------------------------------------------------------------

/// Map an entries failure onto the jobs error (message-preserving:
/// every variant stringifies without loss).
fn map_entries(error: EntriesError) -> DispatchJobsError {
    match error {
        EntriesError::Store(store) => DispatchJobsError::Store(DispatchError::from(store)),
        EntriesError::Clock(clock) => {
            DispatchJobsError::Store(DispatchError::Db(clock.to_string()))
        }
        EntriesError::AgentUserCollision(collision) => {
            DispatchJobsError::Store(DispatchError::Db(collision.to_string()))
        }
    }
}

/// Open the insertion transaction, run the services
/// `dispatch_scheduler_run`, roll back when the outcome says so,
/// commit, then drain post-commit dispatch. The `dispatch_driver!`
/// discipline, written out because the services call takes
/// `(seam, req)` instead of the continuation argument shape.
pub async fn dispatch_scheduler_run<C, H, P>(
    pool: &PgPool,
    deps: StoreDeps<C, H, P>,
    binding_id: Uuid,
    now: DateTime<Utc>,
) -> Result<DispatchOutcome<SchedulerOutcome>, DispatchJobsError>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    let cloud = deps.cloud.clone();
    let tx = Transaction::begin(pool).await?;
    let mut store = LiveCreationStore::new(tx, pool, deps);
    let outcome =
        services_dispatch_scheduler_run(&mut store, &SchedulerRequest { binding_id, now })
            .await
            .map_err(map_entries)?;
    if store.dispatch_rollback_requested() {
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await?;
        return Ok(DispatchOutcome {
            outcome,
            dispatched: Vec::new(),
            dispatch_decisions: Vec::new(),
            terminal_effects: Vec::new(),
        });
    }
    if let Some(run_id) = outcome.run {
        tracing::info!(%run_id, %binding_id, "scheduler.dispatch: created run");
    }
    let (tx, pool, cache, dispatches, deferred, terminal_effects) = store.into_parts();
    tx.commit().await?;
    let dispatch_decisions =
        drain_dispatch(pool, &cloud, &cache, dispatches, deferred, now).await?;
    let dispatched = dispatch_decisions.iter().map(|(id, _)| *id).collect();
    Ok(DispatchOutcome {
        outcome,
        dispatched,
        dispatch_decisions,
        terminal_effects,
    })
}

// ---------------------------------------------------------------------------
// Live SchedulerDispatch
// ---------------------------------------------------------------------------

/// The produced run's status line for the `(run, fail_reason)`
/// contract: `status` + `error` off the committed row.
const RUN_STATUS_SQL: &str = r#"SELECT status, error FROM agent_run WHERE id = $1"#;

/// Map a services outcome onto the `(run, fail_reason)` contract
/// (`service.py:848`): a produced run (even FAILED) carries
/// `fail_reason: None`; only the short-circuits return `(None,
/// reason)`. A produced run whose row then misses is `Err` (task
/// failure — never a fabricated status). Pure so the offline tests
/// pin it.
fn to_dispatch_result(
    outcome: &SchedulerOutcome,
    fetched: Option<(String, String)>,
) -> Result<SchedulerDispatchResult, String> {
    match (outcome.run, outcome.error.clone(), fetched) {
        (Some(id), _, Some((status, error))) => Ok(SchedulerDispatchResult {
            run: Some(DispatchRun { id, status, error }),
            fail_reason: None,
        }),
        (Some(_), _, None) => Err("run row missing after dispatch".to_owned()),
        (None, error, _) => Ok(SchedulerDispatchResult {
            run: None,
            fail_reason: error,
        }),
    }
}

/// Build the live [`SchedulerDispatch`]: each call prefetches
/// presence for the binding's creator chain (actor, project lead,
/// default assignee), consults the installed assistant seam for the
/// llm-gate verdicts, runs the jobs driver, and maps the outcome. A
/// transport-level failure outside the `(run, fail_reason)` contract
/// is `Err` (task failure).
pub fn live_scheduler_dispatch(
    pool: PgPool,
    cloud: CloudAgentSettings,
    managed: ManagedRunnerSettings,
    toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    assistant_seam: Arc<dyn AssistantModelSeam>,
) -> SchedulerDispatch {
    Arc::new(move |binding: BindingForDispatch| {
        let pool = pool.clone();
        let cloud = cloud.clone();
        let managed = managed.clone();
        let toolsets = toolsets.clone();
        let assistant_seam = assistant_seam.clone();
        Box::pin(async move {
            let now = Utc::now();
            let (lead, default) = match binding.project_id {
                Some(project_id) => project_leads(&pool, project_id)
                    .await
                    .map_err(|error| error.to_string())?,
                None => (None, None),
            };
            let universe = order_universe(binding.actor_id, None, lead, default, &[]);
            let presence = Arc::new(
                fetch_presence(&pool, &universe)
                    .await
                    .map_err(|error| error.to_string())?,
            );
            let verdicts = Arc::new(gate_verdicts(&assistant_seam, &presence).await);
            let deps = StoreDeps {
                cloud,
                managed,
                cache: RedisAdmissionCache::from_env(),
                has_usable_llm_config: gate_fn(verdicts),
                toolsets,
                llm_profile: profile_fn(presence),
                now_unix_secs: now.timestamp(),
            };
            let outcome = dispatch_scheduler_run(&pool, deps, binding.binding_id, now)
                .await
                .map_err(|error| error.to_string())?;
            let fetched = match outcome.outcome.run {
                Some(id) => {
                    let row: Option<(String, String)> = sqlx::query_as(RUN_STATUS_SQL)
                        .bind(id)
                        .fetch_optional(&pool)
                        .await
                        .map_err(|error| error.to_string())?;
                    row
                }
                None => None,
            };
            to_dispatch_result(&outcome.outcome, fetched)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::orchestration::entries::SCHEDULER_RUN_INSERT_SQL;

    #[test]
    fn dispatch_result_mapping_pins_contract() {
        let id = Uuid::from_u128(42);
        // Produced run: fail_reason unset, status line carried.
        let ok = to_dispatch_result(
            &SchedulerOutcome {
                run: Some(id),
                error: None,
            },
            Some(("queued".to_owned(), String::new())),
        )
        .expect("produced run maps");
        assert_eq!(
            ok.run,
            Some(DispatchRun {
                id,
                status: "queued".to_owned(),
                error: String::new(),
            })
        );
        assert_eq!(ok.fail_reason, None);
        // A FAILED run still answers (run, None) — the binding pins
        // last_run and writes no last_error.
        let failed = to_dispatch_result(
            &SchedulerOutcome {
                run: Some(id),
                error: None,
            },
            Some(("failed".to_owned(), "prompt build failed: boom".to_owned())),
        )
        .expect("failed run maps");
        assert_eq!(failed.fail_reason, None);
        assert_eq!(
            failed.run.as_ref().map(|run| run.status.as_str()),
            Some("failed")
        );
        // Short-circuit: (None, reason).
        let declined = to_dispatch_result(
            &SchedulerOutcome {
                run: None,
                error: Some("no-creator".to_owned()),
            },
            None,
        )
        .expect("short-circuit maps");
        assert_eq!(declined.run, None);
        assert_eq!(declined.fail_reason.as_deref(), Some("no-creator"));
        // Produced run with a missing row: task failure, never a
        // fabricated status.
        assert!(to_dispatch_result(
            &SchedulerOutcome {
                run: Some(id),
                error: None,
            },
            None,
        )
        .is_err());
    }

    #[test]
    fn scheduler_insert_binds_match_new_scheduler_run_order() {
        // The impl binds $1..$10 in NewSchedulerRun field order; the
        // statement must carry exactly those placeholders.
        for n in 1..=10 {
            assert!(
                SCHEDULER_RUN_INSERT_SQL.contains(&format!("${n}")),
                "missing ${n}"
            );
        }
        assert!(!SCHEDULER_RUN_INSERT_SQL.contains("$11"));
    }

    #[test]
    fn run_status_sql_parses() {
        let parsed = sqlparser::parser::Parser::parse_sql(
            &sqlparser::dialect::PostgreSqlDialect {},
            RUN_STATUS_SQL,
        )
        .expect("SQL parses");
        assert_eq!(parsed.len(), 1);
    }
}
