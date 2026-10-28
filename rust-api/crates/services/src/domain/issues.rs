//! OSS issue writes: `issues`, `issue_sequences`, `issue_assignees`
//! (ORM-03, PIDASHCONV-1521 part A).
//!
//! Every OSS write to the three issue tables lives in this module; the app
//! and v1 handlers (parts A/B), then drafts, intake, agent tools, sync,
//! automation and the ticker (part C) call these functions instead of
//! executing SQL text. Reads stay where they are (design D5).
//!
//! Two execution shapes, by writer kind:
//!
//! * Entity saves (`create`, `update`, `partial_update`, `soft_delete`,
//!   `archive`, `unarchive`) run the row write on the caller's [`WriteTxn`]
//!   through `ActiveModel::insert` / `update`, so the `Issue.save()` hooks
//!   fire ([`pidash_db::entity::issue_hooks`]). The post-commit effects run
//!   as ONE queued chain step, in the handlers' exact order, because the
//!   handlers short-circuit with `?`: a transition-driver failure skips the
//!   m2m legs on create, a visit-delete failure skips the deleted activity
//!   on destroy. Per-effect queue steps cannot express that; the chain runs
//!   as one step and records its outcome in the [`ChainOutcome`] the driver
//!   checks after [`run`](crate::domain::run) returns (the drain precedes
//!   the return, so the outcome is final).
//! * Bulk writes (`bulk_archive_issues`, `bulk_soft_delete_issues`) are
//!   pool-based: Django runs them as `bulk_update` / queryset `delete`
//!   with no `save()` and no signals, and the port runs them autocommit,
//!   so they take no transaction at all.
//!
//! Pre-save legs (update m2m wipes, archive activity) run inside the work
//! closure on the pool BEFORE the entity save, exactly where Django runs
//! them: they persist even when the save itself fails, so they must not
//! join the [`WriteTxn`].
//!
//! Fetch-gated effects stay driver-sequenced: create's `model_activity`
//! and version task run after the driver's response-row fetch (a miss 500s
//! with the model task never enqueued), so they are public functions the
//! driver calls at the original point — queueing them would enqueue tasks
//! Django skips. The origin resolves per effect at its original point for
//! the same reason (`base_host` raises past the save).
//!
//! Actor stamping comes from [`WriteCtx`]: `created_by` on create,
//! `updated_by` on every save (`BaseModel.save` sets it to the requesting
//! user on every update). A nil actor stamps `None` (Django's
//! missing-crum default); the HTTP drivers always pass their request user.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, NaiveDate, Timelike, Utc};
use pidash_db::entity::issues;
use sea_orm::{ActiveModelTrait, Set};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::activity::{TaskEnqueueHook, ISSUE_ACTIVITY_TASK, MODEL_ACTIVITY_TASK};
use super::ctx::WriteCtx;
use super::error::DomainError;
use super::transition::IssueTransitionHook;
use super::txn::WriteTxn;
use crate::app_issues::serializers_create::{
    m2m_batches, m2m_insert_sql, ASSIGNEE_CLEAR_SQL, DEFAULT_ASSIGNEE_EXISTS_SQL, LABEL_CLEAR_SQL,
};
use crate::orchestration::entries::{FireOutcome, FireRequest};

/// `issue_description_version_task`
/// (`bgtasks/issue_description_version_task.py:44`): full Celery name.
pub const VERSION_TASK: &str =
    "pi_dash.bgtasks.issue_description_version_task.issue_description_version_task";

/// `soft_delete_related_objects` (`bgtasks/deletion_task.py:18`): full
/// Celery name.
pub const SOFT_DELETE_TASK: &str = "pi_dash.bgtasks.deletion_task.soft_delete_related_objects";

/// `now()` truncated to microseconds: Postgres `timestamptz` keeps micros,
/// and two successive `now()` calls must not drift on the sub-micro tail.
/// Mirrors the handlers' `utc_now_micros`.
fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    now.with_nanosecond(now.nanosecond() / 1000 * 1000)
        .unwrap_or(now)
}

fn is_integrity_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => db.code().is_some_and(|code| code.starts_with("23")),
        _ => false,
    }
}

/// The acting user id for audit stamps: a nil actor (system paths with no
/// request) stamps `None`, Django's missing-crum default.
fn actor_or_none(ctx: &WriteCtx) -> Option<Uuid> {
    if ctx.actor.user_id.is_nil() {
        None
    } else {
        Some(ctx.actor.user_id)
    }
}

/// `json.dumps` separators (`", "` after items, `": "` after keys): the
/// single copy of the two handler-local formatters (byte-identical).
struct SpacedFormatter;

impl serde_json::ser::Formatter for SpacedFormatter {
    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_value<W>(&mut self, writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        writer.write_all(b": ")
    }

    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }
}

/// Render `value` with [`SpacedFormatter`]. Drivers use it for the
/// activity `requested_data` / `current_instance` dumps they build
/// pre-save; `None` on serialization failure (the drivers' 500 arm).
pub fn to_spaced_json<T: serde::Serialize>(value: &T) -> Option<String> {
    let mut buf = Vec::new();
    let mut ser = serde_json::ser::Serializer::with_formatter(&mut buf, SpacedFormatter);
    value.serialize(&mut ser).ok()?;
    String::from_utf8(buf).ok()
}

// ---------------------------------------------------------------------------
// The row
// ---------------------------------------------------------------------------

/// Every `issues` column as owned values — the INSERT, the full-column
/// UPDATE and the hook inputs all bind from here. Drivers assemble it
/// (create) or overlay validated input onto a fetched row (update); the
/// post-save [`issues::Model`] converts back for drivers that render from
/// the saved row.
#[derive(Debug, Clone)]
pub struct IssueFieldSet {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub state_id: Option<Uuid>,
    pub point: Option<i32>,
    pub estimate_point_id: Option<Uuid>,
    pub name: String,
    pub description_json: Value,
    pub description_html: String,
    pub description_stripped: Option<String>,
    pub description_binary: Option<Vec<u8>>,
    pub priority: String,
    pub complexity_score: i32,
    pub start_date: Option<NaiveDate>,
    pub target_date: Option<NaiveDate>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub completed_at: Option<DateTime<Utc>>,
    pub archived_at: Option<NaiveDate>,
    pub is_draft: bool,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub type_id: Option<Uuid>,
    pub git_work_branch: String,
    pub workpad: String,
    pub created_via: Option<String>,
    pub assigned_pod_id: Option<Uuid>,
    pub agent_executor: Option<String>,
}

impl IssueFieldSet {
    /// The create INSERT: every column `Set`, with `created_by` stamped
    /// from the actor and `updated_by` left `None`. The hooks assign the
    /// pod default, state default, `completed_at`, stripped text, sequence
    /// and sort order.
    fn into_insert_active_model(self, created_by: Option<Uuid>) -> issues::ActiveModel {
        issues::ActiveModel {
            id: Set(self.id),
            created_at: Set(self.created_at.into()),
            updated_at: Set(self.updated_at.into()),
            created_by_id: Set(created_by),
            updated_by_id: Set(None),
            deleted_at: Set(self.deleted_at.map(Into::into)),
            project_id: Set(self.project_id),
            workspace_id: Set(self.workspace_id),
            parent_id: Set(self.parent_id),
            state_id: Set(self.state_id),
            point: Set(self.point),
            estimate_point_id: Set(self.estimate_point_id),
            name: Set(self.name),
            description_json: Set(self.description_json),
            description_html: Set(self.description_html),
            description_stripped: Set(self.description_stripped),
            description_binary: Set(self.description_binary),
            priority: Set(self.priority),
            complexity_score: Set(self.complexity_score),
            start_date: Set(self.start_date),
            target_date: Set(self.target_date),
            sequence_id: Set(self.sequence_id),
            sort_order: Set(self.sort_order),
            completed_at: Set(self.completed_at.map(Into::into)),
            archived_at: Set(self.archived_at),
            is_draft: Set(self.is_draft),
            external_source: Set(self.external_source),
            external_id: Set(self.external_id),
            type_id: Set(self.type_id),
            git_work_branch: Set(self.git_work_branch),
            workpad: Set(self.workpad),
            created_via: Set(self.created_via),
            assigned_pod_id: Set(self.assigned_pod_id),
            agent_executor: Set(self.agent_executor),
        }
    }

