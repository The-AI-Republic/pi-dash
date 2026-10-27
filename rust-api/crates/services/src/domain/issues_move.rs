//! Move a work item to another project: the domain entry point.
//!
//! [`move_to_project`] runs the shared [`move_work_item_to_project`]
//! kernel ([`crate::app_issues::issue_move`], `utils/issue_move.py:122-393`)
//! on the [`WriteTxn`]'s SeaORM transaction — guards, span and refetch
//! all on the one transaction — and queues the post-commit chain (the
//! handoff-creation drain, the cancel send, the pod drains, the
//! completion comment, the transition fire, the two activity enqueues)
//! onto the [`WriteTxn`] queue, in today's driver order. Both view
//! drivers (app + v1) call it with [`MoveOptions::django`]; the options
//! exist for callers the two views do not serve (downstream overlays).
//!
//! [`move_work_item_to_project`]: crate::app_issues::issue_move::move_work_item_to_project

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend, DbErr, QueryResult, Statement};
use serde_json::Value;
use uuid::Uuid;

use crate::app_issues::issue_move::{
    move_work_item_to_project, repoint_sql, HandoffRunRow, MoveError, MoveOptions, MovePostCommit,
    MoveResult, MoveStore, MovedIssueRow, PodRow, ProjectRow, RepointTarget, SourceIssueRow,
    StateRow, StoreError, ASSIGNEE_IDS_SQL, ASSIGNEE_PRUNE_SQL, ASSIGNEE_REPOINT_SQL,
    CHILDREN_DETACH_SQL, COMMENT_IDS_SQL, CYCLE_DELETE_SQL, DESCRIPTION_IDS_SQL,
    HANDOFF_PARENT_UPDATE_SQL, HANDOFF_RUNS_LOCK_SQL, INERT_RUN_UPDATE_SQL, ISSUE_MOVE_UPDATE_SQL,
    ISSUE_MOVE_UPDATE_STAMP_SQL, LABEL_DELETE_SQL, LABEL_IDS_SQL, MEMBER_EXISTS_SQL,
    MODULE_DELETE_SQL, MOVED_ISSUE_REFETCH_SQL, PROJECT_IDENTIFIER_SQL,
    PROJECT_RESOLVE_BY_IDENTIFIER_SQL, PROJECT_RESOLVE_BY_PK_SQL, RELATION_DELETE_SQL,
    SEQUENCE_CREATE_SQL, SEQUENCE_DETACH_SQL, SOURCE_ISSUE_LOCK_SQL, SOURCE_ISSUE_SQL,
    WORKSPACE_SLUG_SQL,
};
use crate::dispatch::tools::{
    CREATE_ADVISORY_LOCK_SQL, CREATE_SAVE_DEFAULT_STATE_SQL, CREATE_SAVE_FALLBACK_STATE_SQL,
    CREATE_SEQ_SCAN_SQL,
};
use crate::dispatch::DeferredConsume;
use crate::orchestration::blockers::{has_open_blockers_sql, summary_sql, BlockerRow};
use pidash_db::app_project::models::project::ProjectLookup;
use pidash_db::runner_enroll::columns::pod::DEFAULT_FOR_PROJECT_ID_SQL;

use super::issues::{
    enqueue_hook, run_chain, transition_hook, ChainEffect, ChainOutcome, FireSpec,
};
use super::txn::{TxnConn, WriteTxn};
use super::{issues_move_creation, DomainError, WriteCtx};

/// The handoff-creation seam's post-commit outboxes, minus the admission
/// cache: dispatch ids (collected by `dispatch_after_commit`), deferred
/// admission consumes, and finalized-run ids awaiting their terminal
/// publish pairs. The api-side [`MovePostCommitHook`] implementation
/// supplies its own cache clone when it drains these through the
/// existing `drain_creation_outboxes`.
#[derive(Debug, Clone, Default)]
pub struct MoveCreationOutboxes {
    pub dispatches: Vec<Uuid>,
    pub deferred: Vec<DeferredConsume>,
    pub terminal_effects: Vec<Uuid>,
}

/// Why a post-commit drain leg failed. The hook implementation logs the
/// specific line (preserving today's driver warnings) and returns this
/// as the leg's failure; the chain records the stop and the driver
/// answers `ServerError` with the move standing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveDrainError(pub String);

impl std::fmt::Display for MoveDrainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for MoveDrainError {}

/// The move's post-commit drains as an object-safe hook (the
/// [`TaskEnqueueHook`](super::activity::TaskEnqueueHook) precedent:
/// boxed futures, no generics). The drains need the binary's pool,
/// pubsub and runner settings, so the api binary implements this over
/// its existing drain functions, the driver registers it on the
/// [`HookRegistry`](super::HookRegistry), and the move chain calls it
/// from its after-commit step.
pub trait MovePostCommitHook: Send + Sync {
    /// Drain the handoff-creation outboxes (the driver's
    /// `take_creation_outboxes` block): terminal publishes (best-effort),
    /// then one `dispatch_agent_run` per id with its pod drain.
    /// Fallible — [`ChainStop::CreationDrain`].
    ///
    /// [`ChainStop::CreationDrain`]: super::issues::ChainStop::CreationDrain
    fn drain_creation(
        &self,
        outboxes: MoveCreationOutboxes,
        terminal_created_by: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = Result<(), MoveDrainError>> + Send + '_>>;

    /// Drain one source pod (`MovePostCommit::DrainPod`). Fallible —
    /// [`ChainStop::PodDrain`].
    ///
    /// [`ChainStop::PodDrain`]: super::issues::ChainStop::PodDrain
    fn drain_pod(
        &self,
        pod_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), MoveDrainError>> + Send + '_>>;

    /// Send one cancel frame (`MovePostCommit::SendCancel`): best-effort
    /// with logged warnings, never fails (Django's
    /// `_send_project_move_cancel`, `:70-107`, never raises).
    fn send_cancel(
        &self,
        runner_id: Uuid,
        run_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// The hook the driver registered, for the move chain legs.
pub(crate) fn drain_hook(txn: &super::txn::WriteTxn) -> Arc<dyn MovePostCommitHook> {
    txn.hooks()
        .get::<dyn MovePostCommitHook>()
        .expect("driver registered MovePostCommitHook")
}

/// The admission cache as an object-safe hook: the two
/// [`AdmissionCache`](crate::dispatch::AdmissionCache) reads the
/// execution resolve needs, with string errors. The binary implements
/// this over its own cache and the creation seam adapts it back.
pub trait MoveAdmissionCache: Send + Sync {
    fn bucket_count(&self, key: &str) -> Result<Option<i64>, String>;
    fn add_or_incr(&self, key: &str, timeout_secs: i64) -> Result<(), String>;
}

/// The handoff-creation runtime facts: executor settings, the pool for
/// the resolve's short transaction plus the BYOK probe, the unix clock,
/// and the toolset/assistant/admission seams. All types are
/// services-reachable; the driver builds this from its request state.
pub struct CreationDeps {
    pub pool: sqlx::PgPool,
    pub cloud: pidash_db::config::CloudAgentSettings,
    pub managed: pidash_db::config::ManagedRunnerSettings,
    pub now_unix_secs: i64,
    pub toolsets: Arc<dyn crate::extensions::CloudAgentToolsetsSeam>,
    pub assistant_seam: Arc<dyn crate::extensions::AssistantModelSeam>,
    pub admission: Arc<dyn MoveAdmissionCache>,
}

/// The per-call inputs: the URL/body facts, the driver's resolved
/// origin + web base (each driver keeps its own computation, including
/// the app empty-string divergence), the frozen clock, the pre-move
/// state snapshot, the caller options, and the creation facts.
pub struct MoveInput<'a> {
    pub slug: &'a str,
    pub project_id: Uuid,
    pub pk: Uuid,
    pub target_ref: &'a Value,
    pub origin: String,
    pub web_base_url: Option<&'a str>,
    pub now: DateTime<Utc>,
    pub prev_state: Option<Uuid>,
    pub options: MoveOptions,
    pub creation: CreationDeps,
}

/// A recoverable move outcome: the kernel's guard failures as values
/// (the span wrote nothing, so the transaction commits empty). The
/// drivers map these onto today's denials byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveRejection {
    /// A 400/403/409 with its exact message.
    Issue { message: String, status_code: u16 },
    /// The source issue is not visible — the standard DRF 404 detail.
    SourceNotFound,
    /// The target project does not resolve — `"Project not found"`.
    TargetNotFound,
    /// No source membership under [`MoveGuards::Dual`]; the caller maps
    /// it to its own 403 shape.
    ///
    /// [`MoveGuards::Dual`]: crate::app_issues::issue_move::MoveGuards::Dual
    SourceDenied,
}

/// What [`move_to_project`] returns: the already-there row (plus the
/// representation pair the v1 render needs), the moved row plus its
/// post-commit chain outcome, or a recoverable rejection.
pub enum MoveDomainOutcome {
    AlreadyThere {
        row: SourceIssueRow,
        workspace_slug: Option<String>,
        project_identifier: Option<String>,
    },
    Moved {
        row: MovedIssueRow,
        chain: ChainOutcome,
    },
    Rejected(MoveRejection),
}

fn store_error(error: DbErr) -> StoreError {
    StoreError(error.to_string())
}

/// Run raw SQL with bound values on the transaction, returning at most
/// one row.
async fn query_one(
    conn: &TxnConn<'_>,
    sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Option<QueryResult>, StoreError> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Postgres,
        sql,
        values,
    ))
    .await
    .map_err(store_error)
}

