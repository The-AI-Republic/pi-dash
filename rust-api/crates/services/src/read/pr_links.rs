//! GitHub PR-link list/detail reads.
//!
//! Verbatim moves of the `pidash-api` handler SQL (OVL-05 pattern,
//! PIDASHCONV-1661) plus the MCP tool querysets (Rule 5: queries on OSS
//! tables live here). The v1 handlers call these and map
//! [`Error`](super::Error) onto their denial type.

use sqlx::PgPool;
use uuid::Uuid;

use super::{DbError, Error};

/// PR-link list rows in fixture order (`github_pr.py:37-49`, fixture
/// `github_pr_list_queryset`): the join-span form verbatim — soft-delete
/// scope on the link table only, `DISTINCT`, `-created_at`. `$1` slug,
/// `$2` project id, `$3` issue id, `$4` member id. (Moved from the v1
/// work-items handlers by PIDASHCONV-1661.)
pub const PR_LIST_SQL: &str = r#"SELECT DISTINCT "github_pull_request_links"."created_at", "github_pull_request_links"."updated_at", "github_pull_request_links"."created_by_id", "github_pull_request_links"."updated_by_id", "github_pull_request_links"."deleted_at", "github_pull_request_links"."id", "github_pull_request_links"."project_id", "github_pull_request_links"."workspace_id", "github_pull_request_links"."issue_id", "github_pull_request_links"."repo_owner", "github_pull_request_links"."repo_name", "github_pull_request_links"."pr_number", "github_pull_request_links"."url", "github_pull_request_links"."title", "github_pull_request_links"."state", "github_pull_request_links"."merged", "github_pull_request_links"."draft", "github_pull_request_links"."pr_updated_at" FROM "github_pull_request_links" INNER JOIN "workspaces" ON ("github_pull_request_links"."workspace_id" = "workspaces"."id") INNER JOIN "projects" ON ("github_pull_request_links"."project_id" = "projects"."id") INNER JOIN "project_members" ON ("projects"."id" = "project_members"."project_id") WHERE ("github_pull_request_links"."deleted_at" IS NULL AND "workspaces"."slug" = $1 AND "github_pull_request_links"."project_id" = $2 AND "github_pull_request_links"."issue_id" = $3 AND "project_members"."is_active" AND "project_members"."member_id" = $4 AND "projects"."archived_at" IS NULL) ORDER BY "github_pull_request_links"."created_at" DESC"#;

/// PR-link detail scope + pk (`github_pr.py:86-98`, fixture
/// `github_pr_detail_queryset`): no archived guard, no `DISTINCT`,
/// `Meta.ordering` (`-created_at`). `$5` pk. (Moved from the v1
/// work-items handlers by PIDASHCONV-1661.)
pub const PR_DETAIL_SQL: &str = r#"SELECT "github_pull_request_links"."created_at", "github_pull_request_links"."updated_at", "github_pull_request_links"."created_by_id", "github_pull_request_links"."updated_by_id", "github_pull_request_links"."deleted_at", "github_pull_request_links"."id", "github_pull_request_links"."project_id", "github_pull_request_links"."workspace_id", "github_pull_request_links"."issue_id", "github_pull_request_links"."repo_owner", "github_pull_request_links"."repo_name", "github_pull_request_links"."pr_number", "github_pull_request_links"."url", "github_pull_request_links"."title", "github_pull_request_links"."state", "github_pull_request_links"."merged", "github_pull_request_links"."draft", "github_pull_request_links"."pr_updated_at" FROM "github_pull_request_links" INNER JOIN "workspaces" ON ("github_pull_request_links"."workspace_id" = "workspaces"."id") INNER JOIN "projects" ON ("github_pull_request_links"."project_id" = "projects"."id") INNER JOIN "project_members" ON ("projects"."id" = "project_members"."project_id") WHERE ("github_pull_request_links"."deleted_at" IS NULL AND "workspaces"."slug" = $1 AND "github_pull_request_links"."project_id" = $2 AND "github_pull_request_links"."issue_id" = $3 AND "project_members"."is_active" AND "project_members"."member_id" = $4 AND "github_pull_request_links"."id" = $5) ORDER BY "github_pull_request_links"."created_at" DESC LIMIT 1"#;

