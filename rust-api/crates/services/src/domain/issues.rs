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
use pidash_db::entity::issues;
use sea_orm::{ActiveModelTrait, ConnectionTrait, DbBackend, DbErr, Set, Statement};
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
    /// The v1 create override `UPDATE` hit an integrity violation — the
    /// port's `save_error` 400 arm (`PAYLOAD_NOT_VALID_BODY`).
    OverrideConflict,
    /// The v1 create override `UPDATE` failed any other way — `ServerError`.
    OverrideFailed,
    /// A v1 create override coercion failed — the port's constant
    /// `BadError("Please provide valid detail")` 400 arm.
    OverrideDetail,
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
}

/// The destroy recent-visit delete, moved verbatim: a HARD delete scoped
/// to the workspace slug + project + issue.
const RECENT_VISIT_DELETE_SQL: &str = "DELETE FROM user_recent_visits AS visit \
    USING workspaces AS workspace WHERE visit.workspace_id = workspace.id \
    AND visit.project_id = $1 AND workspace.slug = $2 AND visit.entity_identifier = $3 \
    AND visit.entity_name = 'issue'";

/// The v1 create raw-data overrides (`api/views/issue.py:505-509`), moved
/// verbatim: a second save limited to `update_fields=["created_at",
/// "created_by"]` on the refetched row. It stays a chain leg between the
/// m2m writes and the activity fan-out (it cannot fold into the INSERT:
/// the fire's entry-run creation reads the pre-override `created_by_id`
/// from the row).
const CREATED_OVERRIDE_SQL: &str =
    r#"UPDATE "issues" SET "created_at" = $1, "created_by_id" = $2 WHERE "id" = $3"#;

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
                match sqlx::query(CREATED_OVERRIDE_SQL)
                    .bind(created_at)
                    .bind(created_by)
                    .bind(issue_id)
                    .execute(pool)
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
            m2m_audit,
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
            m2m_audit,
        )
        .await?;
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
        run_chain(&pool, &transition, &enqueue, chain, &step).await;
    });
    Ok(SaveOutcome {
        issue: model,
        chain: outcome,
    })
}

/// The workpad-PATCH issue read (`api/views/issue.py:3307-3311`), moved
/// verbatim: `LIMIT 21` (executed `.get()`) + `FOR UPDATE OF issues`
/// (`of=("self",)` — a bare lock would 500 on the nullable state join).
/// The triage/archived/draft predicates are load-bearing (a miss 404s);
/// the projected `state_group` goes unused — the save hook re-reads the
/// group itself, in the same transaction and snapshot. Public because the
/// driver probes existence with the SAME text on the pool before
/// validating (a miss 404s ahead of validation errors, as today) — one
/// text, so the probe and the lock can never disagree on the match. The
/// `FOR UPDATE` is harmless in the driver's autocommit probe (the lock
/// releases at statement end).
pub const WORKPAD_SELECT_SQL: &str = r#"SELECT "issues"."description_html", "issues"."state_id",
          "issues"."completed_at", "states"."group" AS "state_group"
   FROM "issues"
   LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id")
   INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id")
   INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id")
   WHERE ("issues"."deleted_at" IS NULL
     AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)
     AND NOT ("issues"."archived_at" IS NOT NULL)
     AND NOT ("projects"."archived_at" IS NOT NULL)
     AND NOT ("issues"."is_draft")
     AND "issues"."id" = $1
     AND "issues"."project_id" = $2
     AND "workspaces"."slug" = $3)
   LIMIT 21 FOR UPDATE OF "issues""#;

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
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            WORKPAD_SELECT_SQL,
            [
                sea_orm::Value::from(input.issue_id),
                sea_orm::Value::from(input.project_id),
                sea_orm::Value::from(input.slug),
            ],
        ))
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
        run_chain(&pool, &transition, &enqueue, chain, &step).await;
    });
    Ok(outcome)
}

/// The agent-tool workpad write (`tools.py:300-307`): a `QuerySet.update`
/// equivalent — bare columns, no hooks, no chain. Contrast
/// [`set_workpad`], which runs the save (hook recomputes plus the
/// completion leg) for the v1 workpad endpoint. Runs on any executor so
/// audit-transaction drivers keep the write inside their transaction.
pub async fn update_workpad<'e, E>(
    ex: E,
    issue_id: Uuid,
    project_id: Uuid,
    workpad: &str,
    updated_at: DateTime<Utc>,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(WORKPAD_BARE_UPDATE_SQL)
        .bind(issue_id)
        .bind(project_id)
        .bind(workpad)
        .bind(updated_at)
        .execute(ex)
        .await
        .map(|done| done.rows_affected())
}

