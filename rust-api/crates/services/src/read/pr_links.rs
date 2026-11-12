//! GitHub PR-link list/detail reads.
//!
//! Verbatim moves of the `pidash-api` handler SQL (OVL-05 pattern,
//! PIDASHCONV-1661) plus the MCP tool querysets (Rule 5: queries on OSS
//! tables live here). The v1 handlers call these and map
//! [`Error`](super::Error) onto their denial type.
//!
//! SeaORM conversion (READ-1, Group B part 2): the three PgRow
//! fetchers return the typed [`PrLinkDbRow`] (the `DateTime<Utc>`
//! instant the old by-name `PgRow` read produced), built with the
//! same join-span shape PR #2957 proved in the v1 handlers.

use chrono::{DateTime, Utc};
use pidash_db::entity::{github_pull_request_links, project_members, projects, workspaces};
use sea_orm::{
    ColumnTrait, EntityTrait, FromQueryResult, JoinType, QueryFilter, QueryOrder, QuerySelect,
    RelationTrait,
};
use sqlx::PgPool;
use uuid::Uuid;

use super::{DbError, Error};

/// One PR-link row: the full field set of `github_pull_request_links`
/// (the old `PR_LIST_SQL`/`PR_DETAIL_SQL` shape; the MCP list/by-id
/// reads select the same rows). Datetimes are the `DateTime<Utc>`
/// instant the old `PgRow` read produced.
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
pub struct PrLinkDbRow {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub id: Uuid,
    pub repo_owner: String,
    pub repo_name: String,
    pub pr_number: i32,
    pub url: String,
    pub title: String,
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    pub pr_updated_at: Option<DateTime<Utc>>,
    pub created_by_id: Option<Uuid>,
    pub issue_id: Uuid,
    pub project_id: Uuid,
    pub updated_by_id: Option<Uuid>,
    pub workspace_id: Uuid,
}

/// `GithubPullRequestLinkViewSet.get_queryset` scope (`github_pr.py:37-48`,
/// fixture `pr_list_queryset`): `INNER JOIN`s to `workspaces`, `projects`
/// and the project's `project_members` rows, the slug / project / issue
/// predicates, and an *active* membership row for the caller (no
/// `deleted_at` guard on the membership; the SQL has none either).
fn pr_link_scope(
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    actor_id: &Uuid,
) -> sea_orm::Select<github_pull_request_links::Entity> {
    github_pull_request_links::Entity::find()
        .join(
            JoinType::InnerJoin,
            github_pull_request_links::Relation::Workspaces.def(),
        )
        .join(
            JoinType::InnerJoin,
            github_pull_request_links::Relation::Projects.def(),
        )
        .join(
            JoinType::InnerJoin,
            projects::Relation::ProjectMembers.def(),
        )
        .filter(github_pull_request_links::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(github_pull_request_links::Column::ProjectId.eq(*project_id))
        .filter(github_pull_request_links::Column::IssueId.eq(*issue_id))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::MemberId.eq(*actor_id))
}

/// PR-link detail scope + pk (`github_pr.py:97-100`): `Meta.ordering`
/// then the `.get()`'s single row — no archived guard, no `DISTINCT`.
fn pr_link_detail_select(
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    actor_id: &Uuid,
    pk: &Uuid,
) -> sea_orm::Select<github_pull_request_links::Entity> {
    pr_link_scope(slug, project_id, issue_id, actor_id)
        .filter(github_pull_request_links::Column::Id.eq(*pk))
        .order_by_desc(github_pull_request_links::Column::CreatedAt)
        .limit(1)
}

/// One PR link in the v1 detail scope, or `None` (the DELETE
/// path's 404 branch).
pub async fn fetch_pr_detail_row(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    actor_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<PrLinkDbRow>, Error> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = pr_link_detail_select(slug, project_id, issue_id, actor_id, pk)
        .into_model::<PrLinkDbRow>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("pr detail fetch", error))?;
    Ok(row)
}

/// The MCP tool scope ([`fetch_pr_link_list`]): the workspace join, the
/// soft-delete guard, and `-created_at` with the `[:limit]` slice.
fn pr_link_mcp_list_select(
    workspace_slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    limit: u64,
) -> sea_orm::Select<github_pull_request_links::Entity> {
    github_pull_request_links::Entity::find()
        .join(
            JoinType::InnerJoin,
            github_pull_request_links::Relation::Workspaces.def(),
        )
        .filter(github_pull_request_links::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(workspace_slug))
        .filter(github_pull_request_links::Column::ProjectId.eq(project_id))
        .filter(github_pull_request_links::Column::IssueId.eq(issue_id))
        .order_by_desc(github_pull_request_links::Column::CreatedAt)
        .limit(limit)
}

/// Full re-read scope for serialization: one live row by pk.
fn pr_link_by_id_select(id: &Uuid) -> sea_orm::Select<github_pull_request_links::Entity> {
    github_pull_request_links::Entity::find_by_id(*id)
        .filter(github_pull_request_links::Column::DeletedAt.is_null())
}