/// One PR link in the v1 detail scope, or `None` (the DELETE
/// path's 404 branch).
pub async fn fetch_pr_detail_row(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    actor_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Error> {
    let row = sqlx::query(PR_DETAIL_SQL)
        .bind(slug)
        .bind(project_id)
        .bind(issue_id)
        .bind(actor_id)
        .bind(pk)
        .fetch_optional(pool)
        .await
        .map_err(|error| DbError::new("pr detail fetch", error))?;
    Ok(row)
}

/// `GithubPullRequestLink.objects.filter(...).order_by("-created_at")`
/// (`pi_dash_cloud/mcp/tools.py:1258-1262`): base-manager soft-delete
/// scope only (the FK traversal carries no related-manager guards),
/// `-created_at` order, `[:limit]` slice. `$1` slug, `$2` project id,
/// `$3` issue id, `$4` limit. The caller checked membership before
/// calling. (Moved from the MCP read tools by PIDASHCONV-1661.)
pub const PR_LINK_LIST_SQL: &str = "SELECT l.id, l.issue_id, l.project_id, l.workspace_id, \
     l.repo_owner, l.repo_name, l.pr_number, l.url, l.title, l.state, l.merged, l.draft, \
     l.pr_updated_at, l.created_at, l.updated_at, l.created_by_id \
     FROM github_pull_request_links AS l \
     INNER JOIN workspaces ON workspaces.id = l.workspace_id \
     WHERE l.deleted_at IS NULL \
       AND workspaces.slug = $1 \
       AND l.project_id = $2 \
       AND l.issue_id = $3 \
     ORDER BY l.created_at DESC LIMIT $4";

/// Full re-read of one link for serialization (attach's
/// `_serialize_model(serializer, link)` over the model instance).
/// (Moved from the MCP read tools by PIDASHCONV-1661.)
pub const PR_LINK_BY_ID_SQL: &str = "SELECT l.id, l.issue_id, l.project_id, l.workspace_id, \
     l.repo_owner, l.repo_name, l.pr_number, l.url, l.title, l.state, l.merged, l.draft, \
     l.pr_updated_at, l.created_at, l.updated_at, l.created_by_id \
     FROM github_pull_request_links AS l \
     WHERE l.deleted_at IS NULL AND l.id = $1";

/// PR links of one issue in the MCP tool scope ([`PR_LINK_LIST_SQL`]).
/// The `count` echoes the SLICED length, like Python's `len(links)`.
pub async fn fetch_pr_link_list(
    pool: &PgPool,
    workspace_slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    limit: i64,
) -> Result<Vec<sqlx::postgres::PgRow>, Error> {
    let rows = sqlx::query(PR_LINK_LIST_SQL)
        .bind(workspace_slug)
        .bind(project_id)
        .bind(issue_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|error| DbError::new("pr-link-list", error))?;
    Ok(rows)
}

/// One PR link by pk ([`PR_LINK_BY_ID_SQL`]), or `None`.
pub async fn fetch_pr_link_by_id(
    pool: &PgPool,
    id: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Error> {
    let row = sqlx::query(PR_LINK_BY_ID_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|error| DbError::new("pr-link-by-id", error))?;
    Ok(row)
}

#[cfg(test)]
mod live {
    use sqlx::Row;

    use super::*;
    use crate::read::test_support::{pool, seed_world};

    async fn seed_member(pool: &PgPool, world: &crate::read::test_support::World) {
        sqlx::query(
            "INSERT INTO project_members (id, role, project_id, workspace_id, member_id, \
             view_props, default_props, sort_order, preferences, is_active, created_at, updated_at) \
             VALUES ($1, 20, $2, $3, $4, '{}', '{}', 65535, '{}', true, now(), now())",
        )
        .bind(Uuid::new_v4())
        .bind(world.project_id)
        .bind(world.workspace_id)
        .bind(world.user_id)
        .execute(pool)
        .await
        .expect("seed member");
    }

    async fn seed_link(
        pool: &PgPool,
        world: &crate::read::test_support::World,
        pr_number: i32,
        created_secs_ago: i32,
    ) -> Uuid {
        let id = Uuid::new_v4();
        // Fresh repo identity per seed: live rows are unique per
        // (`repo_owner`, `repo_name`, `pr_number`), and seeds persist
        // across runs.
        let owner = format!("o-{}", Uuid::new_v4().simple());
        sqlx::query(
            "INSERT INTO github_pull_request_links \
             (id, repo_owner, repo_name, pr_number, url, title, state, merged, draft, \
             issue_id, project_id, workspace_id, created_at, updated_at) \
             VALUES ($1, $2, 'r', $3, 'https://example.test/pr', 'T', 'open', false, false, \
             $4, $5, $6, now() - make_interval(secs => $7), now())",
        )
        .bind(id)
        .bind(&owner)
        .bind(pr_number)
        .bind(world.issue_id)
        .bind(world.project_id)
        .bind(world.workspace_id)
        .bind(created_secs_ago)
        .execute(pool)
        .await
        .expect("seed link");
        id
    }

    #[tokio::test]
    async fn pr_link_reads_hit_miss_and_limit() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        seed_member(&db, &world).await;
        let first = seed_link(&db, &world, 7, 60).await;
        let second = seed_link(&db, &world, 8, 0).await;
        // MCP list: newest first, sliced.
        let rows = fetch_pr_link_list(
            &db,
            &world.workspace_slug,
            world.project_id,
            world.issue_id,
            10,
        )
        .await
        .expect("list");
        assert_eq!(rows.len(), 2);
        let head: Uuid = rows[0].try_get("id").expect("id");
        assert_eq!(head, second);
        let one = fetch_pr_link_list(
            &db,
            &world.workspace_slug,
            world.project_id,
            world.issue_id,
            1,
        )
        .await
        .expect("limited");
        assert_eq!(one.len(), 1);
        // By-id re-read.
        let row = fetch_pr_link_by_id(&db, &first)
            .await
            .expect("by-id")
            .expect("row");
        let number: i32 = row.try_get("pr_number").expect("pr_number");
        assert_eq!(number, 7);
        let missing = fetch_pr_link_by_id(&db, &Uuid::new_v4())
            .await
            .expect("by-id miss");
        assert!(missing.is_none());
        // v1 detail scope: member-guarded hit, unknown pk miss.
        let row = fetch_pr_detail_row(
            &db,
            &world.workspace_slug,
            &world.project_id,
            &world.issue_id,
            &world.user_id,
            &first,
        )
        .await
        .expect("detail")
        .expect("row");
        let number: i32 = row.try_get("pr_number").expect("pr_number");
        assert_eq!(number, 7);
        let missing = fetch_pr_detail_row(
            &db,
            &world.workspace_slug,
            &world.project_id,
            &world.issue_id,
            &world.user_id,
            &Uuid::new_v4(),
        )
        .await
        .expect("detail miss");
        assert!(missing.is_none());
    }
}
