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
//!   `archive`, `unarchive`, `set_workpad`) run the row write on the
//!   caller's [`WriteTxn`]
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
use pidash_db::entity::{
    issue_assignees, issue_labels, issue_sequences, issues, project_members, projects, states,
    user_recent_visits, workspaces,
};
use pidash_db::orm_tx::{into_sqlx, is_integrity_violation};
use sea_orm::sea_query::{Alias, Expr, LockType, OnConflict, Query, SelectStatement};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QuerySelect,
    QueryTrait, Set,
};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::activity::{TaskEnqueueHook, ISSUE_ACTIVITY_TASK, MODEL_ACTIVITY_TASK};
use super::ctx::WriteCtx;
use super::error::DomainError;
use super::issues_move::MoveCreationOutboxes;
use super::transition::IssueTransitionHook;
use super::txn::WriteTxn;

pub use super::issues_move::{
    move_to_project, CreationDeps, MoveAdmissionCache, MoveDomainOutcome, MoveDrainError,
    MoveInput, MovePostCommitHook, MoveRejection,
};
use crate::app_issues::issue_move::{
    issue_activity_kwargs as move_issue_activity_kwargs, move_model_activity_kwargs,
};
pub use crate::app_issues::issue_move::{MoveGuards, MoveOptions};
use crate::app_issues::serializers_create::m2m_batches;
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

/// The two m2m bridge tables the issue legs write (`IssueAssignee` /
/// `IssueLabel`): same columns, a different member column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum M2mTable {
    Assignees,
    Labels,
}

impl M2mTable {
    fn name(self) -> &'static str {
        match self {
            Self::Assignees => "issue_assignees",
            Self::Labels => "issue_labels",
        }
    }
}

/// The audit and scope columns every row of one m2m write shares.
#[derive(Debug, Clone, Copy)]
struct M2mScope {
    issue_id: Uuid,
    project_id: Uuid,
    workspace_id: Uuid,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
}

/// One multi-row m2m `INSERT` (one `bulk_create` chunk): per row a fresh
/// uuid4 and two `now()` calls (`auto_now_add` + `auto_now`, like the
/// per-object instantiation Django bulk-saves), `deleted_at` NULL, the
/// shared audit/scope columns and the member id. `ignore_conflicts` (the
/// update legs) adds Django's bare `ON CONFLICT DO NOTHING`.
fn m2m_insert_statement(
    table: M2mTable,
    members: &[Uuid],
    scope: M2mScope,
    ignore_conflicts: bool,
) -> sea_orm::sea_query::InsertStatement {
    let mut stmt = match table {
        M2mTable::Assignees => {
            issue_assignees::Entity::insert_many(members.iter().map(|&member| {
                let created = now_micros();
                let updated = now_micros();
                issue_assignees::ActiveModel {
                    id: Set(Uuid::new_v4()),
                    created_at: Set(created.into()),
                    updated_at: Set(updated.into()),
                    created_by_id: Set(scope.created_by_id),
                    updated_by_id: Set(scope.updated_by_id),
                    deleted_at: Set(None),
                    project_id: Set(scope.project_id),
                    workspace_id: Set(scope.workspace_id),
                    issue_id: Set(scope.issue_id),
                    assignee_id: Set(member),
                }
            }))
            .into_query()
        }
        M2mTable::Labels => issue_labels::Entity::insert_many(members.iter().map(|&member| {
            let created = now_micros();
            let updated = now_micros();
            issue_labels::ActiveModel {
                id: Set(Uuid::new_v4()),
                created_at: Set(created.into()),
                updated_at: Set(updated.into()),
                created_by_id: Set(scope.created_by_id),
                updated_by_id: Set(scope.updated_by_id),
                deleted_at: Set(None),
                project_id: Set(scope.project_id),
                workspace_id: Set(scope.workspace_id),
                issue_id: Set(scope.issue_id),
                label_id: Set(member),
            }
        }))
        .into_query(),
    };
    if ignore_conflicts {
        stmt.on_conflict(OnConflict::new().do_nothing().to_owned());
    }
    stmt
}

/// One m2m `bulk_create(batch_size=10)` leg: a multi-row INSERT per
/// 10-chunk in input order; an `IntegrityError` swallows the REST of the
/// batches (`except: pass` wraps the whole call), any other error fails.
/// Moved verbatim from the app handlers; runs on any SeaORM connection
/// (drivers pass the pool, tests a rolled-back transaction).
async fn write_m2m_batches(
    db: &impl ConnectionTrait,
    table: M2mTable,
    ids: &[Uuid],
    scope: M2mScope,
    ignore_conflicts: bool,
) -> Result<(), DomainError> {
    for batch in m2m_batches(ids) {
        let stmt = m2m_insert_statement(table, batch, scope, ignore_conflicts);
        match db.execute(db.get_database_backend().build(&stmt)).await {
            Ok(_) => {}
            Err(error) if is_integrity_violation(&error) => break,
            Err(error) => {
                return Err(DomainError::Work(format!(
                    "m2m {} write failed: {error}",
                    table.name()
                )));
            }
        }
    }
    Ok(())
}

/// `ProjectMember.objects.filter(member_id=default, project_id, role>=15,
/// is_active).exists()` (`serializers/issue.py:423-430`), default
/// soft-delete guard included: the default-assignee gate.
fn default_assignee_member_select(default_assignee_id: Uuid, project_id: Uuid) -> SelectStatement {
    project_members::Entity::find()
        .select_only()
        .column(project_members::Column::Id)
        .filter(project_members::Column::MemberId.eq(default_assignee_id))
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::Role.gte(15))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .limit(1)
        .into_query()
}

/// The create default-assignee fallback (`serializers/issue.py:419-444`):
/// empty/missing assignees + a valid project-default member → one
/// `IssueAssignee` row, `IntegrityError` swallowed. Moved verbatim.
async fn write_default_assignee(
    db: &impl ConnectionTrait,
    default_assignee_id: Uuid,
    scope: M2mScope,
) -> Result<(), DomainError> {
    let member = db
        .query_one(
            db.get_database_backend()
                .build(&default_assignee_member_select(
                    default_assignee_id,
                    scope.project_id,
                )),
        )
        .await
        .map_err(|error| DomainError::Work(format!("default-assignee check failed: {error}")))?;
    if member.is_none() {
        return Ok(());
    }
    let stmt = m2m_insert_statement(M2mTable::Assignees, &[default_assignee_id], scope, false);
    match db.execute(db.get_database_backend().build(&stmt)).await {
        Ok(_) => Ok(()),
        Err(error) if is_integrity_violation(&error) => Ok(()),
        Err(error) => Err(DomainError::Work(format!(
            "default-assignee write failed: {error}"
        ))),
    }
}

/// `IssueAssignee` / `IssueLabel` `.objects.filter(issue).delete()`
/// (`:475`, `:496`): a SOFT delete — `SoftDeletionQuerySet.delete`
/// defaults to `soft=True` (`db/mixins.py:48-52`), i.e. `UPDATE … SET
/// deleted_at`, scoped by the default manager (`deleted_at IS NULL`).
/// `updated_at` is NOT touched (queryset `update()` skips `auto_now`).
async fn clear_m2m_links(
    db: &impl ConnectionTrait,
    table: M2mTable,
    issue_id: Uuid,
    deleted_at: DateTime<Utc>,
) -> Result<u64, DbErr> {
    let done = match table {
        M2mTable::Assignees => {
            issue_assignees::Entity::update_many()
                .col_expr(issue_assignees::Column::DeletedAt, Expr::value(deleted_at))
                .filter(issue_assignees::Column::IssueId.eq(issue_id))
                .filter(issue_assignees::Column::DeletedAt.is_null())
                .exec(db)
                .await?
        }
        M2mTable::Labels => {
            issue_labels::Entity::update_many()
                .col_expr(issue_labels::Column::DeletedAt, Expr::value(deleted_at))
                .filter(issue_labels::Column::IssueId.eq(issue_id))
                .filter(issue_labels::Column::DeletedAt.is_null())
                .exec(db)
                .await?
        }
    };
    Ok(done.rows_affected)
}

/// One update m2m leg (`serializers/issue.py:477-511`): present keys
/// soft-delete the live links, then re-insert the effective ids with
/// `ON CONFLICT DO NOTHING`; audit ids come from the PRE-SAVE instance.
async fn update_m2m_leg(
    db: &impl ConnectionTrait,
    table: M2mTable,
    effective: &[Uuid],
    scope: M2mScope,
) -> Result<(), DomainError> {
    clear_m2m_links(db, table, scope.issue_id, now_micros())
        .await
        .map_err(|error| {
            DomainError::Work(format!("m2m {} clear failed: {error}", table.name()))
        })?;
    write_m2m_batches(db, table, effective, scope, true).await
}

// ---------------------------------------------------------------------------
// Activity kwargs (pure builders; the transport is the [`TaskEnqueueHook`])
// ---------------------------------------------------------------------------

/// `issue_activity.delay(type, requested_data, actor_id, issue_id,
/// project_id, current_instance, epoch[, notification][, origin][,
/// subscriber])`. The app views always pass `notification=True` plus an
/// origin (`app/views/issue/base.py:414-423`); the v1 views pass neither
/// (`api/views/issue.py:516-525,844-853`) — hence `None` omits the key.
/// The chain (not this builder) decides whether a missing origin stops
/// the chain (app) or merely omits the key (v1); see
/// [`IssueActivitySpec::require_origin`].
#[allow(clippy::too_many_arguments)]
fn issue_activity_kwargs(
    activity_type: &str,
    requested_data: Value,
    actor_id: &Uuid,
    issue_id: &Uuid,
    project_id: &Uuid,
    current_instance: Value,
    notification: Option<bool>,
    origin: Option<&str>,
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
    if let Some(notification) = notification {
        kwargs.insert("notification".to_owned(), Value::Bool(notification));
    }
    if let Some(origin) = origin {
        kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    }
    if let Some(subscriber) = subscriber {
        kwargs.insert("subscriber".to_owned(), Value::Bool(subscriber));
    }
    kwargs
}

/// `model_activity.delay(...)` for the issue domain legs: the shared
/// [`crate::v1_projects::tasks::model_activity_kwargs`] kernel with
/// `model_name="issue"`. `requested_data` is the RAW object (never
/// JSON-encoded at these sites); the UUIDs ride as hyphenated strings,
/// exactly as Celery's encoder would carry the UUID objects; the
/// `current_instance` `Value` is `Null` or a string at every producer
/// (verified), so `.as_str()` maps it exactly (PIDASHCONV-1623).
fn model_activity_kwargs(
    model_id: &Uuid,
    requested_data: Value,
    current_instance: Value,
    actor_id: &Uuid,
    slug: &str,
    origin: &str,
) -> Map<String, Value> {
    crate::v1_projects::tasks::model_activity_kwargs(
        "issue",
        &model_id.to_string(),
        requested_data,
        current_instance.as_str(),
        &actor_id.to_string(),
        slug,
        Some(origin),
    )
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
    /// The v1 create override `UPDATE` hit an integrity violation — the
    /// port's `save_error` 400 arm (`PAYLOAD_NOT_VALID_BODY`).
    OverrideConflict,
    /// The v1 create override `UPDATE` failed any other way — `ServerError`.
    OverrideFailed,
    /// A v1 create override coercion failed — the port's constant
    /// `BadError("Please provide valid detail")` 400 arm.
    OverrideDetail,
    /// A move's handoff-creation outbox drain failed — the driver's
    /// `drain_creation_outboxes` `?` (`ServerError`, move standing).
    CreationDrain,
    /// A move's source-pod drain failed — the driver's `run_post_commit`
    /// `?` (`ServerError`, move standing).
    PodDrain,
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

    pub(crate) fn record(&self, stop: ChainStop) {
        *self.stop.lock().expect("chain outcome lock") = Some(stop);
    }
}

/// The post-save transition fire arguments (one `fire_after_save` call).
/// `moved_by_run` is the resolved `X-Pi-Dash-Run-Id` run on the v1 patch
/// path (`views/issue.py:794-807`); every other driver passes `None`.
#[derive(Debug, Clone)]
pub struct FireSpec {
    pub dispatch_immediate: bool,
    pub created_by: Option<Uuid>,
    pub moved_by_run: Option<Uuid>,
    pub now: DateTime<Utc>,
}

/// An `issue_activity` enqueue with its origin pre-resolved by the driver
/// at the original point (`base_host` raises past the save). `notification`
/// is `Some(true)` on the app paths and `None` (key omitted) on the v1
/// paths; `require_origin` keeps the app stop-on-`None` while the v1 legs
/// enqueue with the key omitted.
#[derive(Debug, Clone)]
pub struct IssueActivitySpec {
    pub event: String,
    pub requested: Value,
    pub actor: Uuid,
    pub issue: Uuid,
    pub project: Uuid,
    pub current: Value,
    pub notification: Option<bool>,
    pub origin: Option<String>,
    pub require_origin: bool,
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
pub(crate) enum ChainEffect {
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
        table: M2mTable,
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
    CreatedOverride {
        issue_id: Uuid,
        created_at: Result<Option<DateTime<Utc>>, ChainStop>,
        created_by: Result<Option<Uuid>, ChainStop>,
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
    /// A move's handoff-creation outbox drain (first, before the move's
    /// own post-commit work — the driver's `take_creation_outboxes`
    /// block). Fallible: [`ChainStop::CreationDrain`].
    CreationDrain {
        outboxes: MoveCreationOutboxes,
        terminal_created_by: Option<Uuid>,
    },
    /// A move's cancel send (`MovePostCommit::SendCancel`): best-effort
    /// with logged warnings, never stops the chain.
    SendCancel {
        runner_id: Uuid,
        run_id: Uuid,
    },
    /// A move's source-pod drain (`MovePostCommit::DrainPod`).
    /// Fallible: [`ChainStop::PodDrain`].
    DrainPod {
        pod_id: Uuid,
    },
    /// A move's `issue_activity` enqueue, built by the move's own kwargs
    /// builder (frozen-epoch quirk included — not the domain builder's
    /// chain-time epoch). Best-effort, like every enqueue leg.
    MoveIssueActivity {
        event: String,
        requested: String,
        actor: Uuid,
        issue: Uuid,
        project: Uuid,
        current: String,
        epoch: i64,
    },
    /// A move's `model_activity` enqueue, built by the move's own kwargs
    /// builder. Best-effort, like every enqueue leg. `origin` is `None`
    /// for callers with no request host (the MCP layer), rendering JSON
    /// null — never skipped, exactly Django's `origin=None`.
    MoveModelActivity {
        model: Uuid,
        requested: Value,
        current: String,
        actor: Uuid,
        slug: String,
        origin: Option<String>,
    },
}

/// The destroy recent-visit delete: a HARD delete scoped to the
/// workspace slug + project + issue (the slug's workspace by subquery,
/// the `DELETE … USING workspaces` join's rows).
fn recent_visit_delete(
    project_id: Uuid,
    slug: &str,
    issue_id: Uuid,
) -> sea_orm::DeleteMany<user_recent_visits::Entity> {
    user_recent_visits::Entity::delete_many()
        .filter(
            user_recent_visits::Column::WorkspaceId.in_subquery(
                Query::select()
                    .column(workspaces::Column::Id)
                    .from(workspaces::Entity)
                    .and_where(workspaces::Column::Slug.eq(slug))
                    .to_owned(),
            ),
        )
        .filter(user_recent_visits::Column::ProjectId.eq(project_id))
        .filter(user_recent_visits::Column::EntityIdentifier.eq(issue_id))
        .filter(user_recent_visits::Column::EntityName.eq("issue"))
}

/// The v1 create raw-data overrides (`api/views/issue.py:505-509`), moved
/// verbatim: a second save limited to `update_fields=["created_at",
/// "created_by"]` on the refetched row. It stays a chain leg between the
/// m2m writes and the activity fan-out (it cannot fold into the INSERT:
/// the fire's entry-run creation reads the pre-override `created_by_id`
/// from the row). Bare columns: no hooks, `updated_at` untouched.
fn created_override_update(
    issue_id: Uuid,
    created_at: Option<DateTime<Utc>>,
    created_by: Option<Uuid>,
) -> sea_orm::UpdateMany<issues::Entity> {
    issues::Entity::update_many()
        .col_expr(issues::Column::CreatedAt, Expr::value(created_at))
        .col_expr(issues::Column::CreatedById, Expr::value(created_by))
        .filter(issues::Column::Id.eq(issue_id))
}

pub(crate) async fn run_chain(
    pool: &PgPool,
    transition: &Arc<dyn IssueTransitionHook>,
    enqueue: &Arc<dyn TaskEnqueueHook>,
    drain: Option<&Arc<dyn MovePostCommitHook>>,
    chain: Vec<ChainEffect>,
    outcome: &ChainOutcome,
) {
    let db = pidash_db::pool::sea_orm_connection(pool);
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
                    moved_by_run: spec.moved_by_run,
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
                ids,
                issue_id,
                project_id,
                workspace_id,
                created_by,
                updated_by,
                ignore_conflicts,
            } => {
                let scope = M2mScope {
                    issue_id,
                    project_id,
                    workspace_id,
                    created_by_id: created_by,
                    updated_by_id: updated_by,
                };
                if let Err(error) =
                    write_m2m_batches(&db, table, &ids, scope, ignore_conflicts).await
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
                let scope = M2mScope {
                    issue_id,
                    project_id,
                    workspace_id,
                    created_by_id: created_by,
                    updated_by_id: None,
                };
                if let Err(error) = write_default_assignee(&db, default_id, scope).await {
                    tracing::warn!(%error, "post-commit default-assignee leg failed; response fails");
                    outcome.record(ChainStop::M2m);
                    break;
                }
            }
            ChainEffect::CreatedOverride {
                issue_id,
                created_at,
                created_by,
            } => {
                // The coercions resolved pre-save (pure), but their errors
                // surface here, in port order — a bad override 400s/500s
                // with the issue, fire and m2m legs already persisted.
                let created_at = match created_at {
                    Ok(value) => value,
                    Err(stop) => {
                        outcome.record(stop);
                        break;
                    }
                };
                let created_by = match created_by {
                    Ok(value) => value,
                    Err(stop) => {
                        outcome.record(stop);
                        break;
                    }
                };
                match created_override_update(issue_id, created_at, created_by)
                    .exec(&db)
                    .await
                {
                    Ok(_) => {}
                    Err(error) if is_integrity_violation(&error) => {
                        outcome.record(ChainStop::OverrideConflict);
                        break;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "post-commit created-override failed; response fails");
                        outcome.record(ChainStop::OverrideFailed);
                        break;
                    }
                }
            }
            ChainEffect::IssueActivity(spec) => {
                let origin = match spec.origin {
                    Some(origin) => Some(origin),
                    None if spec.require_origin => {
                        outcome.record(ChainStop::Origin);
                        break;
                    }
                    None => None,
                };
                let kwargs = issue_activity_kwargs(
                    &spec.event,
                    spec.requested,
                    &spec.actor,
                    &spec.issue,
                    &spec.project,
                    spec.current,
                    spec.notification,
                    origin.as_deref(),
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
                if let Err(error) = recent_visit_delete(project_id, &slug, issue_id)
                    .exec(&db)
                    .await
                {
                    tracing::warn!(%error, "post-commit visit delete failed; response fails");
                    outcome.record(ChainStop::VisitDelete);
                    break;
                }
            }
            ChainEffect::CreationDrain {
                outboxes,
                terminal_created_by,
            } => {
                let drain = drain.expect("move chain registered MovePostCommitHook");
                if let Err(error) = drain.drain_creation(outboxes, terminal_created_by).await {
                    tracing::warn!(%error, "post-commit creation drain failed; response fails");
                    outcome.record(ChainStop::CreationDrain);
                    break;
                }
            }
            ChainEffect::SendCancel { runner_id, run_id } => {
                let drain = drain.expect("move chain registered MovePostCommitHook");
                drain.send_cancel(runner_id, run_id).await;
            }
            ChainEffect::DrainPod { pod_id } => {
                let drain = drain.expect("move chain registered MovePostCommitHook");
                if let Err(error) = drain.drain_pod(pod_id).await {
                    tracing::warn!(%error, "post-commit pod drain failed; response fails");
                    outcome.record(ChainStop::PodDrain);
                    break;
                }
            }
            ChainEffect::MoveIssueActivity {
                event,
                requested,
                actor,
                issue,
                project,
                current,
                epoch,
            } => {
                let spec = move_issue_activity_kwargs(
                    &event, &requested, &actor, &issue, &project, &current, epoch,
                );
                enqueue
                    .enqueue_task(spec.task, spec.args, spec.kwargs)
                    .await;
            }
            ChainEffect::MoveModelActivity {
                model,
                requested,
                current,
                actor,
                slug,
                origin,
            } => {
                let spec = move_model_activity_kwargs(
                    &model,
                    requested,
                    &current,
                    &actor,
                    &slug,
                    origin.as_deref(),
                );
                enqueue
                    .enqueue_task(spec.task, spec.args, spec.kwargs)
                    .await;
            }
        }
    }
}