    /// `instance.save()` on an update: Django writes every column (no
    /// `update_fields`), so every non-pk column is `Set`; the pk stays
    /// `Unchanged` (the `WHERE`, not the `SET`). `updated_by` is stamped
    /// from the actor; `created_by` rides the fetched row untouched.
    fn into_full_save_active_model(self, updated_by: Option<Uuid>) -> issues::ActiveModel {
        issues::ActiveModel {
            id: sea_orm::Unchanged(self.id),
            created_at: Set(self.created_at.into()),
            updated_at: Set(self.updated_at.into()),
            created_by_id: Set(self.created_by_id),
            updated_by_id: Set(updated_by),
            deleted_at: Set(self.deleted_at.map(Into::into)),
            project_id: Set(self.project_id),
            workspace_id: Set(self.workspace_id),
            parent_id: Set(self.parent_id),
            state_id: Set(self.state_id),
            point: Set(self.point),
            estimate_point_id: Set(self.estimate_point_id),
            name: Set(self.name),
            description_json: Set(self.description_json),
            description_html: Set(self.description_html),
            description_stripped: Set(self.description_stripped),
            description_binary: Set(self.description_binary),
            priority: Set(self.priority),
            complexity_score: Set(self.complexity_score),
            start_date: Set(self.start_date),
            target_date: Set(self.target_date),
            sequence_id: Set(self.sequence_id),
            sort_order: Set(self.sort_order),
            completed_at: Set(self.completed_at.map(Into::into)),
            archived_at: Set(self.archived_at),
            is_draft: Set(self.is_draft),
            external_source: Set(self.external_source),
            external_id: Set(self.external_id),
            type_id: Set(self.type_id),
            git_work_branch: Set(self.git_work_branch),
            workpad: Set(self.workpad),
            created_via: Set(self.created_via),
            assigned_pod_id: Set(self.assigned_pod_id),
            agent_executor: Set(self.agent_executor),
        }
    }
}

impl From<&issues::Model> for IssueFieldSet {
    fn from(model: &issues::Model) -> Self {
        Self {
            id: model.id,
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
            created_by_id: model.created_by_id,
            updated_by_id: model.updated_by_id,
            deleted_at: model.deleted_at.map(Into::into),
            project_id: model.project_id,
            workspace_id: model.workspace_id,
            parent_id: model.parent_id,
            state_id: model.state_id,
            point: model.point,
            estimate_point_id: model.estimate_point_id,
            name: model.name.clone(),
            description_json: model.description_json.clone(),
            description_html: model.description_html.clone(),
            description_stripped: model.description_stripped.clone(),
            description_binary: model.description_binary.clone(),
            priority: model.priority.clone(),
            complexity_score: model.complexity_score,
            start_date: model.start_date,
            target_date: model.target_date,
            sequence_id: model.sequence_id,
            sort_order: model.sort_order,
            completed_at: model.completed_at.map(Into::into),
            archived_at: model.archived_at,
            is_draft: model.is_draft,
            external_source: model.external_source.clone(),
            external_id: model.external_id.clone(),
            type_id: model.type_id,
            git_work_branch: model.git_work_branch.clone(),
            workpad: model.workpad.clone(),
            created_via: model.created_via.clone(),
            assigned_pod_id: model.assigned_pod_id,
            agent_executor: model.agent_executor.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// m2m legs
// ---------------------------------------------------------------------------

/// One m2m `bulk_create(batch_size=10)` leg: a multi-row INSERT per
/// 10-chunk in input order; an `IntegrityError` swallows the REST of the
/// batches (`except: pass` wraps the whole call), any other error fails.
/// Moved verbatim from the app handlers; generic over the executor so
/// drivers run it on the pool and tests on a rolled-back transaction.
#[allow(clippy::too_many_arguments)]
async fn write_m2m_batches<'e, E>(
    ex: E,
    table: &str,
    member_column: &str,
    ids: &[Uuid],
    issue_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    ignore_conflicts: bool,
) -> Result<(), DomainError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
{
    for batch in m2m_batches(ids) {
        let sql = m2m_insert_sql(table, member_column, batch.len(), ignore_conflicts);
        let mut query = sqlx::query(&sql);
        for member_id in batch {
            // `auto_now_add` + `auto_now`: two `now()` calls per row, like
            // the per-object instantiation Django bulk-saves.
            let created = now_micros();
            let updated = now_micros();
            query = query
                .bind(Uuid::new_v4())
                .bind(created)
                .bind(updated)
                .bind(created_by_id)
                .bind(updated_by_id)
                .bind(project_id)
                .bind(workspace_id)
                .bind(issue_id)
                .bind(member_id);
        }
        match query.execute(ex).await {
            Ok(_) => {}
            Err(error) if is_integrity_violation(&error) => break,
            Err(error) => {
                return Err(DomainError::Work(format!(
                    "m2m {table} write failed: {error}"
                )));
            }
        }
    }
    Ok(())
}

/// The create default-assignee fallback (`serializers/issue.py:419-444`):
/// empty/missing assignees + a valid project-default member → one
/// `IssueAssignee` row, `IntegrityError` swallowed. Moved verbatim.
async fn write_default_assignee<'e, E>(
    ex: E,
    default_assignee_id: &Uuid,
    issue_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
    created_by_id: Option<Uuid>,
) -> Result<(), DomainError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
{
    let member: Option<(i32,)> = sqlx::query_as(DEFAULT_ASSIGNEE_EXISTS_SQL)
        .bind(default_assignee_id)
        .bind(project_id)
        .fetch_optional(ex)
        .await
        .map_err(|error| DomainError::Work(format!("default-assignee check failed: {error}")))?;
    if member.is_none() {
        return Ok(());
    }
    let sql = m2m_insert_sql("issue_assignees", "assignee_id", 1, false);
    let created = now_micros();
    let updated = now_micros();
    match sqlx::query(&sql)
        .bind(Uuid::new_v4())
        .bind(created)
        .bind(updated)
        .bind(created_by_id)
        .bind(None::<Uuid>)
        .bind(project_id)
        .bind(workspace_id)
        .bind(issue_id)
        .bind(default_assignee_id)
        .execute(ex)
        .await
    {
        Ok(_) => Ok(()),
        Err(error) if is_integrity_violation(&error) => Ok(()),
        Err(error) => Err(DomainError::Work(format!(
            "default-assignee write failed: {error}"
        ))),
    }
}

/// One update m2m leg (`serializers/issue.py:477-511`): present keys
/// soft-delete the live links, then re-insert the effective ids with
/// `ON CONFLICT DO NOTHING`; audit ids come from the PRE-SAVE instance.
/// Moved verbatim; the clear statement is the shared const (identical text
/// to the handlers' `format!` for both tables).
#[allow(clippy::too_many_arguments)]
async fn update_m2m_leg<'e, E>(
    ex: E,
    table: &str,
    member_column: &str,
    present: bool,
    effective: &[Uuid],
    issue_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
    audit: (Option<Uuid>, Option<Uuid>),
) -> Result<(), DomainError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
{
    if !present {
        return Ok(());
    }
    let clear = match table {
        "issue_assignees" => ASSIGNEE_CLEAR_SQL,
        _ => LABEL_CLEAR_SQL,
    };
    sqlx::query(clear)
        .bind(now_micros())
        .bind(issue_id)
        .execute(ex)
        .await
        .map_err(|error| DomainError::Work(format!("m2m {table} clear failed: {error}")))?;
    write_m2m_batches(
        ex,
        table,
        member_column,
        effective,
        issue_id,
        project_id,
        workspace_id,
        audit.0,
        audit.1,
        true,
    )
    .await
}

// ---------------------------------------------------------------------------
// Activity kwargs (pure builders; the transport is the [`TaskEnqueueHook`])
// ---------------------------------------------------------------------------

/// `issue_activity.delay(type, requested_data, actor_id, issue_id,
/// project_id, current_instance, epoch, notification, origin[, subscriber])`.
#[allow(clippy::too_many_arguments)]
fn issue_activity_kwargs(
    activity_type: &str,
    requested_data: Value,
    actor_id: &Uuid,
    issue_id: &Uuid,
    project_id: &Uuid,
    current_instance: Value,
    origin: &str,
    subscriber: Option<bool>,
) -> Map<String, Value> {
    let mut kwargs = Map::new();
    kwargs.insert("type".to_owned(), Value::String(activity_type.to_owned()));
    kwargs.insert("requested_data".to_owned(), requested_data);
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert("current_instance".to_owned(), current_instance);
    kwargs.insert(
        "epoch".to_owned(),
        Value::Number(serde_json::Number::from(Utc::now().timestamp())),
    );
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    if let Some(subscriber) = subscriber {
        kwargs.insert("subscriber".to_owned(), Value::Bool(subscriber));
    }
    kwargs
}

/// `model_activity.delay(model_name, model_id, requested_data,
/// current_instance, actor_id, slug, origin)`: `requested_data` is the RAW
/// object (never JSON-encoded at these sites); the UUID actor rides as a
/// string, exactly as Celery's encoder would carry the UUID object.
fn model_activity_kwargs(
    model_id: &Uuid,
    requested_data: Value,
    current_instance: Value,
    actor_id: &Uuid,
    slug: &str,
    origin: &str,
) -> Map<String, Value> {
    let mut kwargs = Map::new();
    kwargs.insert("model_name".to_owned(), Value::String("issue".to_owned()));
    kwargs.insert("model_id".to_owned(), Value::String(model_id.to_string()));
    kwargs.insert("requested_data".to_owned(), requested_data);
    kwargs.insert("current_instance".to_owned(), current_instance);
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    kwargs
}

