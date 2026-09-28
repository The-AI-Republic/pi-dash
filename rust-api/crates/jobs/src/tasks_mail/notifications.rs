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
use uuid::Uuid;

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
}