/// `GithubPullRequestLink.objects.filter(...).order_by("-created_at")`
/// (`pi_dash_cloud/mcp/tools.py:1258-1262`): base-manager soft-delete
/// scope only (the FK traversal carries no related-manager guards),
/// `-created_at` order, `[:limit]` slice. The caller checked membership
/// before calling. (Moved from the MCP read tools by PIDASHCONV-1661.)
pub async fn fetch_pr_link_list(
    pool: &PgPool,
    workspace_slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    limit: i64,
) -> Result<Vec<PrLinkDbRow>, Error> {
    // Django slices `[:limit]` and raises on a negative limit (the old
    // driver also errored); keep the failure instead of clamping.
    if limit < 0 {
        return Err(Error::Failed("pr-link-list"));
    }
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = pr_link_mcp_list_select(
        workspace_slug,
        project_id,
        issue_id,
        u64::try_from(limit).unwrap_or(u64::MAX),
    )
    .into_model::<PrLinkDbRow>()
    .all(&db)
    .await
    .map_err(|error| DbError::from_orm("pr-link-list", error))?;
    Ok(rows)
}

/// Full re-read of one link for serialization (attach's
/// `_serialize_model(serializer, link)` over the model instance):
/// soft-deleted rows are invisible, or `None`. (Moved from the MCP read
/// tools by PIDASHCONV-1661.)
pub async fn fetch_pr_link_by_id(pool: &PgPool, id: &Uuid) -> Result<Option<PrLinkDbRow>, Error> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = pr_link_by_id_select(id)
        .into_model::<PrLinkDbRow>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("pr-link-by-id", error))?;
    Ok(row)
}

#[cfg(test)]
mod statement {
    use sea_orm::{DbBackend, QueryTrait};

    use super::*;

    #[test]
    fn pr_link_detail_scope_pins_joins_and_membership_guard() {
        let sql = pr_link_scope("acme", &Uuid::nil(), &Uuid::nil(), &Uuid::nil())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            sql.contains(r#"INNER JOIN "workspaces" ON "github_pull_request_links"."workspace_id" = "workspaces"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "projects" ON "github_pull_request_links"."project_id" = "projects"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "project_members" ON "projects"."id" = "project_members"."project_id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""github_pull_request_links"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(sql.contains(r#""workspaces"."slug" = 'acme'"#), "{sql}");
        assert!(
            sql.contains(r#""project_members"."is_active" = TRUE"#),
            "{sql}"
        );
        // No `deleted_at` guard on the membership row (Q2 ported bug 7).
        assert!(!sql.contains(r#""project_members"."deleted_at""#), "{sql}");
    }

    #[test]
    fn pr_link_detail_pins_scope_pk_order_and_limit() {
        let sql = pr_link_detail_select(
            "acme",
            &Uuid::nil(),
            &Uuid::nil(),
            &Uuid::nil(),
            &Uuid::nil(),
        )
        .build(DbBackend::Postgres)
        .to_string();
        assert!(sql.starts_with("SELECT "), "{sql}");
        assert!(!sql.contains("DISTINCT"), "{sql}");
        assert!(
            sql.contains(r#""github_pull_request_links"."id" = "#),
            "{sql}"
        );
        // No archived guard on the detail scope.
        assert!(
            !sql.contains(r#""projects"."archived_at" IS NULL"#),
            "{sql}"
        );
        assert!(
            sql.ends_with(r#" ORDER BY "github_pull_request_links"."created_at" DESC LIMIT 1"#),
            "{sql}"
        );
    }

    #[test]
    fn pr_link_mcp_list_pins_workspace_scope_and_order() {
        let sql = pr_link_mcp_list_select("acme", Uuid::nil(), Uuid::nil(), 10)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.starts_with("SELECT "), "{sql}");
        assert!(!sql.contains("DISTINCT"), "{sql}");
        assert!(
            sql.contains(r#"INNER JOIN "workspaces" ON "github_pull_request_links"."workspace_id" = "workspaces"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""github_pull_request_links"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(sql.contains(r#""workspaces"."slug" = 'acme'"#), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY "github_pull_request_links"."created_at" DESC LIMIT 10"#),
            "{sql}"
        );
    }

    #[test]
    fn pr_link_by_id_pins_pk_and_soft_delete_scope() {
        let sql = pr_link_by_id_select(&Uuid::nil())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            sql.contains(r#""github_pull_request_links"."id" = "#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""github_pull_request_links"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(!sql.contains("JOIN"), "{sql}");
    }
}

#[cfg(test)]
mod live {
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
        assert_eq!(rows[0].id, second);
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
        // MCP list with a negative limit errors like Django's slice.
        let negative = fetch_pr_link_list(
            &db,
            &world.workspace_slug,
            world.project_id,
            world.issue_id,
            -1,
        )
        .await;
        assert!(negative.is_err());
        // By-id re-read.
        let row = fetch_pr_link_by_id(&db, &first)
            .await
            .expect("by-id")
            .expect("row");
        assert_eq!(row.pr_number, 7);
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
        assert_eq!(row.pr_number, 7);
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
