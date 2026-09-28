//! Description-mention helpers (PIDASHCONV-214, T3).
//!
//! Port of `apps/api/pi_dash/bgtasks/notification_task.py` (`:37-:131`):
//! `update_mentions_for_issue` (`:37-52`), `get_new_mentions` (`:53-68`),
//! `get_removed_mentions` (`:69-83`), `extract_mentions_as_subscribers`
//! (`:84-114`), `extract_mentions` (`:115-131`). T4 builds the comment
//! helpers and the `notifications` task on top of these.
//!
//! Fixture: `rust-api/fixtures/tasks_mail/notifications/` (`F-NOTIF`).
//! No ported bugs in these five units; the stale-`receiver_id` bug the
//! fixture records (PORT BUG 3, `:573-576` and `:622-625`) lives in the
//! task body, which is T4's scope.

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::email_notification::py_str;
use super::mention_ids_in_html;

/// One `issue_mentions` row to insert: `update_mentions_for_issue` builds
/// `IssueMention(mention_id=…, issue=…, project=…,
/// workspace_id=project.workspace_id)` per new id and `bulk_create`s them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewIssueMention {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub mention_id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
}

/// One `issue_subscribers` row to insert: `extract_mentions_as_subscribers`
/// returns the bulk list; the caller (`notifications` task, T4) writes it
/// with `bulk_create(…, batch_size=100, ignore_conflicts=True)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewIssueSubscriber {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub subscriber_id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
}

/// Django `bulk_create(…, batch_size=100)`: rows go out in chunks of 100.
pub const BULK_BATCH_SIZE: usize = 100;

/// `issue_mentions` INSERT column list. Field defaults apply at Python
/// construction time (`id=uuid4`), `auto_now_add`/`auto_now` stamp
/// `created_at`/`updated_at`, and the nullable audit columns stay NULL in a
/// worker context (no current user), so every column is written explicitly.
pub const ISSUE_MENTION_INSERT_SQL: &str = "INSERT INTO issue_mentions (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, issue_id, mention_id, project_id, workspace_id) VALUES ";

/// `IssueMention.objects.filter(issue=…, mention__in=…).delete()` goes
/// through `SoftDeletionQuerySet.delete(soft=True)`: an UPDATE stamping
/// `deleted_at`, scoped to live rows by the default manager — never a hard
/// DELETE. The statement always runs, even with an empty removal list
/// (`IN ()` would be a syntax error, so an empty array binds instead and
/// matches nothing, exactly like Django's empty-`__in`).
pub const ISSUE_MENTION_SOFT_DELETE_SQL: &str = "UPDATE issue_mentions SET deleted_at = $1 WHERE deleted_at IS NULL AND issue_id = $2 AND mention_id = ANY($3)";

/// Guard reads in `extract_mentions_as_subscribers`, one per candidate, in
/// Python evaluation order. Every read carries the default manager's
/// `deleted_at IS NULL` scope (`SoftDeletionManager`), including the
/// `issues` creator check and the `projects` workspace lookup.
pub const SUBSCRIBER_GUARD_SQL: &str = "SELECT EXISTS(SELECT 1 FROM issue_subscribers WHERE deleted_at IS NULL AND issue_id = $1 AND subscriber_id = $2 AND project_id = $3)";
pub const ASSIGNEE_GUARD_SQL: &str = "SELECT EXISTS(SELECT 1 FROM issue_assignees WHERE deleted_at IS NULL AND project_id = $1 AND issue_id = $2 AND assignee_id = $3)";
pub const CREATOR_GUARD_SQL: &str =
    "SELECT EXISTS(SELECT 1 FROM issues WHERE deleted_at IS NULL AND project_id = $1 AND id = $2 AND created_by_id = $3)";
pub const MEMBER_GUARD_SQL: &str = "SELECT EXISTS(SELECT 1 FROM project_members WHERE deleted_at IS NULL AND project_id = $1 AND member_id = $2 AND is_active = TRUE)";
pub const PROJECT_WORKSPACE_SQL: &str =
    "SELECT workspace_id FROM projects WHERE deleted_at IS NULL AND id = $1";

/// Port of `update_mentions_for_issue(issue, project, new_mentions,
/// removed_mention)` (`:37-52`).
///
/// Inserts one `issue_mentions` row per new id in batches of 100, then
/// soft-deletes the removed ids. The soft-delete UPDATE always runs, even
/// when both lists are empty.
pub async fn update_mentions_for_issue(
    db: &sqlx::PgPool,
    issue_id: Uuid,
    project_id: Uuid,
    workspace_id: Uuid,
    new_mentions: &[Uuid],
    removed_mentions: &[Uuid],
    now: DateTime<Utc>,
) -> Result<Vec<NewIssueMention>, sqlx::Error> {
    let mut inserted = Vec::with_capacity(new_mentions.len());
    for chunk in new_mentions.chunks(BULK_BATCH_SIZE) {
        let mut builder = sqlx::QueryBuilder::new(ISSUE_MENTION_INSERT_SQL);
        let mut first = true;
        for mention_id in chunk {
            let row = NewIssueMention {
                id: Uuid::new_v4(),
                issue_id,
                mention_id: *mention_id,
                project_id,
                workspace_id,
            };
            if !first {
                builder.push(", ");
            }
            first = false;
            builder.push("(");
            builder.push_bind(row.id);
            builder.push(", ");
            builder.push_bind(now);
            builder.push(", ");
            builder.push_bind(now);
            builder.push(", NULL, NULL, NULL, ");
            builder.push_bind(row.issue_id);
            builder.push(", ");
            builder.push_bind(row.mention_id);
            builder.push(", ");
            builder.push_bind(row.project_id);
            builder.push(", ");
            builder.push_bind(row.workspace_id);
            builder.push(")");
            inserted.push(row);
        }
        builder.build().execute(db).await?;
    }
    sqlx::query(ISSUE_MENTION_SOFT_DELETE_SQL)
        .bind(now)
        .bind(issue_id)
        .bind(removed_mentions)
        .execute(db)
        .await?;
    Ok(inserted)
}

/// Port of `get_new_mentions(requested_instance, current_instance)`
/// (`:53-68`): mentions in the newer description JSON that are absent from
/// the older one. Order-preserving set difference over the newer list.
pub fn get_new_mentions(requested_instance: &str, current_instance: &str) -> Vec<String> {
    let older = extract_mentions(current_instance);
    let newer = extract_mentions(requested_instance);
    newer
        .into_iter()
        .filter(|mention| !older.contains(mention))
        .collect()
}

/// Port of `get_removed_mentions(requested_instance, current_instance)`
/// (`:69-83`): mentions in the older description JSON that are absent from
/// the newer one.
pub fn get_removed_mentions(requested_instance: &str, current_instance: &str) -> Vec<String> {
    let older = extract_mentions(current_instance);
    let newer = extract_mentions(requested_instance);
    older
        .into_iter()
        .filter(|mention| !newer.contains(mention))
        .collect()
}

/// Port of `extract_mentions_as_subscribers(project_id, issue_id, mentions)`
/// (`:84-114`).
///
/// `mentions` is the already-filtered id set. A candidate becomes a row
/// only when all four guards hold: not already a subscriber, not an
/// assignee, not the issue creator, and an active project member. The
/// `Project.objects.get(pk=…)` read resolves `workspace_id`; a missing
/// project propagates the error exactly like `DoesNotExist` propagates in
/// Python. Nothing is written here — the returned rows are the
/// `bulk_mention_subscribers` list the task bulk-creates.
pub async fn extract_mentions_as_subscribers(
    db: &sqlx::PgPool,
    project_id: Uuid,
    issue_id: Uuid,
    mentions: &[Uuid],
) -> Result<Vec<NewIssueSubscriber>, sqlx::Error> {
    let mut subscribers = Vec::new();
    for mention_id in mentions {
        let subscribed: bool = sqlx::query_scalar(SUBSCRIBER_GUARD_SQL)
            .bind(issue_id)
            .bind(mention_id)
            .bind(project_id)
            .fetch_one(db)
            .await?;
        let assigned: bool = sqlx::query_scalar(ASSIGNEE_GUARD_SQL)
            .bind(project_id)
            .bind(issue_id)
            .bind(mention_id)
            .fetch_one(db)
            .await?;
        let creator: bool = sqlx::query_scalar(CREATOR_GUARD_SQL)
            .bind(project_id)
            .bind(issue_id)
            .bind(mention_id)
            .fetch_one(db)
            .await?;
        let active_member: bool = sqlx::query_scalar(MEMBER_GUARD_SQL)
            .bind(project_id)
            .bind(mention_id)
            .fetch_one(db)
            .await?;
        if !subscribed && !assigned && !creator && active_member {
            let workspace_id: Uuid = sqlx::query_scalar(PROJECT_WORKSPACE_SQL)
                .bind(project_id)
                .fetch_one(db)
                .await?;
            subscribers.push(NewIssueSubscriber {
                id: Uuid::new_v4(),
                issue_id,
                subscriber_id: *mention_id,
                project_id,
                workspace_id,
            });
        }
    }
    Ok(subscribers)
}

/// Port of `extract_mentions(issue_instance)` (`:115-131`).
///
/// `issue_instance` is the JSON-encoded description dict. Any failure —
/// malformed JSON, a non-object payload, a missing/non-string
/// `description_html`, or a matched tag without `entity_identifier` —
/// yields `[]`, mirroring the broad `except Exception`. The surviving ids
/// are deduplicated and sorted: Python returns `list(set(…))` whose order
/// is hash-randomized per process, so the port fixes the deterministic
/// representative the fixture goldens record.
pub fn extract_mentions(issue_instance: &str) -> Vec<String> {
    let data: serde_json::Value = match serde_json::from_str(issue_instance) {
        Ok(data) => data,
        Err(_) => return Vec::new(),
    };
    let html = match data
        .get("description_html")
        .and_then(|value| value.as_str())
    {
        Some(html) => html,
        None => return Vec::new(),
    };
    match mention_ids_in_html(html) {
        Some(mut ids) => {
            ids.sort();
            ids.dedup();
            ids
        }
        None => Vec::new(),
    }
}

// ============ T4 (PIDASHCONV-215): comment helpers + notifications task ============
//
// Ports `notification_task.py:133-189` (comment-mention helpers plus the
// single mention-notification row builder) and `:190-674` (the
// `notifications` shared task). The description-mention helpers above (T3)
// are reused, never forked.
//
// Fixtures: `F-NOTIF` (helper goldens + per-type DB before/after),
// `F-WIRE-MAIL` (task name + plain-`@shared_task` options),
// `F-COMMON` (broad-except swallow: `print(e)` to stdout, then return).
//
// Ported bugs (kept verbatim, listed in the PR):
// - PORT BUG 3 (`:573-576`, `:622-625`): the description-mention email logs
//   set `receiver_id=subscriber` — the stale loop variable from the
//   subscriber loop, not `mention_id`. When that loop never ran the name is
//   unbound (`NameError`) and the whole task dies via the outer `except`.
// - STALE ACTIVITY (`:559-567`): the single description-mention
//   `Notification` reads `old/new_identifier` from `issue_activity`, the
//   stale variable of the earlier subscriber/activity loop. Unbound when
//   that loop never iterated — same `NameError` death.
// - UUID TRAP (`:263-265`): `comment_mentions` is re-filtered with
//   `UUID(mention)` on every activity; a non-UUID mention raises
//   `ValueError` and kills the whole task (in-memory bulks discarded,
//   already-executed writes stay).