pub(crate) fn transition_hook(txn: &WriteTxn) -> Arc<dyn IssueTransitionHook> {
    txn.hooks()
        .get::<dyn IssueTransitionHook>()
        .expect("driver registered IssueTransitionHook")
}

pub(crate) fn enqueue_hook(txn: &WriteTxn) -> Arc<dyn TaskEnqueueHook> {
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

/// The v1 create raw-data overrides (`api/views/issue.py:505-509`): the
/// driver-resolved coercions of the raw `created_at` / `created_by` (key
/// absent: now / the actor). A `Result` because the coercions are pure
/// but their failures surface post-commit, in port order, with the row,
/// fire and m2m legs already persisted — the `Err` carries the
/// pre-mapped [`ChainStop`] (`OverrideDetail` for the constant 400,
/// `OverrideFailed` for the 500 arms).
#[derive(Debug, Clone)]
pub struct CreatedOverride {
    pub created_at: Result<Option<DateTime<Utc>>, ChainStop>,
    pub created_by: Result<Option<Uuid>, ChainStop>,
}

/// `POST .../issues/`: the assembled row, its m2m plan, the fire spec and
/// the created-activity parts. The origin resolves post-save (a `None`
/// stops the chain ahead of the activity on the app path); the app driver
/// calls [`emit_model_activity`] and [`emit_version_task`] after its
/// response-row fetch, while the v1 driver queues its model activity in
/// the chain (`model_activity`) and its raw-data overrides ahead of the
/// activity (`created_override`) — Django runs both before the render.
/// `activity_notification` / `activity_require_origin` select the
/// `issue_activity` key shape: `Some(true)` / `true` on the app path,
/// `None` / `false` on the v1 path (keys omitted).
pub struct CreateIssue {
    pub fields: IssueFieldSet,
    pub assignees: Vec<Uuid>,
    pub default_assignee: Option<Uuid>,
    pub labels: Vec<Uuid>,
    pub fire: FireSpec,
    pub origin: Option<String>,
    pub activity_requested: Value,
    pub activity_notification: Option<bool>,
    pub activity_require_origin: bool,
    /// Skip the created activity: drivers whose view enqueues its own
    /// shaped activity (the intake `intake=` extra kwarg) keep that leg
    /// driver-side and pass `true`.
    pub skip_activity: bool,
    /// Skip the creation fire: drivers that drive their own post-commit
    /// fire (the agent tools' bespoke `fire_create_transition`) pass
    /// `true` so the chain stays empty.
    pub skip_fire: bool,
    pub model_activity: Option<ModelActivitySpec>,
    pub created_override: Option<CreatedOverride>,
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
    let mut chain = Vec::new();
    if !input.skip_fire {
        chain.push(ChainEffect::FireTransition {
            issue_id: model.id,
            prev_state: None,
            current_state: model.state_id,
            spec: input.fire,
            actor_id: actor,
        });
    }
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
            table: M2mTable::Assignees,
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
            table: M2mTable::Labels,
            ids: input.labels,
            issue_id: model.id,
            project_id: model.project_id,
            workspace_id: model.workspace_id,
            created_by: model.created_by_id,
            updated_by: None,
            ignore_conflicts: false,
        });
    }
    // The v1 raw-data overrides land between the m2m writes and the
    // activity fan-out (`api/views/issue.py:505-525`); the app path has
    // none. The v1 `issue_activity` omits `notification`/`origin`
    // (`api/views/issue.py:516-525`); the app leg keeps both plus the
    // stop-on-`None` origin.
    if let Some(over) = input.created_override {
        chain.push(ChainEffect::CreatedOverride {
            issue_id: model.id,
            created_at: over.created_at,
            created_by: over.created_by,
        });
    }
    if !input.skip_activity {
        chain.push(ChainEffect::IssueActivity(IssueActivitySpec {
            event: "issue.activity.created".to_owned(),
            requested: input.activity_requested,
            actor,
            issue: model.id,
            project: model.project_id,
            current: Value::Null,
            notification: input.activity_notification,
            origin: input.origin,
            require_origin: input.activity_require_origin,
            subscriber: None,
        }));
    }
    if let Some(spec) = input.model_activity {
        chain.push(ChainEffect::ModelActivity(spec));
    }
    let outcome = ChainOutcome::new();
    let step = outcome.clone();
    let pool = pool.clone();
    txn.on_commit(move || async move {
        run_chain(&pool, &transition, &enqueue, None, chain, &step).await;
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
/// pre-save state snapshot and the fire spec. `m2m_audit` is the PRE-save
/// row's audit pair: Django reads it off the instance before assigning
/// the validated values (`serializers/issue.py:375-379`), so a driver
/// whose overlay can move `created_by` passes the fetched values, not the
/// overlaid ones.
pub struct UpdateIssue {
    pub fields: IssueFieldSet,
    pub assignees: Option<Vec<Uuid>>,
    pub labels: Option<Vec<Uuid>>,
    pub m2m_audit: (Option<Uuid>, Option<Uuid>),
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
        input.m2m_audit,
        None,
        input.prev_state,
        input.fire,
        None,
    )
    .await
}

/// The PATCH activity fan-out, built pre-save by the driver: the spaced
/// body dump, the pre-update detail dump (doubling as the version task's
/// `updated_issue`) and the raw body for `model_activity`. `None` is the
/// `skip_activity` description-only skip: no task at all. The v1 views
/// enqueue no version task (`api/views/issue.py:844-865`) and omit the
/// `issue_activity` notification/origin keys — `emit_version`,
/// `notification` and `require_origin` carry those per-view shapes.
pub struct UpdateActivity {
    pub origin: Option<String>,
    pub requested: String,
    pub current_instance: String,
    pub raw_body: Value,
    pub slug: String,
    pub notification: Option<bool>,
    pub require_origin: bool,
    pub emit_version: bool,
}

/// `PATCH .../issues/<pk>/` (`base.py:621-721`): like [`update`], plus
/// the activity trio unless skipped.
pub struct PartialIssue {
    pub fields: IssueFieldSet,
    pub assignees: Option<Vec<Uuid>>,
    pub labels: Option<Vec<Uuid>>,
    pub m2m_audit: (Option<Uuid>, Option<Uuid>),
    pub sequence: Option<Result<i32, ()>>,
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
        input.m2m_audit,
        input.sequence,
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
    mut fields: IssueFieldSet,
    assignees: Option<Vec<Uuid>>,
    labels: Option<Vec<Uuid>>,
    m2m_audit: (Option<Uuid>, Option<Uuid>),
    sequence: Option<Result<i32, ()>>,
    prev_state: Option<Uuid>,
    fire: FireSpec,
    activity: Option<UpdateActivity>,
) -> Result<SaveOutcome, DomainError> {
    // The m2m legs run ahead of the row write (`update()` pops them
    // first), on the pool: they persist even when the save fails, so they
    // stay out of the transaction. Audit ids come from the pre-save row.
    let scope = M2mScope {
        issue_id: fields.id,
        project_id: fields.project_id,
        workspace_id: fields.workspace_id,
        created_by_id: m2m_audit.0,
        updated_by_id: m2m_audit.1,
    };
    let db = pidash_db::pool::sea_orm_connection(pool);
    if let Some(ids) = &assignees {
        update_m2m_leg(&db, M2mTable::Assignees, ids, scope).await?;
    }
    if let Some(ids) = &labels {
        update_m2m_leg(&db, M2mTable::Labels, ids, scope).await?;
    }
    // The save-time `sequence_id` int4 check (DRF never range-checks the
    // integer — the `DataError` surfaces at save, past the m2m legs, as a
    // 500). The driver pre-computes the coercion; `None` (key absent, or a
    // driver that validated up front) keeps the assembled value.
    if let Some(checked) = sequence {
        fields.sequence_id =
            checked.map_err(|()| DomainError::Work("sequence_id out of int4 range".to_owned()))?;
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
            notification: activity.notification,
            origin: activity.origin.clone(),
            require_origin: activity.require_origin,
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
        if activity.emit_version {
            chain.push(ChainEffect::VersionTask {
                updated_issue: activity.current_instance,
                issue_id: model.id,
                user_id: actor,
                is_creating: None,
            });
        }
    }
    let outcome = ChainOutcome::new();
    let step = outcome.clone();
    let pool = pool.clone();
    txn.on_commit(move || async move {
        run_chain(&pool, &transition, &enqueue, None, chain, &step).await;
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
/// `DELETE ...`: the fetched row (the driver stamps `updated_at`;
/// `deleted_at` rides the fields when the driver owns that clock too —
/// v1 stamps it separately — else the domain stamps now), the pre-save
/// snapshot, the fire spec and the per-view shapes: the v1 delete runs
/// the completion pair and skips the recent-visit delete, while the app
/// delete does the reverse; the `deleted` activity keys follow the same
/// app/v1 split as every other activity leg. (`completion: false` on the
/// app path preserves a pre-existing divergence: Django's post_save
/// receiver runs the pair on every delete save,
/// `bgtasks/github_signals.py:44-75`; the app port never has. Tracked
/// separately; the migration keeps both behaviors.)
pub struct SoftDeleteIssue {
    pub fields: IssueFieldSet,
    pub prev_state: Option<Uuid>,
    pub fire: FireSpec,
    pub origin: Option<String>,
    pub slug: String,
    pub completion: bool,
    pub visit_delete: bool,
    pub activity_current: Value,
    pub activity_notification: Option<bool>,
    pub activity_require_origin: bool,
    pub activity_subscriber: Option<bool>,
    /// Skip the deleted activity: drivers whose view enqueues no
    /// `issue.activity.deleted` (the intake destroy) pass `true`.
    pub skip_activity: bool,
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
    if fields.deleted_at.is_none() {
        fields.deleted_at = Some(now_micros());
    }
    let model = fields
        .into_full_save_active_model(actor_or_none(ctx))
        .update(&txn.connection())
        .await
        .map_err(DomainError::Db)?;
    // `to_spaced_json({"issue_id": ...})` is a single key, so no
    // separator is ever involved: the `format!` is byte-identical to the
    // v1 `django_dumps` of the same map, and infallible (UUIDs need no
    // escaping) — one `requested` serves both views.
    let requested = format!("{{\"issue_id\": \"{}\"}}", model.id);
    let mut chain = Vec::with_capacity(5);
    if input.completion {
        chain.push(ChainEffect::CompletionIfChanged {
            issue_id: model.id,
            prev_state: input.prev_state,
            current_state: model.state_id,
        });
    }
    chain.push(ChainEffect::FireTransition {
        issue_id: model.id,
        prev_state: input.prev_state,
        current_state: model.state_id,
        spec: input.fire,
        actor_id: actor,
    });
    chain.push(ChainEffect::SoftDeleteTask { issue_id: model.id });
    if input.visit_delete {
        chain.push(ChainEffect::VisitDelete {
            project_id: model.project_id,
            slug: input.slug,
            issue_id: model.id,
        });
    }
    if !input.skip_activity {
        chain.push(ChainEffect::IssueActivity(IssueActivitySpec {
            event: "issue.activity.deleted".to_owned(),
            requested: Value::String(requested),
            actor,
            issue: model.id,
            project: model.project_id,
            current: input.activity_current,
            notification: input.activity_notification,
            origin: input.origin,
            require_origin: input.activity_require_origin,
            subscriber: input.activity_subscriber,
        }));
    }
    let outcome = ChainOutcome::new();
    let step = outcome.clone();
    let pool = pool.clone();
    txn.on_commit(move || async move {
        run_chain(&pool, &transition, &enqueue, None, chain, &step).await;
    });
    Ok(SaveOutcome {
        issue: model,
        chain: outcome,
    })
}

/// The workpad-PATCH issue read (`api/views/issue.py:3307-3311`):
/// `LIMIT 21` (executed `.get()`) + `FOR UPDATE OF issues`
/// (`of=("self",)` — a bare lock would 500 on the nullable state join).
/// `states` is a LEFT join, `projects` / `workspaces` INNER. The
/// triage/archived/draft predicates are load-bearing (a miss 404s); the
/// projected `state_group` goes unused — the save hook re-reads the group
/// itself, in the same transaction and snapshot. Public because the
/// driver probes existence with the SAME statement on the pool before
/// validating (a miss 404s ahead of validation errors, as today) — one
/// builder, so the probe and the lock can never disagree on the match.
/// The `FOR UPDATE` is harmless in the driver's autocommit probe (the
/// lock releases at statement end).
pub fn workpad_select(issue_id: Uuid, project_id: Uuid, slug: &str) -> SelectStatement {
    let state_group = Expr::col((states::Entity, states::Column::Group));
    Query::select()
        .column((issues::Entity, issues::Column::DescriptionHtml))
        .column((issues::Entity, issues::Column::StateId))
        .column((issues::Entity, issues::Column::CompletedAt))
        .expr_as(state_group.clone(), Alias::new("state_group"))
        .from(issues::Entity)
        .left_join(
            states::Entity,
            Expr::col((issues::Entity, issues::Column::StateId))
                .equals((states::Entity, states::Column::Id)),
        )
        .inner_join(
            projects::Entity,
            Expr::col((issues::Entity, issues::Column::ProjectId))
                .equals((projects::Entity, projects::Column::Id)),
        )
        .inner_join(
            workspaces::Entity,
            Expr::col((issues::Entity, issues::Column::WorkspaceId))
                .equals((workspaces::Entity, workspaces::Column::Id)),
        )
        .and_where(Expr::col((issues::Entity, issues::Column::DeletedAt)).is_null())
        .and_where(
            state_group
                .clone()
                .eq("triage")
                .and(state_group.is_not_null())
                .not(),
        )
        .and_where(Expr::col((issues::Entity, issues::Column::ArchivedAt)).is_null())
        .and_where(Expr::col((projects::Entity, projects::Column::ArchivedAt)).is_null())
        .and_where(Expr::col((issues::Entity, issues::Column::IsDraft)).not())
        .and_where(Expr::col((issues::Entity, issues::Column::Id)).eq(issue_id))
        .and_where(Expr::col((issues::Entity, issues::Column::ProjectId)).eq(project_id))
        .and_where(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug))
        .limit(21)
        .lock_with_tables(LockType::Update, [issues::Entity])
        .to_owned()
}

/// `PATCH .../workpad/`: the issue, project and slug for the locked read,
/// the validated workpad text and the driver-stamped `updated_at` (the
/// response renders the stamp, so the driver owns the clock).
pub struct SetWorkpad {
    pub issue_id: Uuid,
    pub project_id: Uuid,
    pub slug: String,
    pub workpad: String,
    pub updated_at: DateTime<Utc>,
}

/// `PATCH .../issues/<pk>/workpad/` (`api/views/issue.py:3287-3320`): the
/// locked read plus the sparse `save()` (workpad, audit and hook
/// recomputes) inside the domain transaction, then the completion pair
/// when the state moved. No orchestration fire — the port has none.
///
/// The driver probes existence with the same `SELECT` on the pool BEFORE
/// validating (a miss 404s ahead of validation errors, as today), then
/// calls this after validation; a lock-read miss here (a mid-flight
/// delete) is `RecordNotFound`, the drivers' `NotFound` arm.
pub async fn set_workpad(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    pool: &PgPool,
    input: SetWorkpad,
) -> Result<ChainOutcome, DomainError> {
    let row = txn
        .connection()
        .query_one(sea_orm::DbBackend::Postgres.build(&workpad_select(
            input.issue_id,
            input.project_id,
            &input.slug,
        )))
        .await?
        .ok_or_else(|| DomainError::Db(DbErr::RecordNotFound("workpad issue".to_owned())))?;
    let description_html: String = row.try_get("", "description_html")?;
    let prev_state: Option<Uuid> = row.try_get("", "state_id")?;
    let completed_at: Option<sea_orm::prelude::DateTimeWithTimeZone> =
        row.try_get("", "completed_at")?;
    // The sparse save: only the workpad, audit and hook recomputes move —
    // current values ride `Unchanged` for the hook reads (the archive
    // shape). Hook-`Set` fields join the `UPDATE`, so `completed_at` /
    // `description_stripped` (and the default state on the NULL leg) land
    // exactly the ported columns.
    let model = issues::ActiveModel {
        id: sea_orm::Unchanged(input.issue_id),
        project_id: sea_orm::Unchanged(input.project_id),
        workpad: Set(input.workpad),
        description_html: sea_orm::Unchanged(description_html),
        state_id: sea_orm::Unchanged(prev_state),
        completed_at: sea_orm::Unchanged(completed_at),
        updated_at: Set(input.updated_at.into()),
        updated_by_id: Set(actor_or_none(ctx)),
        ..Default::default()
    }
    .update(&txn.connection())
    .await?;
    let transition = transition_hook(txn);
    let enqueue = enqueue_hook(txn);
    let outcome = ChainOutcome::new();
    let step = outcome.clone();
    let pool = pool.clone();
    let chain = vec![ChainEffect::CompletionIfChanged {
        issue_id: model.id,
        prev_state,
        current_state: model.state_id,
    }];
    txn.on_commit(move || async move {
        run_chain(&pool, &transition, &enqueue, None, chain, &step).await;
    });
    Ok(outcome)
}

/// The agent-tool workpad write (`tools.py:300-307`): a `QuerySet.update`
/// equivalent — bare columns, no hooks, no chain. Contrast
/// [`set_workpad`], which runs the save (hook recomputes plus the
/// completion leg) for the v1 workpad endpoint. Runs on any SeaORM connection so
/// audit-transaction drivers keep the write inside their transaction.
pub async fn update_workpad(
    db: &impl ConnectionTrait,
    issue_id: Uuid,
    project_id: Uuid,
    workpad: &str,
    updated_at: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    issues::Entity::update_many()
        .col_expr(issues::Column::Workpad, Expr::value(workpad))
        .col_expr(issues::Column::UpdatedAt, Expr::value(updated_at))
        .filter(issues::Column::Id.eq(issue_id))
        .filter(issues::Column::ProjectId.eq(project_id))
        .filter(issues::Column::DeletedAt.is_null())
        .exec(db)
        .await
        .map(|done| done.rows_affected)
        .map_err(into_sqlx)
}

/// `set_workpad` write (`orchestration/workpad.py:79-84`):
/// `update_fields=["workpad", "updated_at"]`, rendered in `_meta`
/// order per the fixture (`updated_at`, then `workpad`).
fn workpad_save_update(
    issue_id: Uuid,
    body: Option<&str>,
    now: DateTime<Utc>,
) -> sea_orm::UpdateMany<issues::Entity> {
    issues::Entity::update_many()
        .col_expr(issues::Column::UpdatedAt, Expr::value(now))
        .col_expr(issues::Column::Workpad, Expr::value(body.unwrap_or("")))
        .filter(issues::Column::Id.eq(issue_id))
}

/// Overwrite the workpad body; `None`/empty clears it
/// (`body or ""`). `now` stands in for the `auto_now` stamp. A
/// missing row is a silent no-op (Python's `save()` would attempt an
/// `INSERT` fallback there — unreachable from live callers and
/// deliberately not recreated). Contrast [`set_workpad`], which runs
/// the PATCH-flow save (hook recomputes plus the completion leg),
/// and [`update_workpad`], the agent-tool `QuerySet.update` shape —
/// this is the orchestration `save(update_fields)` shape. Runs on any
/// SeaORM connection. Moved from
/// `pidash_db::orchestration::workpad::set_workpad` (PIDASHCONV-1630).
pub async fn update_workpad_save(
    db: &impl ConnectionTrait,
    issue_id: Uuid,
    body: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    workpad_save_update(issue_id, body, now)
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// The agent-tool transition save (`tools.py:334-335`): a
/// `save(update_fields=["state", "updated_at", "updated_by"])`
/// equivalent — the three columns land with no hook-recompute writes
/// (`completed_at`/`description_stripped` stay untouched, exactly what
/// the field list writes) and no chain: the driver routes completion,
/// fire and orchestration itself, as today. Runs on any SeaORM connection so
/// audit-transaction drivers keep the write inside their transaction
/// (and under their row lock — a separate domain transaction would
/// deadlock against it).
pub async fn update_issue_state(
    db: &impl ConnectionTrait,
    issue_id: Uuid,
    state_id: Uuid,
    updated_at: DateTime<Utc>,
    updated_by_id: Uuid,
) -> Result<u64, sqlx::Error> {
    issues::Entity::update_many()
        .col_expr(issues::Column::StateId, Expr::value(state_id))
        .col_expr(issues::Column::UpdatedAt, Expr::value(updated_at))
        .col_expr(issues::Column::UpdatedById, Expr::value(updated_by_id))
        .filter(issues::Column::Id.eq(issue_id))
        .exec(db)
        .await
        .map(|done| done.rows_affected)
        .map_err(into_sqlx)
}

/// N3 Issue INSERT (`views/intake.py:145-152`). `$1` = id, `$2` =
/// created_at, `$3` = updated_at, `$4` = name, `$5` = description_json, `$6`
/// = description_html, `$7` = priority, `$8` = project id, `$9` = triage
/// state id. Python-side fallbacks (`:146-149`):
/// description_json `{}` / description_html `"<p></p>"` / priority `"low"`
/// (NOT the validated `"none"` — QUIRK-priority-default); a present-but-null
/// key passes `None` through. No `workspace_id` kwarg
/// (QUIRK-no-workspace-insert). Moved verbatim from
/// `space::queries::intake_assets` (PIDASHCONV-1630): `issues` is a
/// covered table, so the builder lives in domain/.
pub fn issue_insert_sql() -> String {
    "INSERT INTO \"issues\" (\"id\", \"created_at\", \"updated_at\", \"name\", \"description_json\", \"description_html\", \"priority\", \"project_id\", \"state_id\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)".to_string()
}

/// The sync-mirror full-row INSERT both providers share
/// (`Issue.objects.create`, `git_sync_task.py:92-97`; the
/// `update_or_create` create arm, `github_sync_task.py:135-140`, via the
/// `Issue.save()` creation path `issue.py:267-340`): pod/state/sequence/
/// sort-order resolved driver-side, then one 34-column row. The audit
/// columns carry the actor directly — the two Python writes (the create,
/// whose audit `BaseModel.save` clobbers to NULL in the worker, plus the
/// follow-up `filter(pk).update(created_by, updated_by)`
/// `git_sync_task.py:108-112` / `github_sync_task.py:156-160`, collapse
/// into this one statement with identical final bytes.
///
/// Bare columns, no hooks, no chain: the driver holds its scan `sqlx`
/// transaction around the issue, sequence and sync rows (per-issue
/// atomicity), and a separate domain transaction would split that unit —
/// the same in-transaction justification as [`update_workpad`]. The sync
/// create fires no signal legs (`state is None` on the git arm, so the
/// orchestration receiver sees no change; the completion receiver skips
/// creates), so there is nothing to chain.
pub struct MirrorIssueInsert {
    pub issue_id: Uuid,
    pub now: DateTime<Utc>,
    pub actor_id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub state_id: Option<Uuid>,
    pub name: String,
    pub description_html: String,
    /// The save recompute (`issue.py:330-334`), computed driver-side via
    /// `strip_html_text` — passed through, never re-stripped here.
    pub description_stripped: String,
    pub sequence_id: i64,
    pub sort_order: f64,
    pub completed_at: Option<DateTime<Utc>>,
    pub external_source: String,
    pub external_id: String,
    pub pod_id: Option<Uuid>,
}

/// `issues.sequence_id` is `int4`: an out-of-range driver value fails the
/// write, as the server-side cast of the old `int8` bind did.
fn int4(value: i64) -> Result<i32, sqlx::Error> {
    i32::try_from(value).map_err(|_| sqlx::Error::Protocol("integer out of range".to_owned()))
}

/// Execute one bare-column `issues` INSERT (no hooks) for its rows-affected
/// count.
async fn insert_issue_row(
    db: &impl ConnectionTrait,
    row: issues::ActiveModel,
) -> Result<u64, sqlx::Error> {
    issues::Entity::insert(row)
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)
}

/// Execute one bare-column `issue_sequences` INSERT for its rows-affected
/// count.
async fn insert_sequence_row(
    db: &impl ConnectionTrait,
    row: issue_sequences::ActiveModel,
) -> Result<u64, sqlx::Error> {
    issue_sequences::Entity::insert(row)
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)
}

fn mirror_issue_row(insert: &MirrorIssueInsert) -> Result<issues::ActiveModel, sqlx::Error> {
    Ok(issues::ActiveModel {
        id: Set(insert.issue_id),
        created_at: Set(insert.now.into()),
        updated_at: Set(insert.now.into()),
        created_by_id: Set(Some(insert.actor_id)),
        updated_by_id: Set(Some(insert.actor_id)),
        deleted_at: Set(None),
        project_id: Set(insert.project_id),
        workspace_id: Set(insert.workspace_id),
        parent_id: Set(None),
        state_id: Set(insert.state_id),
        point: Set(None),
        estimate_point_id: Set(None),
        name: Set(insert.name.clone()),
        description_json: Set(serde_json::json!({})),
        description_html: Set(insert.description_html.clone()),
        description_stripped: Set(Some(insert.description_stripped.clone())),
        description_binary: Set(None),
        priority: Set("none".to_owned()),
        complexity_score: Set(0),
        start_date: Set(None),
        target_date: Set(None),
        sequence_id: Set(int4(insert.sequence_id)?),
        sort_order: Set(insert.sort_order),
        completed_at: Set(insert.completed_at.map(Into::into)),
        archived_at: Set(None),
        is_draft: Set(false),
        external_source: Set(Some(insert.external_source.clone())),
        external_id: Set(Some(insert.external_id.clone())),
        type_id: Set(None),
        git_work_branch: Set(String::new()),
        workpad: Set(String::new()),
        created_via: Set(None),
        assigned_pod_id: Set(insert.pod_id),
        agent_executor: Set(None),
    })
}

pub async fn insert_mirror_issue(
    db: &impl ConnectionTrait,
    insert: &MirrorIssueInsert,
) -> Result<u64, sqlx::Error> {
    insert_issue_row(db, mirror_issue_row(insert)?).await
}

/// The `IssueSequence.objects.create(issue, sequence, project)`
/// companion row (`issue.py:340`): `ProjectBaseModel` audit columns (NULL
/// in the worker — the crum clobber), `deleted false`. The row id is
/// generated here; callers never read it back.
pub struct MirrorIssueSequence {
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub issue_id: Uuid,
    pub sequence: i64,
    pub now: DateTime<Utc>,
}

fn mirror_sequence_row(seq: &MirrorIssueSequence) -> issue_sequences::ActiveModel {
    issue_sequences::ActiveModel {
        id: Set(Uuid::new_v4()),
        created_at: Set(seq.now.into()),
        updated_at: Set(seq.now.into()),
        created_by_id: Set(None),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        project_id: Set(seq.project_id),
        workspace_id: Set(seq.workspace_id),
        issue_id: Set(Some(seq.issue_id)),
        sequence: Set(seq.sequence),
        deleted: Set(false),
    }
}

pub async fn insert_issue_sequence(
    db: &impl ConnectionTrait,
    seq: &MirrorIssueSequence,
) -> Result<u64, sqlx::Error> {
    insert_sequence_row(db, mirror_sequence_row(seq)).await
}

/// The git-mirror UPDATE arm (`Issue.objects.filter(pk).update(...)`,
/// `git_sync_task.py:100-108`): absolute values, no `save()` side
/// effects — the stripped form is `_safe_render`'s value passed through
/// unchanged (`QuerySet.update` never recomputes it), and state/sequence/
/// sort-order are untouched. The shared audit restamp (`:108-112`)
/// collapses into the same statement (identical final bytes).
pub struct GitMirrorIssueUpdate {
    pub issue_id: Uuid,
    pub name: String,
    pub description_html: String,
    pub description_stripped: String,
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub external_source: String,
    pub external_id: String,
    pub now: DateTime<Utc>,
}

fn git_mirror_update(upd: &GitMirrorIssueUpdate) -> sea_orm::UpdateMany<issues::Entity> {
    issues::Entity::update_many()
        .col_expr(issues::Column::Name, Expr::value(upd.name.as_str()))
        .col_expr(
            issues::Column::DescriptionHtml,
            Expr::value(upd.description_html.as_str()),
        )
        .col_expr(
            issues::Column::DescriptionStripped,
            Expr::value(upd.description_stripped.as_str()),
        )
        .col_expr(
            issues::Column::DescriptionJson,
            Expr::value(serde_json::json!({})),
        )
        .col_expr(issues::Column::WorkspaceId, Expr::value(upd.workspace_id))
        .col_expr(issues::Column::CreatedById, Expr::value(upd.actor_id))
        .col_expr(issues::Column::UpdatedById, Expr::value(upd.actor_id))
        .col_expr(
            issues::Column::ExternalSource,
            Expr::value(upd.external_source.as_str()),
        )
        .col_expr(
            issues::Column::ExternalId,
            Expr::value(upd.external_id.as_str()),
        )
        .col_expr(issues::Column::UpdatedAt, Expr::value(upd.now))
        .filter(issues::Column::Id.eq(upd.issue_id))
}

pub async fn update_git_mirror_issue(
    db: &impl ConnectionTrait,
    upd: &GitMirrorIssueUpdate,
) -> Result<u64, sqlx::Error> {
    git_mirror_update(upd)
        .exec(db)
        .await
        .map(|done| done.rows_affected)
        .map_err(into_sqlx)
}

/// The github-mirror UPDATE arm (`update_or_create` via a full
/// `Issue.save()`, `github_sync_task.py:135-140`, `issue.py:288-350`):
/// `save()` rewrites the whole row, but only the render triple, the
/// driver-recomputed `completed_at` and the audit restamp can differ.
/// The stripped form is the save recompute (`issue.py:345-350`),
/// computed driver-side via `strip_html_text` and passed through. The
/// audit restamp (`github_sync_task.py:156-160`) collapses into the same
/// statement (identical final bytes). Bare columns, no hooks, no chain:
/// the update arm changes no state, so no signal fires.
pub struct GithubMirrorIssueUpdate {
    pub issue_id: Uuid,
    pub name: String,
    pub description_html: String,
    pub description_stripped: String,
    pub actor_id: Uuid,
    pub completed_at: Option<DateTime<Utc>>,
    pub now: DateTime<Utc>,
}

fn github_mirror_update(upd: &GithubMirrorIssueUpdate) -> sea_orm::UpdateMany<issues::Entity> {
    issues::Entity::update_many()
        .col_expr(issues::Column::Name, Expr::value(upd.name.as_str()))
        .col_expr(
            issues::Column::DescriptionHtml,
            Expr::value(upd.description_html.as_str()),
        )
        .col_expr(
            issues::Column::DescriptionStripped,
            Expr::value(upd.description_stripped.as_str()),
        )
        .col_expr(
            issues::Column::DescriptionJson,
            Expr::value(serde_json::json!({})),
        )
        .col_expr(issues::Column::CreatedById, Expr::value(upd.actor_id))
        .col_expr(issues::Column::UpdatedById, Expr::value(upd.actor_id))
        .col_expr(issues::Column::CompletedAt, Expr::value(upd.completed_at))
        .col_expr(issues::Column::UpdatedAt, Expr::value(upd.now))
        .filter(issues::Column::Id.eq(upd.issue_id))
}

pub async fn update_github_mirror_issue(
    db: &impl ConnectionTrait,
    upd: &GithubMirrorIssueUpdate,
) -> Result<u64, sqlx::Error> {
    github_mirror_update(upd)
        .exec(db)
        .await
        .map(|done| done.rows_affected)
        .map_err(into_sqlx)
}

// ---------------------------------------------------------------------------
// Agent-tool twins: the tool's create/update for callers that must stay
// inside their own `sqlx` transaction (the `insert_mirror_issue`
// precedent). All hook-adjacent values are resolved driver-side and bound
// verbatim here.
// ---------------------------------------------------------------------------

/// The agent-tool issue create (`Issue.objects.create`,
/// `assistant/tools/issues.py:265-277`, via the `Issue.save()` creation
/// path `issue.py:267-350`): pod/state/sequence/sort-order/completed
/// resolved driver-side (the tool's project lock, advisory lock, MAX
/// reads and the `None`-state quirk), then one row plus the sequence
/// companion row. The impersonated actor rides `created_by`
/// (`impersonate(user)`), `updated_by` stays NULL (no follow-up restamp
/// on this path), `created_via` is the tool's, and `git_work_branch` /
/// `workpad` are the `''` model defaults. Bare columns, no hooks, no
/// chain: the driver holds its own `sqlx` transaction around the lock,
/// the reads and both rows, and runs its own fan-out afterwards — the
/// same in-transaction justification as [`insert_mirror_issue`].
pub struct ToolIssueInsert<'a> {
    pub issue_id: Uuid,
    pub now: DateTime<Utc>,
    pub name: &'a str,
    pub description_html: &'a str,
    /// The save recompute (`issue.py:341-350`), computed driver-side
    /// via `ml_strip_tags` — `None` for empty HTML — passed through,
    /// never re-stripped here.
    pub description_stripped: Option<&'a str>,
    pub priority: &'a str,
    pub sequence_id: i64,
    pub sort_order: f64,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_via: &'a str,
    pub created_by: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub state_id: Option<Uuid>,
    pub pod_id: Option<Uuid>,
}

fn tool_issue_row(insert: &ToolIssueInsert<'_>) -> Result<issues::ActiveModel, sqlx::Error> {
    Ok(issues::ActiveModel {
        id: Set(insert.issue_id),
        created_at: Set(insert.now.into()),
        updated_at: Set(insert.now.into()),
        deleted_at: Set(None),
        point: Set(None),
        name: Set(insert.name.to_owned()),
        description_json: Set(Value::Object(Map::new())),
        description_html: Set(insert.description_html.to_owned()),
        description_stripped: Set(insert.description_stripped.map(str::to_owned)),
        description_binary: Set(None),
        priority: Set(insert.priority.to_owned()),
        complexity_score: Set(0),
        start_date: Set(None),
        target_date: Set(None),
        sequence_id: Set(int4(insert.sequence_id)?),
        sort_order: Set(insert.sort_order),
        completed_at: Set(insert.completed_at.map(Into::into)),
        archived_at: Set(None),
        is_draft: Set(false),
        external_source: Set(None),
        external_id: Set(None),
        git_work_branch: Set(String::new()),
        workpad: Set(String::new()),
        created_via: Set(Some(insert.created_via.to_owned())),
        agent_executor: Set(None),
        created_by_id: Set(Some(insert.created_by)),
        updated_by_id: Set(None),
        project_id: Set(insert.project_id),
        workspace_id: Set(insert.workspace_id),
        parent_id: Set(insert.parent_id),
        state_id: Set(insert.state_id),
        estimate_point_id: Set(None),
        type_id: Set(None),
        assigned_pod_id: Set(insert.pod_id),
    })
}

pub async fn insert_tool_issue(
    db: &impl ConnectionTrait,
    insert: &ToolIssueInsert<'_>,
) -> Result<u64, sqlx::Error> {
    insert_issue_row(db, tool_issue_row(insert)?).await
}

/// The `IssueSequence.objects.create(issue, sequence, project)`
/// companion row of the agent-tool create (`issue.py:340`): the
/// impersonated actor rides `created_by`, `updated_by` stays NULL (no
/// follow-up restamp on this path), `deleted` false. Contrast
/// [`MirrorIssueSequence`], whose worker path binds NULL twice. The row
/// id is generated here; callers never read it back.
pub struct ToolIssueSequence {
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub issue_id: Uuid,
    pub sequence: i64,
    pub created_by: Uuid,
    pub now: DateTime<Utc>,
}

fn tool_sequence_row(seq: &ToolIssueSequence) -> issue_sequences::ActiveModel {
    issue_sequences::ActiveModel {
        id: Set(Uuid::new_v4()),
        created_at: Set(seq.now.into()),
        updated_at: Set(seq.now.into()),
        created_by_id: Set(Some(seq.created_by)),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        project_id: Set(seq.project_id),
        workspace_id: Set(seq.workspace_id),
        issue_id: Set(Some(seq.issue_id)),
        sequence: Set(seq.sequence),
        deleted: Set(false),
    }
}

pub async fn insert_tool_issue_sequence(
    db: &impl ConnectionTrait,
    seq: &ToolIssueSequence,
) -> Result<u64, sqlx::Error> {
    insert_sequence_row(db, tool_sequence_row(seq)).await
}

/// The agent-tool issue update (`locked.save(update_fields=...)`,
/// `assistant/tools/issues.py:337-369`): exactly the tool's columns —
/// name, the description pair, priority, state, parent, plus the audit
/// pair. `description_stripped` is recomputed in memory but NOT written
/// (it is absent from `update_fields`), so a description edit leaves
/// the column stale — ported as-is. `completed_at` is untouched (the
/// create-only `adding` arm never runs). Bare columns, no hooks, no
/// chain: the driver keeps the write inside its fused save+drive
/// transaction (a separate domain transaction would deadlock against
/// its row lock and cannot carry the drive's savepoints), and routes
/// the orchestration transition itself — the same in-transaction
/// justification as [`update_issue_state`].
pub struct ToolIssueUpdate<'a> {
    pub issue_id: Uuid,
    pub name: Option<&'a str>,
    pub description_html: Option<&'a str>,
    pub priority: Option<&'a str>,
    /// Set only when the state arm runs (`to_state is not None`,
    /// `issues.py:351`): an explicit empty `state_id` that resolves to
    /// no state skips the arm entirely — no write, no `changed` entry.
    pub state_given: bool,
    pub state_id: Option<Uuid>,
    pub parent_touched: bool,
    pub parent_id: Option<Uuid>,
    pub updated_by_id: Uuid,
    pub now: DateTime<Utc>,
}