/// `issue_description_version_task.delay(updated_issue, issue_id, user_id[,
/// is_creating])`: the update site OMITS `is_creating` (it is not `False`).
fn version_task_kwargs(
    updated_issue: &str,
    issue_id: &Uuid,
    user_id: &Uuid,
    is_creating: Option<bool>,
) -> Map<String, Value> {
    let mut kwargs = Map::new();
    kwargs.insert(
        "updated_issue".to_owned(),
        Value::String(updated_issue.to_owned()),
    );
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    kwargs.insert("user_id".to_owned(), Value::String(user_id.to_string()));
    if let Some(is_creating) = is_creating {
        kwargs.insert("is_creating".to_owned(), Value::Bool(is_creating));
    }
    kwargs
}

/// `soft_delete_related_objects.delay(app_label, model_name, pk,
/// using=None)`: positional ids + the explicit `using` kwarg.
fn soft_delete_args(issue_id: &Uuid) -> (Vec<Value>, Map<String, Value>) {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    (
        vec![
            Value::String("db".to_owned()),
            Value::String("issue".to_owned()),
            Value::String(issue_id.to_string()),
        ],
        kwargs,
    )
}

// ---------------------------------------------------------------------------
// The post-commit chain
// ---------------------------------------------------------------------------

/// Where a post-commit chain stopped early. Every variant maps to the
/// drivers' `ServerError`, exactly the handlers' `?` arms today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainStop {
    /// The transition driver failed (deps build) — the `fire_after_save` `?`.
    Fire,
    /// An m2m leg failed past its integrity swallow — the leg's `?`.
    M2m,
    /// The destroy visit-delete failed — its `?`.
    VisitDelete,
    /// The origin failed to resolve (`base_host` raises past the save).
    Origin,
}

/// The recorded outcome of one queued chain. The drain precedes
/// [`run`](crate::domain::run)'s return, so when the driver reads this the
/// chain has finished: `None` is full success, `Some` names the leg that
/// stopped it (later legs never ran, exactly the handlers' `?` order).
#[derive(Debug, Clone, Default)]
pub struct ChainOutcome {
    stop: Arc<Mutex<Option<ChainStop>>>,
}

impl ChainOutcome {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stopped(&self) -> Option<ChainStop> {
        *self.stop.lock().expect("chain outcome lock")
    }

    fn record(&self, stop: ChainStop) {
        *self.stop.lock().expect("chain outcome lock") = Some(stop);
    }
}

/// The post-save transition fire arguments (one `fire_after_save` call).
#[derive(Debug, Clone)]
pub struct FireSpec {
    pub dispatch_immediate: bool,
    pub created_by: Option<Uuid>,
    pub now: DateTime<Utc>,
}

/// An `issue_activity` enqueue with its origin pre-resolved by the driver
/// at the original point (`base_host` raises past the save).
#[derive(Debug, Clone)]
pub struct IssueActivitySpec {
    pub event: String,
    pub requested: Value,
    pub actor: Uuid,
    pub issue: Uuid,
    pub project: Uuid,
    pub current: Value,
    pub origin: Option<String>,
    pub subscriber: Option<bool>,
}

/// A `model_activity` enqueue with its origin pre-resolved by the driver.
#[derive(Debug, Clone)]
pub struct ModelActivitySpec {
    pub model: Uuid,
    pub requested: Value,
    pub current: Value,
    pub actor: Uuid,
    pub slug: String,
    pub origin: Option<String>,
}

/// One post-commit effect, in handler order. The chain runs as a single
/// queued step (see the module docs): fallible legs record a [`ChainStop`]
/// and skip every later leg.
enum ChainEffect {
    CompletionIfChanged {
        issue_id: Uuid,
        prev_state: Option<Uuid>,
        current_state: Option<Uuid>,
    },
    FireTransition {
        issue_id: Uuid,
        prev_state: Option<Uuid>,
        current_state: Option<Uuid>,
        spec: FireSpec,
        actor_id: Uuid,
    },
    M2mInsert {
        table: &'static str,
        member_column: &'static str,
        ids: Vec<Uuid>,
        issue_id: Uuid,
        project_id: Uuid,
        workspace_id: Uuid,
        created_by: Option<Uuid>,
        updated_by: Option<Uuid>,
        ignore_conflicts: bool,
    },
    DefaultAssignee {
        default_id: Uuid,
        issue_id: Uuid,
        project_id: Uuid,
        workspace_id: Uuid,
        created_by: Option<Uuid>,
    },
    IssueActivity(IssueActivitySpec),
    ModelActivity(ModelActivitySpec),
    VersionTask {
        updated_issue: String,
        issue_id: Uuid,
        user_id: Uuid,
        is_creating: Option<bool>,
    },
    SoftDeleteTask {
        issue_id: Uuid,
    },
    VisitDelete {
        project_id: Uuid,
        slug: String,
        issue_id: Uuid,
    },
}

/// The destroy recent-visit delete, moved verbatim: a HARD delete scoped
/// to the workspace slug + project + issue.
const RECENT_VISIT_DELETE_SQL: &str = "DELETE FROM user_recent_visits AS visit \
    USING workspaces AS workspace WHERE visit.workspace_id = workspace.id \
    AND visit.project_id = $1 AND workspace.slug = $2 AND visit.entity_identifier = $3 \
    AND visit.entity_name = 'issue'";

async fn run_chain(
    pool: &PgPool,
    transition: &Arc<dyn IssueTransitionHook>,
    enqueue: &Arc<dyn TaskEnqueueHook>,
    chain: Vec<ChainEffect>,
    outcome: &ChainOutcome,
) {
    for effect in chain {
        match effect {
            // `github_signals.trigger_completion_comment`
            // (`bgtasks/github_signals.py:44-75`): best-effort, the save
            // stands. Django fires it from `Issue.save()`, before the
            // orchestration receiver.
            ChainEffect::CompletionIfChanged {
                issue_id,
                prev_state,
                current_state,
            } => {
                if prev_state != current_state {
                    transition
                        .enqueue_completion_comment(issue_id, prev_state, current_state)
                        .await;
                }
            }
            ChainEffect::FireTransition {
                issue_id,
                prev_state,
                current_state,
                spec,
                actor_id,
            } => {
                let request = FireRequest {
                    issue_id,
                    prev_state_id: prev_state,
                    current_state_id: current_state,
                    dispatch_immediate: spec.dispatch_immediate,
                    moved_by_run: None,
                    now: spec.now,
                    jitter_secs: 0.0,
                    created_by: spec.created_by,
                };
                match transition.fire(actor_id, request).await {
                    Ok(FireOutcome::Failed { log_line, .. }) => {
                        tracing::error!("{log_line}");
                    }
                    Ok(_) => {}
                    Err(()) => {
                        outcome.record(ChainStop::Fire);
                        break;
                    }
                }
            }
            ChainEffect::M2mInsert {
                table,
                member_column,
                ids,
                issue_id,
                project_id,
                workspace_id,
                created_by,
                updated_by,
                ignore_conflicts,
            } => {
                if let Err(error) = write_m2m_batches(
                    pool,
                    table,
                    member_column,
                    &ids,
                    &issue_id,
                    &project_id,
                    &workspace_id,
                    created_by,
                    updated_by,
                    ignore_conflicts,
                )
                .await
                {
                    tracing::warn!(%error, "post-commit m2m leg failed; response fails");
                    outcome.record(ChainStop::M2m);
                    break;
                }
            }
            ChainEffect::DefaultAssignee {
                default_id,
                issue_id,
                project_id,
                workspace_id,
                created_by,
            } => {
                if let Err(error) = write_default_assignee(
                    pool,
                    &default_id,
                    &issue_id,
                    &project_id,
                    &workspace_id,
                    created_by,
                )
                .await
                {
                    tracing::warn!(%error, "post-commit default-assignee leg failed; response fails");
                    outcome.record(ChainStop::M2m);
                    break;
                }
            }
            ChainEffect::IssueActivity(spec) => {
                let Some(origin) = spec.origin else {
                    outcome.record(ChainStop::Origin);
                    break;
                };
                let kwargs = issue_activity_kwargs(
                    &spec.event,
                    spec.requested,
                    &spec.actor,
                    &spec.issue,
                    &spec.project,
                    spec.current,
                    &origin,
                    spec.subscriber,
                );
                enqueue
                    .enqueue_task(ISSUE_ACTIVITY_TASK, vec![], kwargs)
                    .await;
            }
            ChainEffect::ModelActivity(spec) => {
                let Some(origin) = spec.origin else {
                    outcome.record(ChainStop::Origin);
                    break;
                };
                let kwargs = model_activity_kwargs(
                    &spec.model,
                    spec.requested,
                    spec.current,
                    &spec.actor,
                    &spec.slug,
                    &origin,
                );
                enqueue
                    .enqueue_task(MODEL_ACTIVITY_TASK, vec![], kwargs)
                    .await;
            }
            ChainEffect::VersionTask {
                updated_issue,
                issue_id,
                user_id,
                is_creating,
            } => {
                let kwargs = version_task_kwargs(&updated_issue, &issue_id, &user_id, is_creating);
                enqueue.enqueue_task(VERSION_TASK, vec![], kwargs).await;
            }
            ChainEffect::SoftDeleteTask { issue_id } => {
                let (args, kwargs) = soft_delete_args(&issue_id);
                enqueue.enqueue_task(SOFT_DELETE_TASK, args, kwargs).await;
            }
            ChainEffect::VisitDelete {
                project_id,
                slug,
                issue_id,
            } => {
                if let Err(error) = sqlx::query(RECENT_VISIT_DELETE_SQL)
                    .bind(project_id)
                    .bind(slug.as_str())
                    .bind(issue_id)
                    .execute(pool)
                    .await
                {
                    tracing::warn!(%error, "post-commit visit delete failed; response fails");
                    outcome.record(ChainStop::VisitDelete);
                    break;
                }
            }
        }
    }
}

