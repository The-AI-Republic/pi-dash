//! Identity reads: user, profile and file-asset lookups.
//!
//! Verbatim moves of the `pidash-api` handler helpers (OVL-05,
//! PIDASHCONV-1543); the handlers call these and map [`Error`](super::Error)
//! onto their denial type.
//!
//! SeaORM conversion (READ-1): reads go through entities, decode by name,
//! preserving joins, NULLs and soft-delete scopes exactly.

use chrono::{DateTime, Utc};
use pidash_db::entity::{file_assets, profiles, users, workspaces};
use sea_orm::{ColumnTrait, EntityTrait, FromQueryResult, JoinType, QueryFilter, QuerySelect};
use sqlx::PgPool;
use uuid::Uuid;

use super::{DbError, Error};

/// The `profiles` columns the settings branches read: `id` plus the
/// last-workspace FK (`app/views/user/...`, moved from
/// `api::app_workspace::handlers_user`).
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
pub struct ProfileRow {
    pub id: Uuid,
    pub last_workspace_id: Option<Uuid>,
}

/// `Profile.objects.get(user=...)` (`queries_user::profile_by_user_sql`
/// shape; `profiles` carries no `deleted_at`). A miss is
/// [`NotFound`](Error::NotFound).
pub async fn fetch_profile(pool: &PgPool, user_id: Uuid) -> Result<ProfileRow, Error> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    profiles::Entity::find()
        .filter(profiles::Column::UserId.eq(user_id))
        .select_only()
        .columns([profiles::Column::Id, profiles::Column::LastWorkspaceId])
        .into_model::<ProfileRow>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("fetch-profile", error))?
        .ok_or(Error::NotFound)
}

/// One `file_assets` row for `asset_url` rendering: the plain-manager
/// columns (forward-FK access runs through `_base_manager`, unscoped —
/// the `queries_user` bug-13 precedent). Moved from
/// `api::app_workspace::handlers_user`.
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    file_assets::Entity::find_by_id(asset_id)
        .select_only()
        .columns([
            file_assets::Column::Id,
            file_assets::Column::EntityType,
            file_assets::Column::WorkspaceId,
            file_assets::Column::ProjectId,
            file_assets::Column::IssueId,
        ])
        .into_model::<AssetRow>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("fetch-asset", error))?
        .ok_or(Error::NotFound)
}

/// Full `users` projection shared by `me`, `session`, and `user_detail`.
/// Every user column rides a LEFT JOIN somewhere, so all read `Option`
/// and the caller decides what a missing row means. Decode by column
/// name via SeaORM (`FromQueryResult` requires unique output names,
/// enforced by the `column_as` aliases below).
/// Moved from `api::license::handlers_admin`.
#[derive(Debug, Clone, FromQueryResult)]
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
/// Kept for source compatibility (space/issues docs reference it); the
/// SeaORM builder below is authoritative.
pub const USER_SHAPE_SELECT: &str = "u.id AS id, u.first_name AS first_name, u.last_name AS last_name, u.avatar AS avatar, u.avatar_asset_id AS avatar_asset_id, fa.id AS avatar_asset_row, fa.entity_type AS avatar_asset_entity, fa.workspace_id AS avatar_asset_workspace, fa.project_id AS avatar_asset_project, fa.issue_id AS avatar_asset_issue, w.slug AS avatar_asset_workspace_slug, u.cover_image AS cover_image, u.date_joined AS date_joined, u.display_name AS display_name, u.email AS email, u.is_active AS is_active, u.is_bot AS is_bot, u.is_email_verified AS is_email_verified, u.user_timezone AS user_timezone, u.username AS username, u.is_password_autoset AS is_password_autoset, u.last_login_medium AS last_login_medium";

