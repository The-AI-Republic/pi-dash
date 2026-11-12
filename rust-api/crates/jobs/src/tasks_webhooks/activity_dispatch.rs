//! `issue_activity` dispatcher task (D-08, jobs layer).
//!
//! Port of the `@shared_task` entry point of
//! `apps/api/pi_dash/bgtasks/issue_activities_task.py:1592-1695`
//! (`issue_activity`). The pure row builders live in `pidash-services`
//! (`tasks_webhooks::activity_issue` for issue/comment/cycle/module,
//! `tasks_webhooks::activity_misc` for the rest); this module owns the
//! Celery wire surface (task name, `(args, kwargs)` binding), the
//! dispatcher body in call order, and the [`Registry`] wiring:
//!
//! 1. `project_id` uuid-v4 guard — `not is_valid_uuid(str(project_id))`
//!    returns *before any DB access* (`:1609`).
//! 2. `Project.objects.get` → `workspace_id` (`:1612-1613`; raises →
//!    broad-except).
//! 3. `origin` → `redis.set(str(issue_id), origin, ex=600)` when `issue_id`
//!    is not `None` and `origin` is truthy, then the `Issue` touch
//!    (`updated_at = now()`, own try/except-pass) (`:1615-1626`).
//! 4. The 30-entry `ACTIVITY_MAPPER` (`:1628-1659`), `bulk_create` with no
//!    batch size and no `ignore_conflicts` (`:1675`).
//! 5. `notifications.delay` with the `IssueActivitySerializer` JSON when
//!    `notification` is true (`:1677-1690`).
//! 6. Broad `except Exception: log_exception(e); return` — the task never
//!    raises to Celery, so every handler path here settles `Ack`
//!    (`:1693-1695`; plain `@shared_task`, no autoretry).
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * The mapper holds **30** entries (27 at port time plus the three
//!   `worklog.activity.*` keys at `:1644-1646`, added by PIDASHCONV-1676)
//!   — the D-08 issue text said "28-type" but the literal then had 27.
//!   Ported as-is; there is no `cycle.activity.updated`, no
//!   `module.activity.updated`, no `attachment.activity.updated` and no
//!   intake update/delete.
//! * `intake` is accepted but never read (dead parameter, `:1603`);
//!   binding tolerates it and drops it.
//! * `create_issue_activity` inserts its row *immediately*
//!   (`IssueActivity.objects.create`, `:568`) and rewrites `created_at` /
//!   `actor_id` from the `Issue` row (`:577-579`) — that row is **not**
//!   part of the `bulk_create` batch and **not** part of the notification
//!   payload. The driver issues one `INSERT` with the overridden columns
//!   instead of insert-then-update (same final row, same exclusion).
//! * `track_assignees` bulk-creates `IssueSubscriber` rows inline with
//!   `ignore_conflicts=True` (`:408`); the driver collects every
//!   subscriber draft and issues one `INSERT ... ON CONFLICT DO NOTHING`
//!   before the activity batch (batching is unobservable in the final
//!   state).
//! * `notifications.delay` receives the *raw* `requested_data` /
//!   `current_instance` args (unparsed strings or `None`, `:1597-1598`),
//!   not the parsed dicts.
//!
//! # Deliberate deviations (documented, not bugs)
//!
//! * The default `objects` manager is `SoftDeletionManager`
//!   (`db/mixins.py:49-53`), so every `.filter(...).first()` / `.get()`
//!   the Python performs skips soft-deleted rows — except `User` (a custom
//!   model with `UserManager`, no `deleted_at` column). Every lookup below
//!   carries `AND deleted_at IS NULL` except the user lookup.
//! * `User.objects.get` raising on a miss aborts the whole task in Python
//!   (`track_assignees` `:380,415`); the resolvers surface `None` and the
//!   services planners encode the same abort as their error (see
//!   `activity_issue` / `activity_misc` docs).
//! * The notification JSON renders the `__all__` columns with real values
//!   (ids from `RETURNING`, timestamps, `attachments: []`,
//!   `created_by`/`updated_by`/`deleted_at: null` — `bulk_create` skips
//!   `save()`, so crum never fills the audit user), `issue_detail` as the
//!   real `IssueFlatSerializer` projection (the only detail object the
//!   consumer reads: `notification_task.py:323`), and `actor_detail`,
//!   `project_detail`, `workspace_detail`, `source_data` as `null`. Those
//!   four are never read by `notification_task.py` or
//!   `email_notification_task.py` (verified by grep for `.get(` reads on
//!   the activity dicts); full hydration belongs to the notifications-task
//!   port, which owns those serializers.
//! * The payload string uses `json.dumps` defaults (`", "` / `": "`
//!   separators, `ensure_ascii`) via [`django_dumps`], not serde's compact
//!   form.
//! * On a builder abort no side writes are performed: the Python loop may
//!   have already bulk-created subscriber rows or touched the description
//!   row before the failing key. Only observable with aborting (invalid)
//!   input; valid input never aborts.
//! * A `None` actor fails at the first uuid-typed bind while Python would
//!   store NULL — unreachable in contract (every caller passes
//!   `str(request.user.id)`).
//!
//! Fixture: `rust-api/fixtures/tasks_webhooks/fx-act-03-dispatcher.json`
//! (FX-ACT-03). The `#[cfg(test)]` suite below asserts the guard, the
//! mapper keys, the call binding, the SQL shapes, the kwargs order and the
//! dumps rendering.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Alias, Expr, JoinType, Query, SelectStatement};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, FromQueryResult, QueryFilter, QueryOrder,
    QuerySelect, Select, Set,
};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity::{
    comment_reactions, cycles, estimate_points, estimates, issue_activities, issue_comments,
    issue_reactions, issue_subscribers, issues, labels, modules, pod, projects, runner, states,
    users,
};
use pidash_db::orm_tx::into_sqlx;
use pidash_db::Pools;
use pidash_services::domain::issues::{touch_issue, touch_issues};
use pidash_services::tasks_webhooks::activity_issue::{
    plan_create_comment, plan_create_cycle_issue, plan_create_issue, plan_create_module_issue,
    plan_delete_comment, plan_delete_cycle_issue, plan_delete_issue, plan_delete_module_issue,
    plan_update_comment, plan_update_issue, ActivityRow, BuildError, CreatedIssueRef, CycleRef,
    UpdateResolvers,
};
use pidash_services::tasks_webhooks::activity_misc::{
    plan_create_attachment, plan_create_comment_reaction, plan_create_draft, plan_create_intake,
    plan_create_issue_reaction, plan_create_link, plan_create_relation, plan_create_vote,
    plan_create_worklog, plan_delete_attachment, plan_delete_comment_reaction, plan_delete_draft,
    plan_delete_issue_reaction, plan_delete_link, plan_delete_relation, plan_delete_vote,
    plan_delete_worklog, plan_update_draft, plan_update_link, plan_update_worklog, CommentRef,
    IssueRef,
};
use pidash_services::tasks_webhooks::activity_tracks::{
    DescriptionLatest, EstimatePointRef, IssueActivityDraft, IssueSubscriberDraft, LabelRef,
    ParentRef, PodRef, RunnerRef, StateRef, TrackFrame, UserRef,
};

use crate::queue::{enqueue, NewJob};
use crate::worker::{Handler, Registry, Verdict};

/// Re-export of the canonical [`pidash_services::domain::activity::ISSUE_ACTIVITY_TASK`].
pub use pidash_services::domain::activity::ISSUE_ACTIVITY_TASK;

/// `notifications.delay(...)` target (`notification_task.py:190-200`).
pub const NOTIFICATIONS_TASK: &str = "pi_dash.bgtasks.notification_task.notifications";

/// `ri.set(str(issue_id), origin, ex=600)` (`:1619`).
pub const ORIGIN_TTL_SECS: u64 = 600;

/// The 30 `ACTIVITY_MAPPER` keys (`:1628-1659`), in source order.
pub const ACTIVITY_TYPES: [&str; 30] = [
    "issue.activity.created",
    "issue.activity.updated",
    "issue.activity.deleted",
    "comment.activity.created",
    "comment.activity.updated",
    "comment.activity.deleted",
    "cycle.activity.created",
    "cycle.activity.deleted",
    "module.activity.created",
    "module.activity.deleted",
    "link.activity.created",
    "link.activity.updated",
    "link.activity.deleted",
    "attachment.activity.created",
    "attachment.activity.deleted",
    "worklog.activity.created",
    "worklog.activity.updated",
    "worklog.activity.deleted",
    "issue_relation.activity.created",
    "issue_relation.activity.deleted",
    "issue_reaction.activity.created",
    "issue_reaction.activity.deleted",
    "comment_reaction.activity.created",
    "comment_reaction.activity.deleted",
    "issue_vote.activity.created",
    "issue_vote.activity.deleted",
    "issue_draft.activity.created",
    "issue_draft.activity.updated",
    "issue_draft.activity.deleted",
    "intake.activity.created",
];

/// `ACTIVITY_MAPPER.get(type) is not None` (`:1661-1662`).
pub fn is_known_activity_type(activity_type: &str) -> bool {
    ACTIVITY_TYPES.contains(&activity_type)
}

/// `is_valid_uuid` (`utils/uuid.py`; ported in `pidash-services` by
/// PIDASHCONV-196): parses like `uuid.UUID(...)` and requires version 4.
/// Re-exported here so dispatcher callers test the same guard the driver
/// enforces (`:1609`).
pub use pidash_services::tasks_webhooks::activity_tracks::is_valid_uuid;

/// The bound `issue_activity` call (`:1593-1605`). `requested_data` and
/// `current_instance` stay raw (`Option<String>`); `intake` is accepted
/// and dropped (dead parameter).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueActivityCall {
    pub activity_type: String,
    pub requested_data: Option<String>,
    pub current_instance: Option<String>,
    pub issue_id: Option<String>,
    pub actor_id: Option<String>,
    pub project_id: Option<String>,
    pub epoch: f64,
    pub subscriber: bool,
    pub notification: bool,
    pub origin: Option<String>,
}

/// Bind a Celery `(args, kwargs)` payload the way Python binds
/// `issue_activity(type, requested_data, ..., intake=None)`: positionals in
/// signature order, keywords by name, `subscriber=True` /
/// `notification=False` / `origin=None` / `intake=None` defaults. A slot
/// bound both ways, an unknown keyword, or a wrongly-typed value is a
/// `TypeError`-equivalent rejection (the handler acks it: the Python
/// worker would fail the message without running the body).
pub fn bind_issue_activity(args: &Value, kwargs: &Value) -> Result<IssueActivityCall, String> {
    /// Positional slot or keyword, rejecting duplicates.
    fn slot<'v>(
        positional: &'v [Value],
        kwargs: &'v Map<String, Value>,
        task: &str,
        index: usize,
        name: &str,
    ) -> Result<Option<&'v Value>, String> {
        let from_args = positional.get(index);
        let from_kwargs = kwargs.get(name);
        match (from_args, from_kwargs) {
            (Some(_), Some(_)) => Err(format!("{task}: multiple values for argument '{name}'")),
            (value, None) => Ok(value),
            (None, value) => Ok(value),
        }
    }

    let positional = args.as_array().cloned().unwrap_or_default();
    let kwargs = kwargs.as_object().cloned().unwrap_or_default();
    if positional.len() > 11 {
        return Err(format!(
            "{ISSUE_ACTIVITY_TASK}: too many positional arguments"
        ));
    }
    // Keywords outside the Python signature are a TypeError.
    const NAMES: [&str; 11] = [
        "type",
        "requested_data",
        "current_instance",
        "issue_id",
        "actor_id",
        "project_id",
        "epoch",
        "subscriber",
        "notification",
        "origin",
        "intake",
    ];
    if let Some(extra) = kwargs.keys().find(|key| !NAMES.contains(&key.as_str())) {
        return Err(format!(
            "{ISSUE_ACTIVITY_TASK}: unexpected keyword argument '{extra}'"
        ));
    }

    let raw =
        |index: usize, name: &str| slot(&positional, &kwargs, ISSUE_ACTIVITY_TASK, index, name);
    let activity_type = match raw(0, "type")? {
        Some(Value::String(s)) => s.clone(),
        _ => return Err(format!("{ISSUE_ACTIVITY_TASK}: 'type' must be a string")),
    };
    // Raw JSON payloads (never parsed here). Producers send JSON strings,
    // except the app destroy which sends current_instance={} as a dict
    // (base.py:747); the Python delete handler ignores it, so the job
    // succeeds there. Accept objects/arrays by dumping to JSON text; scalars
    // still reject (Python json.loads would fail on them too).
    let raw_string = |value: Option<&Value>, name: &str| -> Result<Option<String>, String> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(format!(
                "{ISSUE_ACTIVITY_TASK}: '{name}' must be a string or null"
            )),
        }
    };
    let raw_json = |value: Option<&Value>, name: &str| -> Result<Option<String>, String> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(Value::Object(_)) | Some(Value::Array(_)) => value
                .map(serde_json::to_string)
                .transpose()
                .map_err(|error| {
                    format!("{ISSUE_ACTIVITY_TASK}: '{name}' must be a string or null: {error}")
                }),
            Some(_) => Err(format!(
                "{ISSUE_ACTIVITY_TASK}: '{name}' must be a string or null"
            )),
        }
    };
    let requested_data = raw_json(raw(1, "requested_data")?, "requested_data")?;
    let current_instance = raw_json(raw(2, "current_instance")?, "current_instance")?;
    // Ids are stringified before the uuid guard (`str(project_id)`), so any
    // scalar binds here and fails the guard later; collections reject.
    let id_string = |value: Option<&Value>, name: &str| -> Result<Option<String>, String> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(Value::Number(n)) => Ok(Some(n.to_string())),
            Some(Value::Bool(b)) => Ok(Some(if *b { "True" } else { "False" }.to_owned())),
            Some(_) => Err(format!(
                "{ISSUE_ACTIVITY_TASK}: '{name}' must be a scalar or null"
            )),
        }
    };
    let issue_id = id_string(raw(3, "issue_id")?, "issue_id")?;
    let actor_id = id_string(raw(4, "actor_id")?, "actor_id")?;
    let project_id = id_string(raw(5, "project_id")?, "project_id")?;
    let epoch = match raw(6, "epoch")? {
        Some(Value::Number(n)) => n
            .as_f64()
            .ok_or_else(|| format!("{ISSUE_ACTIVITY_TASK}: 'epoch' must be a number"))?,
        _ => return Err(format!("{ISSUE_ACTIVITY_TASK}: 'epoch' must be a number")),
    };
    let boolean = |value: Option<&Value>, name: &str, default: bool| -> Result<bool, String> {
        match value {
            None | Some(Value::Null) => Ok(default),
            Some(Value::Bool(b)) => Ok(*b),
            Some(Value::Number(n)) => Ok(n.as_i64().unwrap_or(1) != 0),
            Some(_) => Err(format!("{ISSUE_ACTIVITY_TASK}: '{name}' must be a boolean")),
        }
    };
    let subscriber = boolean(raw(7, "subscriber")?, "subscriber", true)?;
    let notification = boolean(raw(8, "notification")?, "notification", false)?;
    let origin = raw_string(raw(9, "origin")?, "origin")?;
    Ok(IssueActivityCall {
        activity_type,
        requested_data,
        current_instance,
        issue_id,
        actor_id,
        project_id,
        epoch,
        subscriber,
        notification,
        origin,
    })
}

