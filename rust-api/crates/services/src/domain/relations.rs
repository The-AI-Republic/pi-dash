//! `domain::relations`: the `IssueRelation` row writes (`relations.py`).
//!
//! Relations carry no `save()` override, so both ops are single
//! statements over the caller's executor (reads, verdict shaping and
//! activity stay with the callers — Django passes different kwargs and
//! reverse sets per view, design D6). Every caller runs pool-direct or
//! inside its own transaction; nothing here needs a [`WriteTxn`].
//!
//! [`WriteTxn`]: super::txn::WriteTxn

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// One `issue_relations` row: explicit audit (`bulk_create` callers pass
/// the actor twice — the ORM skips `save()` — single-create callers the
/// actor plus `NULL`) and explicit stamps (the one-now versus two-now
/// clock shapes stay with the drivers).
pub struct RelationRow<'a> {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub issue_id: Uuid,
    pub related_issue_id: Uuid,
    pub relation_type: &'a str,
}

const RELATE_INSERT_SQL: &str = "INSERT INTO \"issue_relations\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"project_id\", \"workspace_id\", \"issue_id\", \"related_issue_id\", \"relation_type\") VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9, $10)";

const RELATE_INSERT_OR_NOTHING_SQL: &str = "INSERT INTO \"issue_relations\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"project_id\", \"workspace_id\", \"issue_id\", \"related_issue_id\", \"relation_type\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) ON CONFLICT DO NOTHING";

const RELATION_SOFT_DELETE_SQL: &str = "UPDATE \"issue_relations\" SET \"deleted_at\" = $1, \"updated_at\" = $2, \"updated_by_id\" = $3 WHERE \"id\" = $4";

/// The relate create (`relations.py:196-207`, `relation.py` bulk): the
/// bare `INSERT` — a concurrent same-direction pair raises 23505 off the
/// partial unique index and the caller re-reads. The stored ends/type
/// arrive pre-mapped (the 3- versus 4-member reverse sets stay
/// caller-side, design D6).
pub async fn relate_row<'e, E>(ex: E, v: &RelationRow<'_>) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(RELATE_INSERT_SQL)
        .bind(v.id)
        .bind(v.created_at)
        .bind(v.updated_at)
        .bind(v.created_by)
        .bind(v.updated_by)
        .bind(v.project_id)
        .bind(v.workspace_id)
        .bind(v.issue_id)
        .bind(v.related_issue_id)
        .bind(v.relation_type)
        .execute(ex)
        .await?;
    Ok(())
}

/// The `bulk_create(..., ignore_conflicts=True)` shape (`relation.py`):
/// `ON CONFLICT DO NOTHING`, returning whether the row was created.
/// Per-target verdict loops (which report per-row created/conflict)
/// call this once per row; chunk-atomic batch callers use
/// [`relate_rows_or_nothing`] (a per-row loop would persist earlier
/// rows past a mid-chunk FK failure, where the single statement
/// writes nothing).
pub async fn relate_row_or_nothing<'e, E>(ex: E, v: &RelationRow<'_>) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let done = sqlx::query(RELATE_INSERT_OR_NOTHING_SQL)
        .bind(v.id)
        .bind(v.created_at)
        .bind(v.updated_at)
        .bind(v.created_by)
        .bind(v.updated_by)
        .bind(v.project_id)
        .bind(v.workspace_id)
        .bind(v.issue_id)
        .bind(v.related_issue_id)
        .bind(v.relation_type)
        .execute(ex)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// One `bulk_create` chunk (`relation.py:235`, `batch_size=10`): the
/// `UNNEST` multi-row `INSERT ... ON CONFLICT DO NOTHING` the app
/// writer issued, moved verbatim. Single-statement chunk atomicity:
/// an FK failure in the chunk writes nothing (earlier chunks persist,
/// as Django's per-batch autocommit does). `relation_type`,
/// audit and scope bind once from the first row — callers guarantee
/// a homogeneous chunk (both batch writers share all five). Empty
/// input issues no statement.
pub async fn relate_rows_or_nothing<'e, E>(
    ex: E,
    rows: &[RelationRow<'_>],
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let Some(first) = rows.first() else {
        return Ok(());
    };
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let created: Vec<DateTime<Utc>> = rows.iter().map(|row| row.created_at).collect();
    let updated: Vec<DateTime<Utc>> = rows.iter().map(|row| row.updated_at).collect();
    let issue_ids: Vec<Uuid> = rows.iter().map(|row| row.issue_id).collect();
    let related_ids: Vec<Uuid> = rows.iter().map(|row| row.related_issue_id).collect();
    sqlx::query(
        "INSERT INTO issue_relations \
         (id, created_at, updated_at, relation_type, issue_id, related_issue_id, \
         created_by_id, updated_by_id, project_id, workspace_id) \
         SELECT u.id, u.created_at, u.updated_at, $6, u.issue_id, u.related_id, \
         $7, $8, $9, $10 \
         FROM UNNEST($1::uuid[], $2::timestamptz[], $3::timestamptz[], \
         $4::uuid[], $5::uuid[]) \
         AS u(id, created_at, updated_at, issue_id, related_id) \
         ON CONFLICT DO NOTHING",
    )
    .bind(&ids)
    .bind(&created)
    .bind(&updated)
    .bind(&issue_ids)
    .bind(&related_ids)
    .bind(first.relation_type)
    .bind(first.created_by)
    .bind(first.updated_by)
    .bind(first.project_id)
    .bind(first.workspace_id)
    .execute(ex)
    .await?;
    Ok(())
}