/// SeaORM select for [`fetch_user`]: `users` + LEFT JOIN `file_assets`
/// (unscoped) + LEFT JOIN `workspaces` (unscoped), filtered by id.
/// Both joins are deliberately unscoped: the forward-FK descriptor
/// reads through `_base_manager`, so soft-deleted assets/workspaces
/// resolve.
fn fetch_user_query(user_id: Uuid) -> sea_orm::Select<users::Entity> {
    let to_fa = users::Entity::belongs_to(file_assets::Entity)
        .from(users::Column::AvatarAssetId)
        .to(file_assets::Column::Id)
        .into();
    let fa_to_w = file_assets::Entity::belongs_to(workspaces::Entity)
        .from(file_assets::Column::WorkspaceId)
        .to(workspaces::Column::Id)
        .into();
    users::Entity::find()
        .select_only()
        .columns([
            users::Column::Id,
            users::Column::FirstName,
            users::Column::LastName,
            users::Column::Avatar,
            users::Column::AvatarAssetId,
        ])
        .column_as(file_assets::Column::Id, "avatar_asset_row")
        .column_as(file_assets::Column::EntityType, "avatar_asset_entity")
        .column_as(file_assets::Column::WorkspaceId, "avatar_asset_workspace")
        .column_as(file_assets::Column::ProjectId, "avatar_asset_project")
        .column_as(file_assets::Column::IssueId, "avatar_asset_issue")
        .column_as(workspaces::Column::Slug, "avatar_asset_workspace_slug")
        .column_as(users::Column::CoverImage, "cover_image")
        .column_as(users::Column::DateJoined, "date_joined")
        .column_as(users::Column::DisplayName, "display_name")
        .column_as(users::Column::Email, "email")
        .column_as(users::Column::IsActive, "is_active")
        .column_as(users::Column::IsBot, "is_bot")
        .column_as(users::Column::IsEmailVerified, "is_email_verified")
        .column_as(users::Column::UserTimezone, "user_timezone")
        .column_as(users::Column::Username, "username")
        .column_as(users::Column::IsPasswordAutoset, "is_password_autoset")
        .column_as(users::Column::LastLoginMedium, "last_login_medium")
        .join(JoinType::LeftJoin, to_fa)
        .join(JoinType::LeftJoin, fa_to_w)
        .filter(users::Column::Id.eq(user_id))
}

/// Fetch one user with its avatar-asset joins. Both joins are
/// deliberately unscoped: the forward-FK descriptor reads through
/// `_base_manager`, so soft-deleted assets/workspaces resolve.
pub async fn fetch_user(pool: &PgPool, user_id: &Uuid) -> Result<Option<UserShape>, Error> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<UserShape> = fetch_user_query(*user_id)
        .into_model::<UserShape>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("fetch-user", error))?;
    // A present row always carries its PK; its absence means the caller
    // joined a null FK (`user_detail: null`, `user: null`).
    Ok(row.filter(|shape| shape.id.is_some()))
}

#[cfg(test)]
mod tests {
    use sea_orm::DbBackend;
    use sea_orm::QueryTrait;

    use super::*;

    #[test]
    fn fetch_profile_query_scopes_by_user_without_deleted_guard() {
        let sql = profiles::Entity::find()
            .filter(profiles::Column::UserId.eq(Uuid::nil()))
            .select_only()
            .columns([profiles::Column::Id, profiles::Column::LastWorkspaceId])
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#""profiles"."user_id" = "#), "{sql}");
        assert!(sql.contains(r#""profiles"."last_workspace_id""#), "{sql}");
        assert!(!sql.contains("deleted_at"), "{sql}");
    }

    #[test]
    fn fetch_asset_query_is_unscoped() {
        let sql = file_assets::Entity::find_by_id(Uuid::nil())
            .select_only()
            .columns([
                file_assets::Column::Id,
                file_assets::Column::EntityType,
                file_assets::Column::WorkspaceId,
                file_assets::Column::ProjectId,
                file_assets::Column::IssueId,
            ])
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("FROM"), "{sql}");
        assert!(!sql.contains("deleted_at"), "{sql}");
    }

    #[test]
    fn fetch_user_query_joins_unscoped_and_unique_names() {
        let sql = fetch_user_query(Uuid::nil())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("LEFT JOIN"), "{sql}");
        assert!(sql.contains(r#""file_assets""#), "{sql}");
        assert!(sql.contains(r#""workspaces""#), "{sql}");
        assert!(sql.contains(r#""users"."id" = "#), "{sql}");
        assert!(!sql.contains("deleted_at"), "{sql}");
        // Unique output names via aliases.
        for alias in [
            "avatar_asset_row",
            "avatar_asset_entity",
            "avatar_asset_workspace",
            "avatar_asset_project",
            "avatar_asset_issue",
            "avatar_asset_workspace_slug",
        ] {
            assert!(sql.contains(alias), "{sql}");
        }
    }
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