/// The 13 activity types that take the early bare-return path: zero writes,
/// zero reads (`notification_task.py:205-219`).
pub const EARLY_PATH_TYPES: [&str; 13] = [
    "cycle.activity.created",
    "cycle.activity.deleted",
    "module.activity.created",
    "module.activity.deleted",
    "issue_reaction.activity.created",
    "issue_reaction.activity.deleted",
    "comment_reaction.activity.created",
    "comment_reaction.activity.deleted",
    "issue_vote.activity.created",
    "issue_vote.activity.deleted",
    "issue_draft.activity.created",
    "issue_draft.activity.updated",
    "issue_draft.activity.deleted",
];

/// `sender` values used by the task (`:313-317`, `:470`, `:533`).
pub const CREATED_SENDER: &str = "in_app:issue_activities:created";
pub const ASSIGNED_SENDER: &str = "in_app:issue_activities:assigned";
pub const SUBSCRIBED_SENDER: &str = "in_app:issue_activities:subscribed";
pub const MENTION_SENDER: &str = "in_app:issue_activities:mentioned";

/// Live project-member ids, in Python evaluation position `:231-233`.
pub const ACTIVE_MEMBER_IDS_SQL: &str = "SELECT member_id FROM project_members WHERE deleted_at IS NULL AND project_id = $1 AND is_active = TRUE";
/// Subscriber list with the mention/actor exclusion set (`:280-288`).
pub const SUBSCRIBER_LIST_SQL: &str = "SELECT subscriber_id FROM issue_subscribers WHERE deleted_at IS NULL AND project_id = $1 AND issue_id = $2 AND subscriber_id = ANY($3) AND NOT (subscriber_id = ANY($4))";
/// Issue + project + state + workspace slug in one read (`:290`, `:301`,
/// `:377-382`, `:419-424`, `:490-497`). The `states` join is inner: a null or
/// soft-deleted state makes `issue.state.name` raise in Python, so no row
/// must read the same way here. No scope on `workspaces`: Python reaches it
/// through FK attribute access, never a filtered manager.
pub const ISSUE_SHAPE_SQL: &str = "SELECT i.id, i.name, i.sequence_id, i.created_by_id, p.id, p.identifier, p.workspace_id, s.name, s.\"group\", w.slug FROM issues i JOIN projects p ON p.id = i.project_id AND p.deleted_at IS NULL JOIN states s ON s.id = i.state_id AND s.deleted_at IS NULL JOIN workspaces w ON w.id = p.workspace_id WHERE i.deleted_at IS NULL AND i.id = $1";
/// Assignee ids scoped to active members (`:303-307`).
pub const ASSIGNEE_IDS_SQL: &str = "SELECT assignee_id FROM issue_assignees WHERE deleted_at IS NULL AND issue_id = $1 AND project_id = $2 AND assignee_id = ANY($3)";
/// One preference row per subscriber/mention (`:319`, `:467`, `:524`).
/// `DoesNotExist` propagates to the outer `except`: nothing persisted yet at
/// that point except already-executed bulk writes.
pub const PREFERENCE_SQL: &str = "SELECT property_change, state_change, comment, mention, issue_completed FROM user_notification_preferences WHERE deleted_at IS NULL AND user_id = $1";
/// Comment text for the per-activity payload (`:352-361`); a miss is `None`
/// (renders as `""`), never an error.
pub const COMMENT_STRIPPED_SQL: &str = "SELECT comment_stripped FROM issue_comments WHERE deleted_at IS NULL AND id = $1 AND issue_id = $2 AND project_id = $3 AND workspace_id = $4";
/// Completed-state check for the `issue_completed` email rule (`:334-342`).
pub const COMPLETED_STATE_EXISTS_SQL: &str = "SELECT EXISTS(SELECT 1 FROM states WHERE deleted_at IS NULL AND project_id = $1 AND id = $2 AND \"group\" = 'completed')";
/// `get_or_create` read half for the `subscriber` flag (`:295-299`).
pub const SUBSCRIBER_EXISTS_SQL: &str = "SELECT EXISTS(SELECT 1 FROM issue_subscribers WHERE deleted_at IS NULL AND project_id = $1 AND issue_id = $2 AND subscriber_id = $3)";
/// `get_or_create` write half. Inner `try/except: pass`, so errors are
/// ignored; `ON CONFLICT DO NOTHING` covers the race `get_or_create` catches
/// as `IntegrityError`.
pub const SUBSCRIBER_INSERT_ONE_SQL: &str = "INSERT INTO issue_subscribers (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_id, subscriber_id) VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7) ON CONFLICT DO NOTHING";
/// Latest activity, read AFTER the subscriber bulk write (`:461`).
pub const LAST_ACTIVITY_SQL: &str = "SELECT id, verb, field, actor_id, new_value, old_value, created_at FROM issue_activities WHERE deleted_at IS NULL AND issue_id = $1 ORDER BY created_at DESC LIMIT 1";
/// Actor display name for the comment-mention message (`:463`, `:472`).
/// `users` has no `deleted_at` column, so no scope.
pub const ACTOR_NAME_SQL: &str = "SELECT display_name FROM users WHERE id = $1";
/// Subscriber-branch `Notification` bulk shape (`:364-406`, batch 100).
pub const NOTIFICATION_INSERT_SQL: &str = "INSERT INTO notifications (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, data, entity_identifier, entity_name, title, message, message_html, message_stripped, sender, triggered_by_id, receiver_id, read_at, snoozed_till, archived_at) VALUES ";
/// `EmailNotificationLog` bulk shape (`:409-450`, batch 100,
/// `ignore_conflicts=True`).
pub const EMAIL_LOG_INSERT_SQL: &str = "INSERT INTO email_notification_logs (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, receiver_id, triggered_by_id, entity_identifier, entity_name, data, processed_at, sent_at, entity, old_value, new_value) VALUES ";
/// `message_html` column default (`db/models/notification.py:21`): mention
/// rows never set it, so every row carries the literal.
pub const MESSAGE_HTML_DEFAULT: &str = "<p></p>";

/// Port of `extract_comment_mentions(comment_value)` (`:133-142`).
///
/// `comment_value` is the raw comment HTML (not JSON). `None` covers a
/// missing/`None` value: `BeautifulSoup(None)` raises `TypeError`, which the
/// broad `except` turns into `[]` (verified against bs4 4.15). A matched tag
/// without `entity_identifier` likewise yields `[]` for the whole parse.
/// Ids are sorted and deduped: Python returns `list(set(…))` whose order is
/// hash-randomized per process, so the port fixes the deterministic
/// representative, exactly like T3's `extract_mentions`.
pub fn extract_comment_mentions(comment_value: Option<&str>) -> Vec<String> {
    let html = match comment_value {
        Some(html) => html,
        None => return Vec::new(),
    };
    match mention_ids_in_html(html) {
        Some(mut ids) => {
            ids.sort();
            ids.dedup();
            ids
        }
        None => Vec::new(),
    }
}

/// Comment-mention extraction over a wire/activity JSON value: only a JSON
/// string parses, anything else behaves like `BeautifulSoup` raising.
fn comment_mentions_in_value(value: &Value) -> Vec<String> {
    extract_comment_mentions(value.as_str())
}

/// Port of `get_new_comment_mentions(new_value, old_value)` (`:145-154`).
///
/// `old_value` `None` (missing key or JSON null) returns every newer mention;
/// any other value — including a non-string, which parses as empty exactly
/// like `BeautifulSoup` raising — diffs against the older list.
pub fn get_new_comment_mentions_value(new_value: &Value, old_value: Option<&Value>) -> Vec<String> {
    let newer = comment_mentions_in_value(new_value);
    match old_value {
        None | Some(Value::Null) => newer,
        Some(old) => {
            let older = comment_mentions_in_value(old);
            newer
                .into_iter()
                .filter(|mention| !older.contains(mention))
                .collect()
        }
    }
}

/// `str(activity.get(key))` with the `old/new_identifier` falsy rule
/// (`:183-184`, `:393-402`): a missing, null, empty-string, `false`, `0` or
/// otherwise empty value renders as JSON null (Python's `… if … else None`);
/// anything else renders with Python `str()`.
pub fn str_or_none_if_falsy(activity: &Map<String, Value>, key: &str) -> Option<String> {
    match activity.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::Bool(false)) => None,
        Some(Value::Number(n)) => {
            if n.as_f64() == Some(0.0) {
                None
            } else {
                Some(py_str(&Value::Number(n.clone())))
            }
        }
        Some(Value::String(s)) => {
            if s.is_empty() {
                None
            } else {
                Some(s.clone())
            }
        }
        Some(Value::Array(items)) => {
            if items.is_empty() {
                None
            } else {
                Some(py_str(&Value::Array(items.clone())))
            }
        }
        Some(Value::Object(map)) => {
            if map.is_empty() {
                None
            } else {
                Some(py_str(&Value::Object(map.clone())))
            }
        }
        Some(Value::Bool(true)) => Some("True".to_owned()),
    }
}

/// Title coercion for the subscriber-branch rows (`title=…get("comment")`,
/// `:373`): Django's `TextField.get_prep_value` stringifies non-null values
/// and keeps `None` as `NULL` (which then violates the non-null column at
/// bulk time and kills the task via the outer `except`).
pub fn title_from_value(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(other) => Some(py_str(other)),
    }
}

/// Minimal issue/project/state/workspace shape for every notification payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueNotifShape {
    pub id: Uuid,
    pub name: String,
    pub sequence_id: i32,
    pub created_by_id: Option<Uuid>,
    pub project_id: Uuid,
    pub project_identifier: String,
    pub workspace_id: Uuid,
    pub workspace_slug: String,
    pub state_name: String,
    pub state_group: String,
}

/// One notification preference row (`:331-349` rules read these five flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preference {
    pub property_change: bool,
    pub state_change: bool,
    pub comment: bool,
    pub mention: bool,
    pub issue_completed: bool,
}

/// `issue` block of every payload. `sequence_id` stays a JSON number: Python
/// passes it through raw, never `str()` (`:172`, `:379`, `:419`).
fn issue_json(shape: &IssueNotifShape, with_project_scope: bool) -> Map<String, Value> {
    let mut issue = Map::new();
    issue.insert("id".to_owned(), Value::String(shape.id.to_string()));
    issue.insert("name".to_owned(), Value::String(shape.name.clone()));
    issue.insert(
        "identifier".to_owned(),
        Value::String(shape.project_identifier.clone()),
    );
    if with_project_scope {
        issue.insert(
            "project_id".to_owned(),
            Value::String(shape.project_id.to_string()),
        );
        issue.insert(
            "workspace_slug".to_owned(),
            Value::String(shape.workspace_slug.clone()),
        );
    }
    issue.insert(
        "sequence_id".to_owned(),
        Value::Number(serde_json::Number::from(shape.sequence_id)),
    );
    issue.insert(
        "state_name".to_owned(),
        Value::String(shape.state_name.clone()),
    );
    issue.insert(
        "state_group".to_owned(),
        Value::String(shape.state_group.clone()),
    );
    issue
}