/// The unrelate soft delete (`relations.py:235-266`, `relation.py`): the
/// full-save audit stamp (`deleted_at`, `updated_at`, `updated_by`).
/// Returns affected rows (0 = already gone; the sweep/activity stay
/// caller-side — the transactional agent-tool sweep must not move).
pub async fn soft_delete_relation_row<'e, E>(
    ex: E,
    id: Uuid,
    deleted_by: Uuid,
    deleted_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let done = sqlx::query(RELATION_SOFT_DELETE_SQL)
        .bind(deleted_at)
        .bind(updated_at)
        .bind(deleted_by)
        .bind(id)
        .execute(ex)
        .await?;
    Ok(done.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

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

    async fn seed_pair(pool: &sqlx::PgPool, tag: &str) -> (Uuid, Uuid, Uuid, Uuid) {
        let actor = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let state_id = Uuid::new_v4();
        let issue = Uuid::new_v4();
        let sibling = Uuid::new_v4();
        let username = format!("rl-{tag}-{}", actor.simple());
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
        .execute(pool)
        .await
        .expect("seed user");
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, slug, timezone, \
             background_color, owner_id) VALUES ($1, now(), now(), 'rl ws', $2, 'UTC', '#ffffff', \
             $3)",
        )
        .bind(workspace_id)
        .bind(format!("rl-{tag}-{}", workspace_id.simple()))
        .bind(actor)
        .execute(pool)
        .await
        .expect("seed workspace");
        let identifier = format!("R{:08X}", (project_id.as_u128() & 0xffff_ffff) as u32);
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
        .bind(project_id)
        .bind(format!("rl-project-{identifier}"))
        .bind(identifier)
        .bind(workspace_id)
        .bind(actor)
        .execute(pool)
        .await
        .expect("seed project");
        sqlx::query(
            "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
             project_id, workspace_id, sequence, \"group\", \"default\", is_triage) VALUES \
             ($1, now(), now(), 'Backlog', '', '#ff0000', $2, $3, $4, 100.0, 'backlog', true, false)",
        )
        .bind(state_id)
        .bind(format!("rl-backlog-{state_id}"))
        .bind(project_id)
        .bind(workspace_id)
        .execute(pool)
        .await
        .expect("seed state");
        for (id, name) in [(issue, "rl issue"), (sibling, "rl sibling")] {
            sqlx::query(
                "INSERT INTO issues (id, created_at, updated_at, project_id, workspace_id, \
                 state_id, name, description_json, description_html, sequence_id, sort_order, \
                 priority, complexity_score, is_draft, git_work_branch, workpad) VALUES ($1, \
                 now(), now(), $2, $3, $4, $5, '{}', '<p>rl</p>', 3, 10000.0, 'none', 0, false, \
                 '', '')",
            )
            .bind(id)
            .bind(project_id)
            .bind(workspace_id)
            .bind(state_id)
            .bind(name)
            .execute(pool)
            .await
            .expect("seed issue");
        }
        (actor, workspace_id, project_id, issue)
    }

    #[tokio::test]
    async fn live_relate_reports_created_then_conflict() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let (actor, workspace_id, project_id, issue) = seed_pair(&pool, "rel").await;
        let sibling: (Uuid,) =
            sqlx::query_as("SELECT id FROM issues WHERE project_id = $1 AND id <> $2")
                .bind(project_id)
                .bind(issue)
                .fetch_one(&pool)
                .await
                .expect("sibling");
        let now = Utc::now();
        let row = RelationRow {
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            created_by: Some(actor),
            updated_by: Some(actor),
            project_id,
            workspace_id,
            issue_id: issue,
            related_issue_id: sibling.0,
            relation_type: "blocked_by",
        };
        assert!(relate_row_or_nothing(&pool, &row).await.expect("first"));
        let row2 = RelationRow {
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            created_by: Some(actor),
            updated_by: Some(actor),
            project_id,
            workspace_id,
            issue_id: issue,
            related_issue_id: sibling.0,
            relation_type: "blocked_by",
        };
        assert!(!relate_row_or_nothing(&pool, &row2).await.expect("second"));
        // The bare insert surfaces 23505 for the caller's re-read.
        let err = relate_row(&pool, &row2).await.expect_err("conflict");
        let code = match &err {
            sqlx::Error::Database(db) => db.code().map(|c| c.to_string()),
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(code.as_deref(), Some("23505"));
    }

    #[tokio::test]
    async fn live_soft_delete_stamps_audit() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let (actor, workspace_id, project_id, issue) = seed_pair(&pool, "del").await;
        let sibling: (Uuid,) =
            sqlx::query_as("SELECT id FROM issues WHERE project_id = $1 AND id <> $2")
                .bind(project_id)
                .bind(issue)
                .fetch_one(&pool)
                .await
                .expect("sibling");
        let now = Utc::now();
        let id = Uuid::new_v4();
        relate_row(
            &pool,
            &RelationRow {
                id,
                created_at: now,
                updated_at: now,
                created_by: Some(actor),
                updated_by: None,
                project_id,
                workspace_id,
                issue_id: issue,
                related_issue_id: sibling.0,
                relation_type: "blocking",
            },
        )
        .await
        .expect("relate");
        let stamp = Utc::now();
        let affected = soft_delete_relation_row(&pool, id, actor, stamp, stamp)
            .await
            .expect("soft delete");
        assert_eq!(affected, 1);
        let row: (Option<chrono::DateTime<Utc>>, Option<Uuid>) =
            sqlx::query_as("SELECT deleted_at, updated_by_id FROM issue_relations WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read relation");
        assert!(row.0.is_some());
        assert_eq!(row.1, Some(actor));
        let again = soft_delete_relation_row(&pool, id, actor, stamp, stamp)
            .await
            .expect("repeat");
        assert_eq!(again, 1);
    }

    #[tokio::test]
    async fn live_relate_dangling_issue_maps_fk() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let (actor, workspace_id, project_id, _) = seed_pair(&pool, "fk").await;
        let now = Utc::now();
        let err = relate_row(
            &pool,
            &RelationRow {
                id: Uuid::new_v4(),
                created_at: now,
                updated_at: now,
                created_by: Some(actor),
                updated_by: Some(actor),
                project_id,
                workspace_id,
                issue_id: Uuid::new_v4(),
                related_issue_id: Uuid::new_v4(),
                relation_type: "blocked_by",
            },
        )
        .await
        .expect_err("fk");
        // The drivers map 23503 to the 400 arms; the domain preserves it.
        let code = match &err {
            sqlx::Error::Database(db) => db.code().map(|c| c.to_string()),
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(code.as_deref(), Some("23503"));
    }

    #[tokio::test]
    async fn live_batch_chunk_is_atomic_and_idempotent() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let (actor, workspace_id, project_id, issue) = seed_pair(&pool, "batch").await;
        let sibling: (Uuid,) =
            sqlx::query_as("SELECT id FROM issues WHERE project_id = $1 AND id <> $2")
                .bind(project_id)
                .bind(issue)
                .fetch_one(&pool)
                .await
                .expect("sibling");
        let now = Utc::now();
        // A chunk whose second row dangles: the single statement fails
        // and writes nothing (chunk atomicity).
        let bad = vec![
            RelationRow {
                id: Uuid::new_v4(),
                created_at: now,
                updated_at: now,
                created_by: Some(actor),
                updated_by: Some(actor),
                project_id,
                workspace_id,
                issue_id: issue,
                related_issue_id: sibling.0,
                relation_type: "blocked_by",
            },
            RelationRow {
                id: Uuid::new_v4(),
                created_at: now,
                updated_at: now,
                created_by: Some(actor),
                updated_by: Some(actor),
                project_id,
                workspace_id,
                issue_id: issue,
                related_issue_id: Uuid::new_v4(),
                relation_type: "blocked_by",
            },
        ];
        relate_rows_or_nothing(&pool, &bad).await.expect_err("fk");
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM issue_relations WHERE issue_id = $1 AND deleted_at IS NULL",
        )
        .bind(issue)
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(count.0, 0);
        // The clean chunk writes; repeating it is a no-op.
        let good = vec![RelationRow {
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            created_by: Some(actor),
            updated_by: Some(actor),
            project_id,
            workspace_id,
            issue_id: issue,
            related_issue_id: sibling.0,
            relation_type: "blocked_by",
        }];
        relate_rows_or_nothing(&pool, &good).await.expect("write");
        relate_rows_or_nothing(&pool, &good).await.expect("repeat");
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM issue_relations WHERE issue_id = $1 AND deleted_at IS NULL",
        )
        .bind(issue)
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(count.0, 1);
        relate_rows_or_nothing(&pool, &[]).await.expect("empty");
    }
}