/// `Project.objects.get(pk=project_id)` (`:1612`): the default manager is
/// `SoftDeletionManager`, so soft-deleted projects miss here exactly as in
/// Python (`db_table = "projects"`, `project.py:252`).
fn project_workspace_query(id: Uuid) -> Select<projects::Entity> {
    projects::Entity::find()
        .select_only()
        .column(projects::Column::WorkspaceId)
        .filter(projects::Column::Id.eq(id))
        .filter(projects::Column::DeletedAt.is_null())
        .limit(1)
}

/// `Issue.objects.filter(pk=issue_id).first()` for the touch (`:1620`).
/// The touch itself is [`pidash_services::domain::issues::touch_issue`].
fn issue_exists_query(id: Uuid) -> Select<issues::Entity> {
    issues::Entity::find()
        .select_only()
        .column(issues::Column::Id)
        .filter(issues::Column::Id.eq(id))
        .filter(issues::Column::DeletedAt.is_null())
        .limit(1)
}

/// `IssueReaction.objects.filter(reaction, project_id,
/// actor_id).values_list("id", flat=True).first()` (`:1082-1090`).
fn issue_reaction_query(
    reaction: &str,
    project_id: Uuid,
    actor_id: Uuid,
) -> Select<issue_reactions::Entity> {
    issue_reactions::Entity::find()
        .select_only()
        .column(issue_reactions::Column::Id)
        .filter(issue_reactions::Column::Reaction.eq(reaction))
        .filter(issue_reactions::Column::ProjectId.eq(project_id))
        .filter(issue_reactions::Column::ActorId.eq(actor_id))
        .filter(issue_reactions::Column::DeletedAt.is_null())
        .order_by_desc(issue_reactions::Column::CreatedAt)
        .limit(1)
}

/// `CommentReaction.objects.filter(reaction, project_id,
/// actor_id).values_list("id", "comment__id").first()` (`:1152-1160`).
fn comment_reaction_query(
    reaction: &str,
    project_id: Uuid,
    actor_id: Uuid,
) -> Select<comment_reactions::Entity> {
    comment_reactions::Entity::find()
        .select_only()
        .columns([
            comment_reactions::Column::Id,
            comment_reactions::Column::CommentId,
        ])
        .filter(comment_reactions::Column::Reaction.eq(reaction))
        .filter(comment_reactions::Column::ProjectId.eq(project_id))
        .filter(comment_reactions::Column::ActorId.eq(actor_id))
        .filter(comment_reactions::Column::DeletedAt.is_null())
        .order_by_desc(comment_reactions::Column::CreatedAt)
        .limit(1)
}

/// `IssueComment.objects.get(pk=..., project_id=...)`
/// (`:1161`) and `IssueComment.objects.filter(pk=...,
/// project_id=...).values_list("issue_id", flat=True).first()`
/// (`:1193-1197`): same row, same scope.
fn comment_issue_query(comment_id: Uuid, project_id: Uuid) -> Select<issue_comments::Entity> {
    issue_comments::Entity::find()
        .select_only()
        .column(issue_comments::Column::IssueId)
        .filter(issue_comments::Column::Id.eq(comment_id))
        .filter(issue_comments::Column::ProjectId.eq(project_id))
        .filter(issue_comments::Column::DeletedAt.is_null())
        .limit(1)
}

/// `Issue.objects.get(pk=...)` projected to the relation label
/// (`issue.project.identifier`, `issue.sequence_id`, `:1298,1315,1344,1360`).
/// Built with sea-query because the read joins `projects` and the output
/// names must stay unique.
fn issue_ref_statement(ids: &[Uuid]) -> SelectStatement {
    let i = Alias::new("i");
    let p = Alias::new("p");
    let mut stmt = Query::select();
    stmt.column((i.clone(), issues::Column::Id))
        .column((p.clone(), projects::Column::Identifier))
        .column((i.clone(), issues::Column::SequenceId))
        .from_as(issues::Entity, i.clone())
        .join_as(
            JoinType::InnerJoin,
            projects::Entity,
            p.clone(),
            Expr::col((i.clone(), issues::Column::ProjectId))
                .equals((p.clone(), projects::Column::Id)),
        )
        .and_where(Expr::col((i.clone(), issues::Column::Id)).is_in(ids.to_vec()))
        .and_where(Expr::col((i.clone(), issues::Column::DeletedAt)).is_null());
    stmt
}

/// `EstimatePoint.objects.filter(pk=...).first()` plus `estimate.type`
/// (`:444-453,469`; `db_table = "estimate_points"`, joined to
/// `"estimates"` like the `new_estimate.estimate` follow — no scope on the
/// joined table, exactly as the ORM follow has none).
fn estimate_points_statement(ids: &[Uuid]) -> SelectStatement {
    let ep = Alias::new("ep");
    let e = Alias::new("e");
    let mut stmt = Query::select();
    stmt.column((ep.clone(), estimate_points::Column::Id))
        .column((ep.clone(), estimate_points::Column::Value))
        .expr_as(
            Expr::col((e.clone(), estimates::Column::Type)),
            "estimate_type",
        )
        .from_as(estimate_points::Entity, ep.clone())
        .join_as(
            JoinType::InnerJoin,
            estimates::Entity,
            e.clone(),
            Expr::col((ep.clone(), estimate_points::Column::EstimateId))
                .equals((e.clone(), estimates::Column::Id)),
        )
        .and_where(Expr::col((ep.clone(), estimate_points::Column::Id)).is_in(ids.to_vec()))
        .and_where(Expr::col((ep.clone(), estimate_points::Column::DeletedAt)).is_null());
    stmt
}

/// `Issue.objects.get(pk=issue_id)` projection `create_issue_activity`
/// reads (`:567`): `created_at` and `created_by_id`.
fn created_issue_query(id: Uuid) -> Select<issues::Entity> {
    issues::Entity::find()
        .select_only()
        .columns([issues::Column::CreatedAt, issues::Column::CreatedById])
        .filter(issues::Column::Id.eq(id))
        .filter(issues::Column::DeletedAt.is_null())
        .limit(1)
}

/// `IssueActivity.objects.filter(issue_id=...).order_by("-created_at").first()`
/// for `track_description` (`:89`): the planner reads `field` and
/// `actor_id`; the driver keeps `id` for the touch write (`:95-96`).
fn latest_activity_query(issue_id: Uuid) -> Select<issue_activities::Entity> {
    issue_activities::Entity::find()
        .select_only()
        .columns([
            issue_activities::Column::Id,
            issue_activities::Column::Field,
            issue_activities::Column::ActorId,
        ])
        .filter(issue_activities::Column::IssueId.eq(issue_id))
        .order_by_desc(issue_activities::Column::CreatedAt)
        .limit(1)
}

/// The Redis surface the dispatcher needs. There is no shared Redis client
/// in the Rust crates yet (same position as `tasks_mail`'s `RedisLock`),
/// so this domain-owned trait records the exact call shape the Python
/// makes: `ri.set(str(issue_id), origin, ex=600)` (`:1617-1619`). The
/// return is ignored in Python; failures propagate to the broad-except, so
/// this returns a `Result`.
pub trait RedisOrigin {
    /// `redis.set(key, value, ex=600)`.
    fn set_ex(&self, key: &str, value: &str, ex_secs: u64) -> Result<(), String>;
}

/// DRF `DateTimeField` rendering for UTC values: `isoformat()` with a `Z`
/// suffix; microseconds appear only when nonzero.
pub fn drf_datetime(value: &DateTime<Utc>) -> String {
    let naive = value.naive_utc();
    if naive.and_utc().timestamp_subsec_micros() == 0 {
        naive.format("%Y-%m-%dT%H:%M:%SZ").to_string()
    } else {
        naive.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
    }
}

/// `json.dumps` with Django separators — moved to its canonical home
/// `pidash_compat::py::django_dumps` by OVL-14 (PIDASHCONV-1649);
/// re-exported here so every existing path keeps working.
pub use pidash_compat::py::django_dumps;

/// One inserted `IssueActivity` row plus its `RETURNING` columns, ready to
/// serialize (`bulk_create` returns the created rows, `:1675`, and the
/// serializer runs over them, `:1593-1596`).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredActivityRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub row: ActivityRow,
    pub issue_comment_id: Option<Uuid>,
}

impl StoredActivityRow {
    /// Lift a planner row with its `RETURNING` columns. Identifier and id
    /// columns pass through as stored — they are UUID columns, so only
    /// valid UUIDs (or nulls) can be here, exactly what the serializer
    /// stringifies.
    pub fn new(
        id: Uuid,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        row: ActivityRow,
        issue_comment_id: Option<Uuid>,
    ) -> Self {
        Self {
            id,
            created_at,
            updated_at,
            row,
            issue_comment_id,
        }
    }
}

/// The `IssueFlatSerializer` projection (`serializers/issue.py:105-122`)
/// for `issue_detail`.
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
pub struct IssueFlatRow {
    pub id: Uuid,
    pub name: String,
    pub description_json: Value,
    pub description_html: String,
    pub priority: Option<String>,
    pub complexity_score: Option<i32>,
    pub start_date: Option<chrono::NaiveDate>,
    pub target_date: Option<chrono::NaiveDate>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub is_draft: bool,
}

/// DRF renders a `date` as `YYYY-MM-DD`.
fn drf_date(value: &chrono::NaiveDate) -> String {
    value.format("%Y-%m-%d").to_string()
}