/// The tool save's column list: only the touched arms, then the audit
/// pair, in the tool's `update_fields` order.
fn tool_issue_update(upd: &ToolIssueUpdate<'_>) -> sea_orm::UpdateMany<issues::Entity> {
    let mut update = issues::Entity::update_many();
    if let Some(name) = upd.name {
        update = update.col_expr(issues::Column::Name, Expr::value(name));
    }
    if let Some(html) = upd.description_html {
        update = update
            .col_expr(issues::Column::DescriptionHtml, Expr::value(html))
            .col_expr(
                issues::Column::DescriptionJson,
                Expr::value(Value::Object(Map::new())),
            );
    }
    if let Some(priority) = upd.priority {
        update = update.col_expr(issues::Column::Priority, Expr::value(priority));
    }
    if upd.state_given {
        update = update.col_expr(issues::Column::StateId, Expr::value(upd.state_id));
    }
    if upd.parent_touched {
        update = update.col_expr(issues::Column::ParentId, Expr::value(upd.parent_id));
    }
    update
        .col_expr(issues::Column::UpdatedAt, Expr::value(upd.now))
        .col_expr(issues::Column::UpdatedById, Expr::value(upd.updated_by_id))
        .filter(issues::Column::Id.eq(upd.issue_id))
}

pub async fn update_tool_issue(
    db: &impl ConnectionTrait,
    upd: &ToolIssueUpdate<'_>,
) -> Result<u64, sqlx::Error> {
    tool_issue_update(upd)
        .exec(db)
        .await
        .map(|done| done.rows_affected)
        .map_err(into_sqlx)
}

