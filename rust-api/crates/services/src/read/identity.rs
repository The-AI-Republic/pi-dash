//! Identity reads: user, profile and file-asset lookups.
//!
//! Verbatim moves of the `pidash-api` handler helpers (OVL-05,
//! PIDASHCONV-1543); the handlers call these and map [`Error`](super::Error)
//! onto their denial type.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::{DbError, Error};

/// The `profiles` columns the settings branches read: `id` plus the
/// last-workspace FK (`app/views/user/...`, moved from
/// `api::app_workspace::handlers_user`).
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct ProfileRow {
    pub id: Uuid,
    pub last_workspace_id: Option<Uuid>,
}

/// `Profile.objects.get(user=...)` (`queries_user::profile_by_user_sql`
/// shape; `profiles` carries no `deleted_at`). A miss is
/// [`NotFound`](Error::NotFound).
pub async fn fetch_profile(pool: &PgPool, user_id: Uuid) -> Result<ProfileRow, Error> {
    sqlx::query_as("SELECT id, last_workspace_id FROM profiles WHERE profiles.user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| DbError::new("fetch-profile", error))?
        .ok_or(Error::NotFound)
}

/// One `file_assets` row for `asset_url` rendering: the plain-manager
/// columns (forward-FK access runs through `_base_manager`, unscoped —
/// the `queries_user` bug-13 precedent). Moved from
/// `api::app_workspace::handlers_user`.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct AssetRow {
    pub id: Uuid,
    pub entity_type: Option<String>,
    pub workspace_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub issue_id: Option<Uuid>,
}

/// Unscoped asset fetch for one set FK. A dangling FK raises through
/// the `ObjectDoesNotExist` branch (404), like Django's
/// `RelatedObjectDoesNotExist`.
pub async fn fetch_asset(pool: &PgPool, asset_id: Uuid) -> Result<AssetRow, Error> {
    sqlx::query_as(
        "SELECT id, entity_type, workspace_id, project_id, issue_id FROM file_assets WHERE file_assets.id = $1",
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| DbError::new("fetch-asset", error))?
    .ok_or(Error::NotFound)
}

/// Full `users` projection shared by `me`, `session`, and `user_detail`.
/// Every user column rides a LEFT JOIN somewhere, so all read `Option`
/// and the caller decides what a missing row means. `sqlx::FromRow` maps
/// by column name (wide selects exceed the 16-tuple `FromRow` impls), so
/// the SELECT aliases below must match these field names exactly.
/// Moved from `api::license::handlers_admin`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserShape {
    pub id: Option<Uuid>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub avatar: Option<String>,
    pub avatar_asset_id: Option<Uuid>,
    pub avatar_asset_row: Option<Uuid>,
    pub avatar_asset_entity: Option<String>,
    pub avatar_asset_workspace: Option<Uuid>,
    pub avatar_asset_project: Option<Uuid>,
    pub avatar_asset_issue: Option<Uuid>,
    pub avatar_asset_workspace_slug: Option<String>,
    pub cover_image: Option<String>,
    pub date_joined: Option<DateTime<Utc>>,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub is_active: Option<bool>,
    pub is_bot: Option<bool>,
    pub is_email_verified: Option<bool>,
    pub user_timezone: Option<String>,
    pub username: Option<String>,
    pub is_password_autoset: Option<bool>,
    pub last_login_medium: Option<String>,
}

/// The [`UserShape`] column list, in field order.
pub const USER_SHAPE_SELECT: &str = "u.id AS id, u.first_name AS first_name, u.last_name AS last_name, u.avatar AS avatar, u.avatar_asset_id AS avatar_asset_id, fa.id AS avatar_asset_row, fa.entity_type AS avatar_asset_entity, fa.workspace_id AS avatar_asset_workspace, fa.project_id AS avatar_asset_project, fa.issue_id AS avatar_asset_issue, w.slug AS avatar_asset_workspace_slug, u.cover_image AS cover_image, u.date_joined AS date_joined, u.display_name AS display_name, u.email AS email, u.is_active AS is_active, u.is_bot AS is_bot, u.is_email_verified AS is_email_verified, u.user_timezone AS user_timezone, u.username AS username, u.is_password_autoset AS is_password_autoset, u.last_login_medium AS last_login_medium";

/// Fetch one user with its avatar-asset joins. Both joins are
/// deliberately unscoped: the forward-FK descriptor reads through
/// `_base_manager`, so soft-deleted assets/workspaces resolve.
pub async fn fetch_user(pool: &PgPool, user_id: &Uuid) -> Result<Option<UserShape>, Error> {
    let row: Option<UserShape> = sqlx::query_as(&format!(
        "SELECT {USER_SHAPE_SELECT} FROM users u LEFT JOIN file_assets fa ON fa.id = u.avatar_asset_id LEFT JOIN workspaces w ON w.id = fa.workspace_id WHERE u.id = $1"
    ))
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| DbError::new("fetch-user", error))?;
    // A present row always carries its PK; its absence means the caller
    // joined a null FK (`user_detail: null`, `user: null`).
    Ok(row.filter(|shape| shape.id.is_some()))
}

#[cfg(test)]
mod live {
    use uuid::Uuid;

    use super::*;
    use crate::read::test_support::{pool, seed_world};

    #[tokio::test]
    async fn fetch_profile_hit_and_miss() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let hit = fetch_profile(&db, world.user_id).await.expect("hit");
        assert_eq!(hit.id, world.profile_id);
        assert!(hit.last_workspace_id.is_none());
        let err = fetch_profile(&db, Uuid::new_v4()).await.expect_err("miss");
        assert!(matches!(err, Error::NotFound));
    }

    #[tokio::test]
    async fn fetch_asset_hit_and_miss() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let hit = fetch_asset(&db, world.asset_id).await.expect("hit");
        assert_eq!(hit.id, world.asset_id);
        assert_eq!(hit.entity_type.as_deref(), Some("USER_AVATAR"));
        let err = fetch_asset(&db, Uuid::new_v4()).await.expect_err("miss");
        assert!(matches!(err, Error::NotFound));
    }

    #[tokio::test]
    async fn fetch_user_hit_miss_and_unscoped_avatar() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let hit = fetch_user(&db, &world.user_id)
            .await
            .expect("hit")
            .expect("some");
        assert_eq!(hit.id, Some(world.user_id));
        assert_eq!(hit.email.as_deref(), Some(world.user_email.as_str()));
        assert!(fetch_user(&db, &Uuid::new_v4())
            .await
            .expect("miss is None")
            .is_none());
        // Unscoped joins (bug-13 precedent): point the avatar FK at the
        // seeded asset, soft-delete the asset, and it still resolves.
        sqlx::query("UPDATE users SET avatar_asset_id = $1 WHERE id = $2")
            .bind(world.asset_id)
            .bind(world.user_id)
            .execute(&db)
            .await
            .expect("point avatar");
        sqlx::query("UPDATE file_assets SET deleted_at = now() WHERE id = $1")
            .bind(world.asset_id)
            .execute(&db)
            .await
            .expect("soft-delete asset");
        let resolved = fetch_user(&db, &world.user_id)
            .await
            .expect("resolves")
            .expect("some");
        assert_eq!(resolved.avatar_asset_row, Some(world.asset_id));
    }
}