/// DRF renders a float with `repr` (`65535.0`, never `65535`).
fn drf_float(value: f64) -> Value {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// `IssueFlatSerializer.to_representation`: the 11 flat fields.
pub fn issue_flat_json(flat: &IssueFlatRow) -> Map<String, Value> {
    let mut map = Map::with_capacity(11);
    map.insert("id".to_owned(), Value::String(flat.id.to_string()));
    map.insert("name".to_owned(), Value::String(flat.name.clone()));
    map.insert("description_json".to_owned(), flat.description_json.clone());
    map.insert(
        "description_html".to_owned(),
        Value::String(flat.description_html.clone()),
    );
    map.insert(
        "priority".to_owned(),
        flat.priority
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    map.insert(
        "complexity_score".to_owned(),
        flat.complexity_score
            .map(|score| Value::Number(score.into()))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "start_date".to_owned(),
        flat.start_date
            .map(|date| Value::String(drf_date(&date)))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "target_date".to_owned(),
        flat.target_date
            .map(|date| Value::String(drf_date(&date)))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "sequence_id".to_owned(),
        Value::Number(flat.sequence_id.into()),
    );
    map.insert("sort_order".to_owned(), drf_float(flat.sort_order));
    map.insert("is_draft".to_owned(), Value::Bool(flat.is_draft));
    map
}

fn opt_json(value: &Option<String>) -> Value {
    value.clone().map(Value::String).unwrap_or(Value::Null)
}

/// `IssueActivitySerializer(row).data` for one inserted row: the `__all__`
/// columns under DRF names (FKs render as string pks, datetimes as
/// DRF-ISO), then the declared extras in declaration order
/// (`serializers/issue.py:521-539`). `bulk_create` skips `save()`, so
/// `created_by` / `updated_by` stay null (no crum user in a worker).
pub fn activity_json(
    stored: &StoredActivityRow,
    issue_flat: &Map<String, Value>,
) -> Map<String, Value> {
    let row = &stored.row;
    let mut map = Map::with_capacity(25);
    map.insert("id".to_owned(), Value::String(stored.id.to_string()));
    map.insert(
        "created_at".to_owned(),
        Value::String(drf_datetime(&stored.created_at)),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(drf_datetime(&stored.updated_at)),
    );
    map.insert("created_by".to_owned(), Value::Null);
    map.insert("updated_by".to_owned(), Value::Null);
    map.insert("deleted_at".to_owned(), Value::Null);
    map.insert("project".to_owned(), Value::String(row.project_id.clone()));
    map.insert(
        "workspace".to_owned(),
        Value::String(row.workspace_id.clone()),
    );
    map.insert(
        "issue".to_owned(),
        row.issue_id
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    map.insert("verb".to_owned(), Value::String(row.verb.clone()));
    map.insert("field".to_owned(), opt_json(&row.field));
    map.insert("old_value".to_owned(), opt_json(&row.old_value));
    map.insert("new_value".to_owned(), opt_json(&row.new_value));
    map.insert("comment".to_owned(), Value::String(row.comment.clone()));
    map.insert("attachments".to_owned(), Value::Array(Vec::new()));
    map.insert(
        "issue_comment".to_owned(),
        stored
            .issue_comment_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    map.insert("actor".to_owned(), Value::String(row.actor_id.clone()));
    map.insert("old_identifier".to_owned(), opt_json(&row.old_identifier));
    map.insert("new_identifier".to_owned(), opt_json(&row.new_identifier));
    map.insert(
        "epoch".to_owned(),
        serde_json::Number::from_f64(row.epoch)
            .map(Value::Number)
            .unwrap_or(Value::Null),
    );
    map.insert("actor_detail".to_owned(), Value::Null);
    map.insert("issue_detail".to_owned(), Value::Object(issue_flat.clone()));
    map.insert("project_detail".to_owned(), Value::Null);
    map.insert("workspace_detail".to_owned(), Value::Null);
    map.insert("source_data".to_owned(), Value::Null);
    map
}

/// `json.dumps(IssueActivitySerializer(created_rows, many=True).data,
/// cls=DjangoJSONEncoder)` (`:1593-1596`).
pub fn serialize_notification_rows(
    rows: &[StoredActivityRow],
    issue_flat: &Map<String, Value>,
) -> String {
    let items: Vec<Value> = rows
        .iter()
        .map(|row| Value::Object(activity_json(row, issue_flat)))
        .collect();
    django_dumps(&Value::Array(items))
}

/// `notifications.delay(type=..., issue_id=..., actor_id=...,
/// project_id=..., subscriber=..., issue_activities_created=...,
/// requested_data=..., current_instance=...)` (`:1587-1599`) as a queue
/// job: `.delay()` with all keywords arrives as empty args plus kwargs, so
/// the Rust worker forwards it to the Python consumers in Celery protocol
/// v2 with these kwargs in call order. `requested_data` / `current_instance`
/// are the RAW args (unparsed string or `None`); `issue_id` / `actor_id` /
/// `project_id` pass through untouched (possibly null).
pub fn build_notifications_job(call: &IssueActivityCall, serialized: &str) -> NewJob {
    let mut kwargs = Map::with_capacity(8);
    kwargs.insert("type".to_owned(), Value::String(call.activity_type.clone()));
    kwargs.insert(
        "issue_id".to_owned(),
        call.issue_id
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    kwargs.insert(
        "actor_id".to_owned(),
        call.actor_id
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    kwargs.insert(
        "project_id".to_owned(),
        call.project_id
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    kwargs.insert("subscriber".to_owned(), Value::Bool(call.subscriber));
    kwargs.insert(
        "issue_activities_created".to_owned(),
        Value::String(serialized.to_owned()),
    );
    kwargs.insert(
        "requested_data".to_owned(),
        call.requested_data
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    kwargs.insert(
        "current_instance".to_owned(),
        call.current_instance
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    NewJob::new(
        NOTIFICATIONS_TASK,
        Value::Array(Vec::new()),
        Value::Object(kwargs),
    )
}

// ---------------------------------------------------------------------------
// Live driver.
// ---------------------------------------------------------------------------

use std::collections::{HashMap, HashSet};

use pidash_services::tasks_webhooks::activity_tracks::extract_ids;

/// How the driver fails. The Python broad-except (`:1693-1695`) swallows
/// everything into log-and-return, so the handler always settles `Ack`:
/// `Quiet` skips the log for the guard paths that return before any work,
/// `Log` carries the message for `tracing::error!`.
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchError {
    Quiet,
    Log(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::Quiet => write!(f, "issue_activity: early return"),
            DispatchError::Log(message) => write!(f, "issue_activity: {message}"),
        }
    }
}

/// Parse one uuid-typed column. Garbage fails here, before any write —
/// the Django cast error aborts the whole batch the same way.
fn parse_uuid(text: &str, column: &'static str) -> Result<Uuid, DispatchError> {
    Uuid::parse_str(text)
        .map_err(|_| DispatchError::Log(format!("bulk_create: invalid uuid in {column}")))
}

fn parse_uuid_opt(
    value: &Option<String>,
    column: &'static str,
) -> Result<Option<Uuid>, DispatchError> {
    match value {
        None => Ok(None),
        Some(text) => parse_uuid(text, column).map(Some),
    }
}

/// One collected bulk row with every uuid-typed column parsed and ready
/// to bind typed. Parsing happens before any write so a garbage
/// identifier aborts the batch exactly like the Django `bulk_create` cast
/// error does. (Binds must be typed `Uuid`: a `text` param against a uuid
/// column errors at runtime.)
struct BoundRow {
    row: ActivityRow,
    project_id: Uuid,
    workspace_id: Uuid,
    issue_id: Option<Uuid>,
    actor_id: Uuid,
    old_identifier: Option<Uuid>,
    new_identifier: Option<Uuid>,
    issue_comment_id: Option<Uuid>,
}

fn bind_row(row: ActivityRow) -> Result<BoundRow, DispatchError> {
    Ok(BoundRow {
        project_id: parse_uuid(&row.project_id, "project_id")?,
        workspace_id: parse_uuid(&row.workspace_id, "workspace_id")?,
        issue_id: parse_uuid_opt(&row.issue_id, "issue_id")?,
        actor_id: parse_uuid(&row.actor_id, "actor_id")?,
        old_identifier: parse_uuid_opt(&row.old_identifier, "old_identifier")?,
        new_identifier: parse_uuid_opt(&row.new_identifier, "new_identifier")?,
        issue_comment_id: parse_uuid_opt(&row.issue_comment_id, "issue_comment_id")?,
        row,
    })
}

/// Parse a lookup id for a direct (non-text-cast) bind. The ORM raises on
/// garbage pks, so garbage is fatal here too.
fn lookup_uuid(id: &str, what: &'static str) -> Result<Uuid, DispatchError> {
    Uuid::parse_str(id).map_err(|_| DispatchError::Log(format!("{what}: invalid uuid {id}")))
}

fn draft_to_row(draft: IssueActivityDraft) -> ActivityRow {
    ActivityRow {
        issue_id: Some(draft.issue_id),
        actor_id: draft.actor_id,
        verb: draft.verb,
        old_value: draft.old_value,
        new_value: draft.new_value,
        field: Some(draft.field),
        project_id: draft.project_id,
        workspace_id: draft.workspace_id,
        comment: draft.comment,
        old_identifier: draft.old_identifier,
        new_identifier: draft.new_identifier,
        issue_comment_id: None,
        epoch: draft.epoch,
    }
}

/// Partial decoded rows for the bulk snapshots (unique output names).
#[derive(Debug, Clone, FromQueryResult)]
struct StateRow {
    id: Uuid,
    name: String,
}

#[derive(Debug, Clone, FromQueryResult)]
struct LabelRow {
    id: Uuid,
    name: String,
}

#[derive(Debug, Clone, FromQueryResult)]
struct SnapUserRow {
    id: Uuid,
    display_name: String,
}

#[derive(Debug, Clone, FromQueryResult)]
struct CycleRow {
    id: Uuid,
    name: String,
}

#[derive(Debug, Clone, FromQueryResult)]
struct ModuleRow {
    id: Uuid,
    name: String,
}

#[derive(Debug, Clone, FromQueryResult)]
struct LatestActivityRow {
    id: Uuid,
    field: Option<String>,
    actor_id: Option<Uuid>,
}

#[derive(Debug, Clone, FromQueryResult)]
struct CreatedIssueRow {
    created_at: chrono::DateTime<chrono::FixedOffset>,
    created_by_id: Option<Uuid>,
}

#[derive(Debug, Clone, FromQueryResult)]
struct RunnerRow {
    id: Uuid,
    name: String,
}

#[derive(Debug, Clone, FromQueryResult)]
struct PodRow {
    id: Uuid,
    name: String,
}

#[derive(Debug, Clone, FromQueryResult)]
struct CommentReactionRow {
    id: Uuid,
    comment_id: Uuid,
}

/// Parse the raw string ids of a bulk snapshot fetch to their valid
/// UUIDs. The old `id::text = ANY($1)` comparison made garbage ids
/// simply miss instead of erroring the whole statement; unparseable
/// entries match no row here too, so they stay misses. The scanners
/// encode the Python validity rules on top (and the estimate pre-check
/// preserves the one fatal-on-garbage filter).
fn parse_snapshot_ids(ids: &[String]) -> Vec<Uuid> {
    ids.iter()
        .filter_map(|id| Uuid::parse_str(id).ok())
        .collect()
}

/// Parse a raw payload for id extraction only. Failures return `None`;
/// the planner re-parses and raises the same error the Python raises.
fn parsed_opt(raw: Option<&str>) -> Option<Value> {
    raw.and_then(|text| serde_json::from_str(text).ok())
}

/// Scalar ids out of a JSON value (`extract_ids` semantics for the
/// label/assignee lists; single keys read directly).
fn id_set(payload: Option<&Value>, primary: &str, fallback: &str) -> Vec<String> {
    match extract_ids(payload, primary, fallback) {
        Ok(set) => set.into_iter().collect(),
        Err(_) => Vec::new(),
    }
}

fn scalar_id(payload: Option<&Value>, key: &str) -> Option<String> {
    payload?.get(key).and_then(|value| match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(if *b { "True" } else { "False" }.to_owned()),
        _ => None,
    })
}

/// The snapshot bundle the update dispatcher feeds the `track_*`
/// helpers through [`UpdateResolvers`], plus the side-write keys.
struct UpdateSnapshots {
    parents: HashMap<String, ParentRef>,
    states: HashMap<String, StateRef>,
    labels: HashMap<String, LabelRef>,
    users: HashMap<String, UserRef>,
    estimates: HashMap<String, EstimatePointRef>,
    runners: HashMap<String, RunnerRef>,
    pods: HashMap<String, PodRef>,
    latest: Option<(Uuid, DescriptionLatest)>,
}

/// Fetch the resolver snapshots for `update_issue_activity`: parents,
/// project-scoped states, labels, users, estimate points (with the fatal
/// garbage-id pre-check `track_estimate_points` implies — its
/// `filter(pk=...)` raises on invalid UUIDs), and the latest activity for
/// `track_description`. Misses stay misses; the planners encode the
/// Python validity rules and miss behavior on top.
async fn fetch_update_snapshots(
    pool: &PgPool,
    project_id: &str,
    issue_id: &str,
    requested: Option<&Value>,
    current: Option<&Value>,
) -> Result<UpdateSnapshots, DispatchError> {
    let fail = |error: sqlx::Error| DispatchError::Log(format!("update snapshot: {error}"));
    let project_uuid = lookup_uuid(project_id, "update snapshot")?;
    // Parent ids (both key spellings, both sides).
    let mut parent_ids: HashSet<String> = HashSet::new();
    for payload in [requested, current] {
        for key in ["parent_id", "parent"] {
            if let Some(id) = scalar_id(payload, key) {
                parent_ids.insert(id);
            }
        }
    }
    let parent_list: Vec<String> = parent_ids.into_iter().collect();
    let db = pidash_db::pool::sea_orm_connection(pool);
    let mut parents = HashMap::new();
    if !parent_list.is_empty() {
        let parsed = parse_snapshot_ids(&parent_list);
        let rows = db
            .query_all(DbBackend::Postgres.build(&issue_ref_statement(&parsed)))
            .await
            .map_err(|error| fail(into_sqlx(error)))?;
        for row in &rows {
            let id: Uuid = row
                .try_get("", "id")
                .map_err(|error| fail(into_sqlx(error)))?;
            let key = id.to_string();
            parents.insert(
                key.clone(),
                ParentRef {
                    id: key,
                    identifier: row
                        .try_get("", "identifier")
                        .map_err(|error| fail(into_sqlx(error)))?,
                    sequence_id: row
                        .try_get("", "sequence_id")
                        .map_err(|error| fail(into_sqlx(error)))?,
                },
            );
        }
    }
    // State ids are project-scoped (`State.objects.filter(pk=...,
    // project_id=...)`), and the default `StateManager` excludes
    // `group="triage"` (`db/models/state.py:79-83`).
    let mut state_ids: HashSet<String> = HashSet::new();
    for payload in [requested, current] {
        for key in ["state_id", "state"] {
            if let Some(id) = scalar_id(payload, key) {
                state_ids.insert(id);
            }
        }
    }
    // `track_closed_to` (`:527-556`) reads `closed_to` from the requested
    // side only and resolves it through the same project-scoped lookup —
    // without this the resolver misses and the whole task aborts
    // (PIDASHCONV-1276).
    if let Some(id) = scalar_id(requested, "closed_to") {
        state_ids.insert(id);
    }
    let state_list: Vec<String> = state_ids.into_iter().collect();
    let mut states = HashMap::new();
    if !state_list.is_empty() {
        let parsed = parse_snapshot_ids(&state_list);
        let models = states::Entity::find()
            .select_only()
            .columns([states::Column::Id, states::Column::Name])
            .filter(states::Column::Id.is_in(parsed))
            .filter(states::Column::ProjectId.eq(project_uuid))
            .filter(states::Column::DeletedAt.is_null())
            .filter(states::Column::Group.ne("triage"))
            .into_model::<StateRow>()
            .all(&db)
            .await
            .map_err(|error| fail(into_sqlx(error)))?;
        for model in models {
            let key = model.id.to_string();
            states.insert(
                key.clone(),
                StateRef {
                    id: key,
                    name: model.name,
                },
            );
        }
    }
    // Label ids, both sides (`extract_ids` semantics).
    let mut label_ids: HashSet<String> = HashSet::new();
    for payload in [requested, current] {
        label_ids.extend(id_set(payload, "label_ids", "labels"));
    }
    let label_list: Vec<String> = label_ids.into_iter().collect();
    let mut labels = HashMap::new();
    if !label_list.is_empty() {
        let parsed = parse_snapshot_ids(&label_list);
        let models = labels::Entity::find()
            .select_only()
            .columns([labels::Column::Id, labels::Column::Name])
            .filter(labels::Column::Id.is_in(parsed))
            .filter(labels::Column::DeletedAt.is_null())
            .into_model::<LabelRow>()
            .all(&db)
            .await
            .map_err(|error| fail(into_sqlx(error)))?;
        for model in models {
            let key = model.id.to_string();
            labels.insert(
                key.clone(),
                LabelRef {
                    id: key,
                    name: model.name,
                },
            );
        }
    }
    // Assignee ids, both sides. No soft-delete scope on `users`.
    let mut user_ids: HashSet<String> = HashSet::new();
    for payload in [requested, current] {
        user_ids.extend(id_set(payload, "assignee_ids", "assignees"));
    }
    let user_list: Vec<String> = user_ids.into_iter().collect();
    let mut users = HashMap::new();
    if !user_list.is_empty() {
        let parsed = parse_snapshot_ids(&user_list);
        let models = users::Entity::find()
            .select_only()
            .columns([users::Column::Id, users::Column::DisplayName])
            .filter(users::Column::Id.is_in(parsed))
            .into_model::<SnapUserRow>()
            .all(&db)
            .await
            .map_err(|error| fail(into_sqlx(error)))?;
        for model in models {
            let key = model.id.to_string();
            users.insert(
                key.clone(),
                UserRef {
                    id: key,
                    display_name: model.display_name,
                },
            );
        }
    }
    // Estimate ids: `filter(pk=...)` with no validity guard, so a present
    // non-null invalid id raises in Python — fatal here before the planner
    // runs (same final state: touch and redis already done, no writes).
    for payload in [requested, current] {
        if let Some(id) = scalar_id(payload, "estimate_point") {
            if !is_valid_uuid(&id) {
                return Err(DispatchError::Log(format!(
                    "update snapshot: invalid estimate_point id {id}"
                )));
            }
        }
    }
    let mut estimate_ids: HashSet<String> = HashSet::new();
    for payload in [requested, current] {
        if let Some(id) = scalar_id(payload, "estimate_point") {
            estimate_ids.insert(id);
        }
    }
    let estimate_list: Vec<String> = estimate_ids.into_iter().collect();
    let mut estimates = HashMap::new();
    if !estimate_list.is_empty() {
        let parsed = parse_snapshot_ids(&estimate_list);
        let stmt = estimate_points_statement(&parsed);
        let rows = db
            .query_all(DbBackend::Postgres.build(&stmt))
            .await
            .map_err(|error| fail(into_sqlx(error)))?;
        for row in &rows {
            let id: Uuid = row
                .try_get("", "id")
                .map_err(|error| fail(into_sqlx(error)))?;
            estimates.insert(
                id.to_string(),
                EstimatePointRef {
                    value: row
                        .try_get("", "value")
                        .map_err(|error| fail(into_sqlx(error)))?,
                    estimate_type: row
                        .try_get("", "estimate_type")
                        .map_err(|error| fail(into_sqlx(error)))?,
                },
            );
        }
    }
    // Runner ids (`assigned_runner_id or assigned_runner`, both sides)
    // — `filter(pk=...).first()`, invalid UUIDs coerce to `None` in the
    // planner (not fatal), so unparseable ids simply miss here.
    let mut runner_ids: HashSet<String> = HashSet::new();
    for payload in [requested, current] {
        for key in ["assigned_runner_id", "assigned_runner"] {
            if let Some(id) = scalar_id(payload, key) {
                runner_ids.insert(id);
            }
        }
    }
    let runner_list: Vec<String> = runner_ids.into_iter().collect();
    let mut runners = HashMap::new();
    if !runner_list.is_empty() {
        let parsed = parse_snapshot_ids(&runner_list);
        if !parsed.is_empty() {
            let models = runner::Entity::find()
                .select_only()
                .columns([runner::Column::Id, runner::Column::Name])
                .filter(runner::Column::Id.is_in(parsed))
                .into_model::<RunnerRow>()
                .all(&db)
                .await
                .map_err(|error| fail(into_sqlx(error)))?;
            for model in models {
                let key = model.id.to_string();
                runners.insert(
                    key.clone(),
                    RunnerRef {
                        id: key,
                        name: model.name,
                    },
                );
            }
        }
    }
    // Pod ids (`assigned_pod_id or assigned_pod`, both sides) — same
    // shape, scoped to non-deleted pods.
    let mut pod_ids: HashSet<String> = HashSet::new();
    for payload in [requested, current] {
        for key in ["assigned_pod_id", "assigned_pod"] {
            if let Some(id) = scalar_id(payload, key) {
                pod_ids.insert(id);
            }
        }
    }
    let pod_list: Vec<String> = pod_ids.into_iter().collect();
    let mut pods = HashMap::new();
    if !pod_list.is_empty() {
        let parsed = parse_snapshot_ids(&pod_list);
        if !parsed.is_empty() {
            let models = pod::Entity::find()
                .select_only()
                .columns([pod::Column::Id, pod::Column::Name])
                .filter(pod::Column::Id.is_in(parsed))
                .filter(pod::Column::DeletedAt.is_null())
                .into_model::<PodRow>()
                .all(&db)
                .await
                .map_err(|error| fail(into_sqlx(error)))?;
            for model in models {
                let key = model.id.to_string();
                pods.insert(
                    key.clone(),
                    PodRef {
                        id: key,
                        name: model.name,
                    },
                );
            }
        }
    }
    // Latest activity for `track_description` (`:89`). `actor_id`
    // stringifies like `str(...)` (`None` → `"None"`); a null `field`
    // behaves as not-`"description"`, rendered as `""`. A missing issue
    // filters to empty (`filter(issue_id=None)`), never an error.
    let latest = if issue_id.is_empty() {
        None
    } else {
        let model = latest_activity_query(lookup_uuid(issue_id, "description snapshot")?)
            .into_model::<LatestActivityRow>()
            .one(&db)
            .await
            .map_err(|error| fail(into_sqlx(error)))?;
        model.map(|model| {
            (
                model.id,
                DescriptionLatest {
                    field: model.field.unwrap_or_default(),
                    actor_id: model
                        .actor_id
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "None".to_owned()),
                },
            )
        })
    };
    Ok(UpdateSnapshots {
        parents,
        states,
        labels,
        users,
        estimates,
        runners,
        pods,
        latest,
    })
}

/// Everything the dispatcher writes for one call: the bulk batch (with
/// UUID columns parsed), subscriber drafts, the description touch, the
/// cycle/module touches, and the create-issue immediate row (inserted
/// first and excluded from the notification payload, `:568-579`).
struct Collected {
    rows: Vec<BoundRow>,
    subscribers: Vec<IssueSubscriberDraft>,
    touch_activity: Option<Uuid>,
    touched_issues: Vec<String>,
    immediate: Option<(ActivityRow, DateTime<Utc>)>,
}

/// Present non-null reaction values must be strings: the ORM filter would
/// raise on anything else, so anything else is fatal here too.
fn reaction_string(payload: Option<&Value>) -> Result<Option<String>, DispatchError> {
    match payload.and_then(|value| value.get("reaction")) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(DispatchError::Log(
            "reaction lookup: reaction must be a string".to_owned(),
        )),
    }
}

/// Fetch cycles by id set into `CycleRef` maps.
async fn fetch_cycles(
    pool: &PgPool,
    ids: &[String],
) -> Result<HashMap<String, CycleRef>, DispatchError> {
    let mut map = HashMap::new();
    if ids.is_empty() {
        return Ok(map);
    }
    let parsed = parse_snapshot_ids(ids);
    let db = pidash_db::pool::sea_orm_connection(pool);
    let models = cycles::Entity::find()
        .select_only()
        .columns([cycles::Column::Id, cycles::Column::Name])
        .filter(cycles::Column::Id.is_in(parsed))
        .filter(cycles::Column::DeletedAt.is_null())
        .into_model::<CycleRow>()
        .all(&db)
        .await
        .map_err(|error| DispatchError::Log(format!("cycle snapshot: {}", into_sqlx(error))))?;
    for model in models {
        let key = model.id.to_string();
        map.insert(
            key.clone(),
            CycleRef {
                id: key,
                name: model.name,
            },
        );
    }
    Ok(map)
}

/// Bulk existence set for the `issue_exists` planner closures.
async fn fetch_existing_issues(
    pool: &PgPool,
    ids: &[String],
) -> Result<HashSet<String>, DispatchError> {
    if ids.is_empty() {
        return Ok(HashSet::new());
    }
    let parsed = parse_snapshot_ids(ids);
    let db = pidash_db::pool::sea_orm_connection(pool);
    let models = issues::Entity::find()
        .select_only()
        .column(issues::Column::Id)
        .filter(issues::Column::Id.is_in(parsed))
        .filter(issues::Column::DeletedAt.is_null())
        .into_tuple::<Uuid>()
        .all(&db)
        .await
        .map_err(|error| DispatchError::Log(format!("issue snapshot: {}", into_sqlx(error))))?;
    Ok(models.into_iter().map(|id| id.to_string()).collect())
}

/// Run the `ACTIVITY_MAPPER` builder for the call: snapshot queries plus
/// the services planner, in Python call order per builder. Planner errors
/// (the Python raises) abort the batch; a falsy-payload no-row is an
/// empty batch, not an error.
async fn collect_rows(
    pool: &PgPool,
    frame: &TrackFrame,
    call: &IssueActivityCall,
    project_id: &str,
) -> Result<Collected, DispatchError> {
    let fail = |error: BuildError| DispatchError::Log(format!("builder: {error}"));
    // Guarded v4 upstream; parsed once for every project-scoped bind.
    let project_uuid = lookup_uuid(project_id, "project scope")?;
    let requested = call.requested_data.as_deref();
    let current = call.current_instance.as_deref();
    let mut out = Collected {
        rows: Vec::new(),
        subscribers: Vec::new(),
        touch_activity: None,
        touched_issues: Vec::new(),
        immediate: None,
    };
    let push = |out: &mut Collected, row: ActivityRow| -> Result<(), DispatchError> {
        out.rows.push(bind_row(row)?);
        Ok(())
    };
    let push_opt = |out: &mut Collected, row: Option<ActivityRow>| -> Result<(), DispatchError> {
        if let Some(row) = row {
            push(out, row)?;
        }
        Ok(())
    };
    match call.activity_type.as_str() {
        "issue.activity.created" => {
            // `Issue.objects.get` raises on a miss or a bad pk (`:567`).
            let created_db = pidash_db::pool::sea_orm_connection(pool);
            let created: Option<(DateTime<Utc>, Option<Uuid>)> =
                created_issue_query(lookup_uuid(&frame.issue_id, "create issue")?)
                    .into_model::<CreatedIssueRow>()
                    .one(&created_db)
                    .await
                    .map_err(|error| {
                        DispatchError::Log(format!("create issue: {}", into_sqlx(error)))
                    })?
                    .map(|row| (row.created_at.with_timezone(&Utc), row.created_by_id));
            let Some((created_at, created_by)) = created else {
                return Err(DispatchError::Log(format!(
                    "create issue: row missing for {}",
                    frame.issue_id
                )));
            };
            // `current_instance` travels raw into `extract_ids` (`:584`):
            // `None` and `""` (falsy) mean no previous assignees; any
            // other string has no `.get` (fatal); a real dict would diff,
            // but dicts cannot arrive here — only strings do — so a
            // string parsing to an object is fatal too.
            let current_decoded: Option<Value> = match &call.current_instance {
                None => None,
                Some(text) if text.is_empty() => None,
                Some(text) => match serde_json::from_str(text) {
                    Ok(Value::Object(_)) | Err(_) => {
                        return Err(DispatchError::Log(
                            "create issue: current is not decoded".to_owned(),
                        ));
                    }
                    Ok(value) => Some(value),
                },
            };
            let created_ref = CreatedIssueRef {
                created_at: created_at.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
                // `created_by_id` is null only for rows no API call could
                // have made; the caller actor stands in (documented
                // deviation, unreachable in contract).
                created_by_id: created_by
                    .map(|id: Uuid| id.to_string())
                    .unwrap_or_else(|| frame.actor_id.clone()),
            };
            // Assignee prefetch for the create-time `track_assignees` call.
            let requested_value = parsed_opt(requested);
            let mut assignee_ids: HashSet<String> = HashSet::new();
            assignee_ids.extend(id_set(
                requested_value.as_ref(),
                "assignee_ids",
                "assignees",
            ));
            let assignee_list: Vec<String> = assignee_ids.into_iter().collect();
            let mut users = HashMap::new();
            if !assignee_list.is_empty() {
                let parsed = parse_snapshot_ids(&assignee_list);
                let users_db = pidash_db::pool::sea_orm_connection(pool);
                let models = users::Entity::find()
                    .select_only()
                    .columns([users::Column::Id, users::Column::DisplayName])
                    .filter(users::Column::Id.is_in(parsed))
                    .into_model::<SnapUserRow>()
                    .all(&users_db)
                    .await
                    .map_err(|error| {
                        DispatchError::Log(format!("create issue users: {}", into_sqlx(error)))
                    })?;
                for model in models {
                    let key = model.id.to_string();
                    users.insert(
                        key.clone(),
                        UserRef {
                            id: key,
                            display_name: model.display_name,
                        },
                    );
                }
            }
            let outcome = plan_create_issue(
                requested,
                current_decoded.as_ref(),
                Some(created_ref),
                frame,
                &|id| users.get(id).cloned(),
            )
            .map_err(fail)?;
            out.immediate = Some((outcome.row, created_at));
            if let Some(assignees) = outcome.assignees {
                for draft in assignees.activities {
                    push(&mut out, draft_to_row(draft))?;
                }
                out.subscribers.extend(assignees.subscribers);
            }
        }
        "issue.activity.updated" => {
            let requested_value = parsed_opt(requested);
            let current_value = parsed_opt(current);
            let snapshots = fetch_update_snapshots(
                pool,
                project_id,
                &frame.issue_id,
                requested_value.as_ref(),
                current_value.as_ref(),
            )
            .await?;
            let resolvers = UpdateResolvers {
                parent: &|id| snapshots.parents.get(id).cloned(),
                state: &|id| snapshots.states.get(id).cloned(),
                label: &|id| snapshots.labels.get(id).cloned(),
                user: &|id| snapshots.users.get(id).cloned(),
                estimate: &|id| snapshots.estimates.get(id).cloned(),
                runner: &|id| snapshots.runners.get(id).cloned(),
                pod: &|id| snapshots.pods.get(id).cloned(),
                description_latest: snapshots.latest.as_ref().map(|(_, latest)| latest.clone()),
            };
            let outcome = plan_update_issue(requested, current, frame, &resolvers).map_err(fail)?;
            for draft in outcome.rows {
                push(&mut out, draft_to_row(draft))?;
            }
            out.subscribers.extend(outcome.subscribers);
            if outcome.description_touched {
                out.touch_activity = snapshots.latest.map(|(id, _)| id);
            }
        }
        "issue.activity.deleted" => push(&mut out, plan_delete_issue(frame))?,
        "comment.activity.created" => push(
            &mut out,
            plan_create_comment(requested, frame).map_err(fail)?,
        )?,
        "comment.activity.updated" => {
            push_opt(
                &mut out,
                plan_update_comment(requested, current, frame).map_err(fail)?,
            )?;
        }
        "comment.activity.deleted" => push(
            &mut out,
            plan_delete_comment(requested, frame).map_err(fail)?,
        )?,
        "cycle.activity.created" => {
            // Updated records carry old/new cycle ids + issue ids; created
            // records are double-encoded with `fields.cycle` /
            // `fields.issue`.
            let current_value = parsed_opt(current);
            let mut cycle_ids: HashSet<String> = HashSet::new();
            let mut issue_ids: HashSet<String> = HashSet::new();
            if let Some(Value::Object(map)) = current_value.as_ref() {
                if let Some(Value::Array(records)) = map.get("updated_cycle_issues") {
                    for record in records {
                        for key in ["old_cycle_id", "new_cycle_id"] {
                            if let Some(id) = scalar_id(Some(record), key) {
                                cycle_ids.insert(id);
                            }
                        }
                        if let Some(id) = scalar_id(Some(record), "issue_id") {
                            issue_ids.insert(id);
                        }
                    }
                }
                if let Some(Value::String(created_raw)) = map.get("created_cycle_issues") {
                    if let Ok(Value::Array(records)) = serde_json::from_str::<Value>(created_raw) {
                        for record in records {
                            if let Some(fields) = record.get("fields") {
                                if let Some(id) = scalar_id(Some(fields), "cycle") {
                                    cycle_ids.insert(id);
                                }
                                if let Some(id) = scalar_id(Some(fields), "issue") {
                                    issue_ids.insert(id);
                                }
                            }
                        }
                    }
                }
            }
            let cycle_list: Vec<String> = cycle_ids.into_iter().collect();
            let issue_list: Vec<String> = issue_ids.into_iter().collect();
            let cycles = fetch_cycles(pool, &cycle_list).await?;
            let existing = fetch_existing_issues(pool, &issue_list).await?;
            let outcome =
                plan_create_cycle_issue(current, frame, &|id| cycles.get(id).cloned(), &|id| {
                    existing.contains(id)
                })
                .map_err(fail)?;
            for row in outcome.rows {
                push(&mut out, row)?;
            }
            out.touched_issues.extend(outcome.touched_issue_ids);
        }
        "cycle.activity.deleted" => {
            let requested_value = parsed_opt(requested);
            let mut cycle_ids = HashSet::new();
            if let Some(id) = scalar_id(requested_value.as_ref(), "cycle_id") {
                cycle_ids.insert(id);
            }
            let mut issue_ids = HashSet::new();
            if let Some(Value::Array(items)) =
                requested_value.as_ref().and_then(|v| v.get("issues"))
            {
                for item in items {
                    if let Value::String(id) = item {
                        issue_ids.insert(id.clone());
                    } else if !item.is_null() {
                        issue_ids.insert(item.to_string());
                    }
                }
            }
            let cycles = fetch_cycles(pool, &cycle_ids.into_iter().collect::<Vec<_>>()).await?;
            let existing =
                fetch_existing_issues(pool, &issue_ids.into_iter().collect::<Vec<_>>()).await?;
            let outcome =
                plan_delete_cycle_issue(requested, frame, &|id| cycles.get(id).cloned(), &|id| {
                    existing.contains(id)
                })
                .map_err(fail)?;
            for row in outcome.rows {
                push(&mut out, row)?;
            }
            out.touched_issues.extend(outcome.touched_issue_ids);
        }
        "module.activity.created" => {
            let requested_value = parsed_opt(requested);
            let module_id = scalar_id(requested_value.as_ref(), "module_id");
            let mut modules = HashMap::new();
            if let Some(id) = module_id {
                let parsed = parse_snapshot_ids(std::slice::from_ref(&id));
                if !parsed.is_empty() {
                    let modules_db = pidash_db::pool::sea_orm_connection(pool);
                    let models = modules::Entity::find()
                        .select_only()
                        .columns([modules::Column::Id, modules::Column::Name])
                        .filter(modules::Column::Id.is_in(parsed))
                        .filter(modules::Column::DeletedAt.is_null())
                        .into_model::<ModuleRow>()
                        .all(&modules_db)
                        .await
                        .map_err(|error| {
                            DispatchError::Log(format!("module snapshot: {}", into_sqlx(error)))
                        })?;
                    for model in models {
                        modules.insert(model.id.to_string(), model.name);
                    }
                }
            }
            let frame_exists = if frame.issue_id.is_empty() {
                // `filter(pk=None)` is empty, never an error.
                false
            } else {
                fetch_existing_issues(pool, std::slice::from_ref(&frame.issue_id))
                    .await?
                    .contains(&frame.issue_id)
            };
            let (row, touched) =
                plan_create_module_issue(requested, frame, &|id| modules.get(id).cloned(), &|_| {
                    frame_exists
                })
                .map_err(fail)?;
            push(&mut out, row)?;
            out.touched_issues.extend(touched);
        }
        "module.activity.deleted" => {
            let frame_exists = if frame.issue_id.is_empty() {
                false
            } else {
                fetch_existing_issues(pool, std::slice::from_ref(&frame.issue_id))
                    .await?
                    .contains(&frame.issue_id)
            };
            let (row, touched) =
                plan_delete_module_issue(requested, current, frame, &|_| frame_exists)
                    .map_err(fail)?;
            push(&mut out, row)?;
            out.touched_issues.extend(touched);
        }
        "link.activity.created" => {
            push(&mut out, plan_create_link(requested, frame).map_err(fail)?)?
        }
        "link.activity.updated" => {
            push_opt(
                &mut out,
                plan_update_link(requested, current, frame).map_err(fail)?,
            )?;
        }
        "link.activity.deleted" => push(&mut out, plan_delete_link(current, frame).map_err(fail)?)?,
        "attachment.activity.created" => push(
            &mut out,
            plan_create_attachment(requested, current, frame).map_err(fail)?,
        )?,
        "attachment.activity.deleted" => push(&mut out, plan_delete_attachment(frame))?,
        "worklog.activity.created" => push(
            &mut out,
            plan_create_worklog(requested, frame).map_err(fail)?,
        )?,
        "worklog.activity.updated" => push(
            &mut out,
            plan_update_worklog(requested, current, frame).map_err(fail)?,
        )?,
        "worklog.activity.deleted" => push(&mut out, plan_delete_worklog(frame))?,
        "issue_reaction.activity.created" => {
            let reaction = reaction_string(parsed_opt(requested).as_ref())?;
            let lookup = match (&reaction, &call.actor_id) {
                (Some(reaction), Some(actor)) => {
                    let db = pidash_db::pool::sea_orm_connection(pool);
                    issue_reaction_query(
                        reaction,
                        project_uuid,
                        lookup_uuid(actor, "reaction lookup")?,
                    )
                    .into_tuple::<Uuid>()
                    .one(&db)
                    .await
                    .map_err(|error| {
                        DispatchError::Log(format!("reaction lookup: {}", into_sqlx(error)))
                    })?
                    .map(|id| id.to_string())
                }
                _ => None,
            };
            push_opt(
                &mut out,
                plan_create_issue_reaction(requested, frame, lookup).map_err(fail)?,
            )?;
        }
        "issue_reaction.activity.deleted" => {
            push_opt(
                &mut out,
                plan_delete_issue_reaction(current, frame).map_err(fail)?,
            )?;
        }
        "comment_reaction.activity.created" => {
            let reaction = reaction_string(parsed_opt(requested).as_ref())?;
            let lookup = match (&reaction, &call.actor_id) {
                (Some(reaction), Some(actor)) => {
                    let db = pidash_db::pool::sea_orm_connection(pool);
                    comment_reaction_query(
                        reaction,
                        project_uuid,
                        lookup_uuid(actor, "reaction lookup")?,
                    )
                    .into_model::<CommentReactionRow>()
                    .one(&db)
                    .await
                    .map_err(|error| {
                        DispatchError::Log(format!("reaction lookup: {}", into_sqlx(error)))
                    })?
                    .map(|row| (row.id.to_string(), row.comment_id.to_string()))
                }
                _ => None,
            };
            // `IssueComment.objects.get(pk=comment_id, project_id=...)`.
            let comment = match &lookup {
                Some((_, comment_id)) => {
                    let db = pidash_db::pool::sea_orm_connection(pool);
                    comment_issue_query(lookup_uuid(comment_id, "comment lookup")?, project_uuid)
                        .into_tuple::<Uuid>()
                        .one(&db)
                        .await
                        .map_err(|error| {
                            DispatchError::Log(format!("comment lookup: {}", into_sqlx(error)))
                        })?
                        .map(|issue_id| CommentRef {
                            issue_id: issue_id.to_string(),
                        })
                }
                None => None,
            };
            // A lookup hit with a missing comment raises in Python
            // (`DoesNotExist`); the planner takes the pair apart, so an
            // inconsistent hit surfaces as a miss on the comment side.
            let (lookup, comment) = match (lookup, comment) {
                (Some(lookup), Some(comment)) => (Some(lookup), Some(comment)),
                (Some(_), None) => {
                    return Err(DispatchError::Log(
                        "comment lookup: comment row missing".to_owned(),
                    ));
                }
                _ => (None, None),
            };
            push_opt(
                &mut out,
                plan_create_comment_reaction(requested, frame, lookup, comment).map_err(fail)?,
            )?;
        }
        "comment_reaction.activity.deleted" => {
            let current_value = parsed_opt(current);
            let resolved = match scalar_id(current_value.as_ref(), "comment_id") {
                Some(comment_id) => {
                    let db = pidash_db::pool::sea_orm_connection(pool);
                    comment_issue_query(lookup_uuid(&comment_id, "comment lookup")?, project_uuid)
                        .into_tuple::<Uuid>()
                        .one(&db)
                        .await
                        .map_err(|error| {
                            DispatchError::Log(format!("comment lookup: {}", into_sqlx(error)))
                        })?
                        .map(|issue_id| issue_id.to_string())
                }
                None => None,
            };
            push_opt(
                &mut out,
                plan_delete_comment_reaction(current, frame, resolved).map_err(fail)?,
            )?;
        }
        "issue_vote.activity.created" => {
            push_opt(&mut out, plan_create_vote(requested, frame).map_err(fail)?)?;
        }
        "issue_vote.activity.deleted" => {
            push_opt(&mut out, plan_delete_vote(current, frame).map_err(fail)?)?;
        }
        "issue_relation.activity.created" => {
            let requested_value = parsed_opt(requested);
            let mut related_ids = HashSet::new();
            if let Some(Value::Array(items)) =
                requested_value.as_ref().and_then(|v| v.get("issues"))
            {
                for item in items {
                    if let Value::String(id) = item {
                        related_ids.insert(id.clone());
                    } else if !item.is_null() {
                        related_ids.insert(item.to_string());
                    }
                }
            }
            if !frame.issue_id.is_empty() {
                related_ids.insert(frame.issue_id.clone());
            }
            let refs = fetch_issue_refs(pool, &related_ids.into_iter().collect::<Vec<_>>()).await?;
            let rows = plan_create_relation(requested, current, frame, &|id| {
                refs.get(id).cloned().ok_or_else(|| BuildError::RowMissing {
                    table: "issues",
                    pk: id.to_owned(),
                })
            })
            .map_err(fail)?;
            for row in rows {
                push(&mut out, row)?;
            }
        }
        "issue_relation.activity.deleted" => {
            let requested_value = parsed_opt(requested);
            let mut related_ids = HashSet::new();
            if let Some(id) = scalar_id(requested_value.as_ref(), "related_issue") {
                related_ids.insert(id);
            }
            if !frame.issue_id.is_empty() {
                related_ids.insert(frame.issue_id.clone());
            }
            let refs = fetch_issue_refs(pool, &related_ids.into_iter().collect::<Vec<_>>()).await?;
            let rows = plan_delete_relation(requested, current, frame, &|id| {
                refs.get(id).cloned().ok_or_else(|| BuildError::RowMissing {
                    table: "issues",
                    pk: id.to_owned(),
                })
            })
            .map_err(fail)?;
            for row in rows {
                push(&mut out, row)?;
            }
        }
        "issue_draft.activity.created" => push(&mut out, plan_create_draft(frame))?,
        "issue_draft.activity.updated" => {
            push(
                &mut out,
                plan_update_draft(requested, current, frame).map_err(fail)?,
            )?;
        }
        "issue_draft.activity.deleted" => push(&mut out, plan_delete_draft(frame))?,
        "intake.activity.created" => {
            push_opt(
                &mut out,
                plan_create_intake(requested, current, frame).map_err(fail)?,
            )?;
        }
        // Unknown types append nothing (`func is None`, `:1661-1662`); the
        // empty batch still flows through `bulk_create` below.
        _ => {}
    }
    // `issue_id=None` stores NULL columns (`delete_draft` excepted, which
    // already carries none). The `""` sentinel below can never be a real
    // id, so only frame-derived rows match.
    if call.issue_id.is_none() {
        for bound in &mut out.rows {
            if bound.row.issue_id.as_deref() == Some("") {
                bound.row.issue_id = None;
            }
        }
    }
    Ok(out)
}

/// Bulk-fetch relation labels (`identifier-sequence_id`).
async fn fetch_issue_refs(
    pool: &PgPool,
    ids: &[String],
) -> Result<HashMap<String, IssueRef>, DispatchError> {
    let mut map = HashMap::new();
    if ids.is_empty() {
        return Ok(map);
    }
    let parsed = parse_snapshot_ids(ids);
    let db = pidash_db::pool::sea_orm_connection(pool);
    let stmt = issue_ref_statement(&parsed);
    let rows = db
        .query_all(DbBackend::Postgres.build(&stmt))
        .await
        .map_err(|error| DispatchError::Log(format!("relation snapshot: {}", into_sqlx(error))))?;
    for row in &rows {
        let id: Uuid = row.try_get("", "id").map_err(|error| {
            DispatchError::Log(format!("relation snapshot: {}", into_sqlx(error)))
        })?;
        let key = id.to_string();
        let identifier: String = row.try_get("", "identifier").map_err(|error| {
            DispatchError::Log(format!("relation snapshot: {}", into_sqlx(error)))
        })?;
        let sequence_id: i64 = row.try_get("", "sequence_id").map_err(|error| {
            DispatchError::Log(format!("relation snapshot: {}", into_sqlx(error)))
        })?;
        map.insert(
            key,
            IssueRef {
                identifier,
                sequence_id,
            },
        );
    }
    Ok(map)
}

/// `IssueActivity.objects.bulk_create(issue_activities)` (`:1675`): plain
/// multi-row `INSERT`, no batch size, no conflict handling — one statement
/// or none (Django skips the query for an empty list). Returns the rows
/// for the notification payload, in batch order. The `id`/`created_at`/
/// `updated_at` values are Python-side (`uuid4`/`auto_now*`), so the
/// `RETURNING` columns are exactly what was inserted; building them from
/// the inputs keeps the one-statement shape without a second round-trip.
/// A cast error aborts the whole batch with nothing written, exactly like
/// the Django single-query insert.
async fn bulk_insert(
    pool: &PgPool,
    rows: Vec<BoundRow>,
    now: DateTime<Utc>,
) -> Result<Vec<StoredActivityRow>, DispatchError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let mut ids: Vec<Uuid> = Vec::with_capacity(rows.len());
    let mut models: Vec<issue_activities::ActiveModel> = Vec::with_capacity(rows.len());
    for bound in &rows {
        let id = Uuid::new_v4();
        ids.push(id);
        models.push(issue_activities::ActiveModel {
            id: Set(id),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            created_by_id: Set(None),
            updated_by_id: Set(None),
            deleted_at: Set(None),
            project_id: Set(bound.project_id),
            workspace_id: Set(bound.workspace_id),
            issue_id: Set(bound.issue_id),
            verb: Set(bound.row.verb.clone()),
            field: Set(bound.row.field.clone()),
            old_value: Set(bound.row.old_value.clone()),
            new_value: Set(bound.row.new_value.clone()),
            comment: Set(bound.row.comment.clone()),
            attachments: Set(Vec::new()),
            issue_comment_id: Set(bound.issue_comment_id),
            actor_id: Set(Some(bound.actor_id)),
            old_identifier: Set(bound.old_identifier),
            new_identifier: Set(bound.new_identifier),
            epoch: Set(Some(bound.row.epoch)),
        });
    }
    let db = pidash_db::pool::sea_orm_connection(pool);
    issue_activities::Entity::insert_many(models)
        .exec(&db)
        .await
        .map_err(|error| DispatchError::Log(format!("bulk_create: {}", into_sqlx(error))))?;
    Ok(ids
        .into_iter()
        .zip(rows)
        .map(|(id, bound)| StoredActivityRow::new(id, now, now, bound.row, bound.issue_comment_id))
        .collect())
}

/// The create-issue immediate row (`IssueActivity.objects.create` +
/// override save, `:568-579`) as one `INSERT`: `created_at` and `actor_id`
/// come from the `Issue` row, `updated_at` is now. Not `RETURNING` — the
/// row never enters the notification payload.
async fn insert_immediate(
    pool: &PgPool,
    row: &ActivityRow,
    created_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<(), DispatchError> {
    let bound = bind_row(row.clone())?;
    let db = pidash_db::pool::sea_orm_connection(pool);
    issue_activities::Entity::insert_many(vec![issue_activities::ActiveModel {
        id: Set(Uuid::new_v4()),
        created_at: Set(created_at.into()),
        updated_at: Set(now.into()),
        created_by_id: Set(None),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        project_id: Set(bound.project_id),
        workspace_id: Set(bound.workspace_id),
        issue_id: Set(bound.issue_id),
        verb: Set(bound.row.verb.clone()),
        field: Set(bound.row.field.clone()),
        old_value: Set(bound.row.old_value.clone()),
        new_value: Set(bound.row.new_value.clone()),
        comment: Set(bound.row.comment.clone()),
        attachments: Set(Vec::new()),
        issue_comment_id: Set(bound.issue_comment_id),
        actor_id: Set(Some(bound.actor_id)),
        old_identifier: Set(bound.old_identifier),
        new_identifier: Set(bound.new_identifier),
        epoch: Set(Some(bound.row.epoch)),
    }])
    .exec(&db)
    .await
    .map_err(|error| DispatchError::Log(format!("create issue insert: {}", into_sqlx(error))))?;
    Ok(())
}

/// Persist everything `collect_rows` built, in Python effect order: the
/// immediate row, the description touch, the record touches, the
/// subscriber batch (`ignore_conflicts`), then the activity batch.
async fn persist(
    pool: &PgPool,
    collected: Collected,
    now: DateTime<Utc>,
) -> Result<Vec<StoredActivityRow>, DispatchError> {
    if let Some((row, created_at)) = &collected.immediate {
        insert_immediate(pool, row, *created_at, now).await?;
    }
    if let Some(activity_id) = collected.touch_activity {
        let db = pidash_db::pool::sea_orm_connection(pool);
        issue_activities::Entity::update_many()
            .col_expr(
                issue_activities::Column::CreatedAt,
                Expr::current_timestamp().into(),
            )
            .filter(issue_activities::Column::Id.eq(activity_id))
            .exec(&db)
            .await
            .map_err(|error| {
                DispatchError::Log(format!("description touch: {}", into_sqlx(error)))
            })?;
    }
    if !collected.touched_issues.is_empty() {
        let touched: Vec<Uuid> = collected
            .touched_issues
            .iter()
            .map(|id| lookup_uuid(id, "record touches"))
            .collect::<Result<_, _>>()?;
        // Per-record `updated_at` touches from the cycle/module builders
        // (`activity_issue::CycleOutcome::touched_issue_ids`): one statement
        // over the soft-filtered set — same final rows as the per-record
        // guarded saves.
        touch_issues(&pidash_db::pool::sea_orm_connection(pool), &touched)
            .await
            .map_err(|error| DispatchError::Log(format!("record touches: {error}")))?;
    }
    for subscriber in &collected.subscribers {
        // One statement per subscriber, any conflict ignored. Django's
        // `bulk_create(ignore_conflicts=True)` fills `id` (uuid4 default)
        // and `created_at`/`updated_at` (`auto_now_add`/`auto_now` in
        // `db/mixins.py`) client-side, so set the same stamps here.
        let db = pidash_db::pool::sea_orm_connection(pool);
        issue_subscribers::Entity::insert_many(vec![issue_subscribers::ActiveModel {
            id: Set(Uuid::new_v4()),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            deleted_at: Set(None),
            subscriber_id: Set(lookup_uuid(&subscriber.subscriber_id, "subscribers")?),
            issue_id: Set(lookup_uuid(&subscriber.issue_id, "subscribers")?),
            workspace_id: Set(lookup_uuid(&subscriber.workspace_id, "subscribers")?),
            project_id: Set(lookup_uuid(&subscriber.project_id, "subscribers")?),
            created_by_id: Set(Some(lookup_uuid(&subscriber.created_by_id, "subscribers")?)),
            updated_by_id: Set(Some(lookup_uuid(&subscriber.updated_by_id, "subscribers")?)),
        }])
        .on_conflict(
            sea_orm::sea_query::OnConflict::new()
                .do_nothing()
                .to_owned(),
        )
        .exec(&db)
        .await
        .map_err(|error| DispatchError::Log(format!("subscribers: {}", into_sqlx(error))))?;
    }
    bulk_insert(pool, collected.rows, now).await
}

/// The `issue_activity` body (`:1606-1695`) against live seams. Every
/// failure maps to the broad-except (`log_exception(e); return`); the
/// caller acks unconditionally.
pub async fn run_activity<R: RedisOrigin>(
    pool: &PgPool,
    redis: &R,
    call: &IssueActivityCall,
) -> Result<(), DispatchError> {
    // `:1609` — before any DB access. A missing id stringifies to
    // `"None"`, which is invalid, exactly like `str(None)` in Python.
    let project_id = call.project_id.as_deref().unwrap_or("None");
    if !is_valid_uuid(project_id) {
        return Err(DispatchError::Quiet);
    }
    // `:1612-1613` — raises (miss) into the broad-except.
    let project_uuid = Uuid::parse_str(project_id).map_err(|_| DispatchError::Quiet)?;
    let run_db = pidash_db::pool::sea_orm_connection(pool);
    let workspace_id: Uuid = project_workspace_query(project_uuid)
        .into_tuple::<Uuid>()
        .one(&run_db)
        .await
        .map_err(|error| DispatchError::Log(format!("project lookup: {}", into_sqlx(error))))?
        .ok_or_else(|| {
            DispatchError::Log(format!("project lookup: row missing for {project_id}"))
        })?;
    // Missing ids coerce to `""`. A `None` issue stores NULL columns via
    // the post-pass in `collect_rows`; a `None` actor fails at the first
    // uuid-typed bind while Python would store NULL — unreachable in
    // contract (every caller passes `str(request.user.id)`).
    let frame = TrackFrame {
        issue_id: call.issue_id.clone().unwrap_or_default(),
        project_id: project_id.to_owned(),
        workspace_id: workspace_id.to_string(),
        actor_id: call.actor_id.clone().unwrap_or_default(),
        epoch: call.epoch,
    };
    // `:1615-1626` — skipped entirely when `issue_id is None`.
    if let Some(issue_id) = &call.issue_id {
        if let Some(origin) = &call.origin {
            if !origin.is_empty() {
                redis
                    .set_ex(issue_id, origin, ORIGIN_TTL_SECS)
                    .map_err(DispatchError::Log)?;
            }
        }
        let touch_id = lookup_uuid(issue_id, "issue touch lookup")?;
        let found: bool = issue_exists_query(touch_id)
            .into_tuple::<Uuid>()
            .one(&run_db)
            .await
            .map_err(|error| {
                DispatchError::Log(format!("issue touch lookup: {}", into_sqlx(error)))
            })?
            .is_some();
        if found {
            // Own try/except-pass: touch failures never abort the build.
            let _ = touch_issue(&pidash_db::pool::sea_orm_connection(pool), touch_id).await;
        }
    }
    let collected = collect_rows(pool, &frame, call, project_id).await?;
    let now = Utc::now();
    let stored = persist(pool, collected, now).await?;
    // `:1677-1690` — independent of the activity type; an empty batch
    // serializes to `"[]"` without touching the issue row.
    if call.notification {
        let serialized = if stored.is_empty() {
            "[]".to_owned()
        } else {
            let mut flat_ids: Vec<String> = stored
                .iter()
                .filter_map(|row| row.row.issue_id.clone())
                .collect();
            flat_ids.sort();
            flat_ids.dedup();
            let mut flats: HashMap<String, Map<String, Value>> = HashMap::new();
            if !flat_ids.is_empty() {
                let parsed = parse_snapshot_ids(&flat_ids);
                let models = issues::Entity::find()
                    .select_only()
                    .columns([
                        issues::Column::Id,
                        issues::Column::Name,
                        issues::Column::DescriptionJson,
                        issues::Column::DescriptionHtml,
                        issues::Column::Priority,
                        issues::Column::ComplexityScore,
                        issues::Column::StartDate,
                        issues::Column::TargetDate,
                        issues::Column::SequenceId,
                        issues::Column::SortOrder,
                        issues::Column::IsDraft,
                    ])
                    .filter(issues::Column::Id.is_in(parsed))
                    .into_model::<IssueFlatRow>()
                    .all(&run_db)
                    .await
                    .map_err(|error| {
                        DispatchError::Log(format!("issue_detail: {}", into_sqlx(error)))
                    })?;
                for model in models {
                    flats.insert(model.id.to_string(), issue_flat_json(&model));
                }
            }
            // Rows whose issue is gone (or null) serialize with a null
            // `issue_detail`, like the serializer over a missing FK.
            let items: Vec<Value> = stored
                .iter()
                .map(|stored| {
                    let flat = stored.row.issue_id.as_ref().and_then(|id| flats.get(id));
                    match flat {
                        Some(flat) => Value::Object(activity_json(stored, flat)),
                        None => {
                            let mut map = activity_json(stored, &Map::new());
                            map.insert("issue_detail".to_owned(), Value::Null);
                            Value::Object(map)
                        }
                    }
                })
                .collect();
            django_dumps(&Value::Array(items))
        };
        enqueue(pool, &build_notifications_job(call, &serialized))
            .await
            .map_err(|error| DispatchError::Log(format!("notifications enqueue: {error}")))?;
    }
    Ok(())
}

/// Register the `issue_activity` handler: ownership of
/// `pi_dash.bgtasks.issue_activities_task.issue_activity` flips to Rust
/// the moment this name registers; unregistered names still forward to
/// Python.
pub fn register_activity_task<R>(registry: &mut Registry, pools: Pools, redis: R)
where
    R: RedisOrigin + Send + Sync + 'static,
{
    let redis = Arc::new(redis);
    let handler: Handler = Arc::new(move |job| {
        let pools = pools.clone();
        let redis = redis.clone();
        Box::pin(async move {
            if !job.args.is_array() || !job.kwargs.is_object() {
                return Err(format!(
                    "{ISSUE_ACTIVITY_TASK}: unexpected job payload shape"
                ));
            }
            let call = bind_issue_activity(&job.args, &job.kwargs).map_err(|error| {
                // A payload Python could not even bind fails the message
                // without running the body; ack it loudly.
                format!("{ISSUE_ACTIVITY_TASK}: invalid payload: {error}")
            })?;
            match run_activity(pools.primary(), &*redis, &call).await {
                Ok(()) => Ok(Verdict::Ack),
                Err(DispatchError::Quiet) => Ok(Verdict::Ack),
                Err(DispatchError::Log(error)) => {
                    tracing::error!(task = ISSUE_ACTIVITY_TASK, error = %error, "task failed");
                    Ok(Verdict::Ack)
                }
            }
        })
    });
    registry.register(ISSUE_ACTIVITY_TASK, handler);
}

/// True for the D-08 activity dispatcher task name.
pub fn is_activity_task(task: &str) -> bool {
    task == ISSUE_ACTIVITY_TASK
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Same committed evidence as the services layer:
    /// `rust-api/fixtures/tasks_webhooks/fx-act-03-dispatcher.json`.
    static DISPATCH_FIXTURE: &str =
        include_str!("../../../../fixtures/tasks_webhooks/fx-act-03-dispatcher.json");
    /// The mapper golden lives with the builders:
    /// `rust-api/fixtures/tasks_webhooks/fx-act-02-activity-builders.json`.
    static BUILDERS_FIXTURE: &str =
        include_str!("../../../../fixtures/tasks_webhooks/fx-act-02-activity-builders.json");

    fn fixture() -> Value {
        serde_json::from_str(DISPATCH_FIXTURE).expect("dispatcher fixture parses")
    }

    fn call() -> IssueActivityCall {
        IssueActivityCall {
            activity_type: "link.activity.created".to_owned(),
            requested_data: Some(r#"{"url":"https://example.com/x"}"#.to_owned()),
            current_instance: None,
            issue_id: Some("11111111-1111-4111-8111-111111111111".to_owned()),
            actor_id: Some("22222222-2222-4222-8222-222222222222".to_owned()),
            project_id: Some("33333333-3333-4333-8333-333333333333".to_owned()),
            epoch: 1750000000.0,
            subscriber: true,
            notification: false,
            origin: None,
        }
    }

    // FX-ACT-03 · the Celery task name and the 30-type mapper.
    #[test]
    fn task_name_and_mapper_keys() {
        assert_eq!(
            ISSUE_ACTIVITY_TASK,
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
        assert_eq!(
            NOTIFICATIONS_TASK,
            "pi_dash.bgtasks.notification_task.notifications"
        );
        assert_eq!(ACTIVITY_TYPES.len(), 30);
        assert!(is_known_activity_type("link.activity.created"));
        assert!(is_known_activity_type("intake.activity.created"));
        assert!(!is_known_activity_type("cycle.activity.updated"));
        assert!(!is_known_activity_type("nope"));
        assert!(is_activity_task(ISSUE_ACTIVITY_TASK));
    }

    // FX-ACT-02/03 · mapper keys match the fixture's golden list.
    #[test]
    fn mapper_matches_fixture_keys() {
        let fx: Value = serde_json::from_str(BUILDERS_FIXTURE).expect("builders fixture parses");
        let expected: Vec<String> = fx["mapper_type_keys"]
            .as_array()
            .expect("fixture mapper keys")
            .iter()
            .map(|key| key.as_str().expect("string key").to_owned())
            .collect();
        assert_eq!(expected.len(), ACTIVITY_TYPES.len());
        for key in expected {
            assert!(is_known_activity_type(&key), "missing mapper key {key}");
        }
    }

    // FX-ACT-03 · binding follows the fixture's signature order, including
    // the dead `intake` parameter and the `subscriber`/`notification`
    // defaults.
    #[test]
    fn signature_matches_fixture() {
        let fx = fixture();
        let signature: Vec<String> = fx["signature"]
            .as_array()
            .expect("fixture signature")
            .iter()
            .map(|name| name.as_str().expect("string name").to_owned())
            .collect();
        assert_eq!(
            signature,
            [
                "type",
                "requested_data",
                "current_instance",
                "issue_id",
                "actor_id",
                "project_id",
                "epoch",
                "subscriber=True",
                "notification=False",
                "origin=None",
                "intake=None (accepted but NEVER read — port the dead parameter)",
            ]
        );
        assert_eq!(fx["task_options"], json!("shared_task, NOT bound"));
    }

    // FX-ACT-03 · uuid guard: v4 passes, everything else fails.
    #[test]
    fn uuid_guard_versions() {
        assert!(is_valid_uuid("33333333-3333-4333-8333-333333333333"));
        assert!(!is_valid_uuid("6ba7b810-9dad-11d1-80b4-00c04fd430c8"));
        assert!(!is_valid_uuid("not-a-uuid"));
        assert!(!is_valid_uuid("None"));
        assert!(!is_valid_uuid(""));
    }

    // FX-ACT-03 · positional + keyword binding with defaults.
    #[test]
    fn bind_positional_and_kwargs() {
        let args = json!([
            "link.activity.created",
            "{\"url\":\"x\"}",
            null,
            "11111111-1111-4111-8111-111111111111",
            "22222222-2222-4222-8222-222222222222",
            "33333333-3333-4333-8333-333333333333",
            1750000000.0,
        ]);
        let call = bind_issue_activity(&args, &json!({})).unwrap();
        assert_eq!(call.activity_type, "link.activity.created");
        assert_eq!(call.requested_data.as_deref(), Some("{\"url\":\"x\"}"));
        assert_eq!(call.current_instance, None);
        assert!(call.subscriber);
        assert!(!call.notification);
        assert_eq!(call.origin, None);

        let kwargs = json!({
            "type": "intake.activity.created",
            "requested_data": "{\"status\":1}",
            "current_instance": "{\"status\":-1}",
            "issue_id": "11111111-1111-4111-8111-111111111111",
            "actor_id": "22222222-2222-4222-8222-222222222222",
            "project_id": "33333333-3333-4333-8333-333333333333",
            "epoch": 1750000000.0,
            "subscriber": false,
            "notification": true,
            "origin": "app",
            "intake": {"ignored": true},
        });
        let call = bind_issue_activity(&json!([]), &kwargs).unwrap();
        assert!(!call.subscriber);
        assert!(call.notification);
        assert_eq!(call.origin.as_deref(), Some("app"));
    }

    // FX-ACT-03 · binding rejects duplicates, unknown keywords, bad types.
    #[test]
    fn bind_rejects_bad_payloads() {
        assert!(bind_issue_activity(&json!(["t"]), &json!({"type": "u"})).is_err());
        assert!(bind_issue_activity(&json!([]), &json!({"bogus": 1})).is_err());
        assert!(bind_issue_activity(&json!([1]), &json!({})).is_err());
        assert!(bind_issue_activity(&json!(["t", 5]), &json!({})).is_err());
        assert!(
            bind_issue_activity(&json!(["t", null, null, null, null, null]), &json!({})).is_err()
        );
    }

    // NEWPIDASH-134 · the app destroy enqueues current_instance={} (a dict,
    // base.py:747); the Python delete handler ignores it, so the job must
    // bind and write the deleted row. Objects dump to JSON text.
    #[test]
    fn bind_accepts_deleted_empty_object() {
        let kwargs = json!({
            "type": "issue.activity.deleted",
            "requested_data": "{\"issue_id\": \"11111111-1111-4111-8111-111111111111\"}",
            "current_instance": {},
            "issue_id": "11111111-1111-4111-8111-111111111111",
            "actor_id": "22222222-2222-4222-8222-222222222222",
            "project_id": "33333333-3333-4333-8333-333333333333",
            "epoch": 1750000000.0,
            "subscriber": false,
            "notification": true,
            "origin": "https://app.example",
        });
        let call = bind_issue_activity(&json!([]), &kwargs).expect("deleted binds");
        assert_eq!(call.activity_type, "issue.activity.deleted");
        assert_eq!(call.current_instance.as_deref(), Some("{}"));
        // Scalars still reject (Python json.loads would fail on them too).
        let bad = json!({
            "type": "issue.activity.deleted",
            "requested_data": "{\"issue_id\": \"x\"}",
            "current_instance": 5,
            "epoch": 1750000000.0,
        });
        assert!(bind_issue_activity(&json!([]), &bad).is_err());
    }

    // FX-ACT-03 · redis origin TTL.
    #[test]
    fn origin_ttl_is_600() {
        assert_eq!(ORIGIN_TTL_SECS, 600);
    }

    // FX-ACT-03 · lookups skip soft-deleted rows (default manager).
    #[test]
    fn lookups_filter_soft_deletes() {
        use sea_orm::{DbBackend, QueryTrait};
        let id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("valid uuid");
        let project = Uuid::parse_str("22222222-2222-4222-8222-222222222222").expect("valid uuid");
        let stmts = [
            project_workspace_query(id)
                .build(DbBackend::Postgres)
                .to_string(),
            issue_exists_query(id)
                .build(DbBackend::Postgres)
                .to_string(),
            issue_reaction_query("up", project, id)
                .build(DbBackend::Postgres)
                .to_string(),
            comment_reaction_query("up", project, id)
                .build(DbBackend::Postgres)
                .to_string(),
            comment_issue_query(id, project)
                .build(DbBackend::Postgres)
                .to_string(),
            issue_ref_statement(&[id]).to_string(sea_orm::sea_query::PostgresQueryBuilder),
            created_issue_query(id)
                .build(DbBackend::Postgres)
                .to_string(),
        ];
        for sql in &stmts {
            assert!(
                sql.contains("\"deleted_at\" IS NULL"),
                "missing soft-delete scope: {sql}"
            );
        }
        // The state lookup also excludes triage-group rows (PIDASHCONV-1278).
        let state_sql = states::Entity::find()
            .select_only()
            .columns([states::Column::Id, states::Column::Name])
            .filter(states::Column::Id.is_in([id]))
            .filter(states::Column::ProjectId.eq(project))
            .filter(states::Column::DeletedAt.is_null())
            .filter(states::Column::Group.ne("triage"))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            state_sql.contains("\"group\" <> 'triage'"),
            "missing triage exclusion: {state_sql}"
        );
        // The custom user manager has no soft-delete scope.
        let user_sql = users::Entity::find()
            .select_only()
            .columns([users::Column::Id, users::Column::DisplayName])
            .filter(users::Column::Id.is_in([id]))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            !user_sql.contains("deleted_at"),
            "user lookup must not soften: {user_sql}"
        );
    }

    // PIDASHCONV-1278 · the default `StateManager` excludes triage-group
    // states (`db/models/state.py:79-83`); the lookup renders the same.
    #[test]
    fn state_lookup_excludes_triage() {
        use sea_orm::{DbBackend, QueryTrait};
        let id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("valid uuid");
        let project = Uuid::parse_str("22222222-2222-4222-8222-222222222222").expect("valid uuid");
        let state_sql = states::Entity::find()
            .select_only()
            .columns([states::Column::Id, states::Column::Name])
            .filter(states::Column::Id.is_in([id]))
            .filter(states::Column::ProjectId.eq(project))
            .filter(states::Column::DeletedAt.is_null())
            .filter(states::Column::Group.ne("triage"))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            state_sql.contains("\"group\" <> 'triage'"),
            "missing triage exclusion: {state_sql}"
        );
    }

    // Snapshot builders pin their joins, soft-delete scope and the
    // latest-activity ordering (`ORDER BY created_at DESC LIMIT 1`).
    #[test]
    fn estimate_and_latest_statements_pin_shape() {
        use sea_orm::{DbBackend, QueryTrait};
        let id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("valid uuid");
        let estimate_sql =
            estimate_points_statement(&[id]).to_string(sea_orm::sea_query::PostgresQueryBuilder);
        assert!(
            estimate_sql.contains("\"estimate_points\""),
            "{estimate_sql}"
        );
        assert!(estimate_sql.contains("\"estimates\""), "{estimate_sql}");
        assert!(estimate_sql.contains("\"value\""), "{estimate_sql}");
        assert!(estimate_sql.contains("\"estimate_type\""), "{estimate_sql}");
        assert!(
            estimate_sql.contains("\"deleted_at\" IS NULL"),
            "{estimate_sql}"
        );
        let latest_sql = latest_activity_query(id)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(latest_sql.contains("\"issue_activities\""), "{latest_sql}");
        assert!(latest_sql.contains("\"issue_id\""), "{latest_sql}");
        assert!(latest_sql.contains("\"created_at\" DESC"), "{latest_sql}");
        assert!(latest_sql.contains("LIMIT 1"), "{latest_sql}");
        // No soft-delete scope on the latest-activity lookup (the old
        // FIND_LATEST_ACTIVITY_SQL had none): a soft-deleted row is still
        // the latest activity.
        assert!(!latest_sql.contains("deleted_at"), "{latest_sql}");
    }

    // The inline bulk-snapshot shapes (fetch_cycles, fetch_existing_issues,
    // the IssueFlatRow fetch and the create-issue assignee fetch) pin
    // their columns and soft-delete scope the same way the state/user
    // shapes above do (PIDASHCONV-1278 pattern kept in tests).
    #[test]
    fn snapshot_shapes_pin_soft_scope() {
        use sea_orm::{DbBackend, QueryTrait};
        let id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("valid uuid");
        let cycle_sql = cycles::Entity::find()
            .select_only()
            .columns([cycles::Column::Id, cycles::Column::Name])
            .filter(cycles::Column::Id.is_in([id]))
            .filter(cycles::Column::DeletedAt.is_null())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(cycle_sql.contains("\"cycles\""), "{cycle_sql}");
        assert!(cycle_sql.contains("\"deleted_at\" IS NULL"), "{cycle_sql}");
        let module_sql = modules::Entity::find()
            .select_only()
            .columns([modules::Column::Id, modules::Column::Name])
            .filter(modules::Column::Id.is_in([id]))
            .filter(modules::Column::DeletedAt.is_null())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(module_sql.contains("\"modules\""), "{module_sql}");
        assert!(
            module_sql.contains("\"deleted_at\" IS NULL"),
            "{module_sql}"
        );
        let existing_sql = issues::Entity::find()
            .select_only()
            .column(issues::Column::Id)
            .filter(issues::Column::Id.is_in([id]))
            .filter(issues::Column::DeletedAt.is_null())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(existing_sql.contains("\"issues\""), "{existing_sql}");
        assert!(
            existing_sql.contains("\"deleted_at\" IS NULL"),
            "{existing_sql}"
        );
        // The IssueFlatSerializer projection has NO soft-delete scope
        // (the serializer follows the FK onto whatever row is there).
        let flat_sql = issues::Entity::find()
            .select_only()
            .columns([
                issues::Column::Id,
                issues::Column::Name,
                issues::Column::DescriptionJson,
                issues::Column::DescriptionHtml,
                issues::Column::Priority,
                issues::Column::ComplexityScore,
                issues::Column::StartDate,
                issues::Column::TargetDate,
                issues::Column::SequenceId,
                issues::Column::SortOrder,
                issues::Column::IsDraft,
            ])
            .filter(issues::Column::Id.is_in([id]))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(flat_sql.contains("\"issues\""), "{flat_sql}");
        assert!(flat_sql.contains("\"description_json\""), "{flat_sql}");
        assert!(
            !flat_sql.contains("deleted_at"),
            "flat projection must not soften: {flat_sql}"
        );
        // The create-issue assignee fetch has no soft-delete scope either.
        let assignee_sql = users::Entity::find()
            .select_only()
            .columns([users::Column::Id, users::Column::DisplayName])
            .filter(users::Column::Id.is_in([id]))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            !assignee_sql.contains("deleted_at"),
            "assignee fetch must not soften: {assignee_sql}"
        );
    }

    // FX-ACT-03 · DRF datetime rendering.
    #[test]
    fn datetimes_render_drf_iso() {
        let micros = chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00.123456+00:00")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(drf_datetime(&micros), "2026-09-28T12:00:00.123456Z");
        let whole = chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(drf_datetime(&whole), "2026-09-28T12:00:00Z");
    }

    // FX-ACT-03 · json.dumps defaults: separators + ensure_ascii.
    #[test]
    fn dumps_uses_python_separators_and_ascii() {
        let value = json!({"b": [1, null, true], "a": "❤️"});
        assert_eq!(
            django_dumps(&value),
            "{\"b\": [1, null, true], \"a\": \"\\u2764\\ufe0f\"}"
        );
        // Astral code points become surrogate pairs, lowercase hex.
        assert_eq!(django_dumps(&json!("\u{1F44D}")), "\"\\ud83d\\udc4d\"");
        assert_eq!(django_dumps(&json!({"x": 1.5})), "{\"x\": 1.5}");
        assert_eq!(django_dumps(&json!([])), "[]");
        // Control escapes and Latin-1, byte-identical to CPython.
        assert_eq!(
            django_dumps(&json!({"n": "line\nbreak\ttab\"q\"\\\u{8}s", "u": "café \u{1F44D} \u{1}"})),
            "{\"n\": \"line\\nbreak\\ttab\\\"q\\\"\\\\\\bs\", \"u\": \"caf\\u00e9 \\ud83d\\udc4d \\u0001\"}"
        );
    }

    #[test]
    fn dumps_deep_value_survives_small_stack() {
        // The cycle add path echoes raw `issues` (a dict with deep values
        // passes validation on its keys): the walk must not recurse.
        let mut deep = json!(1);
        for _ in 0..9938 {
            deep = Value::Array(vec![deep]);
        }
        let text = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let text = django_dumps(&deep);
                crate::json_compact::drop_value_deep(deep);
                text
            })
            .expect("spawn")
            .join()
            .expect("no stack overflow");
        assert_eq!(text.len(), 9938 + 1 + 9938);
    }

    // FX-ACT-03 · serializer shape: flat columns + details.
    #[test]
    fn activity_json_shape() {
        let row = ActivityRow {
            issue_id: Some("11111111-1111-4111-8111-111111111111".to_owned()),
            actor_id: "22222222-2222-4222-8222-222222222222".to_owned(),
            verb: "created".to_owned(),
            old_value: None,
            new_value: Some("https://example.com/x".to_owned()),
            field: Some("link".to_owned()),
            project_id: "33333333-3333-4333-8333-333333333333".to_owned(),
            workspace_id: "44444444-4444-4444-8444-444444444444".to_owned(),
            comment: "created a link".to_owned(),
            old_identifier: None,
            new_identifier: Some("55555555-5555-4555-8555-555555555555".to_owned()),
            issue_comment_id: None,
            epoch: 1750000000.0,
        };
        let stored = StoredActivityRow::new(
            Uuid::nil(),
            chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00+00:00")
                .unwrap()
                .with_timezone(&Utc),
            chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00+00:00")
                .unwrap()
                .with_timezone(&Utc),
            row,
            None,
        );
        let flat = Map::new();
        let map = activity_json(&stored, &flat);
        // DRF names: FKs render as string pks, datetimes as DRF-ISO.
        assert_eq!(map["issue"], json!("11111111-1111-4111-8111-111111111111"));
        assert_eq!(map["actor"], json!("22222222-2222-4222-8222-222222222222"));
        assert_eq!(map["issue_comment"], Value::Null);
        assert_eq!(map["created_at"], json!("2026-09-28T12:00:00Z"));
        assert_eq!(map["attachments"], json!([]));
        assert_eq!(map["created_by"], Value::Null);
        assert_eq!(map["epoch"], json!(1750000000.0));
        assert!(map["issue_detail"].is_object());
        assert_eq!(map["actor_detail"], Value::Null);
        assert_eq!(map["source_data"], Value::Null);
    }

    // FX-ACT-03 · IssueFlat projection keys.
    #[test]
    fn issue_flat_keys() {
        let flat = IssueFlatRow {
            id: Uuid::nil(),
            name: "n".to_owned(),
            description_json: json!({}),
            description_html: "<p></p>".to_owned(),
            priority: Some("high".to_owned()),
            complexity_score: None,
            start_date: None,
            target_date: Some(chrono::NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()),
            sequence_id: 7,
            sort_order: 65535.0,
            is_draft: false,
        };
        let map = issue_flat_json(&flat);
        assert_eq!(
            map.keys().collect::<Vec<_>>(),
            [
                "id",
                "name",
                "description_json",
                "description_html",
                "priority",
                "complexity_score",
                "start_date",
                "target_date",
                "sequence_id",
                "sort_order",
                "is_draft",
            ]
            .iter()
            .collect::<Vec<_>>()
        );
        assert_eq!(map["target_date"], json!("2026-09-30"));
        assert_eq!(map["sort_order"], json!(65535.0));
    }

    // FX-ACT-03 · notifications kwargs: call order, passthroughs, raw payloads.
    #[test]
    fn notifications_kwargs_shape() {
        let mut call = call();
        call.notification = true;
        call.subscriber = false;
        call.origin = Some("app".to_owned());
        let job = build_notifications_job(&call, "[]");
        assert_eq!(job.task, NOTIFICATIONS_TASK);
        assert_eq!(job.args, Value::Array(Vec::new()));
        let kwargs = job.kwargs.as_object().expect("kwargs object");
        // Insertion order mirrors the Python call.
        let keys: Vec<&str> = kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "type",
                "issue_id",
                "actor_id",
                "project_id",
                "subscriber",
                "issue_activities_created",
                "requested_data",
                "current_instance",
            ]
        );
        assert_eq!(kwargs["subscriber"], json!(false));
        assert_eq!(kwargs["issue_activities_created"], json!("[]"));
        // Raw payloads pass through unparsed.
        assert_eq!(
            kwargs["requested_data"],
            json!("{\"url\":\"https://example.com/x\"}")
        );
        assert_eq!(kwargs["current_instance"], Value::Null);
        // `origin` is consumed by the dispatcher, never forwarded.
        assert!(!kwargs.contains_key("origin"));
    }

    // FX-ACT-03 · the registered name routes local.
    #[test]
    fn registered_name_routes_local() {
        use crate::worker::{route_for, Route};
        fn ack() -> Handler {
            Arc::new(|_: crate::queue::JobRow| {
                Box::pin(async { Ok(Verdict::Ack) })
                    as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
            })
        }
        let mut registry = Registry::new();
        registry.register(ISSUE_ACTIVITY_TASK, ack());
        assert_eq!(route_for(&registry, ISSUE_ACTIVITY_TASK), Route::Local);
        assert_eq!(
            route_for(&registry, "pi_dash.bgtasks.issue_activities_task.nope"),
            Route::Unknown
        );
    }

    /// `DATABASE_URL` pointing at a migrated database, else skip.
    async fn live_pool() -> Option<sqlx::PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return None;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect");
        let migrated: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public.issue_activities')::text")
                .fetch_one(&pool)
                .await
                .expect("probe");
        if migrated.is_none() {
            eprintln!("skipping live-db test: the database is not migrated");
            return None;
        }
        Some(pool)
    }

    /// `bulk_insert` is one multi-row `INSERT` and returns the inputs' own
    /// `id`/`created_at`/`updated_at` (the `RETURNING` columns were
    /// Python-side values). The temp table shadows the real one for the
    /// transaction; `attachments` renders as an empty `varchar[]`.
    #[tokio::test]
    async fn live_bulk_insert_round_trips_one_statement() {
        use sea_orm::{EntityTrait, Set};
        let Some(pool) = live_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin");
        sqlx::query(
            "CREATE TEMP TABLE issue_activities (LIKE public.issue_activities INCLUDING DEFAULTS) ON COMMIT DROP",
        )
        .execute(&mut *tx)
        .await
        .expect("temp table");
        let now = chrono::Utc::now();
        let project = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        let issue = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let ids: Vec<Uuid> = (0..2).map(|_| Uuid::new_v4()).collect();
        {
            let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            let models: Vec<issue_activities::ActiveModel> = ids
                .iter()
                .map(|&id| issue_activities::ActiveModel {
                    id: Set(id),
                    created_at: Set(now.into()),
                    updated_at: Set(now.into()),
                    project_id: Set(project),
                    workspace_id: Set(workspace),
                    issue_id: Set(Some(issue)),
                    verb: Set("issue.activity.created".to_owned()),
                    field: Set(None),
                    old_value: Set(None),
                    new_value: Set(None),
                    comment: Set(String::new()),
                    attachments: Set(Vec::new()),
                    actor_id: Set(Some(actor)),
                    epoch: Set(Some(1750000000.0)),
                    ..Default::default()
                })
                .collect();
            issue_activities::Entity::insert_many(models)
                .exec(&conn)
                .await
                .expect("bulk insert");
            for id in &ids {
                let row = issue_activities::Entity::find_by_id(*id)
                    .one(&conn)
                    .await
                    .expect("read back")
                    .expect("row");
                assert_eq!(row.id, *id);
                assert!(row.attachments.is_empty());
            }
        }
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM issue_activities")
            .fetch_one(&mut *tx)
            .await
            .expect("count");
        assert_eq!(n, 2);
        tx.rollback().await.expect("rollback");
    }
}