/// Run raw SQL with bound values on the transaction, returning every row.
async fn query_all(
    conn: &TxnConn<'_>,
    sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Vec<QueryResult>, StoreError> {
    conn.query_all(Statement::from_sql_and_values(
        DbBackend::Postgres,
        sql,
        values,
    ))
    .await
    .map_err(store_error)
}

/// Run a write on the transaction, returning the affected count.
async fn execute(
    conn: &TxnConn<'_>,
    sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<u64, StoreError> {
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        sql,
        values,
    ))
    .await
    .map(|done| done.rows_affected())
    .map_err(store_error)
}

/// Rebind every `:issue_id` placeholder to `$1` (the blockers builders
/// emit the named form, possibly more than once).
fn issue_param(sql: String) -> String {
    sql.replace(":issue_id", "$1")
}

/// Map a full source-issue row: int4 widen to i64, the uuid `type_id`
/// to its hyphenated text, the naive `archived_at` to a midnight UTC
/// instant.
fn map_source_row(row: &QueryResult) -> Result<SourceIssueRow, StoreError> {
    let point: Option<i32> = row.try_get("", "point").map_err(store_error)?;
    let complexity: i32 = row.try_get("", "complexity_score").map_err(store_error)?;
    let sequence: i32 = row.try_get("", "sequence_id").map_err(store_error)?;
    let type_id: Option<Uuid> = row.try_get("", "type_id").map_err(store_error)?;
    let archived_at: Option<chrono::NaiveDate> =
        row.try_get("", "archived_at").map_err(store_error)?;
    Ok(SourceIssueRow {
        id: row.try_get("", "id").map_err(store_error)?,
        project_id: row.try_get("", "project_id").map_err(store_error)?,
        workspace_id: row.try_get("", "workspace_id").map_err(store_error)?,
        state_id: row.try_get("", "state_id").map_err(store_error)?,
        parent_id: row.try_get("", "parent_id").map_err(store_error)?,
        estimate_point_id: row.try_get("", "estimate_point_id").map_err(store_error)?,
        assigned_pod_id: row.try_get("", "assigned_pod_id").map_err(store_error)?,
        type_id: type_id.map(|id| id.to_string()),
        created_at: row.try_get("", "created_at").map_err(store_error)?,
        updated_at: row.try_get("", "updated_at").map_err(store_error)?,
        deleted_at: row.try_get("", "deleted_at").map_err(store_error)?,
        point: point.map(i64::from),
        name: row.try_get("", "name").map_err(store_error)?,
        description_html: row.try_get("", "description_html").map_err(store_error)?,
        description_binary: row.try_get("", "description_binary").map_err(store_error)?,
        priority: row.try_get("", "priority").map_err(store_error)?,
        complexity_score: i64::from(complexity),
        start_date: row.try_get("", "start_date").map_err(store_error)?,
        target_date: row.try_get("", "target_date").map_err(store_error)?,
        sequence_id: i64::from(sequence),
        sort_order: row.try_get("", "sort_order").map_err(store_error)?,
        completed_at: row.try_get("", "completed_at").map_err(store_error)?,
        archived_at: archived_at
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .map(|naive| naive.and_utc()),
        is_draft: row.try_get("", "is_draft").map_err(store_error)?,
        external_source: row.try_get("", "external_source").map_err(store_error)?,
        external_id: row.try_get("", "external_id").map_err(store_error)?,
        git_work_branch: row.try_get("", "git_work_branch").map_err(store_error)?,
        created_via: row.try_get("", "created_via").map_err(store_error)?,
        agent_executor: row.try_get("", "agent_executor").map_err(store_error)?,
        created_by_id: row.try_get("", "created_by_id").map_err(store_error)?,
        updated_by_id: row.try_get("", "updated_by_id").map_err(store_error)?,
    })
}

/// Map one locked handoff run: the trigger rides raw (the varchar is
/// never parsed), a null `run_config` is `{}`.
fn map_handoff_row(row: &QueryResult) -> Result<HandoffRunRow, StoreError> {
    let status: String = row.try_get("", "status").map_err(store_error)?;
    let run_config: Option<Value> = row.try_get("", "run_config").map_err(store_error)?;
    Ok(HandoffRunRow {
        id: row.try_get("", "id").map_err(store_error)?,
        status: pidash_db::dispatch::AgentRunStatus::from_value(&status).ok_or_else(|| {
            StoreError(format!("move store: unknown agent_run.status {status:?}"))
        })?,
        runner_id: row.try_get("", "runner_id").map_err(store_error)?,
        pod_id: row.try_get("", "pod_id").map_err(store_error)?,
        created_by_id: row.try_get("", "created_by_id").map_err(store_error)?,
        run_config: run_config.unwrap_or(Value::Null),
        trigger: row.try_get("", "trigger").map_err(store_error)?,
    })
}

/// Map one `blockers::summary_sql` row.
fn map_blocker_row(row: &QueryResult) -> Result<BlockerRow, StoreError> {
    let (issue_id, sequence_id, project_identifier, state_name, state_group): (
        Uuid,
        i32,
        String,
        Option<String>,
        Option<String>,
    ) = row.try_get_many_by_index().map_err(store_error)?;
    Ok(BlockerRow {
        issue_id,
        sequence_id,
        project_identifier,
        state_name,
        state_group,
    })
}

/// The single [`MoveStore`] implementation, on the [`WriteTxn`]'s SeaORM
/// transaction: every method — guards, span and refetch — runs on the
/// one transaction (under `READ COMMITTED` each statement still sees the
/// latest committed data, as the pool checkouts did). There is no span
/// to open or commit; [`domain::run`][crate::domain::run] owns it. The
/// handoff-creation outboxes collect in a slot the move drains into the
/// chain.
pub struct SeaMoveStore<'t> {
    conn: TxnConn<'t>,
    creation: CreationDeps,
    outboxes: Mutex<Option<MoveCreationOutboxes>>,
}

impl<'t> SeaMoveStore<'t> {
    pub(crate) fn new(conn: TxnConn<'t>, creation: CreationDeps) -> Self {
        Self {
            conn,
            creation,
            outboxes: Mutex::new(None),
        }
    }

    fn take_outboxes(&self) -> Option<MoveCreationOutboxes> {
        self.outboxes.lock().expect("outbox slot").take()
    }
}

