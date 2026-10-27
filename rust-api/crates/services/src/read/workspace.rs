//! Workspace / project lookups.
//!
//! Verbatim moves of the `pidash-api` handler helpers (OVL-05,
//! PIDASHCONV-1543); the handlers call these and map [`Error`](super::Error)
//! onto their denial type.

use sqlx::PgPool;
use uuid::Uuid;

use super::{DbError, Error};

/// `Workspace.objects.get(slug=slug)`: the id/slug pair, default-manager
/// scoped. A miss is [`NotFound`](Error::NotFound). Moved from
/// `api::app_issues::handlers_labels_attachments`.
pub async fn workspace_row(pool: &PgPool, slug: &str) -> Result<(Uuid, String), Error> {
    let row: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, slug FROM workspaces WHERE slug = $1 AND deleted_at IS NULL")
            .bind(slug)
            .fetch_optional(pool)
            .await
            .map_err(|error| DbError::new("workspace-row", error))?;
    row.ok_or(Error::NotFound)
}

/// The workspace slug for one optional workspace id (the file-asset URL
/// builders). `None` in is `None` out — no query. Consolidates three
/// identical nested copies in `v1_work_items` (`handlers_core`,
/// `handlers_social`, `handlers_activity`).
pub async fn workspace_slug(
    pool: &PgPool,
    workspace_id: Option<Uuid>,
) -> Result<Option<String>, Error> {
    match workspace_id {
        Some(id) => Ok(
            sqlx::query_scalar(r#"SELECT "slug" FROM "workspaces" WHERE "id" = $1"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(|error| DbError::new("file-asset-slug", error))?
                .flatten(),
        ),
        None => Ok(None),
    }
}

/// The workspace that owns a project: `PROJECT_WORKSPACE_LOOKUP_SQL`
/// plus the default-manager `deleted_at` scope (Python's `self.project`
/// traversal misses soft-deleted projects, so the lookup must too). A
/// miss is [`NotFound`](Error::NotFound). Consolidates the twin
/// `PrStore::workspace_of` helpers in `v1_work_items::handlers_pr_links`
/// and `app_issues::handlers_relations`.
pub async fn workspace_of(pool: &PgPool, project_id: &Uuid) -> Result<Uuid, Error> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| DbError::new("project workspace", error))?;
    row.map(|row| row.0).ok_or(Error::NotFound)
}

#[cfg(test)]
mod live {
    use uuid::Uuid;

    use super::*;
    use crate::read::test_support::{pool, seed_world};

    #[tokio::test]
    async fn workspace_row_hit_miss_and_soft_deleted() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let (id, slug) = workspace_row(&db, &world.workspace_slug)
            .await
            .expect("hit");
        assert_eq!(id, world.workspace_id);
        assert_eq!(slug, world.workspace_slug);
        let err = workspace_row(&db, "no-such-slug").await.expect_err("miss");
        assert!(matches!(err, Error::NotFound));
        sqlx::query("UPDATE workspaces SET deleted_at = now() WHERE id = $1")
            .bind(world.workspace_id)
            .execute(&db)
            .await
            .expect("soft-delete");
        let err = workspace_row(&db, &world.workspace_slug)
            .await
            .expect_err("soft-deleted misses");
        assert!(matches!(err, Error::NotFound));
    }

    #[tokio::test]
    async fn workspace_slug_none_missing_and_hit() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        assert!(workspace_slug(&db, None).await.expect("none").is_none());
        assert!(workspace_slug(&db, Some(Uuid::new_v4()))
            .await
            .expect("missing")
            .is_none());
        assert_eq!(
            workspace_slug(&db, Some(world.workspace_id))
                .await
                .expect("hit"),
            Some(world.workspace_slug)
        );
    }

    #[tokio::test]
    async fn workspace_of_hit_miss_and_soft_deleted_project() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        assert_eq!(
            workspace_of(&db, &world.project_id).await.expect("hit"),
            world.workspace_id
        );
        let err = workspace_of(&db, &Uuid::new_v4()).await.expect_err("miss");
        assert!(matches!(err, Error::NotFound));
        sqlx::query("UPDATE projects SET deleted_at = now() WHERE id = $1")
            .bind(world.project_id)
            .execute(&db)
            .await
            .expect("soft-delete");
        let err = workspace_of(&db, &world.project_id)
            .await
            .expect_err("soft-deleted misses");
        assert!(matches!(err, Error::NotFound));
    }
}
