//! Hand-written `ActiveModelBehavior` for `users` (ORM-07, PIDASHCONV-1525).
//!
//! Regen-safe by construction: `sea-orm-cli` rewrites `users.rs` on every
//! regeneration (restoring the empty impl there, which the regen checklist
//! in `super`'s module doc deletes), while this file survives untouched.
//! Forgetting that deletion fails the build with duplicate impls — loud,
//! not a silently lost hook.

use sea_orm::entity::prelude::*;
use sea_orm::ActiveValue::Set;

use super::user_notification_preferences;
use super::users::{ActiveModel, Model};

/// `uuid.uuid4()` for hook-created rows. `pidash-db`'s `uuid` has no `v4`
/// feature (the `license/queries` precedent: generation belongs to the
/// caller), but a hook has no caller to take an id from, so the
/// version/variant bits are set by hand over 122 random bits — the same
/// distribution (the `orchestration/workpad` precedent).
fn new_v4() -> Uuid {
    let mut bytes: [u8; 16] = rand::random();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    /// `post_save` receiver `create_user_notification`
    /// (`db/models/user.py:307-320`): one preferences row — all five
    /// flags true, NULL audit and scope columns — on every non-bot
    /// create. The stamps reuse the user row's: every signup path
    /// freezes one `now` across its user/prefs/profile writes, and the
    /// hook is what preserves that. Updates never fire (`created` is
    /// false) and bots never get a row (`not instance.is_bot`).
    async fn after_save<C>(model: Model, db: &C, insert: bool) -> Result<Model, DbErr>
    where
        C: ConnectionTrait,
    {
        if insert && !model.is_bot {
            let prefs = user_notification_preferences::ActiveModel {
                id: Set(new_v4()),
                created_at: Set(model.created_at),
                updated_at: Set(model.updated_at),
                created_by_id: Set(None),
                updated_by_id: Set(None),
                deleted_at: Set(None),
                user_id: Set(model.id),
                workspace_id: Set(None),
                project_id: Set(None),
                property_change: Set(true),
                state_change: Set(true),
                comment: Set(true),
                mention: Set(true),
                issue_completed: Set(true),
            };
            // Django fires no receivers on the preferences row itself,
            // so the non-hook-firing insert is the faithful call.
            user_notification_preferences::Entity::insert(prefs)
                .exec(db)
                .await?;
        }
        Ok(model)
    }
}

#[cfg(test)]
mod tests {
    use super::super::users::Entity as UserEntity;
    use super::*;
    use sea_orm::{DatabaseConnection, EntityTrait};

    /// Live-db hook proof: a non-bot entity insert gains the preferences
    /// row with the user row's stamps, a bot gains none, and an update
    /// fires nothing more. Skips when `DATABASE_URL` is unset, like
    /// every other live-db test in the workspace.
    async fn live_db() -> Option<DatabaseConnection> {
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return None;
            }
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        Some(crate::sea_orm_connection(&pool))
    }

    fn user_row(id: Uuid, now: DateTimeWithTimeZone, is_bot: bool) -> ActiveModel {
        ActiveModel {
            id: Set(id),
            password: Set(String::new()),
            last_login: Set(None),
            username: Set(format!("hookprobe-{id}")),
            mobile_number: Set(None),
            email: Set(Some(format!("hookprobe-{id}@example.test"))),
            display_name: Set("probe".to_owned()),
            first_name: Set(String::new()),
            last_name: Set(String::new()),
            avatar: Set(String::new()),
            avatar_asset_id: Set(None),
            cover_image: Set(None),
            cover_image_asset_id: Set(None),
            date_joined: Set(now),
            created_at: Set(now),
            updated_at: Set(now),
            last_location: Set(String::new()),
            created_location: Set(String::new()),
            is_superuser: Set(false),
            is_managed: Set(false),
            is_password_expired: Set(false),
            is_active: Set(true),
            is_staff: Set(false),
            is_email_verified: Set(false),
            is_password_autoset: Set(false),
            token: Set(String::new()),
            user_timezone: Set("UTC".to_owned()),
            last_active: Set(None),
            last_login_time: Set(None),
            last_logout_time: Set(None),
            last_login_ip: Set(String::new()),
            last_logout_ip: Set(String::new()),
            last_login_medium: Set("email".to_owned()),
            last_login_uagent: Set(String::new()),
            token_updated_at: Set(None),
            is_bot: Set(is_bot),
            bot_type: Set(None),
            is_email_valid: Set(false),
            masked_at: Set(None),
            is_password_reset_required: Set(false),
        }
    }

    #[tokio::test]
    async fn after_save_creates_prefs_for_non_bot_only() {
        let Some(db) = live_db().await else { return };
        let now = chrono::Utc::now().fixed_offset();
        let human = new_v4();
        let bot = new_v4();
        let saved = user_row(human, now, false)
            .insert(&db)
            .await
            .expect("human user inserts");
        assert_eq!(saved.id, human);
        let prefs = user_notification_preferences::Entity::find()
            .filter(user_notification_preferences::Column::UserId.eq(human))
            .all(&db)
            .await
            .expect("prefs read back");
        assert_eq!(prefs.len(), 1, "one prefs row for the non-bot user");
        let row = &prefs[0];
        // The hook reuses the user row's stamps (Postgres truncates both
        // to microseconds, so the read-back row is the comparison).
        let user_back = UserEntity::find_by_id(human)
            .one(&db)
            .await
            .expect("user reads back")
            .expect("user row");
        assert_eq!(row.created_at, user_back.created_at);
        assert_eq!(row.updated_at, user_back.updated_at);
        assert!(
            row.property_change
                && row.state_change
                && row.comment
                && row.mention
                && row.issue_completed,
            "all five flags true"
        );
        assert!(row.created_by_id.is_none() && row.updated_by_id.is_none());
        assert!(row.workspace_id.is_none() && row.project_id.is_none());
        user_row(bot, now, true)
            .insert(&db)
            .await
            .expect("bot user inserts");
        let bot_prefs = user_notification_preferences::Entity::find()
            .filter(user_notification_preferences::Column::UserId.eq(bot))
            .all(&db)
            .await
            .expect("bot prefs read back");
        assert!(bot_prefs.is_empty(), "no prefs row for the bot user");
        // An update fires nothing more: still exactly one prefs row.
        let mut human_update: ActiveModel = saved.into();
        human_update.first_name = Set("changed".to_owned());
        human_update.update(&db).await.expect("update runs");
        let again = user_notification_preferences::Entity::find()
            .filter(user_notification_preferences::Column::UserId.eq(human))
            .all(&db)
            .await
            .expect("prefs re-read");
        assert_eq!(again.len(), 1, "updates create no second prefs row");
        user_notification_preferences::Entity::delete_many()
            .filter(user_notification_preferences::Column::UserId.is_in([human, bot]))
            .exec(&db)
            .await
            .expect("prefs cleanup");
        UserEntity::delete_many()
            .filter(super::super::users::Column::Id.is_in([human, bot]))
            .exec(&db)
            .await
            .expect("user cleanup");
    }
}