#[allow(async_fn_in_trait)]
impl MoveStore for SeaMoveStore<'_> {
    async fn source_issue(
        &self,
        slug: &str,
        project_id: Uuid,
        pk: Uuid,
    ) -> Result<Option<SourceIssueRow>, StoreError> {
        let row = query_one(
            &self.conn,
            SOURCE_ISSUE_SQL,
            vec![
                sea_orm::Value::from(slug),
                sea_orm::Value::from(project_id),
                sea_orm::Value::from(pk),
            ],
        )
        .await?;
        row.map(|row| map_source_row(&row)).transpose()
    }

    async fn resolve_project(
        &self,
        slug: &str,
        lookup: &ProjectLookup,
    ) -> Result<Option<ProjectRow>, StoreError> {
        let row: Option<(Uuid, Uuid, String)> = match lookup {
            ProjectLookup::Pk(id) => query_one(
                &self.conn,
                PROJECT_RESOLVE_BY_PK_SQL,
                vec![sea_orm::Value::from(*id), sea_orm::Value::from(slug)],
            )
            .await?
            .map(|row| row.try_get_many_by_index().map_err(store_error))
            .transpose()?,
            ProjectLookup::Identifier(identifier) => query_one(
                &self.conn,
                PROJECT_RESOLVE_BY_IDENTIFIER_SQL,
                vec![
                    sea_orm::Value::from(identifier.as_str()),
                    sea_orm::Value::from(slug),
                ],
            )
            .await?
            .map(|row| row.try_get_many_by_index().map_err(store_error))
            .transpose()?,
        };
        Ok(row.map(|(id, workspace_id, identifier)| ProjectRow {
            id,
            workspace_id,
            identifier,
        }))
    }

    async fn can_move_into_target(
        &self,
        slug: &str,
        target_project_id: Uuid,
        actor_id: Uuid,
    ) -> Result<bool, StoreError> {
        let hit = query_one(
            &self.conn,
            MEMBER_EXISTS_SQL,
            vec![
                sea_orm::Value::from(slug),
                sea_orm::Value::from(target_project_id),
                sea_orm::Value::from(actor_id),
            ],
        )
        .await?;
        Ok(hit.is_some())
    }

    async fn can_move_from_source(
        &self,
        slug: &str,
        source_project_id: Uuid,
        actor_id: Uuid,
    ) -> Result<bool, StoreError> {
        let hit = query_one(
            &self.conn,
            MEMBER_EXISTS_SQL,
            vec![
                sea_orm::Value::from(slug),
                sea_orm::Value::from(source_project_id),
                sea_orm::Value::from(actor_id),
            ],
        )
        .await?;
        Ok(hit.is_some())
    }

    async fn representation_workspace_slug(
        &self,
        workspace_id: Uuid,
    ) -> Result<Option<String>, StoreError> {
        let row = query_one(
            &self.conn,
            WORKSPACE_SLUG_SQL,
            vec![sea_orm::Value::from(workspace_id)],
        )
        .await?;
        row.map(|row| {
            let (slug,): (String,) = row.try_get_many_by_index().map_err(store_error)?;
            Ok(slug)
        })
        .transpose()
    }

    async fn representation_project_identifier(
        &self,
        project_id: Uuid,
    ) -> Result<Option<String>, StoreError> {
        let row = query_one(
            &self.conn,
            PROJECT_IDENTIFIER_SQL,
            vec![sea_orm::Value::from(project_id)],
        )
        .await?;
        row.map(|row| {
            let (identifier,): (String,) = row.try_get_many_by_index().map_err(store_error)?;
            Ok(identifier)
        })
        .transpose()
    }

    async fn representation_assignee_ids(&self, issue_id: Uuid) -> Result<Vec<Uuid>, StoreError> {
        let rows = query_all(
            &self.conn,
            ASSIGNEE_IDS_SQL,
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?;
        rows.iter()
            .map(|row| {
                let (id,): (Uuid,) = row.try_get_many_by_index().map_err(store_error)?;
                Ok(id)
            })
            .collect()
    }

    async fn representation_label_ids(&self, issue_id: Uuid) -> Result<Vec<Uuid>, StoreError> {
        let rows = query_all(
            &self.conn,
            LABEL_IDS_SQL,
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?;
        rows.iter()
            .map(|row| {
                let (id,): (Uuid,) = row.try_get_many_by_index().map_err(store_error)?;
                Ok(id)
            })
            .collect()
    }

    async fn representation_blocked_by(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<BlockerRow>, StoreError> {
        let rows = query_all(
            &self.conn,
            &issue_param(summary_sql(false)),
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?;
        rows.iter().map(map_blocker_row).collect()
    }

    async fn representation_blocking(&self, issue_id: Uuid) -> Result<Vec<BlockerRow>, StoreError> {
        let rows = query_all(
            &self.conn,
            &issue_param(summary_sql(true)),
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?;
        rows.iter().map(map_blocker_row).collect()
    }

    async fn representation_has_open_blockers(&self, issue_id: Uuid) -> Result<bool, StoreError> {
        let hit = query_one(
            &self.conn,
            &issue_param(has_open_blockers_sql()),
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?;
        Ok(hit.is_some())
    }

    async fn target_default_state(
        &self,
        target_project_id: Uuid,
    ) -> Result<Option<StateRow>, StoreError> {
        let row = query_one(
            &self.conn,
            CREATE_SAVE_DEFAULT_STATE_SQL,
            vec![sea_orm::Value::from(target_project_id)],
        )
        .await?;
        row.map(|row| {
            let (id,): (Uuid,) = row.try_get_many_by_index().map_err(store_error)?;
            Ok(StateRow { id })
        })
        .transpose()
    }

    async fn target_fallback_state(
        &self,
        target_project_id: Uuid,
    ) -> Result<Option<StateRow>, StoreError> {
        let row = query_one(
            &self.conn,
            CREATE_SAVE_FALLBACK_STATE_SQL,
            vec![sea_orm::Value::from(target_project_id)],
        )
        .await?;
        row.map(|row| {
            let (id,): (Uuid,) = row.try_get_many_by_index().map_err(store_error)?;
            Ok(StateRow { id })
        })
        .transpose()
    }

    async fn lock_source_issue(
        &self,
        slug: &str,
        project_id: Uuid,
        pk: Uuid,
    ) -> Result<Option<SourceIssueRow>, StoreError> {
        // No span to open: the domain transaction is already held.
        let row = query_one(
            &self.conn,
            SOURCE_ISSUE_LOCK_SQL,
            vec![
                sea_orm::Value::from(slug),
                sea_orm::Value::from(project_id),
                sea_orm::Value::from(pk),
            ],
        )
        .await?;
        row.map(|row| map_source_row(&row)).transpose()
    }

    async fn advisory_lock_project(&self, lock_key: i64) -> Result<(), StoreError> {
        execute(
            &self.conn,
            CREATE_ADVISORY_LOCK_SQL,
            vec![sea_orm::Value::from(lock_key)],
        )
        .await?;
        Ok(())
    }

    async fn lock_handoff_runs(&self, issue_id: Uuid) -> Result<Vec<HandoffRunRow>, StoreError> {
        let rows = query_all(
            &self.conn,
            HANDOFF_RUNS_LOCK_SQL,
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?;
        rows.iter().map(map_handoff_row).collect()
    }

    async fn max_target_sequence(
        &self,
        target_project_id: Uuid,
    ) -> Result<Option<i64>, StoreError> {
        let row = query_one(
            &self.conn,
            CREATE_SEQ_SCAN_SQL,
            vec![sea_orm::Value::from(target_project_id)],
        )
        .await?;
        row.map(|row| {
            let (max,): (Option<i64>,) = row.try_get_many_by_index().map_err(store_error)?;
            Ok(max)
        })
        .transpose()
        .map(|hit| hit.flatten())
    }

    async fn target_default_pod(
        &self,
        target_project_id: Uuid,
    ) -> Result<Option<PodRow>, StoreError> {
        let row = query_one(
            &self.conn,
            DEFAULT_FOR_PROJECT_ID_SQL,
            vec![sea_orm::Value::from(target_project_id)],
        )
        .await?;
        row.map(|row| {
            let id: Uuid = row.try_get("", "id").map_err(store_error)?;
            Ok(PodRow { id })
        })
        .transpose()
    }

    async fn save_moved_issue(
        &self,
        issue_id: Uuid,
        fields: &crate::app_issues::issue_move::MovedIssueFields,
        now: &DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let sequence = i32::try_from(fields.sequence_id)
            .map_err(|_| StoreError("move store: sequence_id overflows int4".to_owned()))?;
        let sql = if fields.updated_by_id.is_some() {
            ISSUE_MOVE_UPDATE_STAMP_SQL
        } else {
            ISSUE_MOVE_UPDATE_SQL
        };
        let mut values = vec![
            sea_orm::Value::from(fields.project_id),
            sea_orm::Value::from(fields.workspace_id),
            sea_orm::Value::from(sequence),
            sea_orm::Value::from(fields.state_id),
            sea_orm::Value::from(fields.assigned_pod_id),
            sea_orm::Value::from(None::<Uuid>),
            sea_orm::Value::from(None::<Uuid>),
            sea_orm::Value::from(None::<Uuid>),
            sea_orm::Value::from(*now),
        ];
        if let Some(updated_by) = fields.updated_by_id {
            values.push(sea_orm::Value::from(updated_by));
        }
        values.push(sea_orm::Value::from(issue_id));
        execute(&self.conn, sql, values).await?;
        Ok(())
    }

    async fn cancel_inert_run(&self, run_id: Uuid, now: &DateTime<Utc>) -> Result<(), StoreError> {
        execute(
            &self.conn,
            INERT_RUN_UPDATE_SQL,
            vec![sea_orm::Value::from(run_id), sea_orm::Value::from(*now)],
        )
        .await?;
        Ok(())
    }

    async fn mark_handoff_parent(
        &self,
        run_id: Uuid,
        run_config: &Value,
    ) -> Result<(), StoreError> {
        execute(
            &self.conn,
            HANDOFF_PARENT_UPDATE_SQL,
            vec![
                sea_orm::Value::from(run_id),
                sea_orm::Value::from(run_config.clone()),
            ],
        )
        .await?;
        Ok(())
    }

    async fn detach_old_sequences(
        &self,
        issue_id: Uuid,
        target_project_id: Uuid,
    ) -> Result<u64, StoreError> {
        execute(
            &self.conn,
            SEQUENCE_DETACH_SQL,
            vec![
                sea_orm::Value::from(issue_id),
                sea_orm::Value::from(target_project_id),
            ],
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_target_sequence(
        &self,
        sequence_id: Uuid,
        issue_id: Uuid,
        sequence: i64,
        target_project_id: Uuid,
        target_workspace_id: Uuid,
        now: &DateTime<Utc>,
        actor_id: Uuid,
    ) -> Result<(), StoreError> {
        execute(
            &self.conn,
            SEQUENCE_CREATE_SQL,
            vec![
                sea_orm::Value::from(sequence_id),
                sea_orm::Value::from(*now),
                sea_orm::Value::from(*now),
                sea_orm::Value::from(actor_id),
                sea_orm::Value::from(issue_id),
                sea_orm::Value::from(sequence),
                sea_orm::Value::from(target_project_id),
                sea_orm::Value::from(target_workspace_id),
            ],
        )
        .await?;
        Ok(())
    }

    async fn prune_assignees(
        &self,
        issue_id: Uuid,
        target_project_id: Uuid,
        now: &DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        execute(
            &self.conn,
            ASSIGNEE_PRUNE_SQL,
            vec![
                sea_orm::Value::from(*now),
                sea_orm::Value::from(issue_id),
                sea_orm::Value::from(target_project_id),
            ],
        )
        .await
    }

    async fn repoint_assignees(
        &self,
        issue_id: Uuid,
        target_project_id: Uuid,
        target_workspace_id: Uuid,
    ) -> Result<u64, StoreError> {
        execute(
            &self.conn,
            ASSIGNEE_REPOINT_SQL,
            vec![
                sea_orm::Value::from(target_project_id),
                sea_orm::Value::from(target_workspace_id),
                sea_orm::Value::from(issue_id),
            ],
        )
        .await
    }

    async fn soft_delete_issue_labels(
        &self,
        issue_id: Uuid,
        now: &DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        execute(
            &self.conn,
            LABEL_DELETE_SQL,
            vec![sea_orm::Value::from(*now), sea_orm::Value::from(issue_id)],
        )
        .await
    }

    async fn soft_delete_cycle_issues(
        &self,
        issue_id: Uuid,
        now: &DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        execute(
            &self.conn,
            CYCLE_DELETE_SQL,
            vec![sea_orm::Value::from(*now), sea_orm::Value::from(issue_id)],
        )
        .await
    }

    async fn soft_delete_module_issues(
        &self,
        issue_id: Uuid,
        now: &DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        execute(
            &self.conn,
            MODULE_DELETE_SQL,
            vec![sea_orm::Value::from(*now), sea_orm::Value::from(issue_id)],
        )
        .await
    }

    async fn soft_delete_issue_relations(
        &self,
        issue_id: Uuid,
        now: &DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        execute(
            &self.conn,
            RELATION_DELETE_SQL,
            vec![sea_orm::Value::from(*now), sea_orm::Value::from(issue_id)],
        )
        .await
    }

    async fn detach_children(
        &self,
        issue_id: Uuid,
        target_project_id: Uuid,
    ) -> Result<u64, StoreError> {
        execute(
            &self.conn,
            CHILDREN_DETACH_SQL,
            vec![
                sea_orm::Value::from(issue_id),
                sea_orm::Value::from(target_project_id),
            ],
        )
        .await
    }

    async fn moved_comment_ids(&self, issue_id: Uuid) -> Result<Vec<Uuid>, StoreError> {
        let rows = query_all(
            &self.conn,
            COMMENT_IDS_SQL,
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?;
        rows.iter()
            .map(|row| {
                let (id,): (Uuid,) = row.try_get_many_by_index().map_err(store_error)?;
                Ok(id)
            })
            .collect()
    }

    async fn moved_description_ids(&self, issue_id: Uuid) -> Result<Vec<Uuid>, StoreError> {
        let rows = query_all(
            &self.conn,
            DESCRIPTION_IDS_SQL,
            vec![sea_orm::Value::from(issue_id)],
        )
        .await?;
        rows.iter()
            .map(|row| {
                let (id,): (Uuid,) = row.try_get_many_by_index().map_err(store_error)?;
                Ok(id)
            })
            .collect()
    }

    async fn repoint_related(
        &self,
        target: RepointTarget,
        issue_id: Uuid,
        comment_ids: &[Uuid],
        description_ids: &[Uuid],
        target_project_id: Uuid,
        target_workspace_id: Uuid,
    ) -> Result<u64, StoreError> {
        let sql = repoint_sql(target);
        let mut values = vec![
            sea_orm::Value::from(target_project_id),
            sea_orm::Value::from(target_workspace_id),
            sea_orm::Value::from(issue_id),
        ];
        match target {
            RepointTarget::IssueActivity
            | RepointTarget::CommentReaction
            | RepointTarget::FileAsset => {
                values.push(sea_orm::Value::from(comment_ids.to_vec()));
            }
            RepointTarget::Description => {
                values.push(sea_orm::Value::from(description_ids.to_vec()));
            }
            _ => {}
        }
        execute(&self.conn, sql, values).await
    }

    async fn create_handoff_run(
        &self,
        issue_id: Uuid,
        parent_run: &HandoffRunRow,
        pod_id: Uuid,
        now: &DateTime<Utc>,
    ) -> Result<(), StoreError> {
        use crate::orchestration::creation::{
            create_project_move_handoff_run, CreationSeam, HandoffCreateRequest,
        };
        // No lending dance: the sister seam shares the transaction
        // handle, so the create still sees the move's uncommitted writes
        // (the moved issue, the cancelled parent, the repoints), exactly
        // like Python's in-atomic call (`issue_move.py:353-362`).
        let mut creation = issues_move_creation::SeaHandoffStore::new(self.conn, &self.creation);
        // The locked row is the parent's pre-cancel snapshot, and the
        // `RunView` projection needs the full row: re-read it in-span,
        // under the held lock. The cancel touched only
        // status/ended_at/queue_position, so this is the row Python's
        // mutated in-memory instance describes.
        let parent = creation
            .run(parent_run.id)
            .await
            .map_err(|error| StoreError(error.to_string()))?
            .ok_or_else(|| StoreError("move store: handoff parent vanished".to_owned()))?;
        let outcome = create_project_move_handoff_run(
            &mut creation,
            &HandoffCreateRequest {
                issue_id,
                parent,
                pod_id,
                now: *now,
            },
        )
        .await
        .map_err(|error| StoreError(error.to_string()))?;
        let _created = outcome;
        let (dispatches, deferred, terminal_effects) = creation.outboxes();
        self.outboxes
            .lock()
            .expect("outbox slot")
            .replace(MoveCreationOutboxes {
                dispatches,
                deferred,
                terminal_effects,
            });
        Ok(())
    }

    async fn moved_issue(&self, pk: Uuid) -> Result<Option<MovedIssueRow>, StoreError> {
        // No commit: the domain transaction commits after the chain is
        // queued. The refetch runs in-span, seeing the move's writes.
        let row = query_one(
            &self.conn,
            MOVED_ISSUE_REFETCH_SQL,
            vec![sea_orm::Value::from(pk)],
        )
        .await?;
        row.map(|row| {
            let workspace_slug: String = row.try_get("", "slug").map_err(store_error)?;
            let project_identifier: String = row.try_get("", "identifier").map_err(store_error)?;
            Ok(MovedIssueRow {
                row: map_source_row(&row)?,
                workspace_slug,
                project_identifier,
            })
        })
        .transpose()
    }
}

/// Move a work item to another project on the domain transaction.
///
/// Runs the shared kernel on [`SeaMoveStore`] (guards, span and refetch
/// all on the one SeaORM transaction) and queues the post-commit chain
/// — the handoff-creation drain, the cancel send, the pod drains, the
/// completion comment, the transition fire, the two activity enqueues —
/// in today's driver order. Recoverable guard failures return
/// [`MoveDomainOutcome::Rejected`] (the span wrote nothing); the
/// post-save 409 and storage failures return `Err` so [`run`][crate::domain::run]
/// rolls back.
pub async fn move_to_project(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    input: MoveInput<'_>,
) -> Result<MoveDomainOutcome, DomainError> {
    let actor_id = ctx.actor.user_id;
    let pool = input.creation.pool.clone();
    let store = SeaMoveStore::new(txn.connection(), input.creation);
    let outcome = move_work_item_to_project(
        &store,
        input.slug,
        input.project_id,
        input.pk,
        input.target_ref,
        actor_id,
        &input.origin,
        input.web_base_url,
        &input.now,
        &input.options,
    )
    .await;
    match outcome {
        Ok(MoveResult::AlreadyThere(row)) => {
            // The v1 render needs the pair; the app render ignores it.
            // Two extra reads on a no-op path, no writes either way.
            let workspace_slug = store
                .representation_workspace_slug(row.workspace_id)
                .await
                .map_err(|error| DomainError::Db(DbErr::Custom(error.0)))?;
            let project_identifier = store
                .representation_project_identifier(row.project_id)
                .await
                .map_err(|error| DomainError::Db(DbErr::Custom(error.0)))?;
            Ok(MoveDomainOutcome::AlreadyThere {
                row,
                workspace_slug,
                project_identifier,
            })
        }
        Ok(MoveResult::Moved(moved)) => {
            let transition = transition_hook(txn);
            let enqueue = enqueue_hook(txn);
            let drain = drain_hook(txn);
            let mut chain = Vec::new();
            if let Some(outboxes) = store.take_outboxes() {
                chain.push(ChainEffect::CreationDrain {
                    outboxes,
                    terminal_created_by: Some(actor_id),
                });
            }
            for action in moved.post_commit {
                match action {
                    MovePostCommit::SendCancel { runner_id, run_id } => {
                        chain.push(ChainEffect::SendCancel { runner_id, run_id });
                    }
                    MovePostCommit::DrainPod { pod_id } => {
                        chain.push(ChainEffect::DrainPod { pod_id });
                    }
                }
            }
            let current_state = moved.issue.row.state_id;
            chain.push(ChainEffect::CompletionIfChanged {
                issue_id: input.pk,
                prev_state: input.prev_state,
                current_state,
            });
            chain.push(ChainEffect::FireTransition {
                issue_id: input.pk,
                prev_state: input.prev_state,
                current_state,
                actor_id,
                spec: FireSpec {
                    dispatch_immediate: moved.dispatch_immediate,
                    created_by: Some(actor_id),
                    moved_by_run: None,
                    now: input.now,
                },
            });
            chain.push(ChainEffect::MoveIssueActivity {
                event: "issue.activity.updated".to_owned(),
                requested: moved.requested_data,
                actor: actor_id,
                issue: input.pk,
                project: moved.issue.row.project_id,
                current: moved.current_instance.clone(),
                epoch: input.now.timestamp(),
            });
            chain.push(ChainEffect::MoveModelActivity {
                model: input.pk,
                requested: serde_json::json!({"project": moved.issue.row.project_id}),
                current: moved.current_instance,
                actor: actor_id,
                slug: input.slug.to_owned(),
                origin: input.origin.clone(),
            });
            let outcome = ChainOutcome::new();
            let step = outcome.clone();
            txn.on_commit(move || async move {
                run_chain(&pool, &transition, &enqueue, Some(&drain), chain, &step).await;
            });
            Ok(MoveDomainOutcome::Moved {
                row: moved.issue,
                chain: outcome,
            })
        }
        Err(MoveError::Issue(issue)) => Ok(MoveDomainOutcome::Rejected(MoveRejection::Issue {
            message: issue.message,
            status_code: issue.status_code,
        })),
        Err(MoveError::IssuePostWrite(issue)) => {
            // The only post-save recoverable arm is the multiple-runs
            // 409 (kernel `:264-267`, past the `:223` save): roll back
            // and let the driver answer 409 with the exact message.
            debug_assert_eq!(issue.status_code, 409);
            Err(DomainError::Work(issue.message))
        }
        Err(MoveError::IssueNotFound) => {
            Ok(MoveDomainOutcome::Rejected(MoveRejection::SourceNotFound))
        }
        Err(MoveError::ProjectNotFound) => {
            Ok(MoveDomainOutcome::Rejected(MoveRejection::TargetNotFound))
        }
        Err(MoveError::SourceDenied) => {
            Ok(MoveDomainOutcome::Rejected(MoveRejection::SourceDenied))
        }
        Err(MoveError::Store(error)) => Err(DomainError::Db(DbErr::Custom(error.0))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_issues::issue_move as move_kernel;
    use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
    use std::sync::Mutex;

    use super::super::activity::TaskEnqueueHook;
    use super::super::ctx::WriteActor;
    use super::super::issues::ChainStop;
    use super::super::transition::IssueTransitionHook;
    use crate::orchestration::entries::{FireOutcome, FireRequest};

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn lazy_pool() -> sqlx::PgPool {
        sqlx::PgPool::connect_lazy("postgres://127.0.0.1:1/unused").expect("lazy pool")
    }

    async fn live_pool() -> Option<sqlx::PgPool> {
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return None;
            }
        };
        Some(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(2)
                .connect(&url)
                .await
                .expect("connect to scratch DATABASE_URL"),
        )
    }

    type CommentCall = (Uuid, Option<Uuid>, Option<Uuid>);

    struct RecordingTransition {
        comments: Mutex<Vec<CommentCall>>,
        fires: Mutex<Vec<FireRequest>>,
        fire_result: Mutex<Result<FireOutcome, ()>>,
    }

    impl IssueTransitionHook for RecordingTransition {
        fn enqueue_completion_comment(
            &self,
            issue_id: Uuid,
            prev_state_id: Option<Uuid>,
            current_state_id: Option<Uuid>,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            self.comments
                .lock()
                .expect("lock")
                .push((issue_id, prev_state_id, current_state_id));
            Box::pin(async move {})
        }

        fn fire(
            &self,
            _actor_id: Uuid,
            request: FireRequest,
        ) -> Pin<Box<dyn Future<Output = Result<FireOutcome, ()>> + Send + '_>> {
            self.fires.lock().expect("lock").push(request);
            let result = self.fire_result.lock().expect("lock").clone();
            Box::pin(async move { result })
        }
    }

    type EnqueueCall = (String, Vec<Value>, serde_json::Map<String, Value>);

    struct RecordingEnqueue {
        calls: Mutex<Vec<EnqueueCall>>,
    }

    impl TaskEnqueueHook for RecordingEnqueue {
        fn enqueue_task(
            &self,
            task: &str,
            args: Vec<Value>,
            kwargs: serde_json::Map<String, Value>,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            self.calls
                .lock()
                .expect("lock")
                .push((task.to_owned(), args, kwargs));
            Box::pin(async move {})
        }
    }

    struct RecordingDrain {
        creations: Mutex<Vec<(MoveCreationOutboxes, Option<Uuid>)>>,
        pods: Mutex<Vec<Uuid>>,
        cancels: Mutex<Vec<(Uuid, Uuid)>>,
        creation_result: Mutex<Result<(), MoveDrainError>>,
        pod_result: Mutex<Result<(), MoveDrainError>>,
    }

    impl RecordingDrain {
        fn ok() -> Self {
            Self {
                creations: Mutex::new(Vec::new()),
                pods: Mutex::new(Vec::new()),
                cancels: Mutex::new(Vec::new()),
                creation_result: Mutex::new(Ok(())),
                pod_result: Mutex::new(Ok(())),
            }
        }
    }

    impl MovePostCommitHook for RecordingDrain {
        fn drain_creation(
            &self,
            outboxes: MoveCreationOutboxes,
            terminal_created_by: Option<Uuid>,
        ) -> Pin<Box<dyn Future<Output = Result<(), MoveDrainError>> + Send + '_>> {
            // Record only on success: the twin drains nothing observable
            // before its fallible calls, and the chain breaks on error.
            let result = self.creation_result.lock().expect("lock").clone();
            if result.is_ok() {
                self.creations
                    .lock()
                    .expect("lock")
                    .push((outboxes, terminal_created_by));
            }
            Box::pin(async move { result })
        }

        fn drain_pod(
            &self,
            pod_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), MoveDrainError>> + Send + '_>> {
            let result = self.pod_result.lock().expect("lock").clone();
            if result.is_ok() {
                self.pods.lock().expect("lock").push(pod_id);
            }
            Box::pin(async move { result })
        }

        fn send_cancel(
            &self,
            runner_id: Uuid,
            run_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            self.cancels.lock().expect("lock").push((runner_id, run_id));
            Box::pin(async move {})
        }
    }

    struct FakeAdmission;

    impl MoveAdmissionCache for FakeAdmission {
        fn bucket_count(&self, _key: &str) -> Result<Option<i64>, String> {
            Ok(None)
        }

        fn add_or_incr(&self, _key: &str, _timeout_secs: i64) -> Result<(), String> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct NoAssistant;

    impl crate::extensions::AssistantModelSeam for NoAssistant {
        fn generate_title(
            &self,
            _request: crate::extensions::TitleRequest,
        ) -> crate::extensions::SeamFuture<'_, Result<String, crate::extensions::TitleError>>
        {
            Box::pin(async move { unreachable!("move tests never generate titles") })
        }
    }

    fn test_creation_deps(pool: sqlx::PgPool) -> CreationDeps {
        CreationDeps {
            pool,
            cloud: CloudAgentSettings {
                enabled: false,
                writes_enabled: false,
                github_tools_enabled: false,
                disabled_tools: Vec::new(),
                reconcile_interval_secs: 0,
                model_request_timeout_secs: 0,
                execution_timeout_secs: 0,
                run_soft_limit_secs: 0,
                run_hard_limit_secs: 0,
                stale_grace_secs: 0,
                dispatch_lease_secs: 0,
                dispatch_backoff_secs: 0,
                dispatch_scan_interval_secs: 0,
                sweep_interval_secs: 0,
                dispatch_scan_batch: 0,
                max_queue_age_secs: 0,
                model_request_limit: 0,
                tool_call_limit: 0,
                write_call_limit: 0,
                input_token_limit: 0,
                output_token_limit: 0,
                total_token_limit: 0,
                max_output_tokens_per_request: 0,
                max_queued_per_workspace: 0,
                max_running_per_workspace: 0,
                user_creation_rate_per_minute: 0,
                workspace_creation_rate_per_minute: 0,
                tool_timeout_secs: 0,
                max_tool_result_bytes: 0,
                max_prompt_bytes: 0,
                max_final_result_bytes: 0,
                max_events: 0,
                block_private_urls: false,
            },
            managed: ManagedRunnerSettings {
                enabled: false,
                max_per_user_project: 0,
                queued_max_age_secs: 0,
                graceful_stop_secs: 0,
                sweep_interval_secs: 0,
                desktop_min_version: String::new(),
            },
            now_unix_secs: 0,
            toolsets: Arc::new(crate::extensions::NoExtraToolsets),
            assistant_seam: Arc::new(NoAssistant),
            admission: Arc::new(FakeAdmission),
        }
    }

    fn test_hooks() -> (
        Arc<RecordingTransition>,
        Arc<RecordingEnqueue>,
        Arc<RecordingDrain>,
    ) {
        (
            Arc::new(RecordingTransition {
                comments: Mutex::new(Vec::new()),
                fires: Mutex::new(Vec::new()),
                fire_result: Mutex::new(Ok(FireOutcome::NoTransition)),
            }),
            Arc::new(RecordingEnqueue {
                calls: Mutex::new(Vec::new()),
            }),
            Arc::new(RecordingDrain::ok()),
        )
    }

    #[tokio::test]
    async fn creation_drain_failure_stops_before_pod_drain() {
        let pool = lazy_pool();
        let (transition, enqueue, drain) = test_hooks();
        *drain.creation_result.lock().expect("lock") =
            Err(MoveDrainError("dispatch blew up".to_owned()));
        let erased_transition: Arc<dyn IssueTransitionHook> = transition;
        let erased_enqueue: Arc<dyn TaskEnqueueHook> = enqueue;
        let erased_drain: Arc<dyn MovePostCommitHook> = drain.clone();
        let outcome = ChainOutcome::new();
        run_chain(
            &pool,
            &erased_transition,
            &erased_enqueue,
            Some(&erased_drain),
            vec![
                ChainEffect::CreationDrain {
                    outboxes: MoveCreationOutboxes::default(),
                    terminal_created_by: Some(uid(1)),
                },
                ChainEffect::DrainPod { pod_id: uid(2) },
            ],
            &outcome,
        )
        .await;
        assert_eq!(outcome.stopped(), Some(ChainStop::CreationDrain));
        assert!(drain.pods.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn pod_drain_failure_stops_before_fire() {
        let pool = lazy_pool();
        let (transition, enqueue, drain) = test_hooks();
        *drain.pod_result.lock().expect("lock") =
            Err(MoveDrainError("pod drain blew up".to_owned()));
        // The fire would fail too, proving the stop (not the fire) won.
        *transition.fire_result.lock().expect("lock") = Err(());
        let erased_transition: Arc<dyn IssueTransitionHook> = transition.clone();
        let erased_enqueue: Arc<dyn TaskEnqueueHook> = enqueue;
        let erased_drain: Arc<dyn MovePostCommitHook> = drain.clone();
        let outcome = ChainOutcome::new();
        run_chain(
            &pool,
            &erased_transition,
            &erased_enqueue,
            Some(&erased_drain),
            vec![
                ChainEffect::DrainPod { pod_id: uid(2) },
                ChainEffect::FireTransition {
                    issue_id: uid(3),
                    prev_state: Some(uid(4)),
                    current_state: Some(uid(5)),
                    actor_id: uid(1),
                    spec: FireSpec {
                        dispatch_immediate: true,
                        created_by: Some(uid(1)),
                        moved_by_run: None,
                        now: Utc::now(),
                    },
                },
            ],
            &outcome,
        )
        .await;
        assert_eq!(outcome.stopped(), Some(ChainStop::PodDrain));
        assert!(transition.fires.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn send_cancel_runs_and_never_stops() {
        let pool = lazy_pool();
        let (transition, enqueue, drain) = test_hooks();
        let erased_transition: Arc<dyn IssueTransitionHook> = transition;
        let erased_enqueue: Arc<dyn TaskEnqueueHook> = enqueue;
        let erased_drain: Arc<dyn MovePostCommitHook> = drain.clone();
        let outcome = ChainOutcome::new();
        run_chain(
            &pool,
            &erased_transition,
            &erased_enqueue,
            Some(&erased_drain),
            vec![ChainEffect::SendCancel {
                runner_id: uid(6),
                run_id: uid(7),
            }],
            &outcome,
        )
        .await;
        assert_eq!(outcome.stopped(), None);
        assert_eq!(*drain.cancels.lock().expect("lock"), vec![(uid(6), uid(7))]);
    }

    #[tokio::test]
    async fn move_activity_legs_match_move_builders_byte_for_byte() {
        let pool = lazy_pool();
        let (transition, enqueue, drain) = test_hooks();
        let erased_transition: Arc<dyn IssueTransitionHook> = transition;
        let erased_enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
        let erased_drain: Arc<dyn MovePostCommitHook> = drain;
        let outcome = ChainOutcome::new();
        // A fixed epoch: the legs use the frozen move clock, not the
        // domain builder's chain-time clock.
        let epoch = 1_791_234_567;
        run_chain(
            &pool,
            &erased_transition,
            &erased_enqueue,
            Some(&erased_drain),
            vec![
                ChainEffect::MoveIssueActivity {
                    event: "issue.activity.updated".to_owned(),
                    requested: "r".to_owned(),
                    actor: uid(1),
                    issue: uid(2),
                    project: uid(3),
                    current: "c".to_owned(),
                    epoch,
                },
                ChainEffect::MoveModelActivity {
                    model: uid(2),
                    requested: serde_json::json!({"project": uid(3)}),
                    current: "c".to_owned(),
                    actor: uid(1),
                    slug: "ws".to_owned(),
                    origin: "https://app.example.test".to_owned(),
                },
            ],
            &outcome,
        )
        .await;
        assert_eq!(outcome.stopped(), None);
        let calls = enqueue.calls.lock().expect("lock");
        assert_eq!(calls.len(), 2);
        let expected_issue = move_kernel::issue_activity_kwargs(
            "issue.activity.updated",
            "r",
            &uid(1),
            &uid(2),
            &uid(3),
            "c",
            epoch,
        );
        assert_eq!(calls[0].0, expected_issue.task);
        assert_eq!(calls[0].1, expected_issue.args);
        assert_eq!(calls[0].2, expected_issue.kwargs);
        assert_eq!(
            calls[0].2.get("epoch"),
            Some(&serde_json::Value::from(epoch))
        );
        let expected_model = move_kernel::move_model_activity_kwargs(
            &uid(2),
            serde_json::json!({"project": uid(3)}),
            "c",
            &uid(1),
            "ws",
            "https://app.example.test",
        );
        assert_eq!(calls[1].0, expected_model.task);
        assert_eq!(calls[1].1, expected_model.args);
        assert_eq!(calls[1].2, expected_model.kwargs);
    }

    /// A blank target is a 400 with no store touch: the kernel rejects
    /// before its first query, the transaction commits empty, no chain
    /// step runs.
    #[tokio::test]
    async fn live_blank_target_rejects_without_store_touch() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let db = pidash_db::sea_orm_connection(&pool);
        let actor = Uuid::new_v4();
        let ctx = WriteCtx::new(WriteActor {
            user_id: actor,
            timezone: Some("UTC".to_owned()),
        });
        let (transition, enqueue, drain) = test_hooks();
        let target_ref = serde_json::json!("   ");
        let slug = "ws".to_owned();
        let outcome = super::super::run(&ctx, &db, |txn| {
            let transition_hook: Arc<dyn IssueTransitionHook> = transition.clone();
            let enqueue_hook: Arc<dyn TaskEnqueueHook> = enqueue.clone();
            let drain_hook: Arc<dyn MovePostCommitHook> = drain.clone();
            let pool = pool.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                txn.hooks_mut().register(transition_hook);
                txn.hooks_mut().register(enqueue_hook);
                txn.hooks_mut().register(drain_hook);
                move_to_project(
                    &ctx,
                    txn,
                    MoveInput {
                        slug: &slug,
                        project_id: Uuid::new_v4(),
                        pk: Uuid::new_v4(),
                        target_ref: &target_ref,
                        origin: "https://app.example.test".to_owned(),
                        web_base_url: None,
                        now: Utc::now(),
                        prev_state: None,
                        options: MoveOptions::django(),
                        creation: test_creation_deps(pool),
                    },
                )
                .await
            })
        })
        .await
        .expect("run commits");
        match outcome {
            MoveDomainOutcome::Rejected(MoveRejection::Issue {
                message,
                status_code,
            }) => {
                assert_eq!(message, move_kernel::PROJECT_REQUIRED_MESSAGE);
                assert_eq!(status_code, 400);
            }
            other => panic!("expected blank-target 400, got {}", outcome_name(&other)),
        }
        assert!(enqueue.calls.lock().expect("lock").is_empty());
        assert!(transition.fires.lock().expect("lock").is_empty());
    }

    /// A missing source is the 404 rejection; the resolve never runs.
    #[tokio::test]
    async fn live_missing_source_rejects_not_found() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let db = pidash_db::sea_orm_connection(&pool);
        let actor = Uuid::new_v4();
        let ctx = WriteCtx::new(WriteActor {
            user_id: actor,
            timezone: Some("UTC".to_owned()),
        });
        let (transition, enqueue, drain) = test_hooks();
        let target_ref = serde_json::json!("ENG");
        let slug = "ws".to_owned();
        let outcome = super::super::run(&ctx, &db, |txn| {
            let transition_hook: Arc<dyn IssueTransitionHook> = transition.clone();
            let enqueue_hook: Arc<dyn TaskEnqueueHook> = enqueue.clone();
            let drain_hook: Arc<dyn MovePostCommitHook> = drain.clone();
            let pool = pool.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                txn.hooks_mut().register(transition_hook);
                txn.hooks_mut().register(enqueue_hook);
                txn.hooks_mut().register(drain_hook);
                move_to_project(
                    &ctx,
                    txn,
                    MoveInput {
                        slug: &slug,
                        project_id: Uuid::new_v4(),
                        pk: Uuid::new_v4(),
                        target_ref: &target_ref,
                        origin: "https://app.example.test".to_owned(),
                        web_base_url: None,
                        now: Utc::now(),
                        prev_state: None,
                        options: MoveOptions::django(),
                        creation: test_creation_deps(pool),
                    },
                )
                .await
            })
        })
        .await
        .expect("run commits");
        assert!(matches!(
            outcome,
            MoveDomainOutcome::Rejected(MoveRejection::SourceNotFound)
        ));
    }

    /// End-to-end through `domain::run` with recording hooks: seed a
    /// source issue plus a target project, move it by identifier, and
    /// assert the moved row, the sequence rotation, the quiet drains
    /// (no runs), and the drained chain (comment + fire + two enqueues).
    #[tokio::test]
    async fn live_move_roundtrip_moves_row_and_drains_chain() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let actor = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let slug = format!("mv-{}", workspace_id.simple());
        let source_project = Uuid::new_v4();
        let target_project = Uuid::new_v4();
        let source_state = Uuid::new_v4();
        let issue_id = Uuid::new_v4();
        let target_identifier = format!("M{:08X}", (target_project.as_u128() & 0xffff_ffff) as u32);

        let username = format!("mv-{}", actor.simple());
        sqlx::query(
            "INSERT INTO users (id, password, username, email, first_name, last_name, avatar, \
             date_joined, created_at, updated_at, last_location, created_location, is_superuser, \
             is_managed, is_password_expired, is_active, is_staff, is_email_verified, \
             is_password_autoset, token, user_timezone, last_login_ip, last_logout_ip, \
             last_login_medium, last_login_uagent, is_bot, display_name, is_email_valid, \
             is_password_reset_required) VALUES ($1, '!', $2, $3, 'CT', 'User', '', now(), now(), \
             now(), '', '', false, false, false, true, false, true, false, $4, 'UTC', '', '', '', \
             '', false, $2, true, false)",
        )
        .bind(actor)
        .bind(username.clone())
        .bind(format!("{username}@example.test"))
        .bind("t".repeat(64))
        .execute(&pool)
        .await
        .expect("seed user");
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, slug, timezone, \
             background_color, owner_id) VALUES ($1, now(), now(), 'mv ws', $2, 'UTC', '#ffffff', \
             $3)",
        )
        .bind(workspace_id)
        .bind(&slug)
        .bind(actor)
        .execute(&pool)
        .await
        .expect("seed workspace");
        for (id, identifier) in [
            (
                source_project,
                format!("S{:08X}", (source_project.as_u128() & 0xffff_ffff) as u32),
            ),
            (target_project, target_identifier.clone()),
        ] {
            sqlx::query(
                "INSERT INTO projects (id, name, description, network, identifier, workspace_id, \
                 created_by_id, cycle_view, module_view, issue_views_view, page_view, intake_view, \
                 archive_in, close_in, logo_props, is_time_tracking_enabled, is_issue_type_enabled, \
                 guest_view_all_features, timezone, members_can_edit_states, repo_url, base_branch, \
                 agent_default_interval_seconds, agent_default_max_ticks, agent_ticking_enabled, \
                 is_default, agent_review_default_interval_seconds, default_agent_executor, \
                 agent_test_default_interval_seconds, created_at, updated_at) VALUES ($1, \
                 $2, '', 0, $3, $4, $5, true, true, true, true, false, 0, 0, '{}', true, \
                 true, false, 'UTC', false, '', '', 0, 0, false, false, 0, '', 0, now(), now())",
            )
            .bind(id)
            .bind(format!("mv-project-{identifier}"))
            .bind(identifier)
            .bind(workspace_id)
            .bind(actor)
            .execute(&pool)
            .await
            .expect("seed project");
        }
        sqlx::query(
            "INSERT INTO project_members (id, role, project_id, workspace_id, member_id, \
             view_props, default_props, sort_order, preferences, is_active, created_at, \
             updated_at) VALUES ($1, 15, $2, $3, $4, '{}', '{}', 65535, '{}', true, now(), now())",
        )
        .bind(Uuid::new_v4())
        .bind(target_project)
        .bind(workspace_id)
        .bind(actor)
        .execute(&pool)
        .await
        .expect("seed target membership");
        for (id, project, name, group, sequence, default) in [
            (
                source_state,
                source_project,
                "Backlog",
                "backlog",
                100.0,
                false,
            ),
            (
                Uuid::new_v4(),
                target_project,
                "Triage",
                "triage",
                100.0,
                false,
            ),
            (
                Uuid::new_v4(),
                target_project,
                "Backlog",
                "backlog",
                200.0,
                true,
            ),
        ] {
            sqlx::query(
                "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
                 project_id, workspace_id, sequence, \"group\", \"default\", is_triage) VALUES \
                 ($1, now(), now(), $2, '', '#ff0000', $3, $4, $5, $6, $7, $8, false)",
            )
            .bind(id)
            .bind(name)
            .bind(format!("mv-{name}-{sequence}-{project}"))
            .bind(project)
            .bind(workspace_id)
            .bind(sequence)
            .bind(group)
            .bind(default)
            .execute(&pool)
            .await
            .expect("seed state");
        }
        sqlx::query(
            "INSERT INTO issues (id, created_at, updated_at, project_id, workspace_id, state_id, \
             name, description_json, description_html, sequence_id, sort_order, priority, \
             complexity_score, is_draft, git_work_branch, workpad) VALUES ($1, now(), now(), $2, \
             $3, $4, 'mv issue', '{}', '<p>mv</p>', 3, 10000.0, 'none', 0, false, '', '')",
        )
        .bind(issue_id)
        .bind(source_project)
        .bind(workspace_id)
        .bind(source_state)
        .execute(&pool)
        .await
        .expect("seed issue");

        let db = pidash_db::sea_orm_connection(&pool);
        let ctx = WriteCtx::new(WriteActor {
            user_id: actor,
            timezone: Some("UTC".to_owned()),
        });
        let (transition, enqueue, drain) = test_hooks();
        let target_ref = serde_json::json!(target_identifier);
        let now = Utc::now();
        let outcome = super::super::run(&ctx, &db, |txn| {
            let transition_hook: Arc<dyn IssueTransitionHook> = transition.clone();
            let enqueue_hook: Arc<dyn TaskEnqueueHook> = enqueue.clone();
            let drain_hook: Arc<dyn MovePostCommitHook> = drain.clone();
            let pool = pool.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                txn.hooks_mut().register(transition_hook);
                txn.hooks_mut().register(enqueue_hook);
                txn.hooks_mut().register(drain_hook);
                move_to_project(
                    &ctx,
                    txn,
                    MoveInput {
                        slug: &slug,
                        project_id: source_project,
                        pk: issue_id,
                        target_ref: &target_ref,
                        origin: "https://app.example.test".to_owned(),
                        web_base_url: None,
                        now,
                        prev_state: Some(source_state),
                        options: MoveOptions::django(),
                        creation: test_creation_deps(pool),
                    },
                )
                .await
            })
        })
        .await
        .expect("run commits");

        let (row, chain) = match outcome {
            MoveDomainOutcome::Moved { row, chain } => (row, chain),
            other => panic!("expected Moved, got {}", outcome_name(&other)),
        };
        assert_eq!(chain.stopped(), None);
        assert_eq!(row.row.project_id, target_project);
        assert_eq!(row.row.workspace_id, workspace_id);
        assert_eq!(row.row.sequence_id, 1);
        assert_eq!(row.row.parent_id, None);
        assert_eq!(row.row.assigned_pod_id, None);
        assert_eq!(row.row.updated_by_id, None);

        // The target default state is the seeded backlog row: resolve it
        // for the state assertions below.
        let default_state: (Uuid,) =
            sqlx::query_as("SELECT id FROM states WHERE project_id = $1 AND \"default\"")
                .bind(target_project)
                .fetch_one(&pool)
                .await
                .expect("target default state");
        assert_eq!(row.row.state_id, Some(default_state.0));

        // The sequence row rotated to the target.
        let seq: (i64,) = sqlx::query_as(
            "SELECT sequence FROM issue_sequences WHERE issue_id = $1 AND project_id = $2",
        )
        .bind(issue_id)
        .bind(target_project)
        .fetch_one(&pool)
        .await
        .expect("target sequence row");
        assert_eq!(seq.0, 1);

        // No runs: no cancels, no pod drains, no creation drain.
        assert!(drain.cancels.lock().expect("lock").is_empty());
        assert!(drain.pods.lock().expect("lock").is_empty());
        assert!(drain.creations.lock().expect("lock").is_empty());

        // The chain drained in order: comment, fire, two enqueues.
        assert_eq!(
            *transition.comments.lock().expect("lock"),
            vec![(issue_id, Some(source_state), Some(default_state.0))]
        );
        let fires = transition.fires.lock().expect("lock");
        assert_eq!(fires.len(), 1);
        assert_eq!(fires[0].issue_id, issue_id);
        assert_eq!(fires[0].prev_state_id, Some(source_state));
        assert_eq!(fires[0].current_state_id, Some(default_state.0));
        assert!(fires[0].dispatch_immediate);
        assert_eq!(fires[0].moved_by_run, None);
        assert_eq!(fires[0].created_by, Some(actor));
        let calls = enqueue.calls.lock().expect("lock");
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0].0,
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
        assert_eq!(
            calls[0].2.get("epoch"),
            Some(&serde_json::Value::from(now.timestamp()))
        );
        assert_eq!(calls[1].0, "pi_dash.bgtasks.webhook_task.model_activity");
    }

    fn outcome_name(outcome: &MoveDomainOutcome) -> &'static str {
        match outcome {
            MoveDomainOutcome::AlreadyThere { .. } => "AlreadyThere",
            MoveDomainOutcome::Moved { .. } => "Moved",
            MoveDomainOutcome::Rejected(_) => "Rejected",
        }
    }
}
