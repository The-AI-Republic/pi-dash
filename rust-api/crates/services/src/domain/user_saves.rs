//! App-workspace user-endpoint saves (ORM migration, APPWS-7).
//!
//! SeaORM twins of the sqlx templates the `api::app_workspace::handlers_user`
//! handlers used to splice with `positional()`:
//!
//! * [`save_email`] — `identity::email_save_sql` plus the `User.save()`
//!   side-effect columns.
//! * [`deactivate_user_save`] — `identity::deactivate_user_save_sql` plus
//!   the side-effect columns.
//! * [`deactivate_profile_reset`] —
//!   `identity::deactivate_profile_reset_sql`.
//! * [`patch_profile_flag`] — `identity::onboard_patch_sql` /
//!   `identity::tour_patch_sql`.
//! * [`bulk_deactivate_workspace_members`] —
//!   `workspaces::members::workspace_bulk_deactivate_sql` (the
//!   `projects::members::deactivate_many` twin over
//!   `workspace_members`).
//!
//! These live here (design §4 covered tables — `users`, `profiles`,
//! `workspace_members`) so the write guard stays green; the sqlx
//! templates in `identity` / `workspaces::members` keep their text-pin
//! tests for the remaining callers.
//!
//! Updates run as bare entity statements (no `ActiveModel` hooks —
//! `update_many` never fires `after_save`, matching the hand-written
//! `UPDATE` exactly). Errors stay [`sqlx::Error`] (through
//! [`pidash_db::orm_tx::into_sqlx`]), so the handlers' SQLSTATE
//! classification is unchanged.

use chrono::{DateTime, Utc};
use pidash_db::entity::{profiles, users, workspace_members};
use pidash_db::orm_tx::into_sqlx;
use sea_orm::entity::prelude::DateTimeWithTimeZone;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

/// The `User.save()` side-effect columns shared by the email and
/// deactivation saves (`db/models/user.py:169-187`, derived
/// handler-side by `models_user::user`).
pub struct UserSaveFields<'a> {
    pub display_name: &'a str,
    pub token: &'a str,
    pub token_updated_at: Option<DateTime<Utc>>,
    pub is_staff: bool,
}