const WORKPAD_BARE_UPDATE_SQL: &str =
    "UPDATE issues SET workpad = $3, updated_at = $4 WHERE id = $1 AND project_id = $2 \
     AND deleted_at IS NULL";

/// The agent-tool transition save (`tools.py:334-335`): a
/// `save(update_fields=["state", "updated_at", "updated_by"])`
/// equivalent — the three columns land with no hook-recompute writes
/// (`completed_at`/`description_stripped` stay untouched, exactly what
/// the field list writes) and no chain: the driver routes completion,
/// fire and orchestration itself, as today. Runs on any executor so
/// audit-transaction drivers keep the write inside their transaction
/// (and under their row lock — a separate domain transaction would
/// deadlock against it).
pub async fn update_issue_state<'e, E>(
    ex: E,
    issue_id: Uuid,
    state_id: Uuid,
    updated_at: DateTime<Utc>,
    updated_by_id: Uuid,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(ISSUE_STATE_BARE_UPDATE_SQL)
        .bind(issue_id)
        .bind(state_id)
        .bind(updated_at)
        .bind(updated_by_id)
        .execute(ex)
        .await
        .map(|done| done.rows_affected())
}

const ISSUE_STATE_BARE_UPDATE_SQL: &str =
    "UPDATE issues SET state_id = $2, updated_at = $3, updated_by_id = $4 WHERE id = $1";

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

pub async fn insert_mirror_issue<'e, E>(
    ex: E,
    insert: &MirrorIssueInsert,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(MIRROR_ISSUE_INSERT_SQL)
        .bind(insert.issue_id)
        .bind(insert.now)
        .bind(insert.now)
        .bind(insert.actor_id)
        .bind(insert.actor_id)
        .bind(insert.project_id)
        .bind(insert.workspace_id)
        .bind(insert.state_id)
        .bind(&insert.name)
        .bind(serde_json::json!({}))
        .bind(&insert.description_html)
        .bind(&insert.description_stripped)
        .bind(insert.sequence_id)
        .bind(insert.sort_order)
        .bind(insert.completed_at)
        .bind(&insert.external_source)
        .bind(&insert.external_id)
        .bind(insert.pod_id)
        .execute(ex)
        .await
        .map(|done| done.rows_affected())
}

const MIRROR_ISSUE_INSERT_SQL: &str = "INSERT INTO issues (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, parent_id, state_id, point, estimate_point_id, name, description_json, description_html, description_stripped, description_binary, priority, complexity_score, start_date, target_date, sequence_id, sort_order, completed_at, archived_at, is_draft, external_source, external_id, type_id, git_work_branch, workpad, created_via, assigned_pod_id, agent_executor) VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, NULL, $8, NULL, NULL, $9, $10, $11, $12, NULL, 'none', 0, NULL, NULL, $13, $14, $15, NULL, FALSE, $16, $17, NULL, '', '', NULL, $18, NULL)";

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

pub async fn insert_issue_sequence<'e, E>(
    ex: E,
    seq: &MirrorIssueSequence,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(MIRROR_ISSUE_SEQUENCE_INSERT_SQL)
        .bind(Uuid::new_v4())
        .bind(seq.now)
        .bind(seq.now)
        .bind(seq.project_id)
        .bind(seq.workspace_id)
        .bind(seq.issue_id)
        .bind(seq.sequence)
        .execute(ex)
        .await
        .map(|done| done.rows_affected())
}

const MIRROR_ISSUE_SEQUENCE_INSERT_SQL: &str = "INSERT INTO issue_sequences (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_id, sequence, deleted) VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7, FALSE)";

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

pub async fn update_git_mirror_issue<'e, E>(
    ex: E,
    upd: &GitMirrorIssueUpdate,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(GIT_MIRROR_UPDATE_SQL)
        .bind(upd.issue_id)
        .bind(&upd.name)
        .bind(&upd.description_html)
        .bind(&upd.description_stripped)
        .bind(serde_json::json!({}))
        .bind(upd.workspace_id)
        .bind(upd.actor_id)
        .bind(upd.actor_id)
        .bind(&upd.external_source)
        .bind(&upd.external_id)
        .bind(upd.now)
        .execute(ex)
        .await
        .map(|done| done.rows_affected())
}