fn transition_hook(txn: &WriteTxn) -> Arc<dyn IssueTransitionHook> {
    txn.hooks()
        .get::<dyn IssueTransitionHook>()
        .expect("driver registered IssueTransitionHook")
}

fn enqueue_hook(txn: &WriteTxn) -> Arc<dyn TaskEnqueueHook> {
    txn.hooks()
        .get::<dyn TaskEnqueueHook>()
        .expect("driver registered TaskEnqueueHook")
}

// ---------------------------------------------------------------------------
// Entity saves
// ---------------------------------------------------------------------------

/// What one entity save returns: the saved row (hook outputs included)
/// plus the queued chain's recorded outcome. Drivers check
/// [`ChainOutcome::stopped`] after [`run`](crate::domain::run) and map
/// any stop to `ServerError`, exactly the handlers' `?` arms.
pub struct SaveOutcome {
    pub issue: issues::Model,
    pub chain: ChainOutcome,
}

/// `POST .../issues/`: the assembled row, its m2m plan, the fire spec and
/// the created-activity parts. The origin resolves post-save (a `None`
/// stops the chain ahead of the activity); the driver calls
/// [`emit_model_activity`] and [`emit_version_task`] after its
/// response-row fetch.
pub struct CreateIssue {
    pub fields: IssueFieldSet,
    pub assignees: Vec<Uuid>,
    pub default_assignee: Option<Uuid>,
    pub labels: Vec<Uuid>,
    pub fire: FireSpec,
    pub origin: Option<String>,
    pub activity_requested: Value,
}

/// `POST .../issues/` (`base.py:397-485`): the atomic sequence/sort INSERT
/// (hooks), then the chain — transition fire, m2m + default-assignee legs,
/// created activity — in handler order. No completion comment: Django does
/// not fire `trigger_completion_comment` on create.
pub async fn create(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    pool: &PgPool,
    input: CreateIssue,
) -> Result<SaveOutcome, DomainError> {
    let transition = transition_hook(txn);
    let enqueue = enqueue_hook(txn);
    let actor = ctx.actor.user_id;
    let model = input
        .fields
        .into_insert_active_model(actor_or_none(ctx))
        .insert(&txn.connection())
        .await
        .map_err(DomainError::Db)?;
    let mut chain = vec![ChainEffect::FireTransition {
        issue_id: model.id,
        prev_state: None,
        current_state: model.state_id,
        spec: input.fire,
        actor_id: actor,
    }];
    // Empty/missing assignees + a project default → the fallback leg;
    // explicit ids → the batch leg (`serializers/issue.py:419-444`).
    if input.assignees.is_empty() {
        if let Some(default_id) = input.default_assignee {
            chain.push(ChainEffect::DefaultAssignee {
                default_id,
                issue_id: model.id,
                project_id: model.project_id,
                workspace_id: model.workspace_id,
                created_by: model.created_by_id,
            });
        }
    } else {
        chain.push(ChainEffect::M2mInsert {
            table: "issue_assignees",
            member_column: "assignee_id",
            ids: input.assignees,
            issue_id: model.id,
            project_id: model.project_id,
            workspace_id: model.workspace_id,
            created_by: model.created_by_id,
            updated_by: None,
            ignore_conflicts: false,
        });
    }
    if !input.labels.is_empty() {
        chain.push(ChainEffect::M2mInsert {
            table: "issue_labels",
            member_column: "label_id",
            ids: input.labels,
            issue_id: model.id,
            project_id: model.project_id,
            workspace_id: model.workspace_id,
            created_by: model.created_by_id,
            updated_by: None,
            ignore_conflicts: false,
        });
    }
    chain.push(ChainEffect::IssueActivity(IssueActivitySpec {
        event: "issue.activity.created".to_owned(),
        requested: input.activity_requested,
        actor,
        issue: model.id,
        project: model.project_id,
        current: Value::Null,
        origin: input.origin,
        subscriber: None,
    }));
    let outcome = ChainOutcome::new();
    let step = outcome.clone();
    let pool = pool.clone();
    txn.on_commit(move || async move {
        run_chain(&pool, &transition, &enqueue, chain, &step).await;
    });
    Ok(SaveOutcome {
        issue: model,
        chain: outcome,
    })
}

/// The create follow-ups the driver runs after its response-row fetch (a
/// miss 500s with these never enqueued): `model_activity` then the
/// creating version task. Plain pool-free calls over the enqueue hook —
/// no transaction, exactly the handlers' post-fetch order.
pub async fn emit_model_activity(enqueue: &Arc<dyn TaskEnqueueHook>, spec: ModelActivitySpec) {
    let Some(origin) = spec.origin else { return };
    let kwargs = model_activity_kwargs(
        &spec.model,
        spec.requested,
        spec.current,
        &spec.actor,
        &spec.slug,
        &origin,
    );
    enqueue
        .enqueue_task(MODEL_ACTIVITY_TASK, vec![], kwargs)
        .await;
}

/// See [`emit_model_activity`].
pub async fn emit_version_task(
    enqueue: &Arc<dyn TaskEnqueueHook>,
    updated_issue: &str,
    issue_id: &Uuid,
    user_id: &Uuid,
    is_creating: Option<bool>,
) {
    let kwargs = version_task_kwargs(updated_issue, issue_id, user_id, is_creating);
    enqueue.enqueue_task(VERSION_TASK, vec![], kwargs).await;
}

/// The shared full-save input: the post-overlay row, the present m2m keys
/// with their effective ids (`None` = key absent, leg skipped), the
/// pre-save state snapshot and the fire spec.
pub struct UpdateIssue {
    pub fields: IssueFieldSet,
    pub assignees: Option<Vec<Uuid>>,
    pub labels: Option<Vec<Uuid>>,
    pub prev_state: Option<Uuid>,
    pub fire: FireSpec,
}

/// `PUT .../issues/<pk>/`: m2m legs ahead of the row write (`update()`
/// pops them first), the full-column save, then completion-if-changed and
/// the always-immediate fire. No task enqueues.
pub async fn update(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    pool: &PgPool,
    input: UpdateIssue,
) -> Result<SaveOutcome, DomainError> {
    update_driver(
        ctx,
        txn,
        pool,
        input.fields,
        input.assignees,
        input.labels,
        input.prev_state,
        input.fire,
        None,
    )
    .await
}

/// The PATCH activity trio, built pre-save by the driver: the spaced body
/// dump, the pre-update detail dump (doubling as the version task's
/// `updated_issue`) and the raw body for `model_activity`. `None` is the
/// `skip_activity` description-only skip: no task at all.
pub struct UpdateActivity {
    pub origin: Option<String>,
    pub requested: String,
    pub current_instance: String,
    pub raw_body: Value,
    pub slug: String,
}

/// `PATCH .../issues/<pk>/` (`base.py:621-721`): like [`update`], plus
/// the activity trio unless skipped.
pub struct PartialIssue {
    pub fields: IssueFieldSet,
    pub assignees: Option<Vec<Uuid>>,
    pub labels: Option<Vec<Uuid>>,
    pub prev_state: Option<Uuid>,
    pub fire: FireSpec,
    pub activity: Option<UpdateActivity>,
}