/// `PATCH users/me/email/` save: email + `is_email_verified = False` +
/// `updated_at`, plus the save-derived display name, token, token stamp
/// and staff escalation.
pub async fn save_email(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    email: &str,
    updated_at: DateTime<Utc>,
    derived: &UserSaveFields<'_>,
) -> Result<(), sqlx::Error> {
    users::Entity::update_many()
        .col_expr(users::Column::Email, Expr::value(email))
        .col_expr(users::Column::IsEmailVerified, Expr::value(false))
        .col_expr(
            users::Column::UpdatedAt,
            Expr::value(DateTimeWithTimeZone::from(updated_at)),
        )
        .col_expr(
            users::Column::DisplayName,
            Expr::value(derived.display_name),
        )
        .col_expr(users::Column::Token, Expr::value(derived.token))
        .col_expr(
            users::Column::TokenUpdatedAt,
            Expr::value(derived.token_updated_at.map(DateTimeWithTimeZone::from)),
        )
        .col_expr(users::Column::IsStaff, Expr::value(derived.is_staff))
        .filter(users::Column::Id.eq(user_id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// `DELETE users/me/` user save: password randomize + autoset,
/// inactive, logout stamps, plus the save-derived columns.
pub async fn deactivate_user_save(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    email: &str,
    password_hash: &str,
    last_logout_ip: Option<&str>,
    now: DateTime<Utc>,
    derived: &UserSaveFields<'_>,
) -> Result<(), sqlx::Error> {
    users::Entity::update_many()
        .col_expr(users::Column::Password, Expr::value(password_hash))
        .col_expr(users::Column::IsPasswordAutoset, Expr::value(true))
        .col_expr(users::Column::IsActive, Expr::value(false))
        .col_expr(users::Column::LastLogoutIp, Expr::value(last_logout_ip))
        .col_expr(
            users::Column::LastLogoutTime,
            Expr::value(DateTimeWithTimeZone::from(now)),
        )
        .col_expr(
            users::Column::UpdatedAt,
            Expr::value(DateTimeWithTimeZone::from(now)),
        )
        .col_expr(users::Column::Email, Expr::value(email))
        .col_expr(
            users::Column::DisplayName,
            Expr::value(derived.display_name),
        )
        .col_expr(users::Column::Token, Expr::value(derived.token))
        .col_expr(
            users::Column::TokenUpdatedAt,
            Expr::value(derived.token_updated_at.map(DateTimeWithTimeZone::from)),
        )
        .col_expr(users::Column::IsStaff, Expr::value(derived.is_staff))
        .filter(users::Column::Id.eq(user_id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// `DELETE users/me/` profile reset: null the workspace pointer, clear
/// both onboarding flags, reset the onboarding step.
pub async fn deactivate_profile_reset(
    db: &impl ConnectionTrait,
    profile_id: Uuid,
    onboarding_step: &serde_json::Value,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    profiles::Entity::update_many()
        .col_expr(
            profiles::Column::LastWorkspaceId,
            Expr::value(Option::<Uuid>::None),
        )
        .col_expr(profiles::Column::IsTourCompleted, Expr::value(false))
        .col_expr(profiles::Column::IsOnboarded, Expr::value(false))
        .col_expr(
            profiles::Column::OnboardingStep,
            Expr::value(onboarding_step.clone()),
        )
        .col_expr(
            profiles::Column::UpdatedAt,
            Expr::value(DateTimeWithTimeZone::from(now)),
        )
        .filter(profiles::Column::Id.eq(profile_id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// Which single-flag profile patch [`patch_profile_flag`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileFlag {
    Onboarded,
    TourCompleted,
}

/// `PATCH users/me/onboard/` / `tour-completed/`: one flag plus
/// `updated_at`, addressed by pk. `value` binds as-is — `None` renders
/// NULL (the ported bug-6 path: the NOT NULL column then 400s through
/// the `IntegrityError` branch).
pub async fn patch_profile_flag(
    db: &impl ConnectionTrait,
    profile_id: Uuid,
    flag: ProfileFlag,
    value: Option<bool>,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let column = match flag {
        ProfileFlag::Onboarded => profiles::Column::IsOnboarded,
        ProfileFlag::TourCompleted => profiles::Column::IsTourCompleted,
    };
    profiles::Entity::update_many()
        .col_expr(column, Expr::value(value))
        .col_expr(
            profiles::Column::UpdatedAt,
            Expr::value(DateTimeWithTimeZone::from(now)),
        )
        .filter(profiles::Column::Id.eq(profile_id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// The bulk-deactivate chunk size (`batch_size=100`).
pub const BULK_DEACTIVATE_CHUNK: usize = 100;

/// The `DELETE users/me/` workspace bulk: `is_active = FALSE` over pk
/// chunks (no `updated_at`, no `deleted_at` filter — ported bug 5).
/// An empty collection issues no statement.
pub async fn bulk_deactivate_workspace_members(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    for chunk in ids.chunks(BULK_DEACTIVATE_CHUNK) {
        if chunk.is_empty() {
            continue;
        }
        workspace_members::Entity::update_many()
            .col_expr(workspace_members::Column::IsActive, Expr::value(false))
            .filter(workspace_members::Column::Id.is_in(chunk.iter().copied()))
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DbBackend, QueryTrait};

    #[test]
    fn save_email_stmt_shape() {
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").expect("uuid");
        let stmt = users::Entity::update_many()
            .col_expr(users::Column::Email, Expr::value("x"))
            .col_expr(users::Column::IsEmailVerified, Expr::value(false))
            .col_expr(
                users::Column::UpdatedAt,
                Expr::value(DateTimeWithTimeZone::from(now)),
            )
            .col_expr(users::Column::DisplayName, Expr::value("d"))
            .col_expr(users::Column::Token, Expr::value("t"))
            .col_expr(
                users::Column::TokenUpdatedAt,
                Expr::value(Option::<DateTimeWithTimeZone>::None),
            )
            .col_expr(users::Column::IsStaff, Expr::value(false))
            .filter(users::Column::Id.eq(id));
        let sql = stmt.build(DbBackend::Postgres).to_string();
        assert!(sql.contains(r#""users" SET"#), "{sql}");
        assert!(sql.contains(r#""email" = 'x'"#), "{sql}");
        assert!(sql.contains(r#""is_email_verified" = FALSE"#), "{sql}");
        assert!(sql.contains(r#""display_name" = 'd'"#), "{sql}");
        assert!(sql.contains(r#""token" = 't'"#), "{sql}");
        assert!(sql.contains(r#""token_updated_at" = NULL"#), "{sql}");
        assert!(sql.contains(r#""is_staff" = FALSE"#), "{sql}");
        assert!(sql.contains(&format!(r#""id" = '{id}'"#)), "{sql}");
    }

    async fn scratch_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    fn live_uuid(tag: &str) -> Uuid {
        Uuid::parse_str(&format!("11111111-2222-3333-4444-{tag}")).expect("fixed uuid")
    }

    async fn temp_users(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) {
        sqlx::query("CREATE TEMP TABLE users AS SELECT * FROM public.users WITH NO DATA")
            .execute(&mut **tx)
            .await
            .expect("temp users");
    }

    async fn temp_profiles(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) {
        sqlx::query("CREATE TEMP TABLE profiles AS SELECT * FROM public.profiles WITH NO DATA")
            .execute(&mut **tx)
            .await
            .expect("temp profiles");
    }

    async fn temp_workspace_members(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) {
        sqlx::query(
            "CREATE TEMP TABLE workspace_members AS SELECT * FROM public.workspace_members WITH NO DATA",
        )
        .execute(&mut **tx)
        .await
        .expect("temp workspace_members");
    }

    async fn seed_user(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, id: Uuid, email: &str) {
        sqlx::query(
            "INSERT INTO users (id, username, email, display_name, first_name, last_name, \
             avatar, created_at, updated_at, last_location, created_location, \
             password, is_superuser, is_managed, is_password_expired, is_active, is_staff, \
             is_email_verified, is_password_autoset, is_password_reset_required, token, \
             last_login_ip, last_logout_ip, last_login_medium, last_login_uagent, \
             is_bot, user_timezone, is_email_valid) \
             VALUES ($1, 'ada', $2, 'Ada', 'Ada', 'L', '', now(), now(), '', '', \
             'pbkdf2_sha256$600000$salt$hash', false, false, false, true, false, \
             true, false, false, 'tok', '', '', 'email', '', false, 'UTC', false)",
        )
        .bind(id)
        .bind(email)
        .execute(&mut **tx)
        .await
        .expect("seed user");
    }

    async fn seed_profile(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, id: Uuid, user: Uuid) {
        sqlx::query(
            "INSERT INTO profiles (id, user_id, onboarding_step, updated_at, \
             last_workspace_id, is_onboarded, is_tour_completed) \
             VALUES ($1, $2, '{}', now(), $3, true, true)",
        )
        .bind(id)
        .bind(user)
        .bind(live_uuid("aaaaaaaa0001"))
        .execute(&mut **tx)
        .await
        .expect("seed profile");
    }

    #[tokio::test]
    async fn live_email_save_and_deactivate_save() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        temp_users(&mut tx).await;
        let user = live_uuid("000000000001");
        seed_user(&mut tx, user, "ada@x.io").await;
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        save_email(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            user,
            "new@x.io",
            now,
            &UserSaveFields {
                display_name: "New Name",
                token: "tok2",
                token_updated_at: Some(now),
                is_staff: true,
            },
        )
        .await
        .expect("save email");
        let row: sqlx::postgres::PgRow =
            sqlx::query("SELECT email, is_email_verified, display_name, token, token_updated_at, is_staff FROM users WHERE id = $1")
                .bind(user)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        use sqlx::Row;
        assert_eq!(
            row.try_get::<String, _>("email").expect("email"),
            "new@x.io"
        );
        assert!(!row
            .try_get::<bool, _>("is_email_verified")
            .expect("verified"));
        assert_eq!(
            row.try_get::<String, _>("display_name").expect("name"),
            "New Name"
        );
        assert_eq!(row.try_get::<String, _>("token").expect("token"), "tok2");
        assert!(row
            .try_get::<Option<DateTime<Utc>>, _>("token_updated_at")
            .expect("token stamp")
            .is_some());
        assert!(row.try_get::<bool, _>("is_staff").expect("staff"));

        deactivate_user_save(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            user,
            "new@x.io",
            "pbkdf2_sha256$600000$s$h",
            Some("1.2.3.4"),
            now,
            &UserSaveFields {
                display_name: "New Name",
                token: "tok2",
                token_updated_at: None,
                is_staff: true,
            },
        )
        .await
        .expect("deactivate save");
        let row: sqlx::postgres::PgRow =
            sqlx::query("SELECT password, is_password_autoset, is_active, last_logout_ip, last_logout_time, token_updated_at FROM users WHERE id = $1")
                .bind(user)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(
            row.try_get::<String, _>("password").expect("pw"),
            "pbkdf2_sha256$600000$s$h"
        );
        assert!(row
            .try_get::<bool, _>("is_password_autoset")
            .expect("autoset"));
        assert!(!row.try_get::<bool, _>("is_active").expect("active"));
        assert_eq!(
            row.try_get::<String, _>("last_logout_ip").expect("ip"),
            "1.2.3.4"
        );
        assert!(row
            .try_get::<Option<DateTime<Utc>>, _>("last_logout_time")
            .expect("logout")
            .is_some());
        // A `None` token stamp binds NULL (the pre-existing column was
        // already NULL here).
        assert_eq!(
            row.try_get::<Option<DateTime<Utc>>, _>("token_updated_at")
                .expect("stamp"),
            None
        );
    }

    #[tokio::test]
    async fn live_profile_reset_and_flag_patch() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        temp_profiles(&mut tx).await;
        let (profile, user) = (live_uuid("0000000000aa"), live_uuid("0000000001bb"));
        seed_profile(&mut tx, profile, user).await;
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let step: serde_json::Value = serde_json::from_str(
            r#"{"workspace_join": false, "profile_complete": false, "workspace_create": false, "workspace_invite": false}"#,
        )
        .expect("step json");
        deactivate_profile_reset(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            profile,
            &step,
            now,
        )
        .await
        .expect("profile reset");
        use sqlx::Row;
        let row: sqlx::postgres::PgRow =
            sqlx::query("SELECT last_workspace_id, is_onboarded, is_tour_completed, onboarding_step FROM profiles WHERE id = $1")
                .bind(profile)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(
            row.try_get::<Option<Uuid>, _>("last_workspace_id")
                .expect("ws"),
            None
        );
        assert!(!row.try_get::<bool, _>("is_onboarded").expect("onb"));
        assert!(!row.try_get::<bool, _>("is_tour_completed").expect("tour"));
        assert_eq!(
            row.try_get::<serde_json::Value, _>("onboarding_step")
                .expect("step"),
            step
        );

        patch_profile_flag(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            profile,
            ProfileFlag::Onboarded,
            Some(true),
            now,
        )
        .await
        .expect("onboard flag");
        let onboarded: bool = sqlx::query_scalar("SELECT is_onboarded FROM profiles WHERE id = $1")
            .bind(profile)
            .fetch_one(&mut *tx)
            .await
            .expect("read back");
        assert!(onboarded);
        patch_profile_flag(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            profile,
            ProfileFlag::TourCompleted,
            Some(false),
            now,
        )
        .await
        .expect("tour flag");
        let tour: bool = sqlx::query_scalar("SELECT is_tour_completed FROM profiles WHERE id = $1")
            .bind(profile)
            .fetch_one(&mut *tx)
            .await
            .expect("read back");
        assert!(!tour);
    }

    #[tokio::test]
    async fn live_bulk_deactivate_workspace_members() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        temp_workspace_members(&mut tx).await;
        let (a, b) = (live_uuid("0000000000c1"), live_uuid("0000000000c2"));
        for id in [a, b] {
            sqlx::query("INSERT INTO workspace_members (id, is_active) VALUES ($1, true)")
                .bind(id)
                .execute(&mut *tx)
                .await
                .expect("seed member");
        }
        bulk_deactivate_workspace_members(&pidash_db::orm_tx::TxConn::new(&mut tx), &[a, b])
            .await
            .expect("bulk deactivate");
        bulk_deactivate_workspace_members(&pidash_db::orm_tx::TxConn::new(&mut tx), &[])
            .await
            .expect("empty bulk issues no statement");
        for id in [a, b] {
            let active: bool =
                sqlx::query_scalar("SELECT is_active FROM workspace_members WHERE id = $1")
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await
                    .expect("read back");
            assert!(!active);
        }
    }
}