const GIT_MIRROR_UPDATE_SQL: &str = "UPDATE issues SET name = $2, description_html = $3, description_stripped = $4, description_json = $5, workspace_id = $6, created_by_id = $7, updated_by_id = $8, external_source = $9, external_id = $10, updated_at = $11 WHERE id = $1";

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

pub async fn update_github_mirror_issue<'e, E>(
    ex: E,
    upd: &GithubMirrorIssueUpdate,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(GITHUB_MIRROR_UPDATE_SQL)
        .bind(upd.issue_id)
        .bind(&upd.name)
        .bind(&upd.description_html)
        .bind(&upd.description_stripped)
        .bind(serde_json::json!({}))
        .bind(upd.actor_id)
        .bind(upd.completed_at)
        .bind(upd.now)
        .execute(ex)
        .await
        .map(|done| done.rows_affected())
}

const GITHUB_MIRROR_UPDATE_SQL: &str = "UPDATE issues SET name = $2, description_html = $3, description_stripped = $4, description_json = $5, created_by_id = $6, updated_by_id = $6, completed_at = $7, updated_at = $8 WHERE id = $1";

/// The ticker state-move write (`scheduling.py:700,1113,1468`):
/// `save(update_fields=["state", "updated_at"])` — ONLY these columns.
/// Contrast [`update_issue_state`], which additionally stamps
/// `updated_by_id` (the agent-tool save shape): here the in-memory
/// `updated_by` assignments never reach the row, exactly what the field
/// list writes. Bare columns, no hooks, no chain: the driver routes the
/// orchestration fire itself (`reconcile_on_seam` with the tick-caused
/// event), as today. Runs on any executor so the dispatch seam keeps the
/// write inside its transaction.
pub async fn update_issue_state_no_audit<'e, E>(
    ex: E,
    issue_id: Uuid,
    state_id: Uuid,
    updated_at: DateTime<Utc>,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(TICKER_STATE_UPDATE_SQL)
        .bind(state_id)
        .bind(updated_at)
        .bind(issue_id)
        .execute(ex)
        .await
        .map(|done| done.rows_affected())
}

const TICKER_STATE_UPDATE_SQL: &str =
    "UPDATE issues SET state_id = $1, updated_at = $2 WHERE id = $3";

/// The automation bulk writes (`bulk_update(..., batch_size=100)`):
/// `["archived_at"]` (`issue_automation_task.py:69`) and `["state"]`
/// (`:132`) — a single column each, no `updated_at` touch (`bulk_update`
/// never bumps `auto_now` on unlisted fields), no `save()`, no signals.
/// The driver keeps the batch-100 chunking; the per-issue
/// `issue_activity.delay` fan-out stays driver-enqueued, as today.
pub async fn bulk_set_archived_at<'e, E>(
    ex: E,
    ids: &[Uuid],
    archived_at: NaiveDate,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(BULK_ARCHIVE_SQL)
        .bind(archived_at)
        .bind(ids)
        .execute(ex)
        .await
        .map(|done| done.rows_affected())
}

const BULK_ARCHIVE_SQL: &str = "UPDATE issues SET archived_at = $1 WHERE id = ANY($2)";

pub async fn bulk_set_state<'e, E>(
    ex: E,
    ids: &[Uuid],
    state_id: Option<Uuid>,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(BULK_CLOSE_SQL)
        .bind(state_id)
        .bind(ids)
        .execute(ex)
        .await
        .map(|done| done.rows_affected())
}

