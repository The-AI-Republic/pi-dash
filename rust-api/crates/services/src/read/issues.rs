//! Issue / comment / state reads.
//!
//! Verbatim moves of the `pidash-api` handler helpers (OVL-05,
//! PIDASHCONV-1543); the handlers call these and map [`Error`](super::Error)
//! onto their denial type.
//!
//! SeaORM conversion (READ-1, Part 2): scalar and transactional reads
//! go through entities; the `PgRow` workpad fetchers stay raw until the
//! V1WI caller migrations land (Group B).

use chrono::{DateTime, Utc};
use pidash_db::entity::{issues, projects, states, workspaces};
use pidash_db::orm_tx::TxConn;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use sqlx::PgPool;
use uuid::Uuid;

use super::{DbError, Error};
use crate::orchestration::blockers::{fetch_relations_summary, RelationsSummary};

/// The cached blocker summary (`relations_summary(obj)`): both direction
/// lists plus the open-blockers `Exists`, in that order. Moved from
/// `api::app_issues::handlers_core`; the statements are the SeaORM
/// builder form in `orchestration::blockers`.
pub async fn fetch_blocker_summary(
    pool: &PgPool,
    issue_id: &Uuid,
) -> Result<RelationsSummary, Error> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    fetch_relations_summary(&db, *issue_id)
        .await
        .map_err(|error| DbError::from_orm("fetch-blocker-summary", error).into())
}

/// `IssueCommentSerializer.get_url` (`serializers/issue.py:994-999`):
/// `issue_web_url(workspace.slug, project.identifier, issue.sequence_id)`
/// over the ROW's FKs (fetched, never the path params). Moved from
/// `v1_work_items::handlers_social`.
pub async fn comment_url(
    pool: &PgPool,
    workspace_id: &Uuid,
    project_id: &Uuid,
    issue_id: &Uuid,
    web_base: Option<&str>,
) -> Result<Option<String>, Error> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let slug: Option<String> = workspaces::Entity::find_by_id(*workspace_id)
        .select_only()
        .column(workspaces::Column::Slug)
        .into_tuple::<String>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("comment-url-slug", error))?;
    let identifier: Option<String> = projects::Entity::find_by_id(*project_id)
        .select_only()
        .column(projects::Column::Identifier)
        .into_tuple::<String>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("comment-url-ident", error))?;
    let sequence: Option<i32> = issues::Entity::find_by_id(*issue_id)
        .select_only()
        .column(issues::Column::SequenceId)
        .into_tuple::<i32>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("comment-url-seq", error))?;
    Ok(crate::v1_work_items::shape_issue::issue_url(
        web_base,
        slug.as_deref(),
        identifier.as_deref(),
        sequence.map(i64::from),
    ))
}

/// The NULL-state arm of `Issue.save` (`db/models/issue.py:288-308`):
/// default non-triage state first, else the first non-triage state, both
/// by `Meta.ordering` (`sequence`); with no states at all the issue keeps
/// its NULL state. `completed_at` is untouched in every NULL-state case —
/// the recompute runs only when the issue already had a state (the `else`
/// arm, `issue.py:300-308`). Moved from
/// `v1_work_items::handlers_relations`.
pub async fn resolve_workpad_state(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: &Uuid,
    state_id: Option<Uuid>,
    state_group: Option<&str>,
    completed_at: Option<DateTime<Utc>>,
) -> Result<(Option<Uuid>, Option<DateTime<Utc>>), Error> {
    if let Some(state_id) = state_id {
        let group = state_group.unwrap_or_default();
        if group == "completed" {
            return Ok((Some(state_id), Some(now_utc())));
        }
        return Ok((Some(state_id), None));
    }
    let conn = TxConn::new(&mut *txn);
    let found: Option<(Uuid, String)> = states::Entity::find()
        .filter(states::Column::DeletedAt.is_null())
        .filter(states::Column::Group.ne("triage"))
        .filter(states::Column::IsTriage.eq(false))
        .filter(states::Column::ProjectId.eq(*project_id))
        .filter(states::Column::Default.eq(true))
        .order_by_asc(states::Column::Sequence)
        .select_only()
        .columns([states::Column::Id, states::Column::Group])
        .into_tuple::<(Uuid, String)>()
        .one(&conn)
        .await
        .map_err(|error| DbError::from_orm("workpad-default-state", error))?;
    let found = match found {
        Some(found) => Some(found),
        None => states::Entity::find()
            .filter(states::Column::DeletedAt.is_null())
            .filter(states::Column::Group.ne("triage"))
            .filter(states::Column::IsTriage.eq(false))
            .filter(states::Column::ProjectId.eq(*project_id))
            .order_by_asc(states::Column::Sequence)
            .select_only()
            .columns([states::Column::Id, states::Column::Group])
            .into_tuple::<(Uuid, String)>()
            .one(&conn)
            .await
            .map_err(|error| DbError::from_orm("workpad-random-state", error))?,
    };
    // The `completed_at` recompute lives in the `else` arm of the
    // `self.state is None` branch (`issue.py:300-308`): a stateless issue
    // keeps its `completed_at` untouched whether or not a state is found.
    match found {
        Some((id, _group)) => Ok((Some(id), completed_at)),
        // No states at all: `self.state` stays `None`, `completed_at`
        // untouched (the `else` arm never runs).
        None => Ok((None, completed_at)),
    }
}