/// `issue_activity` block shared by the mention builder and the subscriber
/// rows. `issue_comment`/`activity_time` are `None` when the branch carries
/// neither (`create_mention_notification`, `:176-185`).
fn activity_json(
    activity: &Map<String, Value>,
    issue_comment: Option<&str>,
    force_field: Option<&str>,
    activity_time: Option<Value>,
) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert(
        "id".to_owned(),
        Value::String(py_str(activity.get("id").unwrap_or(&Value::Null))),
    );
    out.insert(
        "verb".to_owned(),
        Value::String(py_str(activity.get("verb").unwrap_or(&Value::Null))),
    );
    out.insert(
        "field".to_owned(),
        Value::String(match force_field {
            Some(field) => field.to_owned(),
            None => py_str(activity.get("field").unwrap_or(&Value::Null)),
        }),
    );
    out.insert(
        "actor".to_owned(),
        Value::String(py_str(activity.get("actor_id").unwrap_or(&Value::Null))),
    );
    out.insert(
        "new_value".to_owned(),
        Value::String(py_str(activity.get("new_value").unwrap_or(&Value::Null))),
    );
    out.insert(
        "old_value".to_owned(),
        Value::String(py_str(activity.get("old_value").unwrap_or(&Value::Null))),
    );
    if let Some(comment) = issue_comment {
        out.insert(
            "issue_comment".to_owned(),
            Value::String(comment.to_owned()),
        );
    }
    for key in ["old_identifier", "new_identifier"] {
        match str_or_none_if_falsy(activity, key) {
            Some(rendered) => out.insert(key.to_owned(), Value::String(rendered)),
            None => out.insert(key.to_owned(), Value::Null),
        };
    }
    if let Some(time) = activity_time {
        out.insert("activity_time".to_owned(), time);
    }
    out
}

/// One unsaved `Notification` row: `create_mention_notification`
/// (`:157-187`) returns the object and the caller bulk-appends it. `title`
/// keeps the Django `TextField` default (`""`): the builder never sets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMentionNotification {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub triggered_by_id: Uuid,
    pub receiver_id: Uuid,
    pub entity_identifier: Uuid,
    pub message: String,
    pub data: Value,
}

/// Port of `create_mention_notification(project, notification_comment, issue,
/// actor_id, mention_id, issue_id, activity)` (`:157-187`). `activity` must
/// be the JSON object; anything else raised `AttributeError` in Python.
pub fn create_mention_notification(
    shape: &IssueNotifShape,
    message: String,
    actor_id: Uuid,
    mention_id: Uuid,
    activity: &Map<String, Value>,
) -> NewMentionNotification {
    let mut data = Map::new();
    data.insert("issue".to_owned(), Value::Object(issue_json(shape, false)));
    data.insert(
        "issue_activity".to_owned(),
        Value::Object(activity_json(activity, None, None, None)),
    );
    NewMentionNotification {
        id: Uuid::new_v4(),
        workspace_id: shape.workspace_id,
        project_id: shape.project_id,
        triggered_by_id: actor_id,
        receiver_id: mention_id,
        entity_identifier: shape.id,
        message,
        data: Value::Object(data),
    }
}

/// Every failure inside the `notifications` body. Python wraps the whole body
/// in `try/except Exception as e: print(e); return` (`:672-674`, F-COMMON),
/// so every variant prints to stdout and acknowledges — never retries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSwallow(pub String);

impl TaskSwallow {
    fn message(context: &str) -> Self {
        Self(context.to_owned())
    }
}

impl From<sqlx::Error> for TaskSwallow {
    fn from(error: sqlx::Error) -> Self {
        Self(error.to_string())
    }
}

/// Bound task arguments (`notifications(type, issue_id, project_id, actor_id,
/// subscriber, issue_activities_created, requested_data, current_instance)`,
/// `:191-200`). The live caller (`issue_activities_task.py:1587`) passes all
/// eight as keywords; the oracle wire case passes them positionally, so both
/// bindings are accepted. Anything else is Python's `TypeError`, swallowed.
#[derive(Debug, Clone)]
pub struct NotificationsInput {
    pub kind: String,
    pub issue_id: Uuid,
    pub project_id: Uuid,
    pub actor_id: Uuid,
    pub subscriber: bool,
    pub activities: Option<Value>,
    pub requested_data: Value,
    pub current_instance: Value,
}

/// Python truthiness for the `subscriber` flag (`:292`): only `None`,
/// `False`, `0`, `""`, `[]` and `{}` are falsy.
pub fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn parse_uuid_field(value: Option<&Value>, field: &str) -> Result<Uuid, TaskSwallow> {
    match value.and_then(Value::as_str) {
        Some(raw) => Uuid::parse_str(raw)
            .map_err(|_| TaskSwallow::message(&format!("notifications: bad UUID for {field}"))),
        None => Err(TaskSwallow::message(&format!(
            "notifications: missing UUID for {field}"
        ))),
    }
}

fn activities_from_value(value: Option<&Value>) -> Result<Option<Value>, TaskSwallow> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => {
            // `json.loads` (`:202-204`): malformed JSON dies here, before
            // the early-path check. Shape checks stay in `run_notifications`
            // so the 13 early-path types keep their zero-touch return.
            let parsed: Value = serde_json::from_str(raw)
                .map_err(|_| TaskSwallow::message("notifications: bad activities JSON"))?;
            Ok(Some(parsed))
        }
        Some(_) => Err(TaskSwallow::message(
            "notifications: activities value is not a string",
        )),
    }
}

/// Bind one queued job to [`NotificationsInput`].
pub fn bind_input(job: &crate::queue::JobRow) -> Result<NotificationsInput, TaskSwallow> {
    match &job.kwargs {
        Value::Object(map) if !map.is_empty() => {
            if !matches!(&job.args, Value::Array(items) if items.is_empty()) {
                return Err(TaskSwallow::message(
                    "notifications: got both positional args and kwargs",
                ));
            }
            let get = |key: &str| {
                map.get(key)
                    .ok_or_else(|| TaskSwallow::message(&format!("notifications: missing {key}")))
            };
            let kind = get("type")?
                .as_str()
                .ok_or_else(|| TaskSwallow::message("notifications: type is not a string"))?
                .to_owned();
            Ok(NotificationsInput {
                kind,
                issue_id: parse_uuid_field(Some(get("issue_id")?), "issue_id")?,
                project_id: parse_uuid_field(Some(get("project_id")?), "project_id")?,
                actor_id: parse_uuid_field(Some(get("actor_id")?), "actor_id")?,
                subscriber: py_truthy(get("subscriber")?),
                activities: activities_from_value(Some(get("issue_activities_created")?))?,
                requested_data: get("requested_data")?.clone(),
                current_instance: get("current_instance")?.clone(),
            })
        }
        _ => {
            let args = job.args.as_array().ok_or_else(|| {
                TaskSwallow::message("notifications: positional args must be a list")
            })?;
            if args.len() != 8 {
                return Err(TaskSwallow::message(
                    "notifications: expected 8 positional args",
                ));
            }
            let kind = args[0]
                .as_str()
                .ok_or_else(|| TaskSwallow::message("notifications: type is not a string"))?
                .to_owned();
            Ok(NotificationsInput {
                kind,
                issue_id: parse_uuid_field(Some(&args[1]), "issue_id")?,
                project_id: parse_uuid_field(Some(&args[2]), "project_id")?,
                actor_id: parse_uuid_field(Some(&args[3]), "actor_id")?,
                subscriber: py_truthy(&args[4]),
                activities: activities_from_value(Some(&args[5]))?,
                requested_data: args[6].clone(),
                current_instance: args[7].clone(),
            })
        }
    }
}

/// One `Notification` bulk row for the subscriber branch (`:364-406`) and the
/// mention branches (`:465-520`, `:530-571`, `:611-659`). `message` is the
/// `JSONField(null=True)` column: the subscriber branch never sets it
/// (`None`, stored `NULL`); every mention row carries its mention text.
#[derive(Debug, Clone)]
pub struct NewNotification {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub triggered_by_id: Uuid,
    pub receiver_id: Uuid,
    pub entity_identifier: Uuid,
    pub title: Option<String>,
    pub sender: &'static str,
    pub message: Option<Value>,
    pub data: Value,
}

/// Map one built mention row onto its bulk `Notification` row (`:469-477`,
/// `:612-620`): the mention text rides the `message` column, `title` keeps
/// the `""` default, `sender` is the mention sender.
fn mention_bulk_row(notification: NewMentionNotification) -> NewNotification {
    NewNotification {
        id: notification.id,
        workspace_id: notification.workspace_id,
        project_id: notification.project_id,
        triggered_by_id: notification.triggered_by_id,
        receiver_id: notification.receiver_id,
        entity_identifier: notification.entity_identifier,
        title: Some(String::new()),
        sender: MENTION_SENDER,
        message: Some(Value::String(notification.message)),
        data: notification.data,
    }
}

/// One `EmailNotificationLog` bulk row (`:409-450`, `:481-519`, `:573-609`,
/// `:622-657`). `entity` keeps the Django `CharField` default (`""`); the
/// task never sets `old/new_value`, so they stay `NULL`.
#[derive(Debug, Clone)]
pub struct NewEmailLog {
    pub id: Uuid,
    pub triggered_by_id: Uuid,
    pub receiver_id: Uuid,
    pub entity_identifier: Uuid,
    pub data: Value,
}

/// Latest `IssueActivity` row for the description-mention branch (`:461`).
#[derive(Debug, Clone)]
pub struct LastActivity {
    pub id: Uuid,
    pub verb: String,
    pub field: Option<String>,
    pub actor_id: Option<Uuid>,
    pub new_value: Option<String>,
    pub old_value: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// `str(last_activity.created_at)`: Django renders an aware datetime as
/// `YYYY-MM-DD HH:MM:SS[.ffffff]+HH:MM` (`USE_TZ=True`, so UTC) — the
/// fractional part appears only when microseconds are nonzero, exactly like
/// `datetime.isoformat(sep=" ")`.
pub fn django_str_datetime(moment: &DateTime<Utc>) -> String {
    if moment.timestamp_subsec_nanos() == 0 {
        moment.format("%Y-%m-%d %H:%M:%S%:z").to_string()
    } else {
        moment.format("%Y-%m-%d %H:%M:%S%.6f%:z").to_string()
    }
}

/// Per-subscriber `sender` branch (`:311-317`): the creator reads
/// `in_app:issue_activities:created`; an assignee (while the creator is not
/// one) reads `:assigned`; everyone else reads `:subscribed`.
pub fn sender_for(subscriber: &Uuid, created_by: Option<Uuid>, assignees: &[Uuid]) -> &'static str {
    if created_by.is_some_and(|creator| &creator == subscriber) {
        CREATED_SENDER
    } else if assignees.contains(subscriber)
        && !created_by.is_some_and(|creator| assignees.contains(&creator))
    {
        ASSIGNED_SENDER
    } else {
        SUBSCRIBED_SENDER
    }
}

/// `send_email` flag chain (`:331-349`): state-change, completed-state,
/// comment, then the `property_change` catch-all.
pub fn should_send_email(
    field: Option<&str>,
    preference: &Preference,
    completed_state_exists: bool,
) -> bool {
    match field {
        Some("state") if preference.state_change => true,
        Some("state") if preference.issue_completed && completed_state_exists => true,
        Some("comment") if preference.comment => true,
        _ => preference.property_change,
    }
}