/// The two `fix_duplicate_sequences` `bulk_update`s (`:90-91`): the
/// issue renumbers (`sequence_id` only), then the mapped sequence rows
/// take the same new values (`sequence` only) — no `updated_at` bump on
/// either (`bulk_update` never touches unlisted fields, verified). Each
/// pair is `(row_id, new_value)`, one UPDATE per pair. Runs on the
/// caller's connection: the repair driver passes its advisory-locked
/// transaction (`TxConn`) and commits. Moved from `FixTx::commit_plan`
/// (PIDASHCONV-1630) minus the commit.
pub async fn apply_fix_plan(
    db: &impl ConnectionTrait,
    issue_updates: &[(Uuid, i64)],
    sequence_updates: &[(Uuid, i64)],
) -> Result<(), sqlx::Error> {
    for (issue_id, new_sequence) in issue_updates {
        issues::Entity::update_many()
            .col_expr(issues::Column::SequenceId, Expr::value(*new_sequence))
            .filter(issues::Column::Id.eq(*issue_id))
            .exec(db)
            .await
            .map_err(into_sqlx)?;
    }
    for (row_id, new_sequence) in sequence_updates {
        issue_sequences::Entity::update_many()
            .col_expr(
                issue_sequences::Column::Sequence,
                Expr::value(*new_sequence),
            )
            .filter(issue_sequences::Column::Id.eq(*row_id))
            .exec(db)
            .await
            .map_err(into_sqlx)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// seed + demo inserts (PIDASHCONV-1632)
// ---------------------------------------------------------------------------

/// One workspace-seed issue row: `create_project_issues` in
/// `bgtasks/workspace_seed_task.py` — `Issue.save` with the sequence
/// id, sort order, pod and `completed_at` already resolved by the
/// caller (the advisory lock, the state-group completion rule and
/// the post-save transition drive stay caller-sequenced).
/// `description_json` is `'{}'`, `updated_by` NULL. Moved verbatim
/// from the seed store.
pub struct SeedIssue<'a> {
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub state_id: Uuid,
    pub created_by: Uuid,
    pub name: &'a str,
    pub description_html: &'a str,
    pub description_stripped: Option<&'a str>,
    pub priority: &'a str,
    pub sequence_id: i64,
    pub sort_order: f64,
    pub completed_at: Option<DateTime<Utc>>,
    pub assigned_pod_id: Option<Uuid>,
    pub now: DateTime<Utc>,
}

fn seed_issue_row(id: Uuid, issue: &SeedIssue<'_>) -> Result<issues::ActiveModel, sqlx::Error> {
    Ok(issues::ActiveModel {
        created_at: Set(issue.now.into()),
        updated_at: Set(issue.now.into()),
        id: Set(id),
        name: Set(issue.name.to_owned()),
        description_json: Set(serde_json::json!({})),
        priority: Set(issue.priority.to_owned()),
        start_date: Set(None),
        target_date: Set(None),
        sequence_id: Set(int4(issue.sequence_id)?),
        created_by_id: Set(Some(issue.created_by)),
        parent_id: Set(None),
        project_id: Set(issue.project_id),
        state_id: Set(Some(issue.state_id)),
        updated_by_id: Set(None),
        workspace_id: Set(issue.workspace_id),
        description_html: Set(issue.description_html.to_owned()),
        description_stripped: Set(issue.description_stripped.map(str::to_owned)),
        completed_at: Set(issue.completed_at.map(Into::into)),
        sort_order: Set(issue.sort_order),
        point: Set(None),
        archived_at: Set(None),
        is_draft: Set(false),
        external_id: Set(None),
        external_source: Set(None),
        description_binary: Set(None),
        estimate_point_id: Set(None),
        type_id: Set(None),
        deleted_at: Set(None),
        git_work_branch: Set(String::new()),
        assigned_pod_id: Set(issue.assigned_pod_id),
        workpad: Set(String::new()),
        created_via: Set(None),
        agent_executor: Set(None),
        complexity_score: Set(0),
    })
}

/// The seed issue-row insert. Returns the new (client-generated) row id.
pub async fn insert_seed_issue(
    db: &impl ConnectionTrait,
    issue: &SeedIssue<'_>,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    insert_issue_row(db, seed_issue_row(id, issue)?).await?;
    Ok(id)
}

/// One workspace-seed sequence row: the `Issue.save` row
/// (`issue.py:343`, NULL audit, workspace from the project) and the
/// explicit `IssueSequence.objects.create` row (`:293-298`, NULL
/// audit, default `sequence = 1`) share one statement shape — the
/// caller passes the resolved sequence. Moved verbatim from the
/// seed store.
pub struct SeedIssueSequence {
    pub issue_id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub sequence: i64,
    pub now: DateTime<Utc>,
}

fn seed_sequence_row(id: Uuid, seq: &SeedIssueSequence) -> issue_sequences::ActiveModel {
    issue_sequences::ActiveModel {
        created_at: Set(seq.now.into()),
        updated_at: Set(seq.now.into()),
        id: Set(id),
        sequence: Set(seq.sequence),
        deleted: Set(false),
        created_by_id: Set(None),
        issue_id: Set(Some(seq.issue_id)),
        project_id: Set(seq.project_id),
        updated_by_id: Set(None),
        workspace_id: Set(seq.workspace_id),
        deleted_at: Set(None),
    }
}

/// The seed sequence-row insert. Returns the new (client-generated) row
/// id.
pub async fn insert_seed_issue_sequence(
    db: &impl ConnectionTrait,
    seq: &SeedIssueSequence,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    insert_sequence_row(db, seed_sequence_row(id, seq)).await?;
    Ok(id)
}

/// One dummy-data issue row: `create_issues (:267-355)` in
/// `bgtasks/dummy_data_task.py` — `bulk_create(...,
/// ignore_conflicts=True)` replayed one row at a time, `ON
/// CONFLICT DO NOTHING`. The caller resolves the truncated name,
/// the `<p>` html, the sequence/sort counters and the draws. Ids
/// are the task's `::uuid` strings. Moved verbatim from the dummy
/// store (PIDASHCONV-1632).
pub struct DemoIssue<'a> {
    pub state_id: &'a str,
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    pub created_by: &'a str,
    pub name: &'a str,
    pub description_html: &'a str,
    pub description_stripped: &'a str,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub start_date: Option<NaiveDate>,
    pub target_date: Option<NaiveDate>,
    pub priority: &'a str,
    pub now: DateTime<Utc>,
}

/// A task `::uuid` string: a malformed id fails the write before any
/// statement runs (the old server-side cast failed the statement).
fn demo_uuid(value: &str) -> Result<Uuid, sqlx::Error> {
    Uuid::parse_str(value).map_err(|error| sqlx::Error::Encode(Box::new(error)))
}

/// The dummy-data issue row: exactly the task's columns, the rest left to
/// the column defaults (NULL), as the old column list did.
fn demo_issue_row(id: Uuid, issue: &DemoIssue<'_>) -> Result<issues::ActiveModel, sqlx::Error> {
    Ok(issues::ActiveModel {
        id: Set(id),
        state_id: Set(Some(demo_uuid(issue.state_id)?)),
        project_id: Set(demo_uuid(issue.project_id)?),
        workspace_id: Set(demo_uuid(issue.workspace_id)?),
        name: Set(issue.name.to_owned()),
        description_html: Set(issue.description_html.to_owned()),
        description_stripped: Set(Some(issue.description_stripped.to_owned())),
        sequence_id: Set(issue.sequence_id),
        sort_order: Set(issue.sort_order),
        start_date: Set(issue.start_date),
        target_date: Set(issue.target_date),
        priority: Set(issue.priority.to_owned()),
        created_by_id: Set(Some(demo_uuid(issue.created_by)?)),
        created_at: Set(issue.now.into()),
        updated_at: Set(issue.now.into()),
        description_json: Set(serde_json::json!({})),
        is_draft: Set(false),
        git_work_branch: Set(String::new()),
        workpad: Set(String::new()),
        complexity_score: Set(0),
        ..Default::default()
    })
}

/// The dummy-data issue-row insert (`ON CONFLICT DO NOTHING`). Generates
/// the row id client-side (as Django does) and returns it for the
/// sequence, activity and link legs (pushed even on conflict, as today).
pub async fn insert_demo_issue(
    db: &impl ConnectionTrait,
    issue: &DemoIssue<'_>,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4();
    issues::Entity::insert(demo_issue_row(id, issue)?)
        .on_conflict(OnConflict::new().do_nothing().to_owned())
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)?;
    Ok(id.to_string())
}

/// One dummy-data sequence row: the per-issue
/// `IssueSequence.objects.create` in `create_issues`. Ids are the
/// task's `::uuid` strings. Moved verbatim from the dummy store.
pub struct DemoIssueSequence<'a> {
    pub issue_id: &'a str,
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    pub sequence: i64,
    pub now: DateTime<Utc>,
}

fn demo_sequence_row(
    id: Uuid,
    seq: &DemoIssueSequence<'_>,
) -> Result<issue_sequences::ActiveModel, sqlx::Error> {
    Ok(issue_sequences::ActiveModel {
        id: Set(id),
        issue_id: Set(Some(demo_uuid(seq.issue_id)?)),
        sequence: Set(seq.sequence),
        project_id: Set(demo_uuid(seq.project_id)?),
        workspace_id: Set(demo_uuid(seq.workspace_id)?),
        created_at: Set(seq.now.into()),
        updated_at: Set(seq.now.into()),
        deleted: Set(false),
        ..Default::default()
    })
}

/// The dummy-data sequence-row insert.
pub async fn insert_demo_issue_sequence(
    db: &impl ConnectionTrait,
    seq: &DemoIssueSequence<'_>,
) -> Result<(), sqlx::Error> {
    insert_sequence_row(db, demo_sequence_row(Uuid::new_v4(), seq)?).await?;
    Ok(())
}

/// One dummy-data assignee row: `create_issue_assignees (:391-413)`
/// in `bgtasks/dummy_data_task.py` — `bulk_create(...,
/// ignore_conflicts=True)` replayed one row at a time. Ids are the
/// task's `::uuid` strings. Moved verbatim from the dummy store.
pub struct DemoIssueAssignee<'a> {
    pub issue_id: &'a str,
    pub assignee_id: &'a str,
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    pub now: DateTime<Utc>,
}

fn demo_assignee_row(
    id: Uuid,
    row: &DemoIssueAssignee<'_>,
) -> Result<issue_assignees::ActiveModel, sqlx::Error> {
    Ok(issue_assignees::ActiveModel {
        id: Set(id),
        issue_id: Set(demo_uuid(row.issue_id)?),
        assignee_id: Set(demo_uuid(row.assignee_id)?),
        project_id: Set(demo_uuid(row.project_id)?),
        workspace_id: Set(demo_uuid(row.workspace_id)?),
        created_at: Set(row.now.into()),
        updated_at: Set(row.now.into()),
        ..Default::default()
    })
}