/// `GET .../workpad/` (`views/issue.py:3275-3279`): the scoped
/// workpad read — live, non-triage, unarchived, non-draft issue in the
/// slug's project. Moved from `v1_work_items::handlers_relations`
/// (PIDASHCONV-1661).
pub async fn fetch_workpad(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Error> {
    // raw-sql: Group B — returns PgRow for handlers_relations; conversion to a typed row requires the V1WI caller migration (PR 2958). Keeps byte-identical join/scope until then.
    let row = sqlx::query(
        r#"SELECT "issues"."workpad", "issues"."updated_at"
           FROM "issues"
           LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id")
           INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id")
           INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id")
           WHERE ("issues"."deleted_at" IS NULL
             AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)
             AND NOT ("issues"."archived_at" IS NOT NULL)
             AND NOT ("projects"."archived_at" IS NOT NULL)
             AND NOT ("issues"."is_draft")
             AND "workspaces"."slug" = $1
             AND "issues"."project_id" = $2
             AND "issues"."id" = $3)"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| DbError::new("workpad-get", error))?;
    Ok(row)
}

/// Payload re-read of one issue's workpad by pk, with no scope
/// predicates: the MCP `get_workpad` tool resolves and gates the issue
/// first, and Python serializes the in-memory object without
/// re-querying, so the re-read must not re-filter — a miss means only
/// a concurrent delete. (Moved from the MCP read tools by
/// PIDASHCONV-1661; Rule 5: queries on OSS tables live here.)
pub async fn fetch_workpad_by_id(
    pool: &PgPool,
    issue_id: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Error> {
    // raw-sql: Group B — returns PgRow for MCP/workpad callers; typed-row conversion requires caller migration.
    let row = sqlx::query(
        r#"SELECT "issues"."workpad", "issues"."updated_at" FROM "issues" WHERE "issues"."id" = $1"#,
    )
    .bind(issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| DbError::new("workpad-by-id", error))?;
    Ok(row)
}

/// The workpad-PATCH `description_stripped` recompute
/// (`db/models/issue.py:346-350`): `None` for empty HTML, else
/// `html_processor.strip_tags` — the entity-decoding MLStripper
/// (`pidash_db::app_pages::strip::ml_strip_tags`), NOT Django's
/// verbatim-entity kernel. Moved from
/// `v1_work_items::handlers_relations`.
pub fn workpad_stripped(description_html: &str) -> Option<String> {
    if description_html.is_empty() {
        None
    } else {
        Some(pidash_db::app_pages::strip::ml_strip_tags(description_html))
    }
}

/// `timezone.now()` truncated to microseconds: Python datetimes carry no
/// nanos, and `timestamptz` stores micros — an untruncated `Utc::now()`
/// would render nanos in the response while the DB row reads back micros.
/// (Moved with [`resolve_workpad_state`]; the handler keeps its own copy
/// for its other callers.)
fn now_utc() -> DateTime<Utc> {
    trunc_micros(Utc::now())
}

/// Truncate an instant to microsecond precision (see [`now_utc`]).
fn trunc_micros(dt: DateTime<Utc>) -> DateTime<Utc> {
    let nanos = dt.timestamp_subsec_nanos();
    dt - chrono::Duration::nanoseconds(i64::from(nanos % 1000))
}

#[cfg(test)]
mod tests {
    use sea_orm::{DbBackend, QueryTrait};

    use super::*;

    #[test]
    fn workpad_state_queries_scope_to_live_non_triage() {
        let sql = states::Entity::find()
            .filter(states::Column::DeletedAt.is_null())
            .filter(states::Column::Group.ne("triage"))
            .filter(states::Column::IsTriage.eq(false))
            .filter(states::Column::ProjectId.eq(Uuid::nil()))
            .filter(states::Column::Default.eq(true))
            .order_by_asc(states::Column::Sequence)
            .select_only()
            .columns([states::Column::Id, states::Column::Group])
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#""states"."deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""states"."group" <> 'triage'"#), "{sql}");
        assert!(sql.contains(r#""states"."is_triage" = FALSE"#), "{sql}");
        assert!(sql.contains(r#""states"."default" = TRUE"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "states"."sequence" ASC"#), "{sql}");
    }

    #[test]
    fn comment_url_queries_are_unscoped() {
        let sql = workspaces::Entity::find_by_id(Uuid::nil())
            .select_only()
            .column(workspaces::Column::Slug)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#""workspaces"."slug""#), "{sql}");
        assert!(!sql.contains("deleted_at"), "{sql}");
    }
}

#[cfg(test)]
mod live {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use crate::read::test_support::{pool, seed_world};

    #[tokio::test]
    async fn fetch_blocker_summary_empty_and_linked() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let empty = fetch_blocker_summary(&db, &world.issue_id)
            .await
            .expect("empty");
        assert!(empty.relations_summary.blocked_by.is_empty());
        assert!(empty.relations_summary.blocking.is_empty());
        assert!(!empty.has_open_blockers);
        // Link a second issue as blocked-by this one.
        let other = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO issues (id, created_at, updated_at, name, description_json, priority,\
             sequence_id, project_id, state_id, workspace_id, description_html, sort_order,\
             is_draft, git_work_branch, workpad, complexity_score)\
             VALUES ($1, now(), now(), 'Other', '{}', 'none', 102, $2, $3, $4, '', 65536,\
             false, '', '', 0)",
        )
        .bind(other)
        .bind(world.project_id)
        .bind(world.state_id)
        .bind(world.workspace_id)
        .execute(&db)
        .await
        .expect("seed other issue");
        sqlx::query(
            "INSERT INTO issue_relations (id, relation_type, issue_id, project_id,\
             related_issue_id, workspace_id, created_at, updated_at)\
             VALUES ($1, 'blocked_by', $2, $3, $4, $5, now(), now())",
        )
        .bind(Uuid::new_v4())
        .bind(world.issue_id)
        .bind(world.project_id)
        .bind(other)
        .bind(world.workspace_id)
        .execute(&db)
        .await
        .expect("seed relation");
        let linked = fetch_blocker_summary(&db, &world.issue_id)
            .await
            .expect("linked");
        let total =
            linked.relations_summary.blocked_by.len() + linked.relations_summary.blocking.len();
        assert_eq!(total, 1);
    }

    #[tokio::test]
    async fn comment_url_hit_and_missing() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let url = comment_url(
            &db,
            &world.workspace_id,
            &world.project_id,
            &world.issue_id,
            Some("http://localhost"),
        )
        .await
        .expect("hit");
        let url = url.expect("some");
        assert!(url.contains(&world.workspace_slug), "{url}");
        assert!(url.contains(&world.project_identifier), "{url}");
        assert!(url.contains("101"), "{url}");
        // Missing rows render through the URL builder without failing.
        let missing = comment_url(
            &db,
            &Uuid::new_v4(),
            &Uuid::new_v4(),
            &Uuid::new_v4(),
            Some("http://localhost"),
        )
        .await
        .expect("missing rows do not fail");
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn resolve_workpad_state_arms() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let mut txn = db.begin().await.expect("txn");
        // Carried state, completed group: fresh stamp.
        let (id, at) = resolve_workpad_state(
            &mut txn,
            &world.project_id,
            Some(world.state_id),
            Some("completed"),
            None,
        )
        .await
        .expect("completed arm");
        assert_eq!(id, Some(world.state_id));
        assert!(at.is_some());
        // Carried state, other group: cleared stamp.
        let (id, at) = resolve_workpad_state(
            &mut txn,
            &world.project_id,
            Some(world.state_id),
            Some("backlog"),
            Some(Utc::now()),
        )
        .await
        .expect("uncleared arm");
        assert_eq!((id, at), (Some(world.state_id), None));
        // NULL state: the seeded default state wins, stamp untouched.
        let stamp = Utc::now();
        let (id, at) = resolve_workpad_state(&mut txn, &world.project_id, None, None, Some(stamp))
            .await
            .expect("default arm");
        assert_eq!(id, Some(world.state_id));
        assert_eq!(at, Some(stamp));
        txn.rollback().await.expect("rollback");
        // NULL state with no states at all: stays NULL, stamp untouched.
        let fresh_project = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO projects (id, created_at, updated_at, name, description, network,\
             identifier, workspace_id, cycle_view, module_view, issue_views_view, page_view,\
             intake_view, archive_in, close_in, logo_props, is_time_tracking_enabled,\
             is_issue_type_enabled, guest_view_all_features, timezone, members_can_edit_states,\
             repo_url, base_branch, agent_default_interval_seconds, agent_default_max_ticks,\
             agent_ticking_enabled, is_default, agent_review_default_interval_seconds,\
             default_agent_executor, agent_test_default_interval_seconds)\
             VALUES ($1, now(), now(), 'Stateless', '', 0, 'STL', $2, true, true, true, true,\
             true, 0, 0, '{}', false, false, false, 'UTC', false, '', '', 0, 0, false, false,\
             0, '', 0)",
        )
        .bind(fresh_project)
        .bind(world.workspace_id)
        .execute(&db)
        .await
        .expect("seed stateless project");
        let mut txn = db.begin().await.expect("txn");
        let (id, at) = resolve_workpad_state(&mut txn, &fresh_project, None, None, Some(stamp))
            .await
            .expect("stateless arm");
        assert_eq!((id, at), (None, Some(stamp)));
        txn.rollback().await.expect("rollback");
    }

    #[test]
    fn workpad_stripped_cases() {
        assert_eq!(workpad_stripped(""), None);
        assert_eq!(
            workpad_stripped("<p>seed comment</p>").as_deref(),
            Some("seed comment")
        );
        // The MLStripper decodes entities (Django's kernel would not).
        assert_eq!(
            workpad_stripped("<p>a &amp; b</p>").as_deref(),
            Some("a & b")
        );
    }

    #[tokio::test]
    async fn workpad_reads_hit_and_miss() {
        use sqlx::Row as _;
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let row = fetch_workpad(
            &db,
            &world.workspace_slug,
            &world.project_id,
            &world.issue_id,
        )
        .await
        .expect("scoped hit")
        .expect("row");
        let body: String = row.try_get("workpad").expect("workpad");
        assert_eq!(body, "");
        let _updated: chrono::DateTime<chrono::Utc> =
            row.try_get("updated_at").expect("updated_at");
        let missing = fetch_workpad(
            &db,
            &world.workspace_slug,
            &world.project_id,
            &Uuid::new_v4(),
        )
        .await
        .expect("scoped miss");
        assert!(missing.is_none());
        let row = fetch_workpad_by_id(&db, &world.issue_id)
            .await
            .expect("by-id hit")
            .expect("row");
        let body: String = row.try_get("workpad").expect("workpad");
        assert_eq!(body, "");
        let missing = fetch_workpad_by_id(&db, &Uuid::new_v4())
            .await
            .expect("by-id miss");
        assert!(missing.is_none());
    }
}