/// Stale-variable reads both bug branches share. `last_subscriber` is the
/// `subscriber` loop variable after the subscriber loop (`:311`); when that
/// loop never ran the name is unbound and both reads die (PORT BUG 3).
pub fn stale_receiver(last_subscriber: Option<Uuid>) -> Result<Uuid, TaskSwallow> {
    last_subscriber.ok_or_else(|| {
        TaskSwallow::message("notifications: unbound subscriber (PORT BUG 3, :573/:622)")
    })
}

/// The stale `issue_activity` read of the single-notification
/// description-mention branch (`:559-567`).
pub fn stale_loop_activity(
    last_loop_activity: Option<&Map<String, Value>>,
) -> Result<&Map<String, Value>, TaskSwallow> {
    last_loop_activity
        .ok_or_else(|| TaskSwallow::message("notifications: unbound issue_activity (:559)"))
}

async fn active_member_ids(db: &sqlx::PgPool, project_id: Uuid) -> Result<Vec<Uuid>, TaskSwallow> {
    Ok(sqlx::query_scalar(ACTIVE_MEMBER_IDS_SQL)
        .bind(project_id)
        .fetch_all(db)
        .await?)
}

async fn fetch_preference(db: &sqlx::PgPool, user_id: Uuid) -> Result<Preference, TaskSwallow> {
    let row: Option<(bool, bool, bool, bool, bool)> = sqlx::query_as(PREFERENCE_SQL)
        .bind(user_id)
        .fetch_optional(db)
        .await?;
    row.map(
        |(property_change, state_change, comment, mention, issue_completed)| Preference {
            property_change,
            state_change,
            comment,
            mention,
            issue_completed,
        },
    )
    .ok_or_else(|| {
        TaskSwallow::message(
            "notifications: UserNotificationPreference matching query does not exist",
        )
    })
}

/// Decoded [`ISSUE_SHAPE_SQL`] row, in select order.
type IssueShapeRow = (
    Uuid,
    String,
    i32,
    Option<Uuid>,
    Uuid,
    String,
    Uuid,
    String,
    String,
    String,
);

async fn fetch_issue_shape(
    db: &sqlx::PgPool,
    issue_id: Uuid,
) -> Result<IssueNotifShape, TaskSwallow> {
    let row: Option<IssueShapeRow> = sqlx::query_as(ISSUE_SHAPE_SQL)
        .bind(issue_id)
        .fetch_optional(db)
        .await?;
    row.map(
        |(
            id,
            name,
            sequence_id,
            created_by_id,
            project_id,
            project_identifier,
            workspace_id,
            state_name,
            state_group,
            workspace_slug,
        )| IssueNotifShape {
            id,
            name,
            sequence_id,
            created_by_id,
            project_id,
            project_identifier,
            workspace_id,
            workspace_slug,
            state_name,
            state_group,
        },
    )
    .ok_or_else(|| TaskSwallow::message("notifications: issue has no live row"))
}

/// Decoded [`LAST_ACTIVITY_SQL`] row, in select order.
type LastActivityRow = (
    Uuid,
    String,
    Option<String>,
    Option<Uuid>,
    Option<String>,
    Option<String>,
    DateTime<Utc>,
);

async fn fetch_last_activity(
    db: &sqlx::PgPool,
    issue_id: Uuid,
) -> Result<Option<LastActivity>, TaskSwallow> {
    let row: Option<LastActivityRow> = sqlx::query_as(LAST_ACTIVITY_SQL)
        .bind(issue_id)
        .fetch_optional(db)
        .await?;
    Ok(row.map(
        |(id, verb, field, actor_id, new_value, old_value, created_at)| LastActivity {
            id,
            verb,
            field,
            actor_id,
            new_value,
            old_value,
            created_at,
        },
    ))
}

fn parse_mention_ids(ids: &[String]) -> Result<Vec<Uuid>, TaskSwallow> {
    ids.iter()
        .map(|raw| {
            Uuid::parse_str(raw)
                .map_err(|_| TaskSwallow::message("notifications: mention id is not a UUID"))
        })
        .collect()
}

/// Bulk-write one `Notification` batch chunk (`bulk_create(…, batch_size=100)`,
/// `:669`). An empty list writes nothing, like Django.
async fn insert_notification_chunk(
    db: &sqlx::PgPool,
    rows: &[NewNotification],
    now: DateTime<Utc>,
) -> Result<(), TaskSwallow> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut builder = sqlx::QueryBuilder::new(NOTIFICATION_INSERT_SQL);
    let mut first = true;
    for row in rows {
        if !first {
            builder.push(", ");
        }
        first = false;
        builder.push("(");
        builder.push_bind(row.id);
        builder.push(", ");
        builder.push_bind(now);
        builder.push(", ");
        builder.push_bind(now);
        builder.push(", NULL, NULL, NULL, ");
        builder.push_bind(row.workspace_id);
        builder.push(", ");
        builder.push_bind(row.project_id);
        builder.push(", ");
        builder.push_bind(&row.data);
        builder.push(", ");
        builder.push_bind(row.entity_identifier);
        builder.push(", 'issue', ");
        builder.push_bind(row.title.clone());
        builder.push(", ");
        builder.push_bind(row.message.clone());
        builder.push(", ");
        builder.push_bind(MESSAGE_HTML_DEFAULT);
        builder.push(", NULL, ");
        builder.push_bind(row.sender);
        builder.push(", ");
        builder.push_bind(row.triggered_by_id);
        builder.push(", ");
        builder.push_bind(row.receiver_id);
        builder.push(", NULL, NULL, NULL)");
    }
    builder.build().execute(db).await?;
    Ok(())
}

/// Bulk-write one `EmailNotificationLog` batch chunk (`bulk_create(…,
/// batch_size=100, ignore_conflicts=True)`, `:670`).
async fn insert_email_log_chunk(
    db: &sqlx::PgPool,
    rows: &[NewEmailLog],
    now: DateTime<Utc>,
) -> Result<(), TaskSwallow> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut builder = sqlx::QueryBuilder::new(EMAIL_LOG_INSERT_SQL);
    let mut first = true;
    for row in rows {
        if !first {
            builder.push(", ");
        }
        first = false;
        builder.push("(");
        builder.push_bind(row.id);
        builder.push(", ");
        builder.push_bind(now);
        builder.push(", ");
        builder.push_bind(now);
        builder.push(", NULL, NULL, NULL, ");
        builder.push_bind(row.receiver_id);
        builder.push(", ");
        builder.push_bind(row.triggered_by_id);
        builder.push(", ");
        builder.push_bind(row.entity_identifier);
        builder.push(", 'issue', ");
        builder.push_bind(&row.data);
        builder.push(", NULL, NULL, '', NULL, NULL)");
    }
    builder.push(" ON CONFLICT DO NOTHING");
    builder.build().execute(db).await?;
    Ok(())
}

async fn insert_notifications(
    db: &sqlx::PgPool,
    rows: &[NewNotification],
    now: DateTime<Utc>,
) -> Result<(), TaskSwallow> {
    for chunk in rows.chunks(BULK_BATCH_SIZE) {
        insert_notification_chunk(db, chunk, now).await?;
    }
    Ok(())
}

async fn insert_email_logs(
    db: &sqlx::PgPool,
    rows: &[NewEmailLog],
    now: DateTime<Utc>,
) -> Result<(), TaskSwallow> {
    for chunk in rows.chunks(BULK_BATCH_SIZE) {
        insert_email_log_chunk(db, chunk, now).await?;
    }
    Ok(())
}

/// Mention-subscriber bulk write (`:454-459`, batch 100,
/// `ignore_conflicts=True`).
async fn insert_mention_subscribers(
    db: &sqlx::PgPool,
    rows: &[NewIssueSubscriber],
    now: DateTime<Utc>,
) -> Result<(), TaskSwallow> {
    for chunk in rows.chunks(BULK_BATCH_SIZE) {
        if chunk.is_empty() {
            continue;
        }
        let mut builder = sqlx::QueryBuilder::new(ISSUE_SUBSCRIBER_INSERT_SQL);
        let mut first = true;
        for row in chunk {
            if !first {
                builder.push(", ");
            }
            first = false;
            builder.push("(");
            builder.push_bind(row.id);
            builder.push(", ");
            builder.push_bind(now);
            builder.push(", ");
            builder.push_bind(now);
            builder.push(", NULL, NULL, NULL, ");
            builder.push_bind(row.project_id);
            builder.push(", ");
            builder.push_bind(row.workspace_id);
            builder.push(", ");
            builder.push_bind(row.issue_id);
            builder.push(", ");
            builder.push_bind(row.subscriber_id);
            builder.push(")");
        }
        builder.push(" ON CONFLICT DO NOTHING");
        builder.build().execute(db).await?;
    }
    Ok(())
}

/// `issue_subscribers` INSERT column list for the mention bulk above. Same
/// ten columns as the single-row statement, kept as its own const so the
/// bulk shape stays pinned next to its test.
pub const ISSUE_SUBSCRIBER_INSERT_SQL: &str = "INSERT INTO issue_subscribers (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_id, subscriber_id) VALUES ";