const BULK_CLOSE_SQL: &str = "UPDATE issues SET state_id = $1 WHERE id = ANY($2)";

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

    #[test]
    fn bare_update_sql_shapes() {
        // Moved verbatim from the retired `dispatch::tools` consts: the
        // agent-tool `QuerySet.update` / `update_fields` equivalents.
        assert!(WORKPAD_BARE_UPDATE_SQL.contains("SET workpad = $3"));
        assert!(WORKPAD_BARE_UPDATE_SQL.contains("project_id = $2"));
        assert!(WORKPAD_BARE_UPDATE_SQL.contains("deleted_at IS NULL"));
        assert!(ISSUE_STATE_BARE_UPDATE_SQL.contains("SET state_id = $2"));
        assert!(ISSUE_STATE_BARE_UPDATE_SQL.contains("updated_by_id = $4"));
    }

    #[test]
    fn sync_bare_sql_shapes() {
        // Moved verbatim from the retired sync consts (`git_sync` /
        // `github_sync`): the mirror INSERT both providers shared, the
        // sequence companion row, and the two provider-shaped UPDATE
        // arms. Full-text pins: the statements moved byte-for-byte, so
        // the jobs parse-suite coverage carries over by construction.
        assert_eq!(
            MIRROR_ISSUE_INSERT_SQL,
            "INSERT INTO issues (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, parent_id, state_id, point, estimate_point_id, name, description_json, description_html, description_stripped, description_binary, priority, complexity_score, start_date, target_date, sequence_id, sort_order, completed_at, archived_at, is_draft, external_source, external_id, type_id, git_work_branch, workpad, created_via, assigned_pod_id, agent_executor) VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, NULL, $8, NULL, NULL, $9, $10, $11, $12, NULL, 'none', 0, NULL, NULL, $13, $14, $15, NULL, FALSE, $16, $17, NULL, '', '', NULL, $18, NULL)"
        );
        assert!(MIRROR_ISSUE_INSERT_SQL.contains("sequence_id"));
        assert!(MIRROR_ISSUE_INSERT_SQL.contains("sort_order"));
        assert!(MIRROR_ISSUE_INSERT_SQL.contains("assigned_pod_id"));
        assert!(MIRROR_ISSUE_INSERT_SQL.contains("'none', 0"));
        assert_eq!(
            MIRROR_ISSUE_SEQUENCE_INSERT_SQL,
            "INSERT INTO issue_sequences (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_id, sequence, deleted) VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7, FALSE)"
        );
        // Git arm: no state/sequence/sort touch, `updated_at` carried.
        assert_eq!(
            GIT_MIRROR_UPDATE_SQL,
            "UPDATE issues SET name = $2, description_html = $3, description_stripped = $4, description_json = $5, workspace_id = $6, created_by_id = $7, updated_by_id = $8, external_source = $9, external_id = $10, updated_at = $11 WHERE id = $1"
        );
        assert!(!GIT_MIRROR_UPDATE_SQL.contains("state_id"));
        assert!(!GIT_MIRROR_UPDATE_SQL.contains("sequence_id"));
        assert!(!GIT_MIRROR_UPDATE_SQL.contains("sort_order"));
        // Github arm: the audit restamp collapses onto one bind.
        assert_eq!(
            GITHUB_MIRROR_UPDATE_SQL,
            "UPDATE issues SET name = $2, description_html = $3, description_stripped = $4, description_json = $5, created_by_id = $6, updated_by_id = $6, completed_at = $7, updated_at = $8 WHERE id = $1"
        );
        assert!(GITHUB_MIRROR_UPDATE_SQL.contains("created_by_id = $6"));
    }

    #[test]
    fn ticker_and_automation_bare_sql_shapes() {
        // Moved verbatim from the retired `orchestration::dispatch`
        // ticker const and the `automation` bulk consts.
        assert_eq!(
            TICKER_STATE_UPDATE_SQL,
            "UPDATE issues SET state_id = $1, updated_at = $2 WHERE id = $3"
        );
        assert!(!TICKER_STATE_UPDATE_SQL.contains("updated_by_id"));
        // Bulk writes touch exactly one column each.
        assert_eq!(
            BULK_ARCHIVE_SQL,
            "UPDATE issues SET archived_at = $1 WHERE id = ANY($2)"
        );
        assert_eq!(
            BULK_CLOSE_SQL,
            "UPDATE issues SET state_id = $1 WHERE id = ANY($2)"
        );
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

    #[test]
    fn created_override_sql_is_the_ported_update() {
        // The v1 create second save, moved verbatim — the guard fails on
        // drift, this test fails on silent reshaping.
        assert_eq!(
            CREATED_OVERRIDE_SQL,
            r#"UPDATE "issues" SET "created_at" = $1, "created_by_id" = $2 WHERE "id" = $3"#
        );
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
            &pool,
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
            &pool,
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
            &pool,
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
            &pool,
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
        let rows = update_issue_state_no_audit(&pool, issue_id, backlog_id, t3)
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
        let rows = bulk_set_archived_at(&pool, &[issue_id], day)
            .await
            .expect("bulk archive");
        assert_eq!(rows, 1);
        let rows = bulk_set_state(&pool, &[issue_id], None)
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
}