/// The dummy-data assignee-row insert (`ON CONFLICT DO NOTHING`).
pub async fn insert_demo_issue_assignee(
    db: &impl ConnectionTrait,
    row: &DemoIssueAssignee<'_>,
) -> Result<(), sqlx::Error> {
    issue_assignees::Entity::insert(demo_assignee_row(Uuid::new_v4(), row)?)
        .on_conflict(OnConflict::new().do_nothing().to_owned())
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// The pod-delete sweep (`pods.py:262-264`):
/// `Issue.objects.filter(assigned_pod=pod).update(assigned_pod=None)`, a
/// `QuerySet.update` (no `save()` hooks) over the live issues.
pub async fn clear_assigned_pod(db: &impl ConnectionTrait, pod_id: Uuid) -> Result<u64, DbErr> {
    use sea_orm::sea_query::Expr;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    issues::Entity::update_many()
        .col_expr(issues::Column::AssignedPodId, Expr::value(None::<Uuid>))
        .filter(issues::Column::DeletedAt.is_null())
        .filter(issues::Column::AssignedPodId.eq(pod_id))
        .exec(db)
        .await
        .map(|done| done.rows_affected)
}

/// The `issue_activity` issue touch (`issue.updated_at =
/// timezone.now(); save(update_fields=["updated_at"])`,
/// `issue_activities_task.py:1535-1536`): no soft-delete guard —
/// the caller probes the live row first. Moved verbatim from the
/// dispatcher (PIDASHCONV-1632).
pub async fn touch_issue(db: &impl ConnectionTrait, issue_id: Uuid) -> Result<(), sqlx::Error> {
    issues::Entity::update_many()
        .col_expr(issues::Column::UpdatedAt, Expr::current_timestamp().into())
        .filter(issues::Column::Id.eq(issue_id))
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// The per-record `updated_at` touches from the cycle/module
/// builders, one statement over the soft-filtered set — same final
/// rows as the per-record guarded saves. Moved verbatim from the
/// dispatcher (PIDASHCONV-1632).
pub async fn touch_issues(
    db: &impl ConnectionTrait,
    issue_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    issues::Entity::update_many()
        .col_expr(issues::Column::UpdatedAt, Expr::current_timestamp().into())
        .filter(issues::Column::Id.is_in(issue_ids.iter().copied()))
        .filter(issues::Column::DeletedAt.is_null())
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// The ticker state-move write (`scheduling.py:700,1113,1468`):
/// `save(update_fields=["state", "updated_at"])` — ONLY these columns.
/// Contrast [`update_issue_state`], which additionally stamps
/// `updated_by_id` (the agent-tool save shape): here the in-memory
/// `updated_by` assignments never reach the row, exactly what the field
/// list writes. Bare columns, no hooks, no chain: the driver routes the
/// orchestration fire itself (`reconcile_on_seam` with the tick-caused
/// event), as today. Runs on any SeaORM connection so the dispatch seam keeps the
/// write inside its transaction.
pub async fn update_issue_state_no_audit(
    db: &impl ConnectionTrait,
    issue_id: Uuid,
    state_id: Uuid,
    updated_at: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    ticker_state_update(issue_id, state_id, updated_at)
        .exec(db)
        .await
        .map(|done| done.rows_affected)
        .map_err(into_sqlx)
}

fn ticker_state_update(
    issue_id: Uuid,
    state_id: Uuid,
    updated_at: DateTime<Utc>,
) -> sea_orm::UpdateMany<issues::Entity> {
    issues::Entity::update_many()
        .col_expr(issues::Column::StateId, Expr::value(state_id))
        .col_expr(issues::Column::UpdatedAt, Expr::value(updated_at))
        .filter(issues::Column::Id.eq(issue_id))
}

/// The estimate-point delete reassignment
/// (`EstimatePointEndpoint.destroy`, `estimate/base.py:185-247`): on
/// the `new_estimate_id` branch, the whole live queryset for the old
/// point moves to the new one — inside the per-issue activity loop
/// (the first emit precedes the update; after the first iteration
/// nothing matches but the emits continue). No `updated_at` touch:
/// a plain queryset `.update()`. The activity fan-out stays
/// caller-enqueued, as today.
pub async fn reassign_estimate_point(
    db: &impl ConnectionTrait,
    new_point_id: Uuid,
    project_id: Uuid,
    slug: &str,
    old_point_id: Uuid,
) -> Result<(), sqlx::Error> {
    reassign_estimate_point_update(new_point_id, project_id, slug, old_point_id)
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// The live old-point queryset (`Issue.issue_objects`-free default
/// manager: `deleted_at IS NULL`, project + workspace slug) as an id
/// subquery over an aliased `issues`, then the one-column UPDATE.
fn reassign_estimate_point_update(
    new_point_id: Uuid,
    project_id: Uuid,
    slug: &str,
    old_point_id: Uuid,
) -> sea_orm::UpdateMany<issues::Entity> {
    let inner = Alias::new("i");
    let ws = Alias::new("w");
    issues::Entity::update_many()
        .col_expr(issues::Column::EstimatePointId, Expr::value(new_point_id))
        .filter(
            issues::Column::Id.in_subquery(
                Query::select()
                    .column((inner.clone(), issues::Column::Id))
                    .from_as(issues::Entity, inner.clone())
                    .join_as(
                        sea_orm::JoinType::InnerJoin,
                        workspaces::Entity,
                        ws.clone(),
                        Expr::col((ws.clone(), workspaces::Column::Id))
                            .equals((inner.clone(), issues::Column::WorkspaceId)),
                    )
                    .and_where(Expr::col((inner.clone(), issues::Column::DeletedAt)).is_null())
                    .and_where(Expr::col((inner.clone(), issues::Column::ProjectId)).eq(project_id))
                    .and_where(Expr::col((ws, workspaces::Column::Slug)).eq(slug))
                    .and_where(Expr::col((inner, issues::Column::EstimatePointId)).eq(old_point_id))
                    .to_owned(),
            ),
        )
}

/// The automation bulk writes (`bulk_update(..., batch_size=100)`):
/// `["archived_at"]` (`issue_automation_task.py:69`) and `["state"]`
/// (`:132`) — a single column each, no `updated_at` touch (`bulk_update`
/// never bumps `auto_now` on unlisted fields), no `save()`, no signals.
/// The driver keeps the batch-100 chunking; the per-issue
/// `issue_activity.delay` fan-out stays driver-enqueued, as today.
pub async fn bulk_set_archived_at(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
    archived_at: NaiveDate,
) -> Result<u64, sqlx::Error> {
    issues::Entity::update_many()
        .col_expr(issues::Column::ArchivedAt, Expr::value(archived_at))
        .filter(issues::Column::Id.is_in(ids.iter().copied()))
        .exec(db)
        .await
        .map(|done| done.rows_affected)
        .map_err(into_sqlx)
}

pub async fn bulk_set_state(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
    state_id: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    issues::Entity::update_many()
        .col_expr(issues::Column::StateId, Expr::value(state_id))
        .filter(issues::Column::Id.is_in(ids.iter().copied()))
        .exec(db)
        .await
        .map(|done| done.rows_affected)
        .map_err(into_sqlx)
}

/// One `bulk_update(writes, ["start_date", "target_date"])` row (the app
/// bulk-dates endpoint): the issue id plus the halves this request wrote
/// (`None` = untouched — Django rewrites the stored value there, a no-op
/// either way, so the twin omits the arm).
pub struct BulkIssueDates {
    pub id: Uuid,
    pub start_date: Option<NaiveDate>,
    pub target_date: Option<NaiveDate>,
}

/// `bulk_update(writes, ["start_date", "target_date"])`: one statement, no
/// signals, `updated_at` untouched. Moved from the app bulk-dates driver:
/// one `CASE` per written side (start, then target), each arm in input
/// order, `ELSE` the stored value; a side with no arms is omitted
/// entirely (an armless `CASE` is a syntax error). Runs on any SeaORM
/// connection so the driver keeps the write on its pool.
pub async fn bulk_update_issue_dates(
    db: &impl ConnectionTrait,
    writes: &[BulkIssueDates],
) -> Result<(), sqlx::Error> {
    let Some(update) = bulk_issue_dates_update(writes) else {
        return Ok(());
    };
    update.exec(db).await.map(|_| ()).map_err(into_sqlx)
}

/// The CASE UPDATE for [`bulk_update_issue_dates`]; `None` for no writes.
fn bulk_issue_dates_update(
    writes: &[BulkIssueDates],
) -> Option<sea_orm::UpdateMany<issues::Entity>> {
    if writes.is_empty() {
        return None;
    }
    let mut update = issues::Entity::update_many();
    for (column, pick) in [
        (
            issues::Column::StartDate,
            (|write: &BulkIssueDates| write.start_date) as fn(&BulkIssueDates) -> Option<NaiveDate>,
        ),
        (issues::Column::TargetDate, |write: &BulkIssueDates| {
            write.target_date
        }),
    ] {
        let mut arms = writes
            .iter()
            .filter_map(|write| pick(write).map(|date| (write.id, date)));
        let Some((first_id, first_date)) = arms.next() else {
            continue;
        };
        let mut case = Expr::case(issues::Column::Id.eq(first_id), Expr::value(first_date));
        for (id, date) in arms {
            case = case.case(issues::Column::Id.eq(id), Expr::value(date));
        }
        update = update.col_expr(column, case.finally(Expr::col(column)).into());
    }
    Some(update.filter(issues::Column::Id.is_in(writes.iter().map(|write| write.id))))
}

/// `bulk_update(sub_issues, ["parent"])` (the app sub-issues link): one
/// statement, no signals, `updated_at` untouched. Moved from the driver.
pub async fn link_sub_issue_parents(
    db: &impl ConnectionTrait,
    parent_id: Uuid,
    found: &[Uuid],
) -> Result<(), sqlx::Error> {
    link_sub_issue_parents_update(parent_id, found)
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

fn link_sub_issue_parents_update(
    parent_id: Uuid,
    found: &[Uuid],
) -> sea_orm::UpdateMany<issues::Entity> {
    issues::Entity::update_many()
        .col_expr(issues::Column::ParentId, Expr::value(parent_id))
        .filter(issues::Column::Id.is_in(found.iter().copied()))
}

/// One `bulk_update(issues, scalar_fields)` row (the app bulk-operation
/// endpoint, `bulk_operations.py:154-174`): the merged scalar values — the
/// property value when present, the stored value otherwise (rewriting the
/// stored value is the same final row, so the twin writes every column
/// unconditionally, as the driver did).
pub struct BulkIssueScalars {
    pub id: Uuid,
    pub state_id: Option<Uuid>,
    pub priority: String,
    pub start_date: Option<NaiveDate>,
    pub target_date: Option<NaiveDate>,
    pub estimate_point_id: Option<Uuid>,
}

/// One row of `Issue.objects.bulk_update(issues, scalar_fields,
/// batch_size=100)` (`bulk_operations.py:154-174`): the scalar UPDATE —
/// state, priority, dates, estimate point, plus the shared `updated_at`
/// bump — no `save()`, no signals (design D4: bulk paths skip hooks
/// exactly as Django bulk paths skip signals). Moved verbatim from the
/// app bulk-operation driver (PIDASHCONV-1711); the per-issue
/// `issue_activity` fan-out stays driver-enqueued, as today. Runs on any
/// SeaORM connection so the driver keeps the write inside its transaction.
pub async fn bulk_update_issue_scalars(
    db: &impl ConnectionTrait,
    write: &BulkIssueScalars,
    updated_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    bulk_issue_scalars_update(write, updated_at)
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

fn bulk_issue_scalars_update(
    write: &BulkIssueScalars,
    updated_at: DateTime<Utc>,
) -> sea_orm::UpdateMany<issues::Entity> {
    issues::Entity::update_many()
        .col_expr(issues::Column::StateId, Expr::value(write.state_id))
        .col_expr(
            issues::Column::Priority,
            Expr::value(write.priority.as_str()),
        )
        .col_expr(issues::Column::StartDate, Expr::value(write.start_date))
        .col_expr(issues::Column::TargetDate, Expr::value(write.target_date))
        .col_expr(
            issues::Column::EstimatePointId,
            Expr::value(write.estimate_point_id),
        )
        .col_expr(issues::Column::UpdatedAt, Expr::value(updated_at))
        .filter(issues::Column::Id.eq(write.id))
}

/// The bulk-operation assignee union (`bulk_operations.py`,
/// `IssueAssignee.objects.bulk_create(..., ignore_conflicts=True)`): one
/// fresh bridge row per `(issue_id, assignee_id)` pair, stamped with the
/// actor and the request's shared `now`; any unique conflict skips the
/// row (`ON CONFLICT DO NOTHING`). The caller already dropped the pairs
/// that are live. Runs on any SeaORM connection, so the driver keeps it
/// inside its transaction (`TxConn`).
pub struct BulkAssigneeUnion<'a> {
    pub pairs: &'a [(Uuid, Uuid)],
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub now: DateTime<Utc>,
}

/// Rows per `INSERT` statement for [`insert_bulk_assignees`]: 9 binds a
/// row stays well under Postgres' 65,535-bind ceiling.
const BULK_ASSIGNEE_CHUNK: usize = 5_000;

/// Insert [`BulkAssigneeUnion`] rows; returns the rows written.
pub async fn insert_bulk_assignees(
    db: &impl ConnectionTrait,
    union: &BulkAssigneeUnion<'_>,
) -> Result<u64, DbErr> {
    use pidash_db::entity::issue_assignees;
    use sea_orm::sea_query::OnConflict;
    use sea_orm::EntityTrait;
    let mut written = 0;
    for chunk in union.pairs.chunks(BULK_ASSIGNEE_CHUNK) {
        let rows = chunk
            .iter()
            .map(|&(issue_id, assignee_id)| issue_assignees::ActiveModel {
                id: Set(Uuid::new_v4()),
                created_at: Set(union.now.into()),
                updated_at: Set(union.now.into()),
                created_by_id: Set(Some(union.actor_id)),
                updated_by_id: Set(Some(union.actor_id)),
                issue_id: Set(issue_id),
                assignee_id: Set(assignee_id),
                project_id: Set(union.project_id),
                workspace_id: Set(union.workspace_id),
                deleted_at: Set(None),
            });
        written += issue_assignees::Entity::insert_many(rows)
            .on_conflict(OnConflict::new().do_nothing().to_owned())
            .exec_without_returning(db)
            .await?;
    }
    Ok(written)
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
        Some(true),
        Some(&pre_activity.origin),
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
        run_chain(&pool, &transition, &enqueue, None, chain, &step).await;
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
        Some(true),
        Some(&row.origin),
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    issues::Entity::update_many()
        .col_expr(issues::Column::ArchivedAt, Expr::value(today))
        .filter(issues::Column::Id.is_in(ids.iter().copied()))
        .exec(&db)
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    bulk_soft_delete_update(ids, *project_id, slug, now_micros())
        .exec(&db)
        .await
        .map_err(|error| DomainError::Work(format!("bulk soft-delete failed: {error}")))?;
    Ok(())
}

/// The bulk soft-delete statement: the `issue_objects` scope (it
/// soft-deletes only in-scope rows; out-of-scope ids still count as
/// misses) — live, in the project, in the slug's workspace, not triage
/// (a live triage state; an issue without a state passes), not archived,
/// the project not archived, not a draft. The project / workspace joins
/// of the old `UPDATE … FROM` are id subqueries over the same rows.
fn bulk_soft_delete_update(
    ids: &[Uuid],
    project_id: Uuid,
    slug: &str,
    deleted_at: DateTime<Utc>,
) -> sea_orm::UpdateMany<issues::Entity> {
    issues::Entity::update_many()
        .col_expr(issues::Column::DeletedAt, Expr::value(deleted_at))
        .filter(issues::Column::DeletedAt.is_null())
        .filter(issues::Column::Id.is_in(ids.iter().copied()))
        .filter(issues::Column::ProjectId.eq(project_id))
        .filter(
            issues::Column::ProjectId.in_subquery(
                Query::select()
                    .column(projects::Column::Id)
                    .from(projects::Entity)
                    .and_where(projects::Column::ArchivedAt.is_null())
                    .to_owned(),
            ),
        )
        .filter(
            issues::Column::WorkspaceId.in_subquery(
                Query::select()
                    .column(workspaces::Column::Id)
                    .from(workspaces::Entity)
                    .and_where(workspaces::Column::Slug.eq(slug))
                    .to_owned(),
            ),
        )
        .filter(
            Expr::exists(
                Query::select()
                    .expr(Expr::val(1))
                    .from(states::Entity)
                    .and_where(
                        Expr::col((states::Entity, states::Column::Id))
                            .equals((issues::Entity, issues::Column::StateId)),
                    )
                    .and_where(Expr::col((states::Entity, states::Column::DeletedAt)).is_null())
                    .and_where(Expr::col((states::Entity, states::Column::Group)).eq("triage"))
                    .to_owned(),
            )
            .not(),
        )
        .filter(issues::Column::ArchivedAt.is_null())
        .filter(issues::Column::IsDraft.eq(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::DbBackend;
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
            Some(true),
            Some("https://app.example"),
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
            Some(true),
            Some("https://app.example"),
            None,
        );
        assert!(!kwargs.contains_key("subscriber"));
        assert!(kwargs.get("requested_data").unwrap().is_null());
    }

    #[test]
    fn v1_activity_kwargs_match_work_tasks_bytes() {
        // The domain v1-shaped legs must emit byte-identical kwargs to the
        // `v1_work_items::tasks` builders they replace (`Map` is
        // insertion-ordered workspace-wide, so key ORDER is load-bearing).
        // `epoch` stamps twice, so it compares separately.
        let mine = issue_activity_kwargs(
            "issue.activity.created",
            Value::String("req".to_owned()),
            &uid(1),
            &uid(2),
            &uid(3),
            Value::Null,
            None,
            None,
            None,
        );
        let theirs = crate::v1_work_items::tasks::issue_activity_kwargs(
            "issue.activity.created",
            Some("req"),
            &uid(1).to_string(),
            &uid(2).to_string(),
            &uid(3).to_string(),
            None,
            0,
        );
        assert!(mine.get("epoch").and_then(Value::as_i64).unwrap_or(0) > 0);
        let mut mine = mine;
        let mut theirs = theirs;
        mine.shift_remove("epoch");
        theirs.shift_remove("epoch");
        assert_eq!(
            serde_json::to_string(&Value::Object(mine)).expect("json"),
            serde_json::to_string(&Value::Object(theirs)).expect("json"),
        );
        let raw = serde_json::json!({"name": "n"});
        let mine_model = model_activity_kwargs(
            &uid(2),
            raw.clone(),
            Value::String("cur".to_owned()),
            &uid(1),
            "ws",
            "https://app.example",
        );
        let theirs_model = crate::v1_work_items::tasks::issue_model_activity_kwargs(
            &uid(2).to_string(),
            raw,
            Some("cur"),
            &uid(1).to_string(),
            "ws",
            "https://app.example",
        );
        assert_eq!(
            serde_json::to_string(&Value::Object(mine_model)).expect("json"),
            serde_json::to_string(&Value::Object(theirs_model)).expect("json"),
        );
    }

    #[test]
    fn draft_activity_kwargs_match_work_tasks_bytes() {
        // The domain app-shaped leg must emit byte-identical kwargs to
        // the `app_workspace::tasks` builder it replaces (`Map` is
        // insertion-ordered workspace-wide, so key ORDER is load-bearing).
        // `epoch` stamps twice, so it compares separately.
        let mine = issue_activity_kwargs(
            "issue.activity.created",
            Value::String("req".to_owned()),
            &uid(1),
            &uid(2),
            &uid(3),
            Value::Null,
            Some(true),
            Some("https://app.example"),
            None,
        );
        let theirs = crate::app_workspace::tasks::draft_issue_created_activity(
            "req".to_owned(),
            &uid(1).to_string(),
            &uid(2).to_string(),
            &uid(3).to_string(),
            0,
            "https://app.example",
        )
        .kwargs();
        assert!(mine.get("epoch").and_then(Value::as_i64).unwrap_or(0) > 0);
        let mut mine = mine;
        let mut theirs = theirs;
        mine.shift_remove("epoch");
        theirs.shift_remove("epoch");
        assert_eq!(
            serde_json::to_string(&Value::Object(mine)).expect("json"),
            serde_json::to_string(&Value::Object(theirs)).expect("json"),
        );
    }

    /// Render a SeaORM / sea-query statement with its values inlined.
    fn render(stmt: sea_orm::Statement) -> String {
        stmt.to_string()
    }

    fn pin_time() -> DateTime<Utc> {
        "2026-11-05T10:00:00Z".parse().expect("pin time")
    }

    #[test]
    fn workpad_and_state_update_shapes() {
        // `orchestration/workpad.py:79-84` `update_fields` order:
        // `updated_at` before `workpad`; `None` writes `''`.
        let issue = uid(1);
        assert_eq!(
            render(workpad_save_update(issue, None, pin_time()).build(DbBackend::Postgres)),
            format!(
                r#"UPDATE "issues" SET "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "workpad" = '' WHERE "issues"."id" = '{issue}'"#
            )
        );
        // The ticker save writes state + `updated_at` only (no audit).
        assert_eq!(
            render(ticker_state_update(issue, uid(2), pin_time()).build(DbBackend::Postgres)),
            format!(
                r#"UPDATE "issues" SET "state_id" = '{}', "updated_at" = '2026-11-05 10:00:00.000000 +00:00' WHERE "issues"."id" = '{issue}'"#,
                uid(2)
            )
        );
    }

    #[test]
    fn sync_rows_and_update_shapes() {
        // The mirror INSERT: every column, the fixed literals of the old
        // statement (`'none'`, `0`, `FALSE`, `''`, `{}`), the actor on
        // both audit columns.
        let insert = MirrorIssueInsert {
            issue_id: uid(1),
            now: pin_time(),
            actor_id: uid(2),
            project_id: uid(3),
            workspace_id: uid(4),
            state_id: None,
            name: "n".to_owned(),
            description_html: "<p>h</p>".to_owned(),
            description_stripped: "h".to_owned(),
            sequence_id: 7,
            sort_order: 65535.0,
            completed_at: None,
            external_source: "github".to_owned(),
            external_id: "42".to_owned(),
            pod_id: Some(uid(5)),
        };
        let row = mirror_issue_row(&insert).expect("row");
        assert_eq!(row.priority, Set("none".to_owned()));
        assert_eq!(row.complexity_score, Set(0));
        assert_eq!(row.created_by_id, Set(Some(uid(2))));
        assert_eq!(row.updated_by_id, Set(Some(uid(2))));
        assert_eq!(row.description_json, Set(serde_json::json!({})));
        assert_eq!(row.sequence_id, Set(7));
        assert_eq!(row.assigned_pod_id, Set(Some(uid(5))));
        assert_eq!(row.git_work_branch, Set(String::new()));
        assert_eq!(row.is_draft, Set(false));
        assert!(mirror_issue_row(&MirrorIssueInsert {
            sequence_id: i64::from(i32::MAX) + 1,
            ..insert
        })
        .is_err());
        let seq = mirror_sequence_row(&MirrorIssueSequence {
            project_id: uid(3),
            workspace_id: uid(4),
            issue_id: uid(1),
            sequence: 7,
            now: pin_time(),
        });
        assert_eq!(seq.created_by_id, Set(None));
        assert_eq!(seq.updated_by_id, Set(None));
        assert_eq!(seq.deleted, Set(false));
        // Git arm: no state/sequence/sort touch, `updated_at` carried.
        let git = render(
            git_mirror_update(&GitMirrorIssueUpdate {
                issue_id: uid(1),
                name: "n".to_owned(),
                description_html: "h".to_owned(),
                description_stripped: "s".to_owned(),
                workspace_id: uid(4),
                actor_id: uid(2),
                external_source: "gitlab".to_owned(),
                external_id: "9".to_owned(),
                now: pin_time(),
            })
            .build(DbBackend::Postgres),
        );
        assert_eq!(
            git,
            format!(
                r#"UPDATE "issues" SET "name" = 'n', "description_html" = 'h', "description_stripped" = 's', "description_json" = '{{}}', "workspace_id" = '{}', "created_by_id" = '{}', "updated_by_id" = '{}', "external_source" = 'gitlab', "external_id" = '9', "updated_at" = '2026-11-05 10:00:00.000000 +00:00' WHERE "issues"."id" = '{}'"#,
                uid(4),
                uid(2),
                uid(2),
                uid(1)
            )
        );
        // Github arm: the audit restamp on both columns, completed carried.
        let github = render(
            github_mirror_update(&GithubMirrorIssueUpdate {
                issue_id: uid(1),
                name: "n".to_owned(),
                description_html: "h".to_owned(),
                description_stripped: "s".to_owned(),
                actor_id: uid(2),
                completed_at: None,
                now: pin_time(),
            })
            .build(DbBackend::Postgres),
        );
        assert_eq!(
            github,
            format!(
                r#"UPDATE "issues" SET "name" = 'n', "description_html" = 'h', "description_stripped" = 's', "description_json" = '{{}}', "created_by_id" = '{}', "updated_by_id" = '{}', "completed_at" = NULL, "updated_at" = '2026-11-05 10:00:00.000000 +00:00' WHERE "issues"."id" = '{}'"#,
                uid(2),
                uid(2),
                uid(1)
            )
        );
    }

    #[test]
    fn tool_rows_and_update_shapes() {
        // The tool create: impersonated `created_by`, `updated_by` NULL,
        // the tool's `created_via`, the `''` model defaults.
        let row = tool_issue_row(&ToolIssueInsert {
            issue_id: uid(1),
            now: pin_time(),
            name: "n",
            description_html: "",
            description_stripped: None,
            priority: "high",
            sequence_id: 3,
            sort_order: 1.0,
            completed_at: None,
            created_via: "assistant",
            created_by: uid(2),
            project_id: uid(3),
            workspace_id: uid(4),
            parent_id: None,
            state_id: Some(uid(5)),
            pod_id: None,
        })
        .expect("row");
        assert_eq!(row.created_by_id, Set(Some(uid(2))));
        assert_eq!(row.updated_by_id, Set(None));
        assert_eq!(row.created_via, Set(Some("assistant".to_owned())));
        assert_eq!(row.external_source, Set(None));
        assert_eq!(row.description_stripped, Set(None));
        let seq = tool_sequence_row(&ToolIssueSequence {
            project_id: uid(3),
            workspace_id: uid(4),
            issue_id: uid(1),
            sequence: 3,
            created_by: uid(2),
            now: pin_time(),
        });
        assert_eq!(seq.created_by_id, Set(Some(uid(2))));
        assert_eq!(seq.updated_by_id, Set(None));
        // The tool save: only the touched arms, then the audit pair.
        let only_audit = ToolIssueUpdate {
            issue_id: uid(1),
            name: None,
            description_html: None,
            priority: None,
            state_given: false,
            state_id: None,
            parent_touched: false,
            parent_id: None,
            updated_by_id: uid(2),
            now: pin_time(),
        };
        assert_eq!(
            render(tool_issue_update(&only_audit).build(DbBackend::Postgres)),
            format!(
                r#"UPDATE "issues" SET "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '{}' WHERE "issues"."id" = '{}'"#,
                uid(2),
                uid(1)
            )
        );
        let every_arm = ToolIssueUpdate {
            name: Some("n"),
            description_html: Some("<p>d</p>"),
            priority: Some("low"),
            state_given: true,
            state_id: None,
            parent_touched: true,
            parent_id: Some(uid(6)),
            ..only_audit
        };
        assert_eq!(
            render(tool_issue_update(&every_arm).build(DbBackend::Postgres)),
            format!(
                r#"UPDATE "issues" SET "name" = 'n', "description_html" = '<p>d</p>', "description_json" = '{{}}', "priority" = 'low', "state_id" = NULL, "parent_id" = '{}', "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '{}' WHERE "issues"."id" = '{}'"#,
                uid(6),
                uid(2),
                uid(1)
            )
        );
    }

    #[test]
    fn estimate_and_bulk_update_shapes() {
        assert_eq!(
            render(
                reassign_estimate_point_update(uid(1), uid(2), "ws", uid(3))
                    .build(DbBackend::Postgres)
            ),
            format!(
                r#"UPDATE "issues" SET "estimate_point_id" = '{}' WHERE "issues"."id" IN (SELECT "i"."id" FROM "issues" AS "i" INNER JOIN "workspaces" AS "w" ON "w"."id" = "i"."workspace_id" WHERE "i"."deleted_at" IS NULL AND "i"."project_id" = '{}' AND "w"."slug" = 'ws' AND "i"."estimate_point_id" = '{}')"#,
                uid(1),
                uid(2),
                uid(3)
            )
        );
        let date = NaiveDate::from_ymd_opt(2026, 10, 1).expect("date");
        let start_only = [BulkIssueDates {
            id: uid(11),
            start_date: Some(date),
            target_date: None,
        }];
        assert_eq!(
            render(
                bulk_issue_dates_update(&start_only)
                    .expect("writes")
                    .build(DbBackend::Postgres)
            ),
            format!(
                r#"UPDATE "issues" SET "start_date" = (CASE WHEN ("issues"."id" = '{0}') THEN '2026-10-01' ELSE "start_date" END) WHERE "issues"."id" IN ('{0}')"#,
                uid(11)
            )
        );
        // Both sides: start arms, then target arms, then the id list; a
        // write with no date on a side contributes no arm there.
        let both = [
            BulkIssueDates {
                id: uid(11),
                start_date: Some(date),
                target_date: None,
            },
            BulkIssueDates {
                id: uid(12),
                start_date: None,
                target_date: Some(date),
            },
        ];
        assert_eq!(
            render(
                bulk_issue_dates_update(&both)
                    .expect("writes")
                    .build(DbBackend::Postgres)
            ),
            format!(
                r#"UPDATE "issues" SET "start_date" = (CASE WHEN ("issues"."id" = '{0}') THEN '2026-10-01' ELSE "start_date" END), "target_date" = (CASE WHEN ("issues"."id" = '{1}') THEN '2026-10-01' ELSE "target_date" END) WHERE "issues"."id" IN ('{0}', '{1}')"#,
                uid(11),
                uid(12)
            )
        );
        assert!(bulk_issue_dates_update(&[]).is_none());
        assert_eq!(
            render(
                link_sub_issue_parents_update(uid(1), &[uid(2), uid(3)]).build(DbBackend::Postgres)
            ),
            format!(
                r#"UPDATE "issues" SET "parent_id" = '{}' WHERE "issues"."id" IN ('{}', '{}')"#,
                uid(1),
                uid(2),
                uid(3)
            )
        );
        assert_eq!(
            render(
                bulk_issue_scalars_update(
                    &BulkIssueScalars {
                        id: uid(1),
                        state_id: None,
                        priority: "high".to_owned(),
                        start_date: None,
                        target_date: Some(date),
                        estimate_point_id: None,
                    },
                    pin_time()
                )
                .build(DbBackend::Postgres)
            ),
            format!(
                r#"UPDATE "issues" SET "state_id" = NULL, "priority" = 'high', "start_date" = NULL, "target_date" = '2026-10-01', "estimate_point_id" = NULL, "updated_at" = '2026-11-05 10:00:00.000000 +00:00' WHERE "issues"."id" = '{}'"#,
                uid(1)
            )
        );
    }

    #[test]
    fn bulk_soft_delete_and_workpad_select_shapes() {
        assert_eq!(
            render(
                bulk_soft_delete_update(&[uid(1)], uid(2), "ws", pin_time())
                    .build(DbBackend::Postgres)
            ),
            format!(
                r#"UPDATE "issues" SET "deleted_at" = '2026-11-05 10:00:00.000000 +00:00' WHERE "issues"."deleted_at" IS NULL AND "issues"."id" IN ('{}') AND "issues"."project_id" = '{}' AND "issues"."project_id" IN (SELECT "id" FROM "projects" WHERE "projects"."archived_at" IS NULL) AND "issues"."workspace_id" IN (SELECT "id" FROM "workspaces" WHERE "workspaces"."slug" = 'ws') AND (NOT EXISTS(SELECT 1 FROM "states" WHERE "states"."id" = "issues"."state_id" AND "states"."deleted_at" IS NULL AND "states"."group" = 'triage')) AND "issues"."archived_at" IS NULL AND "issues"."is_draft" = FALSE"#,
                uid(1),
                uid(2)
            )
        );
        assert_eq!(
            render(DbBackend::Postgres.build(&workpad_select(uid(1), uid(2), "ws"))),
            format!(
                r#"SELECT "issues"."description_html", "issues"."state_id", "issues"."completed_at", "states"."group" AS "state_group" FROM "issues" LEFT JOIN "states" ON "issues"."state_id" = "states"."id" INNER JOIN "projects" ON "issues"."project_id" = "projects"."id" INNER JOIN "workspaces" ON "issues"."workspace_id" = "workspaces"."id" WHERE "issues"."deleted_at" IS NULL AND (NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)) AND "issues"."archived_at" IS NULL AND "projects"."archived_at" IS NULL AND (NOT "issues"."is_draft") AND "issues"."id" = '{}' AND "issues"."project_id" = '{}' AND "workspaces"."slug" = 'ws' LIMIT 21 FOR UPDATE OF "issues""#,
                uid(1),
                uid(2)
            )
        );
    }

    #[test]
    fn chain_leg_statement_shapes() {
        // The v1 create second save: `created_at` + `created_by` only.
        assert_eq!(
            render(
                created_override_update(uid(1), Some(pin_time()), None).build(DbBackend::Postgres)
            ),
            format!(
                r#"UPDATE "issues" SET "created_at" = '2026-11-05 10:00:00.000000 +00:00', "created_by_id" = NULL WHERE "issues"."id" = '{}'"#,
                uid(1)
            )
        );
        assert_eq!(
            render(recent_visit_delete(uid(1), "ws", uid(2)).build(DbBackend::Postgres)),
            format!(
                r#"DELETE FROM "user_recent_visits" WHERE "user_recent_visits"."workspace_id" IN (SELECT "id" FROM "workspaces" WHERE "workspaces"."slug" = 'ws') AND "user_recent_visits"."project_id" = '{}' AND "user_recent_visits"."entity_identifier" = '{}' AND "user_recent_visits"."entity_name" = 'issue'"#,
                uid(1),
                uid(2)
            )
        );
        assert_eq!(
            render(DbBackend::Postgres.build(&default_assignee_member_select(uid(1), uid(2)))),
            format!(
                r#"SELECT "project_members"."id" FROM "project_members" WHERE "project_members"."member_id" = '{}' AND "project_members"."project_id" = '{}' AND "project_members"."role" >= 15 AND "project_members"."is_active" = TRUE AND "project_members"."deleted_at" IS NULL LIMIT 1"#,
                uid(1),
                uid(2)
            )
        );
        let scope = M2mScope {
            issue_id: uid(1),
            project_id: uid(2),
            workspace_id: uid(3),
            created_by_id: Some(uid(4)),
            updated_by_id: None,
        };
        let plain = render(DbBackend::Postgres.build(&m2m_insert_statement(
            M2mTable::Labels,
            &[uid(5), uid(6)],
            scope,
            false,
        )));
        assert!(plain.starts_with(r#"INSERT INTO "issue_labels" ("created_at", "updated_at", "id", "created_by_id", "issue_id", "label_id", "project_id", "updated_by_id", "workspace_id", "deleted_at") VALUES ("#), "{plain}");
        assert_eq!(plain.matches("), (").count(), 1, "two rows: {plain}");
        assert!(!plain.contains("ON CONFLICT"), "{plain}");
        let ignoring = render(DbBackend::Postgres.build(&m2m_insert_statement(
            M2mTable::Assignees,
            &[uid(5)],
            scope,
            true,
        )));
        assert!(
            ignoring.starts_with(r#"INSERT INTO "issue_assignees" ("#),
            "{ignoring}"
        );
        assert!(ignoring.contains(r#""id", "assignee_id", "#), "{ignoring}");
        assert!(ignoring.ends_with(" ON CONFLICT  DO NOTHING"), "{ignoring}");
    }

    #[test]
    fn issue_activity_kwargs_omit_v1_keys() {
        // The v1 `issue_activity` calls carry neither `notification` nor
        // `origin` (`api/views/issue.py:516-525,844-853`).
        let kwargs = issue_activity_kwargs(
            "issue.activity.created",
            Value::String("req".to_owned()),
            &uid(1),
            &uid(2),
            &uid(3),
            Value::Null,
            None,
            None,
            None,
        );
        assert!(!kwargs.contains_key("notification"));
        assert!(!kwargs.contains_key("origin"));
        assert!(!kwargs.contains_key("subscriber"));
        assert_eq!(
            kwargs.get("requested_data").and_then(Value::as_str),
            Some("req")
        );
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
            moved_by_run: None,
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
            None,
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
                    notification: Some(true),
                    origin: Some("https://app.example".to_owned()),
                    require_origin: true,
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
            None,
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
            None,
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
                    notification: Some(true),
                    origin: Some("https://app.example".to_owned()),
                    require_origin: true,
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
                            moved_by_run: None,
                            now: now_micros(),
                        },
                        origin: Some("https://app.example".to_owned()),
                        activity_requested: Value::String("{\"name\": \"svc issue\"}".to_owned()),
                        activity_notification: Some(true),
                        activity_require_origin: true,
                        skip_activity: false,
                        skip_fire: false,
                        model_activity: None,
                        created_override: None,
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
                        m2m_audit: (patched.created_by_id, patched.updated_by_id),
                        sequence: None,
                        fields: patched,
                        assignees: Some(vec![assignee]),
                        labels: None,
                        prev_state: Some(backlog_id),
                        fire: FireSpec {
                            dispatch_immediate: true,
                            created_by: Some(actor),
                            moved_by_run: None,
                            now: now_micros(),
                        },
                        activity: Some(UpdateActivity {
                            origin: Some("https://app.example".to_owned()),
                            requested: "{\"priority\": \"urgent\"}".to_owned(),
                            current_instance: "{\"name\": \"svc issue\"}".to_owned(),
                            raw_body: serde_json::json!({"priority": "urgent"}),
                            slug: "svc-ws".to_owned(),
                            notification: Some(true),
                            require_origin: true,
                            emit_version: true,
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
            None,
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
                    notification: Some(true),
                    origin: None,
                    require_origin: true,
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

    #[tokio::test]
    async fn fire_request_carries_moved_by_run() {
        // The v1 patch stamps the resolved header run on the fire
        // (`views/issue.py:794-807`).
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
        let mut spec = fire_spec();
        spec.moved_by_run = Some(uid(12));
        run_chain(
            &pool,
            &erased_transition,
            &erased_enqueue,
            None,
            vec![ChainEffect::FireTransition {
                issue_id: uid(2),
                prev_state: Some(uid(10)),
                current_state: Some(uid(11)),
                spec,
                actor_id: uid(1),
            }],
            &outcome,
        )
        .await;
        assert!(outcome.stopped().is_none());
        let fires = transition.fires.lock().expect("lock");
        assert_eq!(fires.len(), 1);
        assert_eq!(fires[0].moved_by_run, Some(uid(12)));
        assert!(fires[0].dispatch_immediate);
    }

    #[tokio::test]
    async fn v1_activity_leg_omits_keys_without_stopping() {
        // The v1 legs enqueue with neither key and never stop on the
        // missing origin — the `?` on `app_origin` runs ahead of the
        // model leg instead.
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
            None,
            vec![ChainEffect::IssueActivity(IssueActivitySpec {
                event: "issue.activity.created".to_owned(),
                requested: Value::String("req".to_owned()),
                actor: uid(1),
                issue: uid(2),
                project: uid(3),
                current: Value::Null,
                notification: None,
                origin: None,
                require_origin: false,
                subscriber: None,
            })],
            &outcome,
        )
        .await;
        assert!(outcome.stopped().is_none());
        let calls = enqueue.calls.lock().expect("lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, ISSUE_ACTIVITY_TASK);
        assert!(!calls[0].2.contains_key("notification"));
        assert!(!calls[0].2.contains_key("origin"));
    }

    /// The save-time `sequence_id` check runs past the m2m legs: a bad
    /// sequence 500s with the m2m writes already persisted and the row
    /// untouched; a good one lands on the save.
    #[tokio::test]
    async fn partial_update_sequence_check_runs_past_m2m() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let db = pidash_db::sea_orm_connection(&pool);
        let actor = Uuid::new_v4();
        let old_link = Uuid::new_v4();
        let new_link = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let backlog_id = Uuid::new_v4();
        for user_id in [actor, old_link, new_link] {
            let username = format!("sq-{}", user_id.simple());
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
             background_color, owner_id) VALUES ($1, now(), now(), 'sq ws', $2, 'UTC', \
             '#ffffff', $3)",
        )
        .bind(workspace_id)
        .bind(format!("sq-{workspace_id}"))
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
             'sq project', '', 0, $2, $3, $4, true, true, true, true, false, 0, 0, '{}', true, \
             true, false, 'UTC', false, '', '', 0, 0, false, false, 0, '', 0, now(), now())",
        )
        .bind(project_id)
        .bind(format!(
            "Q{:08X}",
            (project_id.as_u128() & 0xffff_ffff) as u32
        ))
        .bind(workspace_id)
        .bind(actor)
        .execute(&pool)
        .await
        .expect("seed project");
        sqlx::query(
            "INSERT INTO states (id, created_at, updated_at, name, description, color, \
             slug, project_id, workspace_id, sequence, \"group\", \"default\", is_triage) \
             VALUES ($1, now(), now(), 'Backlog', '', '#ff0000', $2, $3, $4, 100.0, \
             'backlog', true, false)",
        )
        .bind(backlog_id)
        .bind(format!("sq-backlog-{project_id}"))
        .bind(project_id)
        .bind(workspace_id)
        .execute(&pool)
        .await
        .expect("seed state");
        let issue_id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO "issues" ("id", "created_at", "updated_at", "deleted_at", "point",
             "name", "description_json", "description_html", "description_stripped",
             "description_binary", "priority", "complexity_score", "start_date",
             "target_date", "sequence_id", "sort_order", "completed_at", "archived_at",
             "is_draft", "external_source", "external_id", "git_work_branch", "workpad",
             "created_via", "agent_executor", "created_by_id", "updated_by_id", "project_id",
             "workspace_id", "parent_id", "state_id", "estimate_point_id", "type_id",
             "assigned_pod_id")
             VALUES ($1, now(), now(), NULL, NULL, 'sq issue', '{}', '<p>Hi</p>', 'Hi',
             NULL, 'none', 0, NULL, NULL, 1, 65535.0, NULL, NULL, false, NULL, NULL, '',
             '', NULL, NULL, $2, NULL, $3, $4, NULL, $5, NULL, NULL, NULL)"#,
        )
        .bind(issue_id)
        .bind(actor)
        .bind(project_id)
        .bind(workspace_id)
        .bind(backlog_id)
        .execute(&pool)
        .await
        .expect("seed issue");
        sqlx::query(
            r#"INSERT INTO "issue_assignees" ("id", "created_at", "updated_at", "created_by_id",
             "updated_by_id", "deleted_at", "project_id", "workspace_id", "issue_id",
             "assignee_id") VALUES ($1, now(), now(), $2, NULL, NULL, $3, $4, $5, $6)"#,
        )
        .bind(Uuid::new_v4())
        .bind(actor)
        .bind(project_id)
        .bind(workspace_id)
        .bind(issue_id)
        .bind(old_link)
        .execute(&pool)
        .await
        .expect("seed assignee link");

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
        let run_patch = |sequence: Option<Result<i32, ()>>| {
            let transition = transition.clone();
            let enqueue = enqueue.clone();
            let pool = pool.clone();
            let ctx = ctx.clone();
            let db = db.clone();
            async move {
                super::super::run(&ctx, &db, |txn| {
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
                                m2m_audit: (Some(actor), None),
                                sequence,
                                fields: IssueFieldSet {
                                    id: issue_id,
                                    created_at: now_micros(),
                                    updated_at: now_micros(),
                                    created_by_id: Some(actor),
                                    updated_by_id: None,
                                    deleted_at: None,
                                    project_id,
                                    workspace_id,
                                    parent_id: None,
                                    state_id: Some(backlog_id),
                                    point: None,
                                    estimate_point_id: None,
                                    name: "sq issue v2".to_owned(),
                                    description_json: Value::Object(Map::new()),
                                    description_html: "<p>Hi</p>".to_owned(),
                                    description_stripped: Some("Hi".to_owned()),
                                    description_binary: None,
                                    priority: "none".to_owned(),
                                    complexity_score: 0,
                                    start_date: None,
                                    target_date: None,
                                    sequence_id: 1,
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
                                },
                                assignees: Some(vec![new_link]),
                                labels: None,
                                prev_state: Some(backlog_id),
                                fire: FireSpec {
                                    dispatch_immediate: true,
                                    created_by: Some(actor),
                                    moved_by_run: None,
                                    now: now_micros(),
                                },
                                activity: None,
                            },
                        )
                        .await
                    })
                })
                .await
            }
        };

        // The bad sequence fails the write, but the m2m leg (sweep + new
        // link) already persisted on the pool, and the row never saved.
        let failed = run_patch(Some(Err(()))).await;
        assert!(matches!(failed, Err(DomainError::Work(_))));
        let swept: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM issue_assignees WHERE issue_id = $1 AND assignee_id = $2 \
             AND deleted_at IS NOT NULL",
        )
        .bind(issue_id)
        .bind(old_link)
        .fetch_one(&pool)
        .await
        .expect("sweep reread");
        assert_eq!(swept.0, 1);
        let live: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM issue_assignees WHERE issue_id = $1 AND assignee_id = $2 \
             AND deleted_at IS NULL",
        )
        .bind(issue_id)
        .bind(new_link)
        .fetch_one(&pool)
        .await
        .expect("link reread");
        assert_eq!(live.0, 1);
        let row: (String, i32) =
            sqlx::query_as(r#"SELECT "name", "sequence_id" FROM "issues" WHERE "id" = $1"#)
                .bind(issue_id)
                .fetch_one(&pool)
                .await
                .expect("row reread");
        assert_eq!(row, ("sq issue".to_owned(), 1));

        // The good sequence lands on the save.
        let outcome = run_patch(Some(Ok(9))).await.expect("good patch");
        assert!(outcome.chain.stopped().is_none());
        assert_eq!(outcome.issue.sequence_id, 9);

        for sql in [
            "DELETE FROM issue_assignees WHERE issue_id = $1",
            "DELETE FROM issues WHERE id = $1",
        ] {
            sqlx::query(sql)
                .bind(issue_id)
                .execute(&pool)
                .await
                .expect("cleanup");
        }
    }

    /// End-to-end through `domain::run` with recording hooks: a workpad
    /// write on a live issue lands the text + audit + hook recomputes with
    /// no completion leg, a NULL-state write takes the default state and
    /// records the completion pair, and a missing issue is `NotFound`.
    #[tokio::test]
    async fn set_workpad_roundtrip() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let db = pidash_db::sea_orm_connection(&pool);
        let actor = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let slug = format!("wp-{workspace_id}");
        let project_id = Uuid::new_v4();
        let backlog_id = Uuid::new_v4();
        let username = format!("wp-{}", actor.simple());
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
        .bind(actor)
        .bind(username.clone())
        .bind(format!("{username}@example.test"))
        .bind("t".repeat(64))
        .execute(&pool)
        .await
        .expect("seed user");
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, slug, timezone, \
             background_color, owner_id) VALUES ($1, now(), now(), 'wp ws', $2, 'UTC', \
             '#ffffff', $3)",
        )
        .bind(workspace_id)
        .bind(&slug)
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
             'wp project', '', 0, $2, $3, $4, true, true, true, true, false, 0, 0, '{}', true, \
             true, false, 'UTC', false, '', '', 0, 0, false, false, 0, '', 0, now(), now())",
        )
        .bind(project_id)
        .bind(format!(
            "W{:08X}",
            (project_id.as_u128() & 0xffff_ffff) as u32
        ))
        .bind(workspace_id)
        .bind(actor)
        .execute(&pool)
        .await
        .expect("seed project");
        sqlx::query(
            "INSERT INTO states (id, created_at, updated_at, name, description, color, \
             slug, project_id, workspace_id, sequence, \"group\", \"default\", is_triage) \
             VALUES ($1, now(), now(), 'Backlog', '', '#ff0000', $2, $3, $4, 100.0, \
             'backlog', true, false)",
        )
        .bind(backlog_id)
        .bind(format!("wp-backlog-{project_id}"))
        .bind(project_id)
        .bind(workspace_id)
        .execute(&pool)
        .await
        .expect("seed state");
        // Raw inserts (no hooks): one live-state issue, one NULL-state.
        let live_id = Uuid::new_v4();
        let bare_id = Uuid::new_v4();
        for (id, state) in [(live_id, Some(backlog_id)), (bare_id, None)] {
            sqlx::query(
                r#"INSERT INTO "issues" ("id", "created_at", "updated_at", "deleted_at", "point",
                 "name", "description_json", "description_html", "description_stripped",
                 "description_binary", "priority", "complexity_score", "start_date",
                 "target_date", "sequence_id", "sort_order", "completed_at", "archived_at",
                 "is_draft", "external_source", "external_id", "git_work_branch", "workpad",
                 "created_via", "agent_executor", "created_by_id", "updated_by_id", "project_id",
                 "workspace_id", "parent_id", "state_id", "estimate_point_id", "type_id",
                 "assigned_pod_id")
                 VALUES ($1, now(), now(), NULL, NULL, 'wp issue', '{}', '<p>Hi</p>', NULL,
                 NULL, 'none', 0, NULL, NULL, 1, 65535.0, NULL, NULL, false, NULL, NULL, '',
                 '', NULL, NULL, $2, NULL, $3, $4, NULL, $5, NULL, NULL, NULL)"#,
            )
            .bind(id)
            .bind(actor)
            .bind(project_id)
            .bind(workspace_id)
            .bind(state)
            .execute(&pool)
            .await
            .expect("seed issue");
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
        // `db` clones per call (`DatabaseConnection` is an `Arc` handle):
        // the `run` future borrows it, so a shared borrow would make the
        // closure `FnOnce`.
        let run_workpad = |issue: Uuid, workpad: &str, stamp: DateTime<Utc>| {
            let transition = transition.clone();
            let enqueue = enqueue.clone();
            let pool = pool.clone();
            let ctx = ctx.clone();
            let slug = slug.clone();
            let workpad = workpad.to_owned();
            let db = db.clone();
            async move {
                super::super::run(&ctx, &db, |txn| {
                    let transition = transition.clone();
                    let enqueue = enqueue.clone();
                    let pool = pool.clone();
                    let ctx = ctx.clone();
                    let slug = slug.clone();
                    let workpad = workpad.clone();
                    Box::pin(async move {
                        let transition_hook: Arc<dyn IssueTransitionHook> = transition;
                        let enqueue_hook: Arc<dyn TaskEnqueueHook> = enqueue;
                        txn.hooks_mut().register(transition_hook);
                        txn.hooks_mut().register(enqueue_hook);
                        set_workpad(
                            &ctx,
                            txn,
                            &pool,
                            SetWorkpad {
                                issue_id: issue,
                                project_id,
                                slug,
                                workpad,
                                updated_at: stamp,
                            },
                        )
                        .await
                    })
                })
                .await
            }
        };

        // The live-state write: text + audit + stripped recompute, the
        // backlog state kept, no completion leg, no fire, no tasks.
        let stamp = now_micros();
        let outcome = run_workpad(live_id, "live notes", stamp)
            .await
            .expect("workpad write");
        assert!(outcome.stopped().is_none());
        let row: (String, Option<Uuid>, Option<DateTime<Utc>>) = sqlx::query_as(
            r#"SELECT "workpad", "state_id", "completed_at" FROM "issues" WHERE "id" = $1"#,
        )
        .bind(live_id)
        .fetch_one(&pool)
        .await
        .expect("workpad reread");
        assert_eq!(row.0, "live notes");
        assert_eq!(row.1, Some(backlog_id));
        assert!(row.2.is_none());
        let audit: (Option<Uuid>, DateTime<Utc>, Option<String>) = sqlx::query_as(
            r#"SELECT "updated_by_id", "updated_at", "description_stripped"
             FROM "issues" WHERE "id" = $1"#,
        )
        .bind(live_id)
        .fetch_one(&pool)
        .await
        .expect("workpad audit reread");
        assert_eq!(audit.0, Some(actor));
        assert_eq!(audit.1, stamp);
        assert_eq!(audit.2.as_deref(), Some("Hi"));
        assert!(transition.comments.lock().expect("lock").is_empty());
        assert!(transition.fires.lock().expect("lock").is_empty());
        assert!(enqueue.calls.lock().expect("lock").is_empty());

        // The NULL-state write: the default state assigns, `completed_at`
        // stays untouched, the completion pair records the move.
        let stamp = now_micros();
        let outcome = run_workpad(bare_id, "bare notes", stamp)
            .await
            .expect("bare workpad write");
        assert!(outcome.stopped().is_none());
        let row: (String, Option<Uuid>, Option<DateTime<Utc>>) = sqlx::query_as(
            r#"SELECT "workpad", "state_id", "completed_at" FROM "issues" WHERE "id" = $1"#,
        )
        .bind(bare_id)
        .fetch_one(&pool)
        .await
        .expect("bare reread");
        assert_eq!(row.0, "bare notes");
        assert_eq!(row.1, Some(backlog_id));
        assert!(row.2.is_none());
        assert_eq!(
            *transition.comments.lock().expect("lock"),
            vec![(bare_id, None, Some(backlog_id))]
        );
        assert!(transition.fires.lock().expect("lock").is_empty());

        // A missing issue is `NotFound` (the drivers' 404 arm).
        let missing = run_workpad(Uuid::new_v4(), "gone", now_micros()).await;
        assert!(missing.is_err_and(|error| error.is_not_found()));

        for id in [live_id, bare_id] {
            sqlx::query("DELETE FROM issues WHERE id = $1")
                .bind(id)
                .execute(&pool)
                .await
                .expect("cleanup issue");
        }
    }

    /// The mirror INSERT reread columns (name, html, stripped, json,
    /// created_by, updated_by, state, sequence, sort, ext_source, ext_id).
    type MirrorRereadRow = (
        String,
        String,
        String,
        Value,
        Option<Uuid>,
        Option<Uuid>,
        Option<Uuid>,
        i32,
        f64,
        Option<String>,
        Option<String>,
    );

    /// The github-arm reread columns (name, html, stripped, completed_at,
    /// state, updated_at).
    type GithubRereadRow = (
        String,
        String,
        String,
        Option<DateTime<Utc>>,
        Option<Uuid>,
        DateTime<Utc>,
    );

    /// The sync/ticker/automation bare writes land their exact columns:
    /// the mirror INSERT carries every sentinel (a bind-order swap fails),
    /// the github arm rewrites the render triple + `completed_at`, the git
    /// arm passes its stripped value through untouched, the ticker move
    /// leaves `updated_by_id` alone, and the bulks touch one column each
    /// (including the NULL-state QUIRK-5 close). (Adopt is in-memory only
    /// per PIDASHCONV-1576 — no persist, no leg here.)
    #[tokio::test]
    async fn sync_ticker_automation_bare_roundtrip() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let actor = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let backlog_id = Uuid::new_v4();
        let done_id = Uuid::new_v4();
        let username = format!("bare-{}", actor.simple());
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
        .bind(actor)
        .bind(username.clone())
        .bind(format!("{username}@example.test"))
        .bind("t".repeat(64))
        .execute(&pool)
        .await
        .expect("seed user");
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, slug, timezone, \
             background_color, owner_id) VALUES ($1, now(), now(), 'bare ws', $2, 'UTC', \
             '#ffffff', $3)",
        )
        .bind(workspace_id)
        .bind(format!("bare-{workspace_id}"))
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
             'bare project', '', 0, $2, $3, $4, true, true, true, true, false, 0, 0, '{}', true, \
             true, false, 'UTC', false, '', '', 0, 0, false, false, 0, '', 0, now(), now())",
        )
        .bind(project_id)
        .bind(format!(
            "B{:08X}",
            (project_id.as_u128() & 0xffff_ffff) as u32
        ))
        .bind(workspace_id)
        .bind(actor)
        .execute(&pool)
        .await
        .expect("seed project");
        for (id, name, group, sequence, default) in [
            (backlog_id, "Backlog", "backlog", 100.0, true),
            (done_id, "Done", "completed", 200.0, false),
        ] {
            sqlx::query(
                "INSERT INTO states (id, created_at, updated_at, name, description, color, \
                 slug, project_id, workspace_id, sequence, \"group\", \"default\", is_triage) \
                 VALUES ($1, now(), now(), $2, '', '#ff0000', $3, $4, $5, $6, $7, $8, false)",
            )
            .bind(id)
            .bind(name)
            .bind(format!("bare-{name}-{sequence}-{project_id}"))
            .bind(project_id)
            .bind(workspace_id)
            .bind(sequence)
            .bind(group)
            .bind(default)
            .execute(&pool)
            .await
            .expect("seed state");
        }

        let t0 = now_micros();
        let issue_id = Uuid::new_v4();
        let rows = insert_mirror_issue(
            &pidash_db::pool::sea_orm_connection(&pool),
            &MirrorIssueInsert {
                issue_id,
                now: t0,
                actor_id: actor,
                project_id,
                workspace_id,
                state_id: None,
                name: "mirror-name".to_owned(),
                description_html: "<p>mirror-html</p>".to_owned(),
                description_stripped: "mirror-stripped".to_owned(),
                sequence_id: 7,
                sort_order: 65535.0,
                completed_at: None,
                external_source: "github".to_owned(),
                external_id: "4242".to_owned(),
                pod_id: None,
            },
        )
        .await
        .expect("mirror insert");
        assert_eq!(rows, 1);
        let row: MirrorRereadRow = sqlx::query_as(
            "SELECT name, description_html, description_stripped, description_json, \
             created_by_id, updated_by_id, state_id, sequence_id, sort_order, \
             external_source, external_id FROM issues WHERE id = $1",
        )
        .bind(issue_id)
        .fetch_one(&pool)
        .await
        .expect("mirror reread");
        assert_eq!(
            row,
            (
                "mirror-name".to_owned(),
                "<p>mirror-html</p>".to_owned(),
                "mirror-stripped".to_owned(),
                Value::Object(Map::new()),
                Some(actor),
                Some(actor),
                None,
                7,
                65535.0,
                Some("github".to_owned()),
                Some("4242".to_owned()),
            )
        );
        let stamps: (DateTime<Utc>, DateTime<Utc>) =
            sqlx::query_as("SELECT created_at, updated_at FROM issues WHERE id = $1")
                .bind(issue_id)
                .fetch_one(&pool)
                .await
                .expect("mirror stamps");
        assert_eq!(stamps, (t0, t0));

        let rows = insert_issue_sequence(
            &pidash_db::pool::sea_orm_connection(&pool),
            &MirrorIssueSequence {
                project_id,
                workspace_id,
                issue_id,
                sequence: 7,
                now: t0,
            },
        )
        .await
        .expect("sequence insert");
        assert_eq!(rows, 1);
        let seq: (Uuid, i64, Option<Uuid>, bool) = sqlx::query_as(
            "SELECT issue_id, sequence, created_by_id, deleted FROM issue_sequences \
             WHERE issue_id = $1",
        )
        .bind(issue_id)
        .fetch_one(&pool)
        .await
        .expect("sequence reread");
        assert_eq!(seq, (issue_id, 7, None, false));

        // Github arm: render triple + completed + auto_now; state kept.
        let t1 = now_micros();
        let rows = update_github_mirror_issue(
            &pidash_db::pool::sea_orm_connection(&pool),
            &GithubMirrorIssueUpdate {
                issue_id,
                name: "gh-name".to_owned(),
                description_html: "<p>gh</p>".to_owned(),
                description_stripped: "gh-stripped".to_owned(),
                actor_id: actor,
                completed_at: Some(t1),
                now: t1,
            },
        )
        .await
        .expect("github update");
        assert_eq!(rows, 1);
        let updated: GithubRereadRow = sqlx::query_as(
            "SELECT name, description_html, description_stripped, completed_at, state_id, \
             updated_at FROM issues WHERE id = $1",
        )
        .bind(issue_id)
        .fetch_one(&pool)
        .await
        .expect("github reread");
        assert_eq!(
            updated,
            (
                "gh-name".to_owned(),
                "<p>gh</p>".to_owned(),
                "gh-stripped".to_owned(),
                Some(t1),
                None,
                t1,
            )
        );

        // Git arm: absolute values, stripped passed through, state kept.
        let t2 = now_micros();
        let rows = update_git_mirror_issue(
            &pidash_db::pool::sea_orm_connection(&pool),
            &GitMirrorIssueUpdate {
                issue_id,
                name: "git-name".to_owned(),
                description_html: "<p>git</p>".to_owned(),
                description_stripped: "raw safe_render value".to_owned(),
                workspace_id,
                actor_id: actor,
                external_source: "gitlab".to_owned(),
                external_id: "99".to_owned(),
                now: t2,
            },
        )
        .await
        .expect("git update");
        assert_eq!(rows, 1);
        let git: (String, String, Option<String>, Option<String>, Option<Uuid>) = sqlx::query_as(
            "SELECT name, description_stripped, external_source, external_id, state_id \
             FROM issues WHERE id = $1",
        )
        .bind(issue_id)
        .fetch_one(&pool)
        .await
        .expect("git reread");
        assert_eq!(
            git,
            (
                "git-name".to_owned(),
                "raw safe_render value".to_owned(),
                Some("gitlab".to_owned()),
                Some("99".to_owned()),
                None,
            )
        );

        // Ticker move: state + `updated_at` only; audit untouched.
        let t3 = now_micros();
        let rows = update_issue_state_no_audit(
            &pidash_db::pool::sea_orm_connection(&pool),
            issue_id,
            backlog_id,
            t3,
        )
        .await
        .expect("ticker move");
        assert_eq!(rows, 1);
        let moved: (Option<Uuid>, DateTime<Utc>, Option<Uuid>) =
            sqlx::query_as("SELECT state_id, updated_at, updated_by_id FROM issues WHERE id = $1")
                .bind(issue_id)
                .fetch_one(&pool)
                .await
                .expect("ticker reread");
        assert_eq!(moved, (Some(backlog_id), t3, Some(actor)));

        // Automation bulks: one column each, NULL close included.
        let day = chrono::NaiveDate::from_ymd_opt(2026, 1, 2).expect("bulk day");
        let rows = bulk_set_archived_at(
            &pidash_db::pool::sea_orm_connection(&pool),
            &[issue_id],
            day,
        )
        .await
        .expect("bulk archive");
        assert_eq!(rows, 1);
        let rows = bulk_set_state(
            &pidash_db::pool::sea_orm_connection(&pool),
            &[issue_id],
            None,
        )
        .await
        .expect("bulk close null");
        assert_eq!(rows, 1);
        let bulked: (Option<NaiveDate>, Option<Uuid>) =
            sqlx::query_as("SELECT archived_at, state_id FROM issues WHERE id = $1")
                .bind(issue_id)
                .fetch_one(&pool)
                .await
                .expect("bulk reread");
        assert_eq!(bulked, (Some(day), None));

        // Cleanup in FK order.
        for sql in [
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
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(actor)
            .execute(&pool)
            .await
            .expect("cleanup user");
    }

    static WORKPAD_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx02_reads/workpad.golden.json");

    #[test]
    fn workpad_save_update_sets_both_update_fields() {
        // The fixture's `update_fields` order (`updated_at`, then
        // `workpad`) on the built statement.
        let parsed: Value = serde_json::from_str(WORKPAD_FIXTURE).expect("fixture parses");
        let rendered = parsed["set_update_sql"][0]
            .as_str()
            .expect("set_update_sql");
        assert!(rendered.starts_with("UPDATE \"issues\" SET \"updated_at\" = "));
        assert!(rendered.contains(", \"workpad\" = "));
        let built =
            render(workpad_save_update(uid(1), Some("w"), pin_time()).build(DbBackend::Postgres));
        assert!(
            built.starts_with("UPDATE \"issues\" SET \"updated_at\" = "),
            "{built}"
        );
        assert!(built.contains(", \"workpad\" = 'w'"), "{built}");
    }

    const WORKPAD_ISSUES_DDL: &str = "CREATE TEMPORARY TABLE issues (
        id UUID PRIMARY KEY, workpad TEXT, updated_at TIMESTAMPTZ NOT NULL)";

    fn live_workpad_uuid(tag: &str) -> Uuid {
        Uuid::parse_str(&format!("11111111-2222-3333-4444-{tag}")).expect("fixed uuid")
    }

    /// `workpad.golden.json` replay: fresh issue reads `""`, set /
    /// overwrite / clear round-trip, `None` and `NULL` read as `""`.
    /// Moved with the write from `pidash_db::orchestration::workpad`
    /// (PIDASHCONV-1630); the read leg still lives there.
    #[tokio::test]
    async fn live_workpad_roundtrip_replays_fixture() {
        use pidash_db::orchestration::fetch_workpad;
        use sqlx::Row;

        let Some(pool) = live_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(WORKPAD_ISSUES_DDL)
            .execute(&mut *tx)
            .await
            .expect("temp issues");
        let issue = live_workpad_uuid("000000003020");
        let epoch: chrono::DateTime<chrono::Utc> =
            "2026-10-03T11:00:00+00:00".parse().expect("epoch");
        sqlx::query("INSERT INTO issues (id, workpad, updated_at) VALUES ($1, '', $2)")
            .bind(issue)
            .bind(epoch)
            .execute(&mut *tx)
            .await
            .expect("seed issue");
        assert_eq!(fetch_workpad(&mut *tx, issue).await.expect("get fresh"), "");
        let now: chrono::DateTime<chrono::Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        update_workpad_save(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            issue,
            Some("## Agent Workpad\n\n- implementing\n"),
            now,
        )
        .await
        .expect("set");
        assert_eq!(
            fetch_workpad(&mut *tx, issue).await.expect("get set"),
            "## Agent Workpad\n\n- implementing\n"
        );
        let stamp: chrono::DateTime<chrono::Utc> =
            sqlx::query("SELECT updated_at FROM issues WHERE id = $1")
                .bind(issue)
                .fetch_one(&mut *tx)
                .await
                .expect("read back")
                .try_get("updated_at")
                .expect("updated_at");
        assert_eq!(stamp, now);
        update_workpad_save(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            issue,
            Some("second"),
            now,
        )
        .await
        .expect("overwrite");
        assert_eq!(
            fetch_workpad(&mut *tx, issue).await.expect("get overwrite"),
            "second"
        );
        update_workpad_save(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            issue,
            Some(""),
            now,
        )
        .await
        .expect("clear");
        assert_eq!(
            fetch_workpad(&mut *tx, issue).await.expect("get cleared"),
            ""
        );
        update_workpad_save(&pidash_db::orm_tx::TxConn::new(&mut tx), issue, None, now)
            .await
            .expect("set none");
        assert_eq!(fetch_workpad(&mut *tx, issue).await.expect("get none"), "");
        // A `NULL` workpad (unreachable on the real `NOT NULL`
        // column) still reads as `""`, like `issue.workpad or ""`.
        sqlx::query("UPDATE issues SET workpad = NULL WHERE id = $1")
            .bind(issue)
            .execute(&mut *tx)
            .await
            .expect("null out");
        assert_eq!(fetch_workpad(&mut *tx, issue).await.expect("get null"), "");
        assert!(matches!(
            fetch_workpad(&mut *tx, live_workpad_uuid("000000009999")).await,
            Err(sqlx::Error::RowNotFound)
        ));
    }

    /// A one-connection pool whose session temp copies (no constraints)
    /// shadow the `public` tables the SeaORM writes below touch; `None`
    /// without `DATABASE_URL`.
    async fn temp_table_pool() -> Option<PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return None;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        for table in [
            "issues",
            "issue_sequences",
            "issue_assignees",
            "issue_labels",
            "states",
            "projects",
            "workspaces",
            "project_members",
            "user_recent_visits",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        Some(pool)
    }

    /// Live: the bare-column issue / sequence / assignee writes on temp
    /// tables — tool, seed and demo rows, the dynamic tool save, the
    /// touches, the workpad/state saves, the estimate reassign (spanning
    /// subquery), the bulk CASE dates, the parent link, the scalars, the
    /// automation bulks, the repair plan, the bulk archive and the scoped
    /// bulk soft delete.
    #[tokio::test]
    async fn live_bare_issue_writes() {
        let Some(pool) = temp_table_pool().await else {
            return;
        };
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let (ws, project, actor, state) = (uid(901), uid(902), uid(903), uid(904));
        sqlx::query("INSERT INTO workspaces (id, slug) VALUES ($1, 'live-ws'), ($2, 'other-ws')")
            .bind(ws)
            .bind(uid(905))
            .execute(&pool)
            .await
            .expect("seed workspaces");
        sqlx::query("INSERT INTO projects (id, archived_at) VALUES ($1, NULL)")
            .bind(project)
            .execute(&pool)
            .await
            .expect("seed project");
        let now = now_micros();

        // Tool create + sequence row, then the dynamic save.
        let tool = uid(1);
        let rows = insert_tool_issue(
            &db,
            &ToolIssueInsert {
                issue_id: tool,
                now,
                name: "tool",
                description_html: "<p>t</p>",
                description_stripped: Some("t"),
                priority: "high",
                sequence_id: 5,
                sort_order: 65535.0,
                completed_at: None,
                created_via: "assistant",
                created_by: actor,
                project_id: project,
                workspace_id: ws,
                parent_id: None,
                state_id: Some(state),
                pod_id: None,
            },
        )
        .await
        .expect("tool insert");
        assert_eq!(rows, 1);
        let rows = insert_tool_issue_sequence(
            &db,
            &ToolIssueSequence {
                project_id: project,
                workspace_id: ws,
                issue_id: tool,
                sequence: 5,
                created_by: actor,
                now,
            },
        )
        .await
        .expect("tool sequence");
        assert_eq!(rows, 1);
        let seq: (i64, Option<Uuid>, Option<Uuid>, bool) = sqlx::query_as(
            "SELECT sequence, created_by_id, updated_by_id, deleted FROM issue_sequences \
             WHERE issue_id = $1",
        )
        .bind(tool)
        .fetch_one(&pool)
        .await
        .expect("tool sequence reread");
        assert_eq!(seq, (5, Some(actor), None, false));
        let later = now + chrono::Duration::seconds(5);
        let rows = update_tool_issue(
            &db,
            &ToolIssueUpdate {
                issue_id: tool,
                name: Some("renamed"),
                description_html: None,
                priority: None,
                state_given: true,
                state_id: None,
                parent_touched: false,
                parent_id: None,
                updated_by_id: actor,
                now: later,
            },
        )
        .await
        .expect("tool update");
        assert_eq!(rows, 1);
        type ToolRow = (
            String,
            String,
            Option<Uuid>,
            Option<Uuid>,
            Option<String>,
            i32,
            DateTime<Utc>,
            Option<Uuid>,
        );
        let row: ToolRow = sqlx::query_as(
            "SELECT name, priority, state_id, created_by_id, created_via, sequence_id, \
             updated_at, updated_by_id FROM issues WHERE id = $1",
        )
        .bind(tool)
        .fetch_one(&pool)
        .await
        .expect("tool reread");
        assert_eq!(
            row,
            (
                "renamed".to_owned(),
                "high".to_owned(),
                None,
                Some(actor),
                Some("assistant".to_owned()),
                5,
                later,
                Some(actor),
            )
        );

        // Seed issue + sequence: client ids returned.
        let seed = insert_seed_issue(
            &db,
            &SeedIssue {
                workspace_id: ws,
                project_id: project,
                state_id: state,
                created_by: actor,
                name: "seed",
                description_html: "<p>s</p>",
                description_stripped: None,
                priority: "none",
                sequence_id: 6,
                sort_order: 1.0,
                completed_at: Some(now),
                assigned_pod_id: None,
                now,
            },
        )
        .await
        .expect("seed insert");
        let seeded: (String, Option<Uuid>, Option<DateTime<Utc>>) =
            sqlx::query_as("SELECT name, updated_by_id, completed_at FROM issues WHERE id = $1")
                .bind(seed)
                .fetch_one(&pool)
                .await
                .expect("seed reread");
        assert_eq!(seeded, ("seed".to_owned(), None, Some(now)));
        let seed_seq = insert_seed_issue_sequence(
            &db,
            &SeedIssueSequence {
                issue_id: seed,
                project_id: project,
                workspace_id: ws,
                sequence: 6,
                now,
            },
        )
        .await
        .expect("seed sequence");
        let seq: (Option<Uuid>, i64) =
            sqlx::query_as("SELECT issue_id, sequence FROM issue_sequences WHERE id = $1")
                .bind(seed_seq)
                .fetch_one(&pool)
                .await
                .expect("seed sequence reread");
        assert_eq!(seq, (Some(seed), 6));

        // Demo rows from `::uuid` strings; a malformed id fails up front.
        let (ws_s, project_s, actor_s, state_s) = (
            ws.to_string(),
            project.to_string(),
            actor.to_string(),
            state.to_string(),
        );
        let demo = DemoIssue {
            state_id: &state_s,
            project_id: &project_s,
            workspace_id: &ws_s,
            created_by: &actor_s,
            name: "demo",
            description_html: "<p>d</p>",
            description_stripped: "d",
            sequence_id: 7,
            sort_order: 2.0,
            start_date: None,
            target_date: None,
            priority: "low",
            now,
        };
        let demo_id = insert_demo_issue(&db, &demo).await.expect("demo insert");
        let demo_uuid = Uuid::parse_str(&demo_id).expect("demo id");
        let demo_row: (String, i32, Option<Uuid>) =
            sqlx::query_as("SELECT name, sequence_id, created_by_id FROM issues WHERE id = $1")
                .bind(demo_uuid)
                .fetch_one(&pool)
                .await
                .expect("demo reread");
        assert_eq!(demo_row, ("demo".to_owned(), 7, Some(actor)));
        assert!(matches!(
            insert_demo_issue(
                &db,
                &DemoIssue {
                    state_id: "not-a-uuid",
                    ..demo
                }
            )
            .await,
            Err(sqlx::Error::Encode(_))
        ));
        insert_demo_issue_sequence(
            &db,
            &DemoIssueSequence {
                issue_id: &demo_id,
                project_id: &project_s,
                workspace_id: &ws_s,
                sequence: 7,
                now,
            },
        )
        .await
        .expect("demo sequence");
        insert_demo_issue_assignee(
            &db,
            &DemoIssueAssignee {
                issue_id: &demo_id,
                assignee_id: &actor_s,
                project_id: &project_s,
                workspace_id: &ws_s,
                now,
            },
        )
        .await
        .expect("demo assignee");
        let demo_links: (i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM issue_sequences WHERE issue_id = $1), \
             (SELECT count(*) FROM issue_assignees WHERE issue_id = $1 AND assignee_id = $2)",
        )
        .bind(demo_uuid)
        .bind(actor)
        .fetch_one(&pool)
        .await
        .expect("demo links");
        assert_eq!(demo_links, (1, 1));

        // Workpad + agent-tool state saves (bare columns).
        assert_eq!(
            update_workpad(&db, tool, project, "pad", later)
                .await
                .expect("workpad"),
            1
        );
        assert_eq!(
            update_workpad(&db, tool, uid(999), "pad", later)
                .await
                .expect("workpad other project"),
            0
        );
        assert_eq!(
            update_issue_state(&db, tool, state, later, uid(906))
                .await
                .expect("state save"),
            1
        );
        let pad: (String, Option<Uuid>, Option<Uuid>) =
            sqlx::query_as("SELECT workpad, state_id, updated_by_id FROM issues WHERE id = $1")
                .bind(tool)
                .fetch_one(&pool)
                .await
                .expect("workpad reread");
        assert_eq!(pad, ("pad".to_owned(), Some(state), Some(uid(906))));

        // Touches: the bare touch ignores soft-delete, the set touch
        // honors it.
        sqlx::query("UPDATE issues SET updated_at = $1")
            .bind(now)
            .execute(&pool)
            .await
            .expect("reset stamps");
        sqlx::query("UPDATE issues SET deleted_at = $1 WHERE id = $2")
            .bind(now)
            .bind(seed)
            .execute(&pool)
            .await
            .expect("soft delete seed");
        touch_issues(&db, &[tool, seed]).await.expect("touch set");
        let touched: Vec<(Uuid,)> =
            sqlx::query_as("SELECT id FROM issues WHERE updated_at > $1 ORDER BY id")
                .bind(now)
                .fetch_all(&pool)
                .await
                .expect("touched");
        assert_eq!(touched, vec![(tool,)]);
        touch_issue(&db, seed).await.expect("touch one");
        let seed_touched: (bool,) =
            sqlx::query_as("SELECT updated_at > $1 FROM issues WHERE id = $2")
                .bind(now)
                .bind(seed)
                .fetch_one(&pool)
                .await
                .expect("seed touched");
        assert_eq!(seed_touched, (true,));
        sqlx::query("UPDATE issues SET deleted_at = NULL WHERE id = $1")
            .bind(seed)
            .execute(&pool)
            .await
            .expect("restore seed");

        // Estimate reassign: only the live issues in the project + slug
        // on the old point move; `updated_at` untouched.
        let (old_point, new_point) = (uid(910), uid(911));
        sqlx::query("UPDATE issues SET estimate_point_id = $1, updated_at = $2")
            .bind(old_point)
            .bind(now)
            .execute(&pool)
            .await
            .expect("set old point");
        sqlx::query("UPDATE issues SET deleted_at = $1 WHERE id = $2")
            .bind(now)
            .bind(demo_uuid)
            .execute(&pool)
            .await
            .expect("soft delete demo");
        reassign_estimate_point(&db, new_point, project, "other-ws", old_point)
            .await
            .expect("reassign wrong slug");
        reassign_estimate_point(&db, new_point, project, "live-ws", old_point)
            .await
            .expect("reassign");
        let points: Vec<(Uuid, Option<Uuid>, DateTime<Utc>)> =
            sqlx::query_as("SELECT id, estimate_point_id, updated_at FROM issues ORDER BY id")
                .fetch_all(&pool)
                .await
                .expect("points");
        for (id, point, stamp) in &points {
            let expected = if *id == demo_uuid {
                old_point
            } else {
                new_point
            };
            assert_eq!((*point, *stamp), (Some(expected), now), "{id}");
        }
        sqlx::query("UPDATE issues SET deleted_at = NULL WHERE id = $1")
            .bind(demo_uuid)
            .execute(&pool)
            .await
            .expect("restore demo");

        // Bulk CASE dates: per-side arms, untouched sides kept.
        let (d1, d2) = (
            NaiveDate::from_ymd_opt(2026, 1, 1).expect("d1"),
            NaiveDate::from_ymd_opt(2026, 2, 2).expect("d2"),
        );
        sqlx::query("UPDATE issues SET start_date = $1, target_date = $1")
            .bind(d2)
            .execute(&pool)
            .await
            .expect("preset dates");
        bulk_update_issue_dates(
            &db,
            &[
                BulkIssueDates {
                    id: tool,
                    start_date: Some(d1),
                    target_date: None,
                },
                BulkIssueDates {
                    id: seed,
                    start_date: None,
                    target_date: Some(d1),
                },
            ],
        )
        .await
        .expect("bulk dates");
        bulk_update_issue_dates(&db, &[]).await.expect("no writes");
        let dates: Vec<(Uuid, Option<NaiveDate>, Option<NaiveDate>)> = sqlx::query_as(
            "SELECT id, start_date, target_date FROM issues WHERE id IN ($1, $2, $3) ORDER BY id",
        )
        .bind(tool)
        .bind(seed)
        .bind(demo_uuid)
        .fetch_all(&pool)
        .await
        .expect("dates");
        for (id, start, target) in dates {
            let expected = if id == tool {
                (Some(d1), Some(d2))
            } else if id == seed {
                (Some(d2), Some(d1))
            } else {
                (Some(d2), Some(d2))
            };
            assert_eq!((start, target), expected, "{id}");
        }

        // Parent link + scalars + automation bulks.
        link_sub_issue_parents(&db, tool, &[seed, demo_uuid])
            .await
            .expect("link");
        bulk_update_issue_scalars(
            &db,
            &BulkIssueScalars {
                id: seed,
                state_id: None,
                priority: "urgent".to_owned(),
                start_date: None,
                target_date: Some(d1),
                estimate_point_id: None,
            },
            later,
        )
        .await
        .expect("scalars");
        type ScalarRow = (
            Option<Uuid>,
            Option<Uuid>,
            String,
            Option<NaiveDate>,
            Option<Uuid>,
            DateTime<Utc>,
        );
        let scalar: ScalarRow = sqlx::query_as(
            "SELECT parent_id, state_id, priority, start_date, estimate_point_id, updated_at \
             FROM issues WHERE id = $1",
        )
        .bind(seed)
        .fetch_one(&pool)
        .await
        .expect("scalar reread");
        assert_eq!(
            scalar,
            (Some(tool), None, "urgent".to_owned(), None, None, later)
        );

        // Repair plan: single-column renumbers on both tables.
        apply_fix_plan(&db, &[(tool, 50)], &[(seed_seq, 60)])
            .await
            .expect("fix plan");
        let renumbered: (i32, i64) = sqlx::query_as(
            "SELECT (SELECT sequence_id FROM issues WHERE id = $1), \
             (SELECT sequence FROM issue_sequences WHERE id = $2)",
        )
        .bind(tool)
        .bind(seed_seq)
        .fetch_one(&pool)
        .await
        .expect("renumbered");
        assert_eq!(renumbered, (50, 60));

        // Bulk archive (pool-based).
        let today = NaiveDate::from_ymd_opt(2026, 11, 5).expect("today");
        bulk_archive_update(&pool, &[demo_uuid], today)
            .await
            .expect("bulk archive");
        bulk_archive_update(&pool, &[], today)
            .await
            .expect("bulk archive empty");
        let archived: (Option<NaiveDate>,) =
            sqlx::query_as("SELECT archived_at FROM issues WHERE id = $1")
                .bind(demo_uuid)
                .fetch_one(&pool)
                .await
                .expect("archived reread");
        assert_eq!(archived, (Some(today),));

        // Bulk soft delete: triage (live state), archived, draft, other
        // slug and archived-project rows are out of scope; a stateless
        // issue and a deleted triage state pass.
        let (triage, dead_triage) = (uid(920), uid(921));
        sqlx::query(
            "INSERT INTO states (id, \"group\", deleted_at) VALUES ($1, 'triage', NULL), \
             ($2, 'triage', $3)",
        )
        .bind(triage)
        .bind(dead_triage)
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed states");
        let scoped = [uid(30), uid(31), uid(32), uid(33), uid(34)];
        for (id, state_id, is_draft) in [
            (scoped[0], None, false),
            (scoped[1], Some(triage), false),
            (scoped[2], Some(dead_triage), false),
            (scoped[3], None, true),
            (scoped[4], Some(state), false),
        ] {
            sqlx::query(
                "INSERT INTO issues (id, project_id, workspace_id, state_id, is_draft) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(id)
            .bind(project)
            .bind(ws)
            .bind(state_id)
            .bind(is_draft)
            .execute(&pool)
            .await
            .expect("seed scoped issue");
        }
        bulk_soft_delete_issues(&pool, &scoped, &project, "other-ws")
            .await
            .expect("soft delete wrong slug");
        bulk_soft_delete_issues(&pool, &scoped, &project, "live-ws")
            .await
            .expect("soft delete");
        let deleted: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM issues WHERE id = ANY($1) AND deleted_at IS NOT NULL ORDER BY id",
        )
        .bind(&scoped[..])
        .fetch_all(&pool)
        .await
        .expect("deleted");
        assert_eq!(deleted, vec![(scoped[0],), (scoped[2],), (scoped[4],)]);
        sqlx::query("UPDATE projects SET archived_at = now()")
            .execute(&pool)
            .await
            .expect("archive project");
        sqlx::query("UPDATE issues SET deleted_at = NULL")
            .execute(&pool)
            .await
            .expect("undelete");
        bulk_soft_delete_issues(&pool, &scoped, &project, "live-ws")
            .await
            .expect("soft delete archived project");
        let none: (i64,) =
            sqlx::query_as("SELECT count(*) FROM issues WHERE deleted_at IS NOT NULL")
                .fetch_one(&pool)
                .await
                .expect("none deleted");
        assert_eq!(none, (0,));
    }

    /// Live: the chain legs on temp tables — the m2m batches (an
    /// integrity violation swallows the rest), the update leg's soft
    /// clear + ignore-conflicts re-insert, the default-assignee gate, the
    /// created override and the recent-visit delete — plus the workpad
    /// lock read's scope.
    #[tokio::test]
    async fn live_chain_leg_writes() {
        let Some(pool) = temp_table_pool().await else {
            return;
        };
        let db = pidash_db::pool::sea_orm_connection(&pool);
        sqlx::query("CREATE UNIQUE INDEX ON issue_labels (issue_id, label_id)")
            .execute(&pool)
            .await
            .expect("label unique");
        let (issue, project, ws, actor) = (uid(1), uid(2), uid(3), uid(4));
        let scope = M2mScope {
            issue_id: issue,
            project_id: project,
            workspace_id: ws,
            created_by_id: Some(actor),
            updated_by_id: None,
        };
        // 12 labels = two batches; the second repeats a first-batch id,
        // so it violates and is swallowed (first batch kept).
        let mut labels: Vec<Uuid> = (100..111).map(uid).collect();
        labels.push(uid(100));
        write_m2m_batches(&db, M2mTable::Labels, &labels, scope, false)
            .await
            .expect("swallowed");
        let count = |sql: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query_as::<_, (i64,)>(sql)
                    .fetch_one(&pool)
                    .await
                    .expect("count")
                    .0
            }
        };
        assert_eq!(count("SELECT count(*) FROM issue_labels").await, 10);
        assert_eq!(
            count("SELECT count(*) FROM issue_labels WHERE created_by_id IS NOT NULL AND updated_by_id IS NULL AND deleted_at IS NULL").await,
            10
        );
        // The update leg: soft-clear the live links, then re-insert with
        // ON CONFLICT DO NOTHING (the unique index keeps the old rows).
        update_m2m_leg(&db, M2mTable::Labels, &[uid(100), uid(200)], scope)
            .await
            .expect("update leg");
        assert_eq!(
            count("SELECT count(*) FROM issue_labels WHERE deleted_at IS NOT NULL").await,
            10
        );
        assert_eq!(
            count("SELECT count(*) FROM issue_labels WHERE deleted_at IS NULL").await,
            1
        );
        // Default assignee: no qualifying member, no row; then a member.
        let member = uid(5);
        write_default_assignee(&db, member, scope)
            .await
            .expect("no member");
        assert_eq!(count("SELECT count(*) FROM issue_assignees").await, 0);
        sqlx::query(
            "INSERT INTO project_members (id, member_id, project_id, role, is_active) \
             VALUES ($1, $2, $3, 10, true), ($4, $2, $3, 20, true)",
        )
        .bind(uid(6))
        .bind(member)
        .bind(project)
        .bind(uid(7))
        .execute(&pool)
        .await
        .expect("seed members");
        write_default_assignee(&db, member, scope)
            .await
            .expect("member");
        let assignee: (Uuid, Option<Uuid>, Option<Uuid>) =
            sqlx::query_as("SELECT assignee_id, created_by_id, updated_by_id FROM issue_assignees")
                .fetch_one(&pool)
                .await
                .expect("assignee");
        assert_eq!(assignee, (member, Some(actor), None));
        update_m2m_leg(&db, M2mTable::Assignees, &[], scope)
            .await
            .expect("assignee clear");
        assert_eq!(
            count("SELECT count(*) FROM issue_assignees WHERE deleted_at IS NULL").await,
            0
        );

        // Created override: two columns only.
        let stamp = now_micros();
        sqlx::query(
            "INSERT INTO issues (id, project_id, workspace_id, updated_at, is_draft) \
             VALUES ($1, $2, $3, $4, false)",
        )
        .bind(issue)
        .bind(project)
        .bind(ws)
        .bind(stamp)
        .execute(&pool)
        .await
        .expect("seed issue");
        let created = stamp - chrono::Duration::days(1);
        created_override_update(issue, Some(created), Some(actor))
            .exec(&db)
            .await
            .expect("override");
        let over: (DateTime<Utc>, Option<Uuid>, DateTime<Utc>) =
            sqlx::query_as("SELECT created_at, created_by_id, updated_at FROM issues")
                .fetch_one(&pool)
                .await
                .expect("override reread");
        assert_eq!(over, (created, Some(actor), stamp));

        // Recent visits: only the slug's workspace, the project, the
        // issue and `entity_name = 'issue'` rows go.
        sqlx::query("INSERT INTO workspaces (id, slug) VALUES ($1, 'live-ws'), ($2, 'other')")
            .bind(ws)
            .bind(uid(8))
            .execute(&pool)
            .await
            .expect("seed workspaces");
        for (id, workspace, name) in [
            (uid(40), ws, "issue"),
            (uid(41), ws, "page"),
            (uid(42), uid(8), "issue"),
        ] {
            sqlx::query(
                "INSERT INTO user_recent_visits (id, workspace_id, project_id, \
                 entity_identifier, entity_name) VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(id)
            .bind(workspace)
            .bind(project)
            .bind(issue)
            .bind(name)
            .execute(&pool)
            .await
            .expect("seed visit");
        }
        let gone = recent_visit_delete(project, "live-ws", issue)
            .exec(&db)
            .await
            .expect("visit delete");
        assert_eq!(gone.rows_affected, 1);
        assert_eq!(count("SELECT count(*) FROM user_recent_visits").await, 2);

        // Workpad lock read: LEFT state join (stateless issue matches),
        // a triage state or an archived project 404s.
        sqlx::query("INSERT INTO projects (id, archived_at) VALUES ($1, NULL)")
            .bind(project)
            .execute(&pool)
            .await
            .expect("seed project");
        let read = |slug: &'static str| {
            let db = db.clone();
            async move {
                db.query_one(DbBackend::Postgres.build(&workpad_select(issue, project, slug)))
                    .await
                    .expect("workpad read")
                    .is_some()
            }
        };
        assert!(read("live-ws").await);
        assert!(!read("other").await);
        sqlx::query("INSERT INTO states (id, \"group\") VALUES ($1, 'triage')")
            .bind(uid(9))
            .execute(&pool)
            .await
            .expect("seed triage");
        sqlx::query("UPDATE issues SET state_id = $1")
            .bind(uid(9))
            .execute(&pool)
            .await
            .expect("to triage");
        assert!(!read("live-ws").await);
        sqlx::query("UPDATE states SET \"group\" = 'started'")
            .execute(&pool)
            .await
            .expect("to started");
        assert!(read("live-ws").await);
        sqlx::query("UPDATE projects SET archived_at = now()")
            .execute(&pool)
            .await
            .expect("archive project");
        assert!(!read("live-ws").await);
    }
}