/// The `notifications` shared-task body (`:201-674`).
///
/// Every `Err` is the outer `except Exception`: the caller prints it to
/// stdout and returns (`F-COMMON`). The task never signals retry.
pub async fn run_notifications(
    db: &sqlx::PgPool,
    input: &NotificationsInput,
) -> Result<(), TaskSwallow> {
    // `json.loads` already ran during binding; the 13 early-path types take
    // the bare return with zero reads and zero writes (`:205-219`) — before
    // any iteration or shape check.
    if EARLY_PATH_TYPES.contains(&input.kind.as_str()) {
        return Ok(());
    }
    let activities_raw = match &input.activities {
        Some(Value::Array(items)) => items,
        Some(_) => {
            // `for … in <non-list>` iterates garbage that then fails `.get`.
            return Err(TaskSwallow::message(
                "notifications: activities value has no 'get'",
            ));
        }
        None => {
            // `for … in None` raises `TypeError` before any write (`:249`).
            return Err(TaskSwallow::message(
                "notifications: 'NoneType' object is not iterable",
            ));
        }
    };
    let mut activities: Vec<&Map<String, Value>> = Vec::with_capacity(activities_raw.len());
    for item in activities_raw {
        activities.push(
            item.as_object()
                .ok_or_else(|| TaskSwallow::message("notifications: activity has no 'get'"))?,
        );
    }
    let now = Utc::now();

    let members = active_member_ids(db, input.project_id).await?;
    let member_strings: std::collections::HashSet<String> =
        members.iter().map(Uuid::to_string).collect();

    // Description-mention diffing, gated on live membership (`:236-238`).
    // `str(member)` never matches a non-string mention id, verbatim.
    let mut new_mentions: Vec<String> = get_new_mentions(
        input.requested_data.as_str().unwrap_or(""),
        input.current_instance.as_str().unwrap_or(""),
    )
    .into_iter()
    .filter(|mention| member_strings.contains(mention))
    .collect();
    new_mentions.sort();
    new_mentions.dedup();
    let removed_mentions: Vec<String> = get_removed_mentions(
        input.requested_data.as_str().unwrap_or(""),
        input.current_instance.as_str().unwrap_or(""),
    );
    let requested_mentions: Vec<String> =
        extract_mentions(input.requested_data.as_str().unwrap_or(""));
    let mention_subscribers = extract_mentions_as_subscribers(
        db,
        input.project_id,
        input.issue_id,
        &parse_mention_ids(&requested_mentions)?,
    )
    .await?;

    // Comment-mention diffing per activity (`:249-265`). The UUID filter
    // re-runs cumulatively and kills the task on a non-UUID mention.
    let member_set: std::collections::HashSet<Uuid> = members.iter().cloned().collect();
    let mut all_comment_mentions: Vec<String> = Vec::new();
    let mut comment_mentions: Vec<String> = Vec::new();
    for activity in &activities {
        if !matches!(activity.get("issue_comment"), None | Some(Value::Null)) {
            all_comment_mentions.extend(comment_mentions_in_value(
                activity.get("new_value").unwrap_or(&Value::Null),
            ));
            let fresh = get_new_comment_mentions_value(
                activity.get("new_value").unwrap_or(&Value::Null),
                activity.get("old_value"),
            );
            comment_mentions.extend(fresh);
            let mut filtered = Vec::with_capacity(comment_mentions.len());
            for mention in &comment_mentions {
                let parsed = Uuid::parse_str(mention).map_err(|_| {
                    TaskSwallow::message("notifications: badly formed hexadecimal UUID string")
                })?;
                if member_set.contains(&parsed) {
                    filtered.push(mention.clone());
                }
            }
            comment_mentions = filtered;
        }
    }
    let comment_mention_subscribers = extract_mentions_as_subscribers(
        db,
        input.project_id,
        input.issue_id,
        &parse_mention_ids(&all_comment_mentions)?,
    )
    .await?;

    // Subscriber list minus the mention/actor exclusion set (`:280-288`),
    // then minus the actor once more (`:309`).
    let mut exclusions = parse_mention_ids(&new_mentions)?;
    exclusions.extend(parse_mention_ids(&comment_mentions)?);
    exclusions.push(input.actor_id);
    let subscriber_ids: Vec<Uuid> = sqlx::query_scalar(SUBSCRIBER_LIST_SQL)
        .bind(input.project_id)
        .bind(input.issue_id)
        .bind(&members)
        .bind(&exclusions)
        .fetch_all(db)
        .await?;
    let issue_subscribers: Vec<Uuid> = subscriber_ids
        .into_iter()
        .filter(|subscriber| *subscriber != input.actor_id)
        .collect();

    let shape = fetch_issue_shape(db, input.issue_id).await?;

    if input.subscriber {
        // `get_or_create` inside `try/except: pass` (`:293-299`).
        let exists: bool = sqlx::query_scalar(SUBSCRIBER_EXISTS_SQL)
            .bind(input.project_id)
            .bind(input.issue_id)
            .bind(input.actor_id)
            .fetch_one(db)
            .await?;
        if !exists {
            let _ = sqlx::query(SUBSCRIBER_INSERT_ONE_SQL)
                .bind(Uuid::new_v4())
                .bind(now)
                .bind(now)
                .bind(input.project_id)
                .bind(shape.workspace_id)
                .bind(input.issue_id)
                .bind(input.actor_id)
                .execute(db)
                .await;
        }
    }

    let assignees: Vec<Uuid> = sqlx::query_scalar(ASSIGNEE_IDS_SQL)
        .bind(input.issue_id)
        .bind(input.project_id)
        .bind(&members)
        .fetch_all(db)
        .await?;

    let mut bulk_notifications: Vec<NewNotification> = Vec::new();
    let mut bulk_email_logs: Vec<NewEmailLog> = Vec::new();
    // Stale loop bindings both bug branches read (`:559`, `:573`, `:622`).
    let mut last_subscriber: Option<Uuid> = None;
    let mut last_loop_activity: Option<&Map<String, Value>> = None;

    for subscriber in &issue_subscribers {
        last_subscriber = Some(*subscriber);
        let sender = sender_for(subscriber, shape.created_by_id, &assignees);
        let preference = fetch_preference(db, *subscriber).await?;
        for activity in &activities {
            last_loop_activity = Some(*activity);
            let detail = activity.get("issue_detail").ok_or_else(|| {
                TaskSwallow::message("notifications: 'NoneType' object has no attribute 'get'")
            })?;
            let detail_id = detail.as_object().ok_or_else(|| {
                TaskSwallow::message("notifications: 'str' object has no attribute 'get'")
            })?;
            if detail_id.get("id").and_then(Value::as_str) != Some(&input.issue_id.to_string()) {
                continue;
            }
            if activity.get("field").and_then(Value::as_str) == Some("description") {
                continue;
            }
            // `State.objects.filter(…).exists()` runs only when the first
            // two arms fail, exactly like the `elif` chain (`:331-343`).
            let field = activity.get("field").and_then(Value::as_str);
            let completed_exists =
                if field == Some("state") && !preference.state_change && preference.issue_completed
                {
                    match activity.get("new_identifier") {
                        None | Some(Value::Null) => false,
                        Some(Value::String(raw)) => match Uuid::parse_str(raw) {
                            Ok(identifier) => {
                                sqlx::query_scalar(COMPLETED_STATE_EXISTS_SQL)
                                    .bind(input.project_id)
                                    .bind(identifier)
                                    .fetch_one(db)
                                    .await?
                            }
                            Err(_) => {
                                return Err(TaskSwallow::message(
                                    "notifications: bad new_identifier UUID",
                                ));
                            }
                        },
                        Some(_) => {
                            return Err(TaskSwallow::message(
                                "notifications: bad new_identifier UUID",
                            ));
                        }
                    }
                } else {
                    false
                };
            let send_email = should_send_email(field, &preference, completed_exists);

            // Comment text when the activity names a comment (`:351-361`).
            let comment_text: Option<String> = match activity.get("issue_comment") {
                Some(value) if py_truthy(value) => {
                    let comment_id = parse_uuid_field(Some(value), "issue_comment")?;
                    let stripped: Option<String> = sqlx::query_scalar(COMMENT_STRIPPED_SQL)
                        .bind(comment_id)
                        .bind(input.issue_id)
                        .bind(input.project_id)
                        .bind(shape.workspace_id)
                        .fetch_optional(db)
                        .await?;
                    Some(stripped.unwrap_or_default())
                }
                _ => None,
            };

            let mut data = Map::new();
            data.insert("issue".to_owned(), Value::Object(issue_json(&shape, false)));
            data.insert(
                "issue_activity".to_owned(),
                Value::Object(activity_json(activity, comment_text.as_deref(), None, None)),
            );
            // `issue_comment` renders as stripped text or `""` (`:390-392`).
            if let Some(activity_block) = data
                .get_mut("issue_activity")
                .and_then(Value::as_object_mut)
            {
                activity_block.insert(
                    "issue_comment".to_owned(),
                    Value::String(comment_text.clone().unwrap_or_default()),
                );
            }
            bulk_notifications.push(NewNotification {
                id: Uuid::new_v4(),
                workspace_id: shape.workspace_id,
                project_id: shape.project_id,
                triggered_by_id: input.actor_id,
                receiver_id: *subscriber,
                entity_identifier: input.issue_id,
                title: title_from_value(activity.get("comment")),
                sender,
                // The subscriber branch never sets `message` (`:364-406`),
                // so the column stays `NULL` like Django's unset default.
                message: None,
                data: Value::Object(data),
            });
            if send_email {
                // The email copy carries the comment text and the raw
                // `activity_time` (`:426-447`).
                let mut email_data = Map::new();
                email_data.insert("issue".to_owned(), Value::Object(issue_json(&shape, true)));
                email_data.insert(
                    "issue_activity".to_owned(),
                    Value::Object(activity_json(
                        activity,
                        comment_text.as_deref(),
                        None,
                        Some(activity.get("created_at").cloned().unwrap_or(Value::Null)),
                    )),
                );
                bulk_email_logs.push(NewEmailLog {
                    id: Uuid::new_v4(),
                    triggered_by_id: input.actor_id,
                    receiver_id: *subscriber,
                    entity_identifier: input.issue_id,
                    data: Value::Object(email_data),
                });
            }
        }
    }

    // Mentioned users become subscribers (`:454-459`, before the reads below).
    let mut mention_rows = mention_subscribers;
    mention_rows.extend(comment_mention_subscribers);
    insert_mention_subscribers(db, &mention_rows, now).await?;

    let last_activity = fetch_last_activity(db, input.issue_id).await?;
    let actor_name: String = sqlx::query_scalar(ACTOR_NAME_SQL)
        .bind(input.actor_id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| TaskSwallow::message("notifications: User matching query does not exist"))?;

    // Comment-mention notifications (`:465-520`).
    for mention_id_str in &comment_mentions {
        if *mention_id_str == input.actor_id.to_string() {
            continue;
        }
        let mention_id = Uuid::parse_str(mention_id_str)
            .map_err(|_| TaskSwallow::message("notifications: mention id is not a UUID"))?;
        let preference = fetch_preference(db, mention_id).await?;
        for activity in &activities {
            let notification = create_mention_notification(
                &shape,
                format!(
                    "{actor_name} has mentioned you in a comment in issue {issue}",
                    issue = shape.name
                ),
                input.actor_id,
                mention_id,
                activity,
            );
            if preference.mention {
                let mut email_data = Map::new();
                email_data.insert("issue".to_owned(), Value::Object(issue_json(&shape, true)));
                email_data.insert(
                    "issue_activity".to_owned(),
                    Value::Object(activity_json(
                        activity,
                        None,
                        Some("mention"),
                        Some(activity.get("created_at").cloned().unwrap_or(Value::Null)),
                    )),
                );
                bulk_email_logs.push(NewEmailLog {
                    id: Uuid::new_v4(),
                    triggered_by_id: input.actor_id,
                    receiver_id: mention_id,
                    entity_identifier: input.issue_id,
                    data: Value::Object(email_data),
                });
            }
            bulk_notifications.push(mention_bulk_row(notification));
        }
    }

    // Description-mention notifications (`:522-659`).
    for mention_id_str in &new_mentions {
        if *mention_id_str == input.actor_id.to_string() {
            continue;
        }
        let mention_id = Uuid::parse_str(mention_id_str)
            .map_err(|_| TaskSwallow::message("notifications: mention id is not a UUID"))?;
        let preference = fetch_preference(db, mention_id).await?;
        let actor_matches_last = last_activity.as_ref().is_some_and(|last| {
            last.field.as_deref() == Some("description") && last.actor_id == Some(input.actor_id)
        });
        match last_activity.as_ref() {
            Some(last) if actor_matches_last => {
                // Single "You have been mentioned" row (`:530-571`). The
                // `old/new_identifier` cells read the STALE `issue_activity`
                // loop variable (`:559-567`); the email row reads the STALE
                // `subscriber` (PORT BUG 3, `:573-576`).
                let stale = stale_loop_activity(last_loop_activity)?;
                let mut data = Map::new();
                data.insert("issue".to_owned(), Value::Object(issue_json(&shape, true)));
                let mut activity_block = Map::new();
                activity_block.insert("id".to_owned(), Value::String(last.id.to_string()));
                activity_block.insert("verb".to_owned(), Value::String(last.verb.clone()));
                activity_block.insert(
                    "field".to_owned(),
                    Value::String(last.field.clone().unwrap_or_else(|| "None".to_owned())),
                );
                activity_block.insert(
                    "actor".to_owned(),
                    Value::String(
                        last.actor_id
                            .map(|actor| actor.to_string())
                            .unwrap_or_else(|| "None".to_owned()),
                    ),
                );
                activity_block.insert(
                    "new_value".to_owned(),
                    Value::String(last.new_value.clone().unwrap_or_else(|| "None".to_owned())),
                );
                activity_block.insert(
                    "old_value".to_owned(),
                    Value::String(last.old_value.clone().unwrap_or_else(|| "None".to_owned())),
                );
                for key in ["old_identifier", "new_identifier"] {
                    match str_or_none_if_falsy(stale, key) {
                        Some(rendered) => {
                            activity_block.insert(key.to_owned(), Value::String(rendered));
                        }
                        None => {
                            activity_block.insert(key.to_owned(), Value::Null);
                        }
                    };
                }
                data.insert("issue_activity".to_owned(), Value::Object(activity_block));
                bulk_notifications.push(NewNotification {
                    id: Uuid::new_v4(),
                    workspace_id: shape.workspace_id,
                    project_id: shape.project_id,
                    triggered_by_id: input.actor_id,
                    receiver_id: mention_id,
                    entity_identifier: input.issue_id,
                    title: Some(String::new()),
                    sender: MENTION_SENDER,
                    // `message=` rides the column (`:539`): Django stores
                    // the string as a JSON string, hence `Value::String`.
                    message: Some(Value::String(format!(
                        "You have been mentioned in the issue {issue}",
                        issue = shape.name
                    ))),
                    data: Value::Object(data),
                });
                if preference.mention {
                    let mut email_data = Map::new();
                    email_data.insert("issue".to_owned(), Value::Object(issue_json(&shape, false)));
                    let mut email_activity = Map::new();
                    email_activity.insert("id".to_owned(), Value::String(last.id.to_string()));
                    email_activity.insert("verb".to_owned(), Value::String(last.verb.clone()));
                    email_activity.insert("field".to_owned(), Value::String("mention".to_owned()));
                    email_activity.insert(
                        "actor".to_owned(),
                        Value::String(
                            last.actor_id
                                .map(|actor| actor.to_string())
                                .unwrap_or_else(|| "None".to_owned()),
                        ),
                    );
                    email_activity.insert(
                        "new_value".to_owned(),
                        Value::String(last.new_value.clone().unwrap_or_else(|| "None".to_owned())),
                    );
                    email_activity.insert(
                        "old_value".to_owned(),
                        Value::String(last.old_value.clone().unwrap_or_else(|| "None".to_owned())),
                    );
                    for key in ["old_identifier", "new_identifier"] {
                        match str_or_none_if_falsy(stale, key) {
                            Some(rendered) => {
                                email_activity.insert(key.to_owned(), Value::String(rendered));
                            }
                            None => {
                                email_activity.insert(key.to_owned(), Value::Null);
                            }
                        };
                    }
                    email_activity.insert(
                        "activity_time".to_owned(),
                        Value::String(django_str_datetime(&last.created_at)),
                    );
                    email_data.insert("issue_activity".to_owned(), Value::Object(email_activity));
                    bulk_email_logs.push(NewEmailLog {
                        id: Uuid::new_v4(),
                        triggered_by_id: input.actor_id,
                        receiver_id: stale_receiver(last_subscriber)?,
                        entity_identifier: input.issue_id,
                        data: Value::Object(email_data),
                    });
                }
            }
            _ => {
                // One mention row per activity (`:611-659`).
                for activity in &activities {
                    let notification = create_mention_notification(
                        &shape,
                        format!(
                            "You have been mentioned in the issue {issue}",
                            issue = shape.name
                        ),
                        input.actor_id,
                        mention_id,
                        activity,
                    );
                    if preference.mention {
                        let mut email_data = Map::new();
                        email_data
                            .insert("issue".to_owned(), Value::Object(issue_json(&shape, false)));
                        email_data.insert(
                            "issue_activity".to_owned(),
                            Value::Object(activity_json(
                                activity,
                                None,
                                Some("mention"),
                                Some(activity.get("created_at").cloned().unwrap_or(Value::Null)),
                            )),
                        );
                        bulk_email_logs.push(NewEmailLog {
                            id: Uuid::new_v4(),
                            triggered_by_id: input.actor_id,
                            receiver_id: stale_receiver(last_subscriber)?,
                            entity_identifier: input.issue_id,
                            data: Value::Object(email_data),
                        });
                    }
                    bulk_notifications.push(mention_bulk_row(notification));
                }
            }
        }
    }

    // Mention bookkeeping, then the two bulk writes (`:661-670`).
    update_mentions_for_issue(
        db,
        input.issue_id,
        input.project_id,
        shape.workspace_id,
        &parse_mention_ids(&new_mentions)?,
        &parse_mention_ids(&removed_mentions)?,
        now,
    )
    .await?;
    insert_notifications(db, &bulk_notifications, now).await?;
    insert_email_logs(db, &bulk_email_logs, now).await?;
    Ok(())
}