pub async fn partial_update(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    pool: &PgPool,
    input: PartialIssue,
) -> Result<SaveOutcome, DomainError> {
    update_driver(
        ctx,
        txn,
        pool,
        input.fields,
        input.assignees,
        input.labels,
        input.prev_state,
        input.fire,
        input.activity,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn update_driver(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    pool: &PgPool,
    fields: IssueFieldSet,
    assignees: Option<Vec<Uuid>>,
    labels: Option<Vec<Uuid>>,
    prev_state: Option<Uuid>,
    fire: FireSpec,
    activity: Option<UpdateActivity>,
) -> Result<SaveOutcome, DomainError> {
    // The m2m legs run ahead of the row write (`update()` pops them
    // first), on the pool: they persist even when the save fails, so they
    // stay out of the transaction. Audit ids come from the pre-save row.
    let audit = (fields.created_by_id, fields.updated_by_id);
    if let Some(ids) = &assignees {
        update_m2m_leg(
            pool,
            "issue_assignees",
            "assignee_id",
            true,
            ids,
            &fields.id,
            &fields.project_id,
            &fields.workspace_id,
            audit,
        )
        .await?;
    }
    if let Some(ids) = &labels {
        update_m2m_leg(
            pool,
            "issue_labels",
            "label_id",
            true,
            ids,
            &fields.id,
            &fields.project_id,
            &fields.workspace_id,
            audit,
        )
        .await?;
    }
    let transition = transition_hook(txn);
    let enqueue = enqueue_hook(txn);
    let actor = ctx.actor.user_id;
    let model = fields
        .into_full_save_active_model(actor_or_none(ctx))
        .update(&txn.connection())
        .await
        .map_err(DomainError::Db)?;
    let mut chain = vec![
        ChainEffect::CompletionIfChanged {
            issue_id: model.id,
            prev_state,
            current_state: model.state_id,
        },
        ChainEffect::FireTransition {
            issue_id: model.id,
            prev_state,
            current_state: model.state_id,
            spec: fire,
            actor_id: actor,
        },
    ];
    if let Some(activity) = activity {
        chain.push(ChainEffect::IssueActivity(IssueActivitySpec {
            event: "issue.activity.updated".to_owned(),
            requested: Value::String(activity.requested),
            actor,
            issue: model.id,
            project: model.project_id,
            current: Value::String(activity.current_instance.clone()),
            origin: activity.origin.clone(),
            subscriber: None,
        }));
        chain.push(ChainEffect::ModelActivity(ModelActivitySpec {
            model: model.id,
            requested: activity.raw_body,
            current: Value::String(activity.current_instance.clone()),
            actor,
            slug: activity.slug,
            origin: activity.origin,
        }));
        chain.push(ChainEffect::VersionTask {
            updated_issue: activity.current_instance,
            issue_id: model.id,
            user_id: actor,
            is_creating: None,
        });
    }
    let outcome = ChainOutcome::new();
    let step = outcome.clone();
    let pool = pool.clone();
    txn.on_commit(move || async move {
        run_chain(&pool, &transition, &enqueue, chain, &step).await;
    });
    Ok(SaveOutcome {
        issue: model,
        chain: outcome,
    })
}

/// `DELETE .../issues/<pk>/` (`base.py:722-755`): `deleted_at` now, then
/// the full `save()` (the state legs run), the fire, the related-objects
/// forward, the hard recent-visit delete and the unsubscribed `deleted`
/// activity. Implied by the issue's "the `update_issue_row` full save"
/// (the third caller of the migrated save).
pub struct SoftDeleteIssue {
    pub fields: IssueFieldSet,
    pub prev_state: Option<Uuid>,
    pub fire: FireSpec,
    pub origin: Option<String>,
    pub slug: String,
}

pub async fn soft_delete(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    pool: &PgPool,
    input: SoftDeleteIssue,
) -> Result<SaveOutcome, DomainError> {
    let transition = transition_hook(txn);
    let enqueue = enqueue_hook(txn);
    let actor = ctx.actor.user_id;
    let mut fields = input.fields;
    fields.deleted_at = Some(now_micros());
    let model = fields
        .into_full_save_active_model(actor_or_none(ctx))
        .update(&txn.connection())
        .await
        .map_err(DomainError::Db)?;
    // `to_spaced_json({"issue_id": ...})` is a single key, so no
    // separator is ever involved: the `format!` is byte-identical and
    // infallible (UUIDs need no escaping).
    let requested = format!("{{\"issue_id\": \"{}\"}}", model.id);
    let chain = vec![
        ChainEffect::FireTransition {
            issue_id: model.id,
            prev_state: input.prev_state,
            current_state: model.state_id,
            spec: input.fire,
            actor_id: actor,
        },
        ChainEffect::SoftDeleteTask { issue_id: model.id },
        ChainEffect::VisitDelete {
            project_id: model.project_id,
            slug: input.slug,
            issue_id: model.id,
        },
        ChainEffect::IssueActivity(IssueActivitySpec {
            event: "issue.activity.deleted".to_owned(),
            requested: Value::String(requested),
            actor,
            issue: model.id,
            project: model.project_id,
            current: Value::Object(Map::new()),
            origin: input.origin,
            subscriber: Some(false),
        }),
    ];
    let outcome = ChainOutcome::new();
    let step = outcome.clone();
    let pool = pool.clone();
    txn.on_commit(move || async move {
        run_chain(&pool, &transition, &enqueue, chain, &step).await;
    });
    Ok(SaveOutcome {
        issue: model,
        chain: outcome,
    })
}

/// An activity enqueue that runs ahead of the row write (the archive
/// endpoints enqueue before saving): the event, the pre-built spaced
/// dumps, and the origin the driver resolved up front.
pub struct PreSaveActivity {
    pub event: String,
    pub requested: String,
    pub actor: Uuid,
    pub project: Uuid,
    pub current_instance: String,
    pub origin: String,
    pub subscriber: Option<bool>,
}

/// `POST .../issues/<pk>/archive/` (`archive.py:258`): the enqueue, the
/// `save()` write (archived + recomputes, no state change so no completion
/// leg), then the explicit post-save fire.
pub struct ArchiveIssue {
    pub issue_id: Uuid,
    pub project_id: Uuid,
    pub state_id: Option<Uuid>,
    pub description_html: String,
    pub archived_at: NaiveDate,
    pub updated_at: DateTime<Utc>,
    pub prev_state: Option<Uuid>,
    pub fire: FireSpec,
    pub pre_activity: PreSaveActivity,
}

pub async fn archive(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    pool: &PgPool,
    input: ArchiveIssue,
) -> Result<SaveOutcome, DomainError> {
    archive_driver(
        ctx,
        txn,
        pool,
        input.issue_id,
        input.project_id,
        input.state_id,
        input.description_html,
        Some(input.archived_at),
        input.updated_at,
        input.prev_state,
        input.fire,
        input.pre_activity,
        false,
    )
    .await
}

/// `DELETE .../issues/<pk>/archive/` (`archive.py:282`): the enqueue, the
/// `save()` write with the NULL-state default assignment, the completion
/// pair when the state moved, then the fire.
pub struct UnarchiveIssue {
    pub issue_id: Uuid,
    pub project_id: Uuid,
    pub state_id: Option<Uuid>,
    pub description_html: String,
    pub updated_at: DateTime<Utc>,
    pub prev_state: Option<Uuid>,
    pub fire: FireSpec,
    pub pre_activity: PreSaveActivity,
}

pub async fn unarchive(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    pool: &PgPool,
    input: UnarchiveIssue,
) -> Result<SaveOutcome, DomainError> {
    archive_driver(
        ctx,
        txn,
        pool,
        input.issue_id,
        input.project_id,
        input.state_id,
        input.description_html,
        None,
        input.updated_at,
        input.prev_state,
        input.fire,
        input.pre_activity,
        true,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn archive_driver(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    pool: &PgPool,
    issue_id: Uuid,
    project_id: Uuid,
    state_id: Option<Uuid>,
    description_html: String,
    archived_at: Option<NaiveDate>,
    updated_at: DateTime<Utc>,
    prev_state: Option<Uuid>,
    fire: FireSpec,
    pre_activity: PreSaveActivity,
    with_completion: bool,
) -> Result<SaveOutcome, DomainError> {
    let transition = transition_hook(txn);
    let enqueue = enqueue_hook(txn);
    let actor = ctx.actor.user_id;
    // The enqueue runs ahead of the row write, on the hook transport.
    let pre_kwargs = issue_activity_kwargs(
        &pre_activity.event,
        Value::String(pre_activity.requested),
        &pre_activity.actor,
        &issue_id,
        &pre_activity.project,
        Value::String(pre_activity.current_instance),
        &pre_activity.origin,
        pre_activity.subscriber,
    );
    enqueue
        .enqueue_task(ISSUE_ACTIVITY_TASK, vec![], pre_kwargs)
        .await;
    // The sparse save: only the archived flag, audit and hook recomputes
    // are `Set` — current values ride `Unchanged` for the hook reads.
    // Hook-`Set` fields join the `UPDATE`, so `completed_at` /
    // `description_stripped` (and the default state on the NULL leg) land
    // exactly the ported columns.
    let am = issues::ActiveModel {
        id: sea_orm::Unchanged(issue_id),
        project_id: sea_orm::Unchanged(project_id),
        state_id: sea_orm::Unchanged(state_id),
        description_html: sea_orm::Unchanged(description_html),
        archived_at: Set(archived_at),
        updated_at: Set(updated_at.into()),
        updated_by_id: Set(actor_or_none(ctx)),
        ..Default::default()
    };
    let model = am
        .update(&txn.connection())
        .await
        .map_err(DomainError::Db)?;
    let mut chain = vec![];
    if with_completion {
        chain.push(ChainEffect::CompletionIfChanged {
            issue_id: model.id,
            prev_state,
            current_state: model.state_id,
        });
    }
    chain.push(ChainEffect::FireTransition {
        issue_id: model.id,
        prev_state,
        current_state: model.state_id,
        spec: fire,
        actor_id: actor,
    });
    let outcome = ChainOutcome::new();
    let step = outcome.clone();
    let pool = pool.clone();
    txn.on_commit(move || async move {
        run_chain(&pool, &transition, &enqueue, chain, &step).await;
    });
    Ok(SaveOutcome {
        issue: model,
        chain: outcome,
    })
}

// ---------------------------------------------------------------------------
// Bulk writes (pool-based: no `save()`, no signals, autocommit)
// ---------------------------------------------------------------------------

/// One bulk-archive row: the per-row activity parts the driver read
/// pre-save.
pub struct BulkArchiveRow {
    pub issue_id: Uuid,
    pub project_id: Uuid,
    pub actor_id: Uuid,
    pub current_instance: String,
    pub origin: String,
}

/// `POST .../bulk-archive-issues/` (`archive.py:306-344`): each row's
/// `issue_activity` runs inside the driver's per-row loop (a later row's
/// gate failure 400s with the earlier rows' tasks already enqueued), and
/// the single `bulk_update(["archived_at"])` runs after — no `save()`,
/// no recomputes, no fire. Pool-based (autocommit), like the port.
pub async fn bulk_archive_activity(
    enqueue: &Arc<dyn TaskEnqueueHook>,
    row: &BulkArchiveRow,
    today: NaiveDate,
) {
    // `json.dumps({"archived_at": str(date), "automation": False})`
    // with default separators — the literal the port builds.
    let requested = format!("{{\"archived_at\": \"{today}\", \"automation\": false}}");
    let kwargs = issue_activity_kwargs(
        "issue.activity.updated",
        Value::String(requested),
        &row.actor_id,
        &row.issue_id,
        &row.project_id,
        Value::String(row.current_instance.clone()),
        &row.origin,
        None,
    );
    enqueue
        .enqueue_task(ISSUE_ACTIVITY_TASK, vec![], kwargs)
        .await;
}

/// See [`bulk_archive_activity`]: the single `archived_at` UPDATE.
pub async fn bulk_archive_update(
    pool: &PgPool,
    ids: &[Uuid],
    today: NaiveDate,
) -> Result<(), DomainError> {
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query("UPDATE issues SET archived_at = $1 WHERE id = ANY($2)")
        .bind(today)
        .bind(ids)
        .execute(pool)
        .await
        .map_err(|error| DomainError::Work(format!("bulk archive failed: {error}")))?;
    Ok(())
}

/// `DELETE .../bulk-delete-issues/` (`base.py:783-809`): the queryset
/// `delete()` issues leg — one scoped `deleted_at` UPDATE, no `save()`,
/// no fire, no activity. Pool-based (autocommit), like the port. The
/// cycle/module link legs stay with the driver (not covered tables).
pub async fn bulk_soft_delete_issues(
    pool: &PgPool,
    ids: &[Uuid],
    project_id: &Uuid,
    slug: &str,
) -> Result<(), DomainError> {
    if ids.is_empty() {
        return Ok(());
    }
    let placeholders = (0..ids.len())
        .map(|index| format!("${}", index + 2))
        .collect::<Vec<_>>()
        .join(", ");
    // The issues leg reuses the `issue_objects` scope (it soft-deletes
    // only in-scope rows; out-of-scope ids still count as misses).
    // Comma-join, not JOIN..ON: Postgres forbids referencing the UPDATE
    // target from a FROM-clause ON expression.
    let project_bind = ids.len() + 2;
    let slug_bind = ids.len() + 3;
    let sql = format!(
        "UPDATE issues AS issue SET deleted_at = $1 \
         FROM projects AS project, workspaces AS workspace \
         WHERE issue.deleted_at IS NULL AND issue.id IN ({placeholders}) \
         AND issue.project_id = ${project_bind} AND issue.project_id = project.id \
         AND issue.workspace_id = workspace.id AND workspace.slug = ${slug_bind} \
         AND NOT EXISTS (SELECT 1 FROM states AS state \
         WHERE state.id = issue.state_id AND state.deleted_at IS NULL \
         AND state.\"group\" = 'triage') \
         AND issue.archived_at IS NULL AND project.archived_at IS NULL \
         AND issue.is_draft = FALSE"
    );
    let mut query = sqlx::query(&sql).bind(now_micros());
    for id in ids {
        query = query.bind(id);
    }
    query = query.bind(project_id).bind(slug);
    query
        .execute(pool)
        .await
        .map_err(|error| DomainError::Work(format!("bulk soft-delete failed: {error}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;

    use crate::orchestration::entries::FireRequest;

    #[test]
    fn spaced_json_uses_dumps_separators() {
        let mut map = Map::new();
        map.insert("b".to_owned(), Value::from(1));
        map.insert(
            "a".to_owned(),
            Value::Array(vec![Value::from(1), Value::from(2)]),
        );
        assert_eq!(
            to_spaced_json(&Value::Object(map)).as_deref(),
            Some("{\"b\": 1, \"a\": [1, 2]}")
        );
        assert_eq!(
            to_spaced_json(&Value::String("x".to_owned())).as_deref(),
            Some("\"x\"")
        );
    }

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn issue_activity_kwargs_carry_every_field() {
        let kwargs = issue_activity_kwargs(
            "issue.activity.updated",
            Value::String("req".to_owned()),
            &uid(1),
            &uid(2),
            &uid(3),
            Value::String("cur".to_owned()),
            "https://app.example",
            Some(false),
        );
        assert_eq!(
            kwargs.get("type").and_then(Value::as_str),
            Some("issue.activity.updated")
        );
        assert_eq!(
            kwargs.get("requested_data").and_then(Value::as_str),
            Some("req")
        );
        assert_eq!(
            kwargs.get("actor_id").and_then(Value::as_str),
            Some(uid(1).to_string()).as_deref()
        );
        assert_eq!(
            kwargs.get("issue_id").and_then(Value::as_str),
            Some(uid(2).to_string()).as_deref()
        );
        assert_eq!(
            kwargs.get("project_id").and_then(Value::as_str),
            Some(uid(3).to_string()).as_deref()
        );
        assert_eq!(
            kwargs.get("current_instance").and_then(Value::as_str),
            Some("cur")
        );
        assert!(kwargs.get("epoch").and_then(Value::as_i64).unwrap_or(0) > 0);
        assert_eq!(
            kwargs.get("notification").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            kwargs.get("origin").and_then(Value::as_str),
            Some("https://app.example")
        );
        assert_eq!(
            kwargs.get("subscriber").and_then(Value::as_bool),
            Some(false)
        );
    }

    #[test]
    fn issue_activity_kwargs_omit_absent_subscriber() {
        let kwargs = issue_activity_kwargs(
            "issue.activity.created",
            Value::Null,
            &uid(1),
            &uid(2),
            &uid(3),
            Value::Null,
            "https://app.example",
            None,
        );
        assert!(!kwargs.contains_key("subscriber"));
        assert!(kwargs.get("requested_data").unwrap().is_null());
    }

    #[test]
    fn model_activity_kwargs_keep_raw_requested_object() {
        let raw = serde_json::json!({"name": "n", "state": null});
        let kwargs = model_activity_kwargs(
            &uid(2),
            raw.clone(),
            Value::String("cur".to_owned()),
            &uid(1),
            "ws",
            "https://app.example",
        );
        assert_eq!(
            kwargs.get("model_name").and_then(Value::as_str),
            Some("issue")
        );
        assert_eq!(kwargs.get("requested_data"), Some(&raw));
        assert_eq!(kwargs.get("slug").and_then(Value::as_str), Some("ws"));
    }

    #[test]
    fn version_task_kwargs_omit_is_creating_on_update() {
        let update = version_task_kwargs("u", &uid(2), &uid(1), None);
        assert!(!update.contains_key("is_creating"));
        let create = version_task_kwargs("u", &uid(2), &uid(1), Some(true));
        assert_eq!(
            create.get("is_creating").and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn soft_delete_args_are_positional_plus_using() {
        let (args, kwargs) = soft_delete_args(&uid(2));
        assert_eq!(
            args,
            vec![
                Value::String("db".to_owned()),
                Value::String("issue".to_owned()),
                Value::String(uid(2).to_string()),
            ]
        );
        assert_eq!(kwargs.get("using"), Some(&Value::Null));
    }

    #[test]
    fn task_names_are_the_celery_wire_names() {
        assert_eq!(
            VERSION_TASK,
            "pi_dash.bgtasks.issue_description_version_task.issue_description_version_task"
        );
        assert_eq!(
            SOFT_DELETE_TASK,
            "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"
        );
    }

    #[test]
    fn actor_or_none_maps_nil_to_missing_crum() {
        let ctx = WriteCtx::new(super::super::ctx::WriteActor {
            user_id: uid(7),
            timezone: None,
        });
        assert_eq!(actor_or_none(&ctx), Some(uid(7)));
        assert_eq!(actor_or_none(&WriteCtx::default()), None);
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

    type EnqueueCall = (String, Vec<Value>, Map<String, Value>);

    struct RecordingEnqueue {
        calls: Mutex<Vec<EnqueueCall>>,
    }

    impl TaskEnqueueHook for RecordingEnqueue {
        fn enqueue_task(
            &self,
            task: &str,
            args: Vec<Value>,
            kwargs: Map<String, Value>,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            self.calls
                .lock()
                .expect("lock")
                .push((task.to_owned(), args, kwargs));
            Box::pin(async move {})
        }
    }

    fn lazy_pool() -> PgPool {
        PgPool::connect_lazy("postgres://127.0.0.1:1/unused").expect("lazy pool")
    }

    fn fire_spec() -> FireSpec {
        FireSpec {
            dispatch_immediate: true,
            created_by: Some(uid(9)),
            now: Utc::now(),
        }
    }

    #[tokio::test]
    async fn chain_runs_in_order_and_records_success() {
        let pool = lazy_pool();
        let transition = Arc::new(RecordingTransition {
            comments: Mutex::new(Vec::new()),
            fires: Mutex::new(Vec::new()),
            fire_result: Mutex::new(Ok(FireOutcome::NoTransition)),
        });
        let erased_transition: Arc<dyn IssueTransitionHook> = transition.clone();
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });
        let erased_enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
        let outcome = ChainOutcome::new();
        run_chain(
            &pool,
            &erased_transition,
            &erased_enqueue,
            vec![
                ChainEffect::CompletionIfChanged {
                    issue_id: uid(2),
                    prev_state: Some(uid(10)),
                    current_state: Some(uid(11)),
                },
                ChainEffect::FireTransition {
                    issue_id: uid(2),
                    prev_state: Some(uid(10)),
                    current_state: Some(uid(11)),
                    spec: fire_spec(),
                    actor_id: uid(1),
                },
                ChainEffect::IssueActivity(IssueActivitySpec {
                    event: "issue.activity.updated".to_owned(),
                    requested: Value::Null,
                    actor: uid(1),
                    issue: uid(2),
                    project: uid(3),
                    current: Value::Null,
                    origin: Some("https://app.example".to_owned()),
                    subscriber: None,
                }),
            ],
            &outcome,
        )
        .await;
        assert!(outcome.stopped().is_none());
        assert_eq!(
            *transition.comments.lock().expect("lock"),
            vec![(uid(2), Some(uid(10)), Some(uid(11)))]
        );
        assert_eq!(transition.fires.lock().expect("lock").len(), 1);
        let calls = enqueue.calls.lock().expect("lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, ISSUE_ACTIVITY_TASK);
    }

    #[tokio::test]
    async fn completion_guard_skips_equal_states() {
        let pool = lazy_pool();
        let transition = Arc::new(RecordingTransition {
            comments: Mutex::new(Vec::new()),
            fires: Mutex::new(Vec::new()),
            fire_result: Mutex::new(Ok(FireOutcome::NoTransition)),
        });
        let erased_transition: Arc<dyn IssueTransitionHook> = transition.clone();
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });
        let erased_enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
        let outcome = ChainOutcome::new();
        run_chain(
            &pool,
            &erased_transition,
            &erased_enqueue,
            vec![ChainEffect::CompletionIfChanged {
                issue_id: uid(2),
                prev_state: Some(uid(10)),
                current_state: Some(uid(10)),
            }],
            &outcome,
        )
        .await;
        assert!(outcome.stopped().is_none());
        assert!(transition.comments.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn fire_failure_stops_the_chain() {
        let pool = lazy_pool();
        let transition = Arc::new(RecordingTransition {
            comments: Mutex::new(Vec::new()),
            fires: Mutex::new(Vec::new()),
            fire_result: Mutex::new(Err(())),
        });
        let erased_transition: Arc<dyn IssueTransitionHook> = transition.clone();
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });
        let erased_enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
        let outcome = ChainOutcome::new();
        run_chain(
            &pool,
            &erased_transition,
            &erased_enqueue,
            vec![
                ChainEffect::FireTransition {
                    issue_id: uid(2),
                    prev_state: None,
                    current_state: Some(uid(11)),
                    spec: fire_spec(),
                    actor_id: uid(1),
                },
                ChainEffect::IssueActivity(IssueActivitySpec {
                    event: "issue.activity.created".to_owned(),
                    requested: Value::Null,
                    actor: uid(1),
                    issue: uid(2),
                    project: uid(3),
                    current: Value::Null,
                    origin: Some("https://app.example".to_owned()),
                    subscriber: None,
                }),
            ],
            &outcome,
        )
        .await;
        assert_eq!(outcome.stopped(), Some(ChainStop::Fire));
        assert!(enqueue.calls.lock().expect("lock").is_empty());
    }

    async fn live_pool() -> Option<PgPool> {
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

    /// End-to-end through `domain::run` with recording hooks: create assigns
    /// the hook outputs and drains its chain, then a state-changing partial
    /// update re-saves, re-runs the m2m leg and drains the trio. The app
    /// contract suite cannot reach this path while the detail renderers
    /// are red at the base commit, so this test pins the service directly.
    #[tokio::test]
    async fn create_then_partial_update_roundtrip() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let db = pidash_db::sea_orm_connection(&pool);
        let actor = Uuid::new_v4();
        let assignee = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let backlog_id = Uuid::new_v4();
        let completed_id = Uuid::new_v4();
        for user_id in [actor, assignee] {
            let username = format!("svc-{}", user_id.simple());
            sqlx::query(
                "INSERT INTO users (id, password, username, email, first_name, last_name, \
                 avatar, date_joined, created_at, updated_at, last_location, created_location, \
                 is_superuser, is_managed, is_password_expired, is_active, is_staff, \
                 is_email_verified, is_password_autoset, token, user_timezone, last_login_ip, \
                 last_logout_ip, last_login_medium, last_login_uagent, is_bot, display_name, \
                 is_email_valid, is_password_reset_required) VALUES ($1, '!', $2, $3, 'CT', \
                 'User', '', now(), now(), now(), '', '', false, false, false, true, false, \
                 true, false, $4, 'UTC', '', '', '', '', false, $2, true, false)",
            )
            .bind(user_id)
            .bind(username.clone())
            .bind(format!("{username}@example.test"))
            .bind("t".repeat(64))
            .execute(&pool)
            .await
            .expect("seed user");
        }
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, slug, timezone, \
             background_color, owner_id) VALUES ($1, now(), now(), 'svc ws', $2, 'UTC', \
             '#ffffff', $3)",
        )
        .bind(workspace_id)
        .bind(format!("svc-{workspace_id}"))
        .bind(actor)
        .execute(&pool)
        .await
        .expect("seed workspace");
        sqlx::query(
            "INSERT INTO projects (id, name, description, network, identifier, workspace_id, \
             created_by_id, cycle_view, module_view, issue_views_view, page_view, intake_view, \
             archive_in, close_in, logo_props, is_time_tracking_enabled, is_issue_type_enabled, \
             guest_view_all_features, timezone, members_can_edit_states, repo_url, base_branch, \
             agent_default_interval_seconds, agent_default_max_ticks, agent_ticking_enabled, \
             is_default, agent_review_default_interval_seconds, default_agent_executor, \
             agent_test_default_interval_seconds, created_at, updated_at) VALUES ($1, \
             'svc project', '', 0, $2, $3, $4, true, true, true, true, false, 0, 0, '{}', true, \
             true, false, 'UTC', false, '', '', 0, 0, false, false, 0, '', 0, now(), now())",
        )
        .bind(project_id)
        .bind(format!(
            "S{:08X}",
            (project_id.as_u128() & 0xffff_ffff) as u32
        ))
        .bind(workspace_id)
        .bind(actor)
        .execute(&pool)
        .await
        .expect("seed project");
        for (id, name, group, sequence, default) in [
            (backlog_id, "Backlog", "backlog", 100.0, true),
            (completed_id, "Done", "completed", 200.0, false),
        ] {
            sqlx::query(
                "INSERT INTO states (id, created_at, updated_at, name, description, color, \
                 slug, project_id, workspace_id, sequence, \"group\", \"default\", is_triage) \
                 VALUES ($1, now(), now(), $2, '', '#ff0000', $3, $4, $5, $6, $7, $8, false)",
            )
            .bind(id)
            .bind(name)
            .bind(format!("svc-{name}-{sequence}-{project_id}"))
            .bind(project_id)
            .bind(workspace_id)
            .bind(sequence)
            .bind(group)
            .bind(default)
            .execute(&pool)
            .await
            .expect("seed state");
        }

        let ctx = WriteCtx::new(super::super::ctx::WriteActor {
            user_id: actor,
            timezone: Some("UTC".to_owned()),
        });
        let transition = Arc::new(RecordingTransition {
            comments: Mutex::new(Vec::new()),
            fires: Mutex::new(Vec::new()),
            fire_result: Mutex::new(Ok(FireOutcome::NoTransition)),
        });
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });

        let fields = IssueFieldSet {
            id: Uuid::new_v4(),
            created_at: now_micros(),
            updated_at: now_micros(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id,
            workspace_id,
            parent_id: None,
            state_id: Some(backlog_id),
            point: None,
            estimate_point_id: None,
            name: "svc issue".to_owned(),
            description_json: Value::Object(Map::new()),
            description_html: "<p>Hi</p>".to_owned(),
            description_stripped: None,
            description_binary: None,
            priority: "none".to_owned(),
            complexity_score: 0,
            start_date: None,
            target_date: None,
            sequence_id: 0,
            sort_order: 65535.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            type_id: None,
            git_work_branch: String::new(),
            workpad: String::new(),
            created_via: None,
            assigned_pod_id: None,
            agent_executor: None,
        };
        let issue_id = fields.id;
        let created = super::super::run(&ctx, &db, |txn| {
            let transition = transition.clone();
            let enqueue = enqueue.clone();
            let pool = pool.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                let transition_hook: Arc<dyn IssueTransitionHook> = transition;
                let enqueue_hook: Arc<dyn TaskEnqueueHook> = enqueue;
                txn.hooks_mut().register(transition_hook);
                txn.hooks_mut().register(enqueue_hook);
                create(
                    &ctx,
                    txn,
                    &pool,
                    CreateIssue {
                        fields,
                        assignees: vec![assignee],
                        default_assignee: None,
                        labels: vec![],
                        fire: FireSpec {
                            dispatch_immediate: true,
                            created_by: Some(actor),
                            now: now_micros(),
                        },
                        origin: Some("https://app.example".to_owned()),
                        activity_requested: Value::String("{\"name\": \"svc issue\"}".to_owned()),
                    },
                )
                .await
            })
        })
        .await
        .expect("domain create");
        assert!(created.chain.stopped().is_none());
        assert_eq!(created.issue.sequence_id, 1);
        assert_eq!(created.issue.sort_order, 65535.0);
        assert_eq!(created.issue.description_stripped.as_deref(), Some("Hi"));
        assert_eq!(created.issue.created_by_id, Some(actor));
        assert!(created.issue.completed_at.is_none());
        // The m2m leg wrote one live assignee link.
        let links: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM issue_assignees WHERE issue_id = $1 AND assignee_id = $2 \
             AND deleted_at IS NULL",
        )
        .bind(issue_id)
        .bind(assignee)
        .fetch_one(&pool)
        .await
        .expect("assignee read");
        assert_eq!(links.0, 1);
        // Create fires the transition but never the completion comment.
        assert_eq!(transition.fires.lock().expect("lock").len(), 1);
        assert!(transition.comments.lock().expect("lock").is_empty());
        assert_eq!(
            enqueue
                .calls
                .lock()
                .expect("lock")
                .iter()
                .map(|call| call.0.clone())
                .collect::<Vec<_>>(),
            vec![ISSUE_ACTIVITY_TASK.to_owned()]
        );

        let mut patched = IssueFieldSet::from(&created.issue);
        patched.name = "svc issue v2".to_owned();
        patched.state_id = Some(completed_id);
        patched.updated_at = now_micros();
        let updated = super::super::run(&ctx, &db, |txn| {
            let transition = transition.clone();
            let enqueue = enqueue.clone();
            let pool = pool.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                let transition_hook: Arc<dyn IssueTransitionHook> = transition;
                let enqueue_hook: Arc<dyn TaskEnqueueHook> = enqueue;
                txn.hooks_mut().register(transition_hook);
                txn.hooks_mut().register(enqueue_hook);
                partial_update(
                    &ctx,
                    txn,
                    &pool,
                    PartialIssue {
                        fields: patched,
                        assignees: Some(vec![assignee]),
                        labels: None,
                        prev_state: Some(backlog_id),
                        fire: FireSpec {
                            dispatch_immediate: true,
                            created_by: Some(actor),
                            now: now_micros(),
                        },
                        activity: Some(UpdateActivity {
                            origin: Some("https://app.example".to_owned()),
                            requested: "{\"priority\": \"urgent\"}".to_owned(),
                            current_instance: "{\"name\": \"svc issue\"}".to_owned(),
                            raw_body: serde_json::json!({"priority": "urgent"}),
                            slug: "svc-ws".to_owned(),
                        }),
                    },
                )
                .await
            })
        })
        .await
        .expect("domain partial update");
        assert!(updated.chain.stopped().is_none());
        assert_eq!(updated.issue.name, "svc issue v2");
        assert!(updated.issue.completed_at.is_some());
        assert_eq!(updated.issue.updated_by_id, Some(actor));
        // The state move fired completion + transition, then the trio.
        assert_eq!(
            *transition.comments.lock().expect("lock"),
            vec![(issue_id, Some(backlog_id), Some(completed_id))]
        );
        assert_eq!(transition.fires.lock().expect("lock").len(), 2);
        assert_eq!(
            enqueue
                .calls
                .lock()
                .expect("lock")
                .iter()
                .map(|call| call.0.clone())
                .collect::<Vec<_>>(),
            vec![
                ISSUE_ACTIVITY_TASK.to_owned(),
                ISSUE_ACTIVITY_TASK.to_owned(),
                MODEL_ACTIVITY_TASK.to_owned(),
                VERSION_TASK.to_owned(),
            ]
        );
        // The re-asserted assignee leg left exactly one live link.
        let links: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM issue_assignees WHERE issue_id = $1 AND deleted_at IS NULL",
        )
        .bind(issue_id)
        .fetch_one(&pool)
        .await
        .expect("assignee reread");
        assert_eq!(links.0, 1);

        // Cleanup in FK order (the chains committed on the pool).
        for sql in [
            "DELETE FROM issue_assignees WHERE issue_id = $1",
            "DELETE FROM issue_labels WHERE issue_id = $1",
            "DELETE FROM issue_sequences WHERE issue_id = $1",
            "DELETE FROM issues WHERE id = $1",
        ] {
            sqlx::query(sql)
                .bind(issue_id)
                .execute(&pool)
                .await
                .expect("cleanup issue rows");
        }
        sqlx::query("DELETE FROM states WHERE project_id = $1")
            .bind(project_id)
            .execute(&pool)
            .await
            .expect("cleanup states");
        sqlx::query("DELETE FROM projects WHERE id = $1")
            .bind(project_id)
            .execute(&pool)
            .await
            .expect("cleanup project");
        sqlx::query("DELETE FROM workspaces WHERE id = $1")
            .bind(workspace_id)
            .execute(&pool)
            .await
            .expect("cleanup workspace");
        for user_id in [actor, assignee] {
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(user_id)
                .execute(&pool)
                .await
                .expect("cleanup user");
        }
    }

    #[tokio::test]
    async fn missing_origin_stops_ahead_of_activity() {
        let pool = lazy_pool();
        let transition = Arc::new(RecordingTransition {
            comments: Mutex::new(Vec::new()),
            fires: Mutex::new(Vec::new()),
            fire_result: Mutex::new(Ok(FireOutcome::NoTransition)),
        });
        let erased_transition: Arc<dyn IssueTransitionHook> = transition.clone();
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });
        let erased_enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
        let outcome = ChainOutcome::new();
        run_chain(
            &pool,
            &erased_transition,
            &erased_enqueue,
            vec![
                ChainEffect::FireTransition {
                    issue_id: uid(2),
                    prev_state: None,
                    current_state: Some(uid(11)),
                    spec: fire_spec(),
                    actor_id: uid(1),
                },
                ChainEffect::IssueActivity(IssueActivitySpec {
                    event: "issue.activity.created".to_owned(),
                    requested: Value::Null,
                    actor: uid(1),
                    issue: uid(2),
                    project: uid(3),
                    current: Value::Null,
                    origin: None,
                    subscriber: None,
                }),
            ],
            &outcome,
        )
        .await;
        // The fire ran (the save + fire persist past the origin failure),
        // the activity never did.
        assert_eq!(outcome.stopped(), Some(ChainStop::Origin));
        assert_eq!(transition.fires.lock().expect("lock").len(), 1);
        assert!(enqueue.calls.lock().expect("lock").is_empty());
    }
}