/// Handler for `pi_dash.bgtasks.notification_task.notifications`: swallow
/// everything (`print(e)` to stdout, F-COMMON) and always acknowledge — the
/// task has plain `@shared_task` options with no `autoretry_for`.
pub fn notifications_handler(pool: sqlx::PgPool) -> crate::worker::Handler {
    std::sync::Arc::new(move |job: crate::queue::JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            match bind_input(&job) {
                Err(swallow) => {
                    println!("{}", swallow.0);
                }
                Ok(input) => {
                    if let Err(swallow) = run_notifications(&pool, &input).await {
                        println!("{}", swallow.0);
                    }
                }
            }
            Ok(crate::worker::Verdict::Ack)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const AAAA: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    const BBBB: &str = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";

    fn desc(ids: &[&str]) -> String {
        let tags: Vec<String> = ids
            .iter()
            .map(|id| {
                format!(
                    "<mention-component entity_name=\"user_mention\" entity_identifier=\"{id}\"></mention-component>"
                )
            })
            .collect();
        serde_json::json!({ "description_html": format!("<p>{}</p>", tags.join(" ") ) }).to_string()
    }

    #[test]
    fn extract_mentions_new_desc_returns_both_ids() {
        assert_eq!(extract_mentions(&desc(&[AAAA, BBBB])), vec![AAAA, BBBB]);
    }

    #[test]
    fn extract_mentions_malformed_json_returns_empty() {
        assert_eq!(extract_mentions("{not json"), Vec::<String>::new());
    }

    #[test]
    fn extract_mentions_missing_key_returns_empty() {
        let no_key = serde_json::json!({ "other": 1 }).to_string();
        assert_eq!(extract_mentions(&no_key), Vec::<String>::new());
        let null_html = serde_json::json!({ "description_html": None::<String> }).to_string();
        assert_eq!(extract_mentions(&null_html), Vec::<String>::new());
        let non_object = "[1, 2]".to_owned();
        assert_eq!(extract_mentions(&non_object), Vec::<String>::new());
    }

    #[test]
    fn extract_mentions_dedupes_and_sorts() {
        assert_eq!(
            extract_mentions(&desc(&[BBBB, AAAA, BBBB])),
            vec![AAAA, BBBB]
        );
    }

    #[test]
    fn extract_mentions_ignores_non_user_tags() {
        let payload = serde_json::json!({
            "description_html": "<mention-component entity_name=\"emoji\" entity_identifier=\"xxxx\"></mention-component>",
        })
        .to_string();
        assert_eq!(extract_mentions(&payload), Vec::<String>::new());
    }

    #[test]
    fn get_new_mentions_reports_newer_only_ids() {
        assert_eq!(
            get_new_mentions(&desc(&[AAAA, BBBB]), &desc(&[AAAA])),
            vec![BBBB.to_owned()]
        );
    }

    #[test]
    fn get_new_mentions_empty_when_nothing_new() {
        assert_eq!(
            get_new_mentions(&desc(&[AAAA]), &desc(&[AAAA, BBBB])),
            Vec::<String>::new()
        );
    }

    #[test]
    fn get_removed_mentions_reports_dropped_ids() {
        assert_eq!(
            get_removed_mentions(&desc(&[AAAA]), &desc(&[AAAA, BBBB])),
            vec![BBBB.to_owned()]
        );
        assert_eq!(
            get_removed_mentions(&desc(&[AAAA, BBBB]), &desc(&[AAAA])),
            Vec::<String>::new()
        );
    }

    #[test]
    fn diff_treats_unparseable_side_as_empty() {
        assert_eq!(
            get_new_mentions(&desc(&[BBBB]), "{broken"),
            vec![BBBB.to_owned()]
        );
        assert_eq!(
            get_removed_mentions("{broken", &desc(&[BBBB])),
            vec![BBBB.to_owned()]
        );
    }

    #[test]
    fn sql_pins_exact_tables_and_columns() {
        assert!(ISSUE_MENTION_INSERT_SQL.starts_with("INSERT INTO issue_mentions (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, issue_id, mention_id, project_id, workspace_id)"));
        assert!(ISSUE_MENTION_SOFT_DELETE_SQL.contains("UPDATE issue_mentions SET deleted_at"));
        assert!(ISSUE_MENTION_SOFT_DELETE_SQL.contains("deleted_at IS NULL"));
        assert!(SUBSCRIBER_GUARD_SQL.contains("FROM issue_subscribers"));
        assert!(ASSIGNEE_GUARD_SQL.contains("FROM issue_assignees"));
        assert!(CREATOR_GUARD_SQL.contains("FROM issues"));
        assert!(MEMBER_GUARD_SQL.contains("FROM project_members"));
        assert!(MEMBER_GUARD_SQL.contains("is_active = TRUE"));
        assert!(PROJECT_WORKSPACE_SQL.contains("FROM projects"));
        assert!(CREATOR_GUARD_SQL.contains("deleted_at IS NULL"));
        assert!(PROJECT_WORKSPACE_SQL.contains("deleted_at IS NULL"));
        assert_eq!(BULK_BATCH_SIZE, 100);
    }

    // ================= T4 tests (PIDASHCONV-215) =================

    const MENTION_A: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    const MENTION_B: &str = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";

    fn comment_html(ids: &[&str]) -> String {
        let tags: Vec<String> = ids
            .iter()
            .map(|id| {
                format!(
                    "<mention-component entity_name=\"user_mention\" entity_identifier=\"{id}\"></mention-component>"
                )
            })
            .collect();
        format!("<p>{}</p>", tags.join(" "))
    }

    fn test_shape() -> IssueNotifShape {
        IssueNotifShape {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("test uuid"),
            name: "Test issue".to_owned(),
            sequence_id: 7,
            created_by_id: Some(
                Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("test uuid"),
            ),
            project_id: Uuid::parse_str("33333333-3333-3333-3333-333333333333").expect("test uuid"),
            project_identifier: "WEB".to_owned(),
            workspace_id: Uuid::parse_str("44444444-4444-4444-4444-444444444444")
                .expect("test uuid"),
            workspace_slug: "acme".to_owned(),
            state_name: "Triage".to_owned(),
            state_group: "triage".to_owned(),
        }
    }

    fn activity_fixture() -> Map<String, Value> {
        serde_json::from_value(serde_json::json!({
            "id": "act-1",
            "verb": "updated",
            "field": "priority",
            "actor_id": MENTION_A,
            "new_value": "high",
            "old_value": "low",
            "old_identifier": "old-state",
            "new_identifier": "new-state",
            "issue_comment": "c1",
            "comment": "priority changed",
            "created_at": "2026-09-01T11:00:00Z",
            "issue_detail": {"id": "11111111-1111-1111-1111-111111111111"},
        }))
        .expect("test activity")
    }

    #[test]
    fn comment_mentions_match_golden() {
        // `F-NOTIF.mention_helpers.golden.json::extract_comment_mentions`.
        assert_eq!(
            extract_comment_mentions(Some(&comment_html(&[MENTION_B]))),
            vec![MENTION_B.to_owned()]
        );
        assert_eq!(
            extract_comment_mentions(Some("<p>no-mentions</p>")),
            Vec::<String>::new()
        );
    }

    #[test]
    fn comment_mentions_none_and_garbage_yield_empty() {
        // `BeautifulSoup(None)` / `BeautifulSoup(123)` raise `TypeError`
        // (verified against bs4 4.15); the broad `except` yields `[]`.
        assert_eq!(extract_comment_mentions(None), Vec::<String>::new());
        assert_eq!(extract_comment_mentions(Some("")), Vec::<String>::new());
        assert_eq!(
            comment_mentions_in_value(&serde_json::json!(123)),
            Vec::<String>::new()
        );
        assert_eq!(
            comment_mentions_in_value(&Value::Null),
            Vec::<String>::new()
        );
    }

    #[test]
    fn comment_mentions_missing_identifier_fails_whole_parse() {
        // Same KeyError trap as `extract_mentions`: one tag without
        // `entity_identifier` discards the entire parse.
        let html = format!(
            "{} <mention-component entity_name=\"user_mention\"></mention-component>",
            comment_html(&[MENTION_A])
        );
        assert_eq!(extract_comment_mentions(Some(&html)), Vec::<String>::new());
    }

    #[test]
    fn comment_mentions_sorted_and_deduped() {
        assert_eq!(
            extract_comment_mentions(Some(&comment_html(&[MENTION_B, MENTION_A, MENTION_B]))),
            vec![MENTION_A.to_owned(), MENTION_B.to_owned()]
        );
    }

    #[test]
    fn new_comment_mentions_none_old_returns_all_newer() {
        // `F-NOTIF.mention_helpers.golden.json::get_new_comment_mentions`.
        let new = serde_json::Value::String(comment_html(&[MENTION_A, MENTION_B]));
        assert_eq!(
            get_new_comment_mentions_value(&new, None),
            vec![MENTION_A.to_owned(), MENTION_B.to_owned()]
        );
        assert_eq!(
            get_new_comment_mentions_value(&new, Some(&Value::Null)),
            vec![MENTION_A.to_owned(), MENTION_B.to_owned()]
        );
    }

    #[test]
    fn new_comment_mentions_diffs_against_older() {
        let new = serde_json::Value::String(comment_html(&[MENTION_A, MENTION_B]));
        let old = serde_json::Value::String(comment_html(&[MENTION_A]));
        assert_eq!(
            get_new_comment_mentions_value(&new, Some(&old)),
            vec![MENTION_B.to_owned()]
        );
        assert_eq!(
            get_new_comment_mentions_value(&new, Some(&new)),
            Vec::<String>::new()
        );
    }

    #[test]
    fn new_comment_mentions_non_string_old_parses_as_empty() {
        // `old_value` present but not a string: `BeautifulSoup` raises, so
        // the older list is empty and every newer mention is new.
        let new = serde_json::Value::String(comment_html(&[MENTION_B]));
        let old = serde_json::json!(42);
        assert_eq!(
            get_new_comment_mentions_value(&new, Some(&old)),
            vec![MENTION_B.to_owned()]
        );
    }

    #[test]
    fn mention_notification_row_shape_matches_fixture() {
        // `F-NOTIF.notifications_task.json::mention_notifications` +
        // `create_mention_notification` (`:157-187`): sender, ids, message,
        // and the exact `data.issue` / `data.issue_activity` key sets.
        let shape = test_shape();
        let activity = activity_fixture();
        let actor = Uuid::parse_str(MENTION_A).expect("test uuid");
        let mention = Uuid::parse_str(MENTION_B).expect("test uuid");
        let row = create_mention_notification(
            &shape,
            "You have been mentioned in the issue Test issue".to_owned(),
            actor,
            mention,
            &activity,
        );
        assert_eq!(row.workspace_id, shape.workspace_id);
        assert_eq!(row.project_id, shape.project_id);
        assert_eq!(row.triggered_by_id, actor);
        assert_eq!(row.receiver_id, mention);
        assert_eq!(row.entity_identifier, shape.id);
        assert_eq!(
            row.message,
            "You have been mentioned in the issue Test issue"
        );
        let issue = row.data["issue"].as_object().expect("issue object");
        let keys: std::collections::HashSet<&str> = issue.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "id",
                "name",
                "identifier",
                "sequence_id",
                "state_name",
                "state_group"
            ]
            .into_iter()
            .collect::<std::collections::HashSet<&str>>()
        );
        assert_eq!(issue["id"], "11111111-1111-1111-1111-111111111111");
        assert_eq!(issue["sequence_id"], 7);
        let block = row.data["issue_activity"]
            .as_object()
            .expect("activity object");
        assert_eq!(block["verb"], "updated");
        assert_eq!(block["field"], "priority");
        assert_eq!(block["actor"], MENTION_A);
        assert_eq!(block["new_value"], "high");
        assert_eq!(block["old_identifier"], "old-state");
        assert!(!block.contains_key("issue_comment"));
        assert!(!block.contains_key("activity_time"));
    }

    #[test]
    fn mention_activity_missing_keys_render_none() {
        // `str(None)` → `"None"`; falsy identifiers → JSON null.
        let shape = test_shape();
        let activity: Map<String, Value> = Map::new();
        let row = create_mention_notification(
            &shape,
            "m".to_owned(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            &activity,
        );
        let block = row.data["issue_activity"]
            .as_object()
            .expect("activity object");
        assert_eq!(block["id"], "None");
        assert_eq!(block["old_identifier"], Value::Null);
    }

    #[test]
    fn str_or_none_if_falsy_mirrors_python_truthiness() {
        let activity: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "missing_is_absent": 1,
            "null": null,
            "empty": "",
            "truthy": "x",
            "zero": 0,
            "nonzero": 3,
            "false_bool": false,
            "true_bool": true,
        }))
        .expect("test map");
        assert_eq!(str_or_none_if_falsy(&activity, "absent"), None);
        assert_eq!(str_or_none_if_falsy(&activity, "null"), None);
        assert_eq!(str_or_none_if_falsy(&activity, "empty"), None);
        assert_eq!(
            str_or_none_if_falsy(&activity, "truthy"),
            Some("x".to_owned())
        );
        assert_eq!(str_or_none_if_falsy(&activity, "zero"), None);
        assert_eq!(
            str_or_none_if_falsy(&activity, "nonzero"),
            Some("3".to_owned())
        );
        assert_eq!(str_or_none_if_falsy(&activity, "false_bool"), None);
        assert_eq!(
            str_or_none_if_falsy(&activity, "true_bool"),
            Some("True".to_owned())
        );
    }

    #[test]
    fn send_email_rules_match_fixture() {
        // `F-NOTIF.notifications_task.json::send_email_rules`.
        let all_off = Preference {
            property_change: false,
            state_change: false,
            comment: false,
            mention: false,
            issue_completed: false,
        };
        let state_on = Preference {
            state_change: true,
            ..all_off
        };
        assert!(should_send_email(Some("state"), &state_on, false));
        let completed_on = Preference {
            issue_completed: true,
            ..all_off
        };
        assert!(!should_send_email(Some("state"), &completed_on, false));
        assert!(should_send_email(Some("state"), &completed_on, true));
        // A completed-state row never rescues a non-state field.
        assert!(!should_send_email(Some("comment"), &completed_on, true));
        let comment_on = Preference {
            comment: true,
            ..all_off
        };
        assert!(should_send_email(Some("comment"), &comment_on, false));
        let catch_all = Preference {
            property_change: true,
            ..all_off
        };
        assert!(should_send_email(Some("priority"), &catch_all, false));
        assert!(should_send_email(None, &catch_all, false));
        assert!(!should_send_email(Some("priority"), &all_off, false));
        assert!(!should_send_email(None, &all_off, false));
    }

    #[test]
    fn sender_branches_match_source() {
        // `:311-317`: creator → created; assignee (creator not one) →
        // assigned; everyone else → subscribed.
        let creator = Uuid::new_v4();
        let assignee = Uuid::new_v4();
        let other = Uuid::new_v4();
        assert_eq!(
            sender_for(&creator, Some(creator), &[assignee]),
            CREATED_SENDER
        );
        assert_eq!(
            sender_for(&assignee, Some(creator), &[assignee]),
            ASSIGNED_SENDER
        );
        // Creator who is also an assignee still reads created.
        assert_eq!(
            sender_for(&creator, Some(creator), &[creator]),
            CREATED_SENDER
        );
        // Assignee loop while the creator is an assignee: subscribed.
        assert_eq!(
            sender_for(&assignee, Some(creator), &[creator, assignee]),
            SUBSCRIBED_SENDER
        );
        assert_eq!(
            sender_for(&other, Some(creator), &[assignee]),
            SUBSCRIBED_SENDER
        );
        assert_eq!(sender_for(&other, None, &[]), SUBSCRIBED_SENDER);
    }

    #[test]
    fn early_path_types_match_fixture() {
        // `F-NOTIF.notifications_task.json::early_path_types`: exactly the
        // 13 types, in fixture order.
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/tasks_mail/notifications/F-NOTIF.notifications_task.json"
        ))
        .expect("fixture must parse");
        let expected: Vec<String> = fixture["early_path_types"]
            .as_array()
            .expect("early_path_types array")
            .iter()
            .map(|name| name.as_str().expect("string").to_owned())
            .collect();
        let actual: Vec<String> = EARLY_PATH_TYPES
            .iter()
            .map(|name| name.to_string())
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn stale_reads_die_when_loops_never_ran() {
        // PORT BUG 3 (`:573-576`, `:622-625`) and the stale `issue_activity`
        // (`:559-567`): unbound names raise, killing the task.
        assert!(stale_receiver(None).is_err());
        assert!(stale_loop_activity(None).is_err());
        let id = Uuid::new_v4();
        assert_eq!(stale_receiver(Some(id)), Ok(id));
    }

    #[test]
    fn sql_pins_task_tables_and_scopes() {
        assert!(ACTIVE_MEMBER_IDS_SQL.contains("FROM project_members"));
        assert!(ACTIVE_MEMBER_IDS_SQL.contains("is_active = TRUE"));
        assert!(ACTIVE_MEMBER_IDS_SQL.contains("deleted_at IS NULL"));
        assert!(SUBSCRIBER_LIST_SQL.contains("FROM issue_subscribers"));
        assert!(SUBSCRIBER_LIST_SQL.contains("NOT (subscriber_id = ANY"));
        assert!(ISSUE_SHAPE_SQL.contains("FROM issues i"));
        assert!(ISSUE_SHAPE_SQL.contains("JOIN projects p"));
        assert!(ISSUE_SHAPE_SQL.contains("JOIN states s"));
        assert!(ISSUE_SHAPE_SQL.contains("JOIN workspaces w"));
        assert!(ISSUE_SHAPE_SQL.contains("s.\"group\""));
        assert!(ASSIGNEE_IDS_SQL.contains("FROM issue_assignees"));
        assert!(PREFERENCE_SQL.contains("FROM user_notification_preferences"));
        assert!(COMMENT_STRIPPED_SQL.contains("FROM issue_comments"));
        assert!(COMPLETED_STATE_EXISTS_SQL.contains("\"group\" = 'completed'"));
        assert!(LAST_ACTIVITY_SQL.contains("ORDER BY created_at DESC LIMIT 1"));
        assert!(ACTOR_NAME_SQL.contains("FROM users"));
        assert!(!ACTOR_NAME_SQL.contains("deleted_at"));
        assert!(NOTIFICATION_INSERT_SQL.starts_with("INSERT INTO notifications (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, data, entity_identifier, entity_name, title, message, message_html, message_stripped, sender, triggered_by_id, receiver_id, read_at, snoozed_till, archived_at)"));
        assert!(EMAIL_LOG_INSERT_SQL.starts_with("INSERT INTO email_notification_logs (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, receiver_id, triggered_by_id, entity_identifier, entity_name, data, processed_at, sent_at, entity, old_value, new_value)"));
        assert!(ISSUE_SUBSCRIBER_INSERT_SQL.starts_with("INSERT INTO issue_subscribers (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_id, subscriber_id)"));
        assert_eq!(MESSAGE_HTML_DEFAULT, "<p></p>");
        assert_eq!(MENTION_SENDER, "in_app:issue_activities:mentioned");
    }

    #[test]
    fn subscriber_list_scope_covers_every_exclusion() {
        // `:280-288`: project + issue + live-member subquery + the
        // mention/actor exclusion set.
        for scope in [
            "deleted_at IS NULL",
            "project_id = $1",
            "issue_id = $2",
            "subscriber_id = ANY($3)",
            "subscriber_id = ANY($4)",
        ] {
            assert!(SUBSCRIBER_LIST_SQL.contains(scope), "{scope}");
        }
    }

    #[test]
    fn bind_accepts_oracle_positional_shape() {
        // `test_mail_tasks.py::MAIL_TASKS` notifications entry: 8
        // positionals with `None` activities and `{}` description payloads.
        use serde_json::json;
        let job = crate::queue::JobRow {
            id: 1,
            celery_id: "wire-case".to_owned(),
            task: super::super::NOTIFICATIONS_TASK.to_owned(),
            args: json!([
                "issue.activity.created",
                "11111111-1111-1111-1111-111111111111",
                "33333333-3333-3333-3333-333333333333",
                "22222222-2222-2222-2222-222222222222",
                [],
                null,
                {},
                {},
            ]),
            kwargs: json!({}),
            queue: "celery".to_owned(),
            status: "queued".to_owned(),
            attempts: 0,
            max_retries: 3,
            visible_at: chrono::Utc::now(),
            claimed_at: None,
            claimed_by: None,
            created_at: chrono::Utc::now(),
            last_error: None,
        };
        let input = bind_input(&job).expect("oracle wire shape must bind");
        assert_eq!(input.kind, "issue.activity.created");
        assert!(!input.subscriber);
        assert!(input.activities.is_none());
        assert!(!EARLY_PATH_TYPES.contains(&input.kind.as_str()));
    }

    #[test]
    fn bind_accepts_live_kwargs_shape() {
        // `issue_activities_task.py:1587`: all eight as keywords.
        use serde_json::json;
        let job = crate::queue::JobRow {
            id: 2,
            celery_id: "live-case".to_owned(),
            task: super::super::NOTIFICATIONS_TASK.to_owned(),
            args: json!([]),
            kwargs: json!({
                "type": "comment.activity.created",
                "issue_id": "11111111-1111-1111-1111-111111111111",
                "project_id": "33333333-3333-3333-3333-333333333333",
                "actor_id": "22222222-2222-2222-2222-222222222222",
                "subscriber": true,
                "issue_activities_created": "[]",
                "requested_data": "{}",
                "current_instance": "{}",
            }),
            queue: "celery".to_owned(),
            status: "queued".to_owned(),
            attempts: 0,
            max_retries: 3,
            visible_at: chrono::Utc::now(),
            claimed_at: None,
            claimed_by: None,
            created_at: chrono::Utc::now(),
            last_error: None,
        };
        let input = bind_input(&job).expect("live kwargs shape must bind");
        assert!(input.subscriber);
        assert_eq!(input.activities, Some(serde_json::json!([])));
    }

    #[test]
    fn bind_rejects_bad_shapes_like_python_type_errors() {
        use serde_json::json;
        let base_kwargs = json!({
            "type": "comment.activity.created",
            "issue_id": "11111111-1111-1111-1111-111111111111",
            "project_id": "33333333-3333-3333-3333-333333333333",
            "actor_id": "22222222-2222-2222-2222-222222222222",
            "subscriber": false,
            "issue_activities_created": null,
            "requested_data": "{}",
            "current_instance": "{}",
        });
        let job_for = |args: Value, kwargs: Value| crate::queue::JobRow {
            id: 3,
            celery_id: "bad".to_owned(),
            task: super::super::NOTIFICATIONS_TASK.to_owned(),
            args,
            kwargs,
            queue: "celery".to_owned(),
            status: "queued".to_owned(),
            attempts: 0,
            max_retries: 3,
            visible_at: chrono::Utc::now(),
            claimed_at: None,
            claimed_by: None,
            created_at: chrono::Utc::now(),
            last_error: None,
        };
        // Wrong positional arity.
        assert!(bind_input(&job_for(json!([1, 2]), json!({}))).is_err());
        // Both positionals and kwargs.
        assert!(bind_input(&job_for(json!([1]), base_kwargs.clone())).is_err());
        // Missing kwarg.
        let mut missing = base_kwargs.as_object().expect("object").clone();
        missing.remove("actor_id");
        assert!(bind_input(&job_for(json!([]), Value::Object(missing))).is_err());
        // Malformed activities JSON (`json.loads` raising).
        let mut bad_json = base_kwargs.as_object().expect("object").clone();
        bad_json.insert("issue_activities_created".to_owned(), json!("{not json"));
        assert!(bind_input(&job_for(json!([]), Value::Object(bad_json))).is_err());
        // Non-list activities JSON binds fine (`json.loads` succeeds); the
        // shape check lives past the early-path return in `run_notifications`.
        let mut dict_activities = base_kwargs.as_object().expect("object").clone();
        dict_activities.insert("issue_activities_created".to_owned(), json!("{}"));
        assert!(bind_input(&job_for(json!([]), Value::Object(dict_activities))).is_ok());
        // Garbage UUID.
        let mut bad_uuid = base_kwargs.as_object().expect("object").clone();
        bad_uuid.insert("issue_id".to_owned(), json!("not-a-uuid"));
        assert!(bind_input(&job_for(json!([]), Value::Object(bad_uuid))).is_err());
    }

    #[test]
    fn py_truthy_matches_subscriber_flag() {
        use serde_json::json;
        assert!(!py_truthy(&json!(null)));
        assert!(!py_truthy(&json!(false)));
        assert!(!py_truthy(&json!(0)));
        assert!(!py_truthy(&json!("")));
        assert!(!py_truthy(&json!([])));
        assert!(!py_truthy(&json!({})));
        assert!(py_truthy(&json!(true)));
        assert!(py_truthy(&json!("x")));
        assert!(py_truthy(&json!([1])));
    }

    #[test]
    fn django_str_datetime_renders_like_python() {
        // `str()` of an aware UTC datetime: `+00:00`, never `UTC`.
        let moment =
            chrono::DateTime::parse_from_rfc3339("2026-09-01T12:00:00Z").expect("test time");
        assert_eq!(
            django_str_datetime(&moment.with_timezone(&chrono::Utc)),
            "2026-09-01 12:00:00+00:00"
        );
        // `str()` keeps microseconds when nonzero (`:605`); `created_at`
        // values from Postgres virtually always carry them.
        let moment =
            chrono::DateTime::parse_from_rfc3339("2026-09-01T12:00:00.123456Z").expect("test time");
        assert_eq!(
            django_str_datetime(&moment.with_timezone(&chrono::Utc)),
            "2026-09-01 12:00:00.123456+00:00"
        );
    }

    #[test]
    fn mention_bulk_row_carries_message_column() {
        // Review finding (PIDASHCONV-215): the `message` column is the
        // mention text on every mention row (`create_mention_notification`
        // sets `message=`, `:157-187`) and stays `NULL` on subscriber rows
        // (`:364-406` never set it). The bulk writer binds this field, so
        // dropping it here would write `NULL` for every mention row.
        let shape = test_shape();
        let activity = activity_fixture();
        let actor = Uuid::parse_str(MENTION_A).expect("test uuid");
        let mention = Uuid::parse_str(MENTION_B).expect("test uuid");
        let built = create_mention_notification(
            &shape,
            "You have been mentioned in the issue Test issue".to_owned(),
            actor,
            mention,
            &activity,
        );
        let row = mention_bulk_row(built);
        assert_eq!(row.sender, MENTION_SENDER);
        assert_eq!(row.title, Some(String::new()));
        assert_eq!(row.receiver_id, mention);
        assert_eq!(
            row.message,
            Some(Value::String(
                "You have been mentioned in the issue Test issue".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn early_path_returns_before_any_shape_check() {
        // `:205-219`: the 13 types return with zero reads and zero writes —
        // even garbage activities never reach a query, so a lazy pool is
        // enough (no database is touched).
        use serde_json::json;
        let pool = sqlx::PgPool::connect_lazy("postgres://127.0.0.1:1/nope").expect("lazy");
        let garbage = NotificationsInput {
            kind: "issue_vote.activity.created".to_owned(),
            issue_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            actor_id: Uuid::new_v4(),
            subscriber: false,
            activities: Some(json!({"not": "a list"})),
            requested_data: json!({}),
            current_instance: json!({}),
        };
        assert_eq!(run_notifications(&pool, &garbage).await, Ok(()));
        // Off the early path the same garbage dies before the first query.
        let main_path = NotificationsInput {
            kind: "comment.activity.created".to_owned(),
            ..garbage
        };
        assert!(run_notifications(&pool, &main_path).await.is_err());
        // Off the early path with `None` activities: `for … in None`.
        let none_path = NotificationsInput {
            kind: "comment.activity.created".to_owned(),
            activities: None,
            ..main_path
        };
        assert!(run_notifications(&pool, &none_path).await.is_err());
    }

    #[tokio::test]
    async fn handler_always_acks_and_swallows() {
        // F-COMMON: `except Exception: print(e); return`. Bad input never
        // reaches the pool, so a lazy pool is enough — no database touched.
        use serde_json::json;
        let pool = sqlx::PgPool::connect_lazy("postgres://127.0.0.1:1/nope").expect("lazy");
        let bad = crate::queue::JobRow {
            id: 4,
            celery_id: "swallow".to_owned(),
            task: super::super::NOTIFICATIONS_TASK.to_owned(),
            args: json!([]),
            kwargs: json!({}),
            queue: "celery".to_owned(),
            status: "queued".to_owned(),
            attempts: 0,
            max_retries: 3,
            visible_at: chrono::Utc::now(),
            claimed_at: None,
            claimed_by: None,
            created_at: chrono::Utc::now(),
            last_error: None,
        };
        assert_eq!(
            super::notifications_handler(pool)(bad).await,
            Ok(crate::worker::Verdict::Ack)
        );
    }
}
