//! OSS identity writes: `users`, `profiles`, `accounts`,
//! `user_notification_preferences` (ORM-07, PIDASHCONV-1525).
//!
//! Every OSS write to the four identity tables lives in this module; the
//! signup, admin, sync and bot-provisioning call sites call these
//! functions instead of executing SQL text. Reads stay where they are
//! (design D5).
//!
//! Every writer takes a SeaORM [`ConnectionTrait`]: the caller's pool
//! (`pidash_db::pool::sea_orm_connection(&pool)`) or the caller's open
//! sqlx transaction (`pidash_db::orm_tx::TxConn::new(&mut tx)`), so no
//! flow restructures its transaction. Two shapes, by writer kind:
//!
//! * [`create_user`] inserts through the `users` entity's
//!   `ActiveModelTrait::insert`, so the `after_save` hook
//!   (`db::entity::users_hooks`, the `create_user_notification` post_save
//!   receiver, `user.py:307-320`) fires and the preferences row lands in
//!   the same transaction. The profile row is service-level, not hook
//!   code: Django creates it explicitly in the adapter
//!   (`adapter/base.py:342`, defaults) and the license admin view
//!   (`admin.py:218`, with `company_name`), never in a receiver (D4).
//! * Everything else is a bare entity statement (`Entity::insert`,
//!   `update_many`, a sea-query `UPDATE`): no hooks fire. The bot insert
//!   ([`create_bot_user`]) and the provisioned insert
//!   ([`insert_provisioned_user`]) deliberately take this shape — bots
//!   get neither hook effect anyway (Django skips bots in the receiver
//!   and creates no profile for them), and the provisioning paths write
//!   the prefs row as the separate explicit
//!   [`insert_user_notification_preference`] call (both create non-bots,
//!   so the receiver gate is statically satisfied; the sync path stamps
//!   it with a fresh `now`). The platform dirty update
//!   ([`update_user_fields`]) rides the same shape (D6 gap
//!   PIDASHCONV-1616).
//! * [`get_agent_system_user`] alone takes `&mut PgConnection`: its
//!   `get_or_create` race seam needs a savepoint on the caller's
//!   transaction (sqlx's nested `begin`), which a [`ConnectionTrait`]
//!   handle cannot open.
//!
//! Errors stay [`sqlx::Error`] (through [`pidash_db::orm_tx::into_sqlx`]),
//! so the callers' SQLSTATE classification (23505 races, 23502 stamps)
//! is unchanged.
//!
//! Per-caller deltas (design D6) ride on the input structs, each citing
//! the Python file:line that has the same difference. Three Rust-side
//! divergences the ORM-07 survey found are fixed here (PIDASHCONV-1560):
//! every signup insert stores `last_active = now` (Django's field
//! default, `user.py:105`) and `avatar = ''` (the field default,
//! `user.py:68`), and every stamps call binds a missing ip/user-agent
//! as NULL into the NOT NULL columns (Django 500s the same way,
//! `adapter/base.py:225-226`). Each was observable only when a later
//! save never ran (a mid-signup database failure).
//!
//! Statement order moves one step on the gitlab path: its prefs insert
//! used to run after the profile insert; the hook fires at the user
//! save on every path now, which is Django's order (`post_save` runs
//! inside `save()`). Final rows are identical.

use chrono::{DateTime, Utc};
use pidash_compat::db::is_unique_violation;
use pidash_db::entity::{accounts, profiles, user_notification_preferences, users};
use pidash_db::orm_tx::{into_sqlx, TxConn};
use sea_orm::sea_query::{Alias, Expr, ExprTrait, Func, Query, SimpleExpr, UpdateStatement};
use sea_orm::ActiveValue::Set;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, FromQueryResult,
    QueryFilter, QuerySelect, UpdateMany,
};
use std::fmt::Write as _;
use uuid::Uuid;

use super::error::DomainError;

/// A bound clock as the entity's `timestamptz` value.
fn at(now: DateTime<Utc>) -> sea_orm::prelude::DateTimeWithTimeZone {
    now.fixed_offset()
}

/// Run one bulk update.
async fn exec_update<E: EntityTrait>(
    db: &impl ConnectionTrait,
    stmt: UpdateMany<E>,
) -> Result<(), sqlx::Error> {
    stmt.exec(db).await.map(|_| ()).map_err(into_sqlx)
}

/// Run one built statement.
async fn exec_stmt(db: &impl ConnectionTrait, stmt: &UpdateStatement) -> Result<(), sqlx::Error> {
    db.execute(DbBackend::Postgres.build(stmt))
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// `users` column, unqualified (the single-table `UPDATE`s).
fn col(column: users::Column) -> Expr {
    Expr::col(column)
}

/// `User.save()`'s empty-name backfill as the stamps spell it
/// (`display_name = ''`): `CASE WHEN "display_name" = '' THEN
/// split_part("email", '@', 1) ELSE "display_name" END`.
fn display_name_backfill_case() -> SimpleExpr {
    Expr::case(
        col(users::Column::DisplayName).eq(""),
        split_part_email_prefix(),
    )
    .finally(col(users::Column::DisplayName))
    .into()
}

/// The password/logout twins' NULL-tolerant spelling of the same
/// backfill: `COALESCE("display_name", '') = ''`.
fn display_name_backfill_coalesce_case() -> SimpleExpr {
    Expr::case(
        Expr::expr(Func::coalesce([
            col(users::Column::DisplayName).into(),
            Expr::val("").into(),
        ]))
        .eq(""),
        split_part_email_prefix(),
    )
    .finally(col(users::Column::DisplayName))
    .into()
}

/// `split_part("email", '@', 1)`.
fn split_part_email_prefix() -> SimpleExpr {
    Func::cust(Alias::new("split_part"))
        .args([
            col(users::Column::Email).into(),
            Expr::val("@").into(),
            Expr::val(1).into(),
        ])
        .into()
}

/// `"is_staff" = CASE WHEN "is_superuser" THEN true ELSE "is_staff" END`.
fn staff_for_superuser_case() -> SimpleExpr {
    Expr::case(col(users::Column::IsSuperuser).eq(true), true)
        .finally(col(users::Column::IsStaff))
        .into()
}

/// `User.save()`'s token rotation when a token is in use: `token = CASE
/// WHEN token_updated_at IS NOT NULL THEN <fresh> ELSE token END` and
/// `token_updated_at = CASE WHEN token_updated_at IS NOT NULL THEN <at>
/// ELSE token_updated_at END`.
fn token_rotation_cases(fresh_token: &str, rotated_at: SimpleExpr) -> (SimpleExpr, SimpleExpr) {
    let in_use = || col(users::Column::TokenUpdatedAt).is_not_null();
    (
        Expr::case(in_use(), fresh_token)
            .finally(col(users::Column::Token))
            .into(),
        Expr::case(in_use(), rotated_at)
            .finally(col(users::Column::TokenUpdatedAt))
            .into(),
    )
}

// ---------------------------------------------------------------------------
// create_user (entity; the after_save hook fires)
// ---------------------------------------------------------------------------

/// Resolved inputs for the service-level profile row
/// (`adapter/base.py:342`, `admin.py:218`): `company_name` is `''` on
/// every path but the license admin create, which passes the instance
/// company name; `background_color` is caller-generated (`'#'` + six
/// hexdigits — the generators differ per path and all stay put).
pub struct NewProfile<'a> {
    pub company_name: &'a str,
    pub background_color: String,
}

/// Resolved inputs for [`create_user`]: everything the signup branches
/// decide. `id` is `None` on the email/magic/admin/google paths, whose
/// ids the function generates, and `Some` where the caller minted one
/// already (gitlab/gitea/github). `username`, `password_hash`,
/// `display_name` and the names are caller-resolved (uuid-hex vs
/// `new_state`, PBKDF2 vs pre-encoded, email-local backfill vs provider
/// display). `avatar` is `''` on every path (Django's field default,
/// `user.py:68`); provider URLs land after the insert. `last_active`
/// is `Some(now)` on every path (Django's field default,
/// `user.py:105`). `login_ip`/`login_uagent` are `Some("")` but
/// the magic path's request values. `is_bot` forwards to the hook
/// (bots get no prefs row); `profile` is `None` for bots (Django
/// creates no profile for them) and `Some` for every human signup.
pub struct NewUser<'a> {
    pub id: Option<Uuid>,
    pub email: &'a str,
    pub username: String,
    pub password_hash: String,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub display_name: String,
    pub avatar: &'a str,
    pub is_email_verified: bool,
    pub is_password_autoset: bool,
    pub last_active: Option<DateTime<Utc>>,
    pub login_ip: Option<&'a str>,
    pub login_uagent: Option<&'a str>,
    pub is_bot: bool,
    pub now: DateTime<Utc>,
    pub profile: Option<NewProfile<'a>>,
}

/// The default `profiles` row (`db/models/user.py:200-273`, every model
/// default) for one user: `company_name` is the one caller-set column
/// (`''` everywhere but the license admin create), `background_color`
/// the caller's draw.
fn default_profile_row(
    id: Uuid,
    user_id: Uuid,
    now: DateTime<Utc>,
    company_name: &str,
    background_color: &str,
) -> profiles::ActiveModel {
    let now = at(now);
    profiles::ActiveModel {
        id: Set(id),
        created_at: Set(now),
        updated_at: Set(now),
        user_id: Set(user_id),
        theme: Set(serde_json::json!({})),
        is_app_rail_docked: Set(true),
        is_tour_completed: Set(false),
        onboarding_step: Set(serde_json::json!({
            "profile_complete": false,
            "workspace_create": false,
            "workspace_invite": false,
            "workspace_join": false,
        })),
        use_case: Set(None),
        role: Set(None),
        is_onboarded: Set(false),
        last_workspace_id: Set(None),
        billing_address_country: Set("INDIA".to_owned()),
        billing_address: Set(None),
        has_billing_address: Set(false),
        company_name: Set(company_name.to_owned()),
        notification_view_mode: Set("full".to_owned()),
        is_smooth_cursor_enabled: Set(false),
        is_mobile_onboarded: Set(false),
        mobile_onboarding_step: Set(serde_json::json!({
            "profile_complete": false,
            "workspace_create": false,
            "workspace_join": false,
        })),
        mobile_timezone_auto_set: Set(false),
        language: Set("en".to_owned()),
        start_of_the_week: Set(0),
        goals: Set(serde_json::json!({})),
        background_color: Set(background_color.to_owned()),
        is_navigation_tour_completed: Set(false),
        has_marketing_email_consent: Set(false),
        is_subscribed_to_changelog: Set(false),
        product_tour: Set(serde_json::json!({
            "work_items": false,
            "cycles": false,
            "modules": false,
            "intake": false,
            "pages": false,
        })),
        settings: Set(serde_json::json!({})),
    }
}

/// `User(...)` + first `save()` for a sign-up
/// (`adapter/base.py:296-328` + `User.save()`, `user.py:169-187`):
/// identity + names + password hash; every other column takes its
/// model default. Runs through the `users` entity so `after_save`
/// fires (preferences row for non-bots, same transaction), then the
/// service-level profile row when `profile` is `Some`. Returns the
/// row id (generated when `NewUser::id` is `None`).
///
/// Every caller passes one frozen `now` for all three writes, like the
/// paths did before; the hook reuses the user row's stamps, so the
/// frozen-`now` shape survives byte for byte.
pub async fn create_user<C>(db: &C, user: &NewUser<'_>) -> Result<Uuid, DomainError>
where
    C: ConnectionTrait,
{
    // `None` ip/user-agent renders NULL into the NOT NULL columns, so the
    // request 500s — the 500 Python answers uncaught (`META.get` is None,
    // `user.py` has no null=True). Every caller maps any error here to its
    // 500 arm, exactly like today's NULL-violation arm.
    let (Some(login_ip), Some(login_uagent)) = (user.login_ip, user.login_uagent) else {
        return Err(DomainError::Work("login ip/user-agent is None".to_owned()));
    };
    let id = user.id.unwrap_or_else(Uuid::new_v4);
    let now = at(user.now);
    let row = users::ActiveModel {
        id: Set(id),
        password: Set(user.password_hash.clone()),
        last_login: Set(None),
        username: Set(user.username.clone()),
        mobile_number: Set(None),
        email: Set(Some(user.email.to_owned())),
        display_name: Set(user.display_name.clone()),
        first_name: Set(user.first_name.to_owned()),
        last_name: Set(user.last_name.to_owned()),
        avatar: Set(user.avatar.to_owned()),
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
        is_email_verified: Set(user.is_email_verified),
        is_password_autoset: Set(user.is_password_autoset),
        token: Set(String::new()),
        user_timezone: Set("UTC".to_owned()),
        last_active: Set(user.last_active.map(at)),
        last_login_time: Set(None),
        last_logout_time: Set(None),
        last_login_ip: Set(login_ip.to_owned()),
        last_logout_ip: Set(String::new()),
        // `last_login_medium` keeps the `"email"` model default until
        // `save_user_data` overwrites it (`user.py:110`).
        last_login_medium: Set("email".to_owned()),
        last_login_uagent: Set(login_uagent.to_owned()),
        token_updated_at: Set(None),
        is_bot: Set(user.is_bot),
        bot_type: Set(None),
        is_email_valid: Set(false),
        masked_at: Set(None),
        is_password_reset_required: Set(false),
    };
    row.insert(db).await.map_err(DomainError::Db)?;
    if let Some(profile) = &user.profile {
        profiles::Entity::insert(default_profile_row(
            Uuid::new_v4(),
            id,
            user.now,
            profile.company_name,
            &profile.background_color,
        ))
        .exec(db)
        .await
        .map_err(DomainError::Db)?;
    }
    Ok(id)
}

// ---------------------------------------------------------------------------
// bots + provisioned users (bare inserts; no hook effects by design)
// ---------------------------------------------------------------------------

/// The one full-row `users` insert shape the bot and provisioned paths
/// share: all 40 physical columns (the moved
/// `db::orchestration::workpad::AGENT_USER_INSERT_SQL` column list), the
/// caller-varying ones here, every other at its Django model default.
struct FullUserRow<'a> {
    id: Uuid,
    username: &'a str,
    email: &'a str,
    password_hash: &'a str,
    display_name: &'a str,
    first_name: &'a str,
    last_name: &'a str,
    avatar: &'a str,
    is_managed: bool,
    is_email_verified: bool,
    is_password_autoset: bool,
    last_login_medium: &'a str,
    user_timezone: &'a str,
    is_bot: bool,
    bot_type: Option<&'a str>,
    now: DateTime<Utc>,
}

fn full_user_row(row: &FullUserRow<'_>) -> users::ActiveModel {
    let now = at(row.now);
    users::ActiveModel {
        password: Set(row.password_hash.to_owned()),
        last_login: Set(None),
        id: Set(row.id),
        username: Set(row.username.to_owned()),
        mobile_number: Set(None),
        email: Set(Some(row.email.to_owned())),
        display_name: Set(row.display_name.to_owned()),
        first_name: Set(row.first_name.to_owned()),
        last_name: Set(row.last_name.to_owned()),
        avatar: Set(row.avatar.to_owned()),
        avatar_asset_id: Set(None),
        cover_image: Set(None),
        cover_image_asset_id: Set(None),
        date_joined: Set(now),
        created_at: Set(now),
        updated_at: Set(now),
        last_location: Set(String::new()),
        created_location: Set(String::new()),
        is_superuser: Set(false),
        is_managed: Set(row.is_managed),
        is_password_expired: Set(false),
        is_active: Set(true),
        is_staff: Set(false),
        is_email_verified: Set(row.is_email_verified),
        is_password_autoset: Set(row.is_password_autoset),
        is_password_reset_required: Set(false),
        token: Set(String::new()),
        last_active: Set(Some(now)),
        last_login_time: Set(None),
        last_logout_time: Set(None),
        last_login_ip: Set(String::new()),
        last_logout_ip: Set(String::new()),
        last_login_medium: Set(row.last_login_medium.to_owned()),
        last_login_uagent: Set(String::new()),
        token_updated_at: Set(None),
        is_bot: Set(row.is_bot),
        bot_type: Set(row.bot_type.map(str::to_owned)),
        user_timezone: Set(row.user_timezone.to_owned()),
        is_email_valid: Set(false),
        masked_at: Set(None),
    }
}

/// Resolved inputs for [`create_bot_user`]: the workpad agent user
/// (`workpad.py:53-61`: `defaults={email, first_name, last_name,
/// is_bot=True}`, everything else at model defaults) and the
/// workspace-seed bot (`workspace_seed.py:522-532`, `is_password_autoset`
/// true, `bot_type 'WORKSPACE_SEED'`, display `'Pi Dash'`) differ only
/// in these fields. `id` is `None` where the function generates one.
pub struct NewBotUser<'a> {
    pub id: Option<Uuid>,
    pub username: &'a str,
    pub email: &'a str,
    pub password_hash: &'a str,
    pub display_name: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub is_password_autoset: bool,
    pub bot_type: Option<&'a str>,
    pub now: DateTime<Utc>,
}

/// The bot `users` row ([`full_user_row`] with the bot fields).
fn bot_user_row(id: Uuid, bot: &NewBotUser<'_>) -> users::ActiveModel {
    full_user_row(&FullUserRow {
        id,
        username: bot.username,
        email: bot.email,
        password_hash: bot.password_hash,
        display_name: bot.display_name,
        first_name: bot.first_name,
        last_name: bot.last_name,
        avatar: "",
        is_managed: false,
        is_email_verified: false,
        is_password_autoset: bot.is_password_autoset,
        last_login_medium: "email",
        user_timezone: "UTC",
        is_bot: true,
        bot_type: bot.bot_type,
        now: bot.now,
    })
}

/// Bot `users` insert for the provisioning flows: the workpad
/// `get_or_create` (under the savepoint [`get_agent_system_user`]
/// opens) and the seed bot. A bare entity insert: like the hook, bots
/// get no preferences row (the receiver skips `is_bot`) and no profile
/// row (neither Django flow creates one). Returns the row id.
pub async fn create_bot_user(
    db: &impl ConnectionTrait,
    bot: &NewBotUser<'_>,
) -> Result<Uuid, sqlx::Error> {
    let id = bot.id.unwrap_or_else(Uuid::new_v4);
    users::Entity::insert(bot_user_row(id, bot))
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    Ok(id)
}

/// `set_unusable_password` + `save(update_fields=["password"])`
/// (`workpad.py:62-65`): the bot insert stores a placeholder password
/// and this stamps the unusable hash right after.
pub async fn stamp_bot_unusable_password(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    unusable_hash: &str,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        users::Entity::update_many()
            .col_expr(users::Column::Password, Expr::value(unusable_hash))
            .filter(users::Column::Id.eq(user_id)),
    )
    .await
}

/// Resolved inputs for [`insert_provisioned_user`]: the two tx-bound
/// provisioning paths' first-save values. The platform upsert
/// (`service.py:255-265`: managed + verified, unusable password,
/// empty names/avatar, medium `"email"`) and the OIDC sync create
/// (`sync.py:91-103`: claims email/names/picture, autoset password,
/// medium `"airepublic"`) differ only in these fields; every other
/// column is fixed-identical. `id` is `None` where the function
/// generates one (both paths do). `user_timezone` is caller-resolved
/// though both paths pass `"UTC"` today. Neither path writes a
/// profile row here (platform never does; sync runs its own
/// `get_or_create`), and the prefs row is the separate explicit
/// [`insert_user_notification_preference`] call.
pub struct ProvisionedUser<'a> {
    pub id: Option<Uuid>,
    pub email: &'a str,
    pub username: &'a str,
    pub password_hash: &'a str,
    pub display_name: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub is_managed: bool,
    pub is_email_verified: bool,
    pub is_password_autoset: bool,
    pub last_login_medium: &'a str,
    pub user_timezone: &'a str,
    pub now: DateTime<Utc>,
}

/// The provisioned `users` row ([`full_user_row`] with the caller's
/// fields, never a bot).
fn provisioned_user_row(id: Uuid, user: &ProvisionedUser<'_>) -> users::ActiveModel {
    full_user_row(&FullUserRow {
        id,
        username: user.username,
        email: user.email,
        password_hash: user.password_hash,
        display_name: user.display_name,
        first_name: user.first_name,
        last_name: user.last_name,
        avatar: user.avatar,
        is_managed: user.is_managed,
        is_email_verified: user.is_email_verified,
        is_password_autoset: user.is_password_autoset,
        last_login_medium: user.last_login_medium,
        user_timezone: user.user_timezone,
        is_bot: false,
        bot_type: None,
        now: user.now,
    })
}

/// `User(...)` + first `save()` for the tx-bound provisioning paths:
/// the platform upsert (`service.py:255-265`) and the OIDC sync
/// `get_or_create` create side (`sync.py:91-103`). A bare entity
/// insert (no `after_save`) and profile-free: the `post_save` prefs
/// effect is the caller's separate
/// [`insert_user_notification_preference`] call. Returns the row id
/// (generated when `ProvisionedUser::id` is `None`).
pub async fn insert_provisioned_user(
    db: &impl ConnectionTrait,
    user: &ProvisionedUser<'_>,
) -> Result<Uuid, sqlx::Error> {
    let id = user.id.unwrap_or_else(Uuid::new_v4);
    users::Entity::insert(provisioned_user_row(id, user))
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    Ok(id)
}

/// The `User` `post_save` effect as an explicit call
/// (`db/models/user.py:307-320`, reached from `service.py:265` and
/// `sync.py:91`): one preferences row — all five flags true, NULL
/// audit and scope columns. The tx-bound callers run the bare user
/// insert, so the hook never fires for them. `row_id` is
/// caller-generated (`uuid4`, both callers); `now` stamps both
/// columns (the platform path reuses the user row's `now`, the sync
/// path a fresh stamp).
pub async fn insert_user_notification_preference(
    db: &impl ConnectionTrait,
    row_id: Uuid,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let now = at(now);
    user_notification_preferences::Entity::insert(user_notification_preferences::ActiveModel {
        created_at: Set(now),
        updated_at: Set(now),
        created_by_id: Set(None),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        id: Set(row_id),
        user_id: Set(user_id),
        workspace_id: Set(None),
        project_id: Set(None),
        property_change: Set(true),
        state_change: Set(true),
        comment: Set(true),
        mention: Set(true),
        issue_completed: Set(true),
    })
    .exec(db)
    .await
    .map(|_| ())
    .map_err(into_sqlx)
}

/// Dirty `users` subset for [`update_user_fields`]: `Some` writes the
/// column (`service.py:267-276`: email always synced, display name
/// filled when empty, username filled when empty).
pub struct UserFieldUpdates<'a> {
    pub username: Option<&'a str>,
    pub email: Option<&'a str>,
    pub display_name: Option<&'a str>,
}

impl UserFieldUpdates<'_> {
    /// No dirty columns: `user.save()` never runs (`service.py:277`).
    pub fn is_empty(&self) -> bool {
        self.username.is_none() && self.email.is_none() && self.display_name.is_none()
    }
}

/// The dirty-fields `UPDATE` for one input combination (fixed `_meta`
/// order: username, email, display_name, updated_at — Django 4.2
/// re-sorts `update_fields`, `base.py:952-957`). `None` when no column
/// is dirty: the save never runs. Pure, so the unit tests pin the
/// shape per combination.
fn user_fields_update(
    user_id: Uuid,
    updates: &UserFieldUpdates<'_>,
    now: DateTime<Utc>,
) -> Option<UpdateMany<users::Entity>> {
    if updates.is_empty() {
        return None;
    }
    let mut stmt = users::Entity::update_many();
    if let Some(username) = updates.username {
        stmt = stmt.col_expr(users::Column::Username, Expr::value(username));
    }
    if let Some(email) = updates.email {
        stmt = stmt.col_expr(users::Column::Email, Expr::value(email));
    }
    if let Some(display_name) = updates.display_name {
        stmt = stmt.col_expr(users::Column::DisplayName, Expr::value(display_name));
    }
    Some(
        stmt.col_expr(users::Column::UpdatedAt, Expr::value(now))
            .filter(users::Column::Id.eq(user_id)),
    )
}

/// `user.save(update_fields=...)` (`service.py:267-280`): only dirty
/// columns plus `updated_at`, in `_meta` order. A no-op when nothing
/// is dirty (the `if user_updates:` guard). Runs on the caller's
/// connection.
pub async fn update_user_fields(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    updates: &UserFieldUpdates<'_>,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    match user_fields_update(user_id, updates, now) {
        Some(stmt) => exec_update(db, stmt).await,
        None => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// agent system user (moved from db::orchestration::workpad)
// ---------------------------------------------------------------------------

/// Reserved bot username (`workpad.py:29`).
pub const AGENT_USERNAME: &str = "pi_dash_agent";
/// Bot email (`workpad.py:30`).
pub const AGENT_USER_EMAIL: &str = "agent@example.com";
/// Bot first name (`workpad.py:31`).
pub const AGENT_USER_FIRST_NAME: &str = "Pi Dash";
/// Bot last name (`workpad.py:32`).
pub const AGENT_USER_LAST_NAME: &str = "Agent";
/// `display_name` the `User.save()` override derives when the field
/// is blank (`db/models/user.py:176-182`): the email prefix.
pub const AGENT_DISPLAY_NAME: &str = "agent";

/// A real human account holds the reserved agent username
/// (`workpad.py:35-41`). The message is verbatim.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("User 'pi_dash_agent' exists but is not a bot; refusing to author agent activity under a human account.")]
pub struct AgentUserCollisionError;

/// Failure modes of [`get_agent_system_user`]: a human holds the
/// reserved name, or the database failed.
#[derive(Debug, thiserror::Error)]
pub enum GetAgentUserError {
    #[error(transparent)]
    Collision(#[from] AgentUserCollisionError),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

/// The agent-user columns this unit touches: identity plus the
/// `is_bot` gate. The email column is nullable in the schema, though
/// the created row always carries [`AGENT_USER_EMAIL`].
#[derive(Debug, Clone, PartialEq, Eq, FromQueryResult)]
pub struct AgentSystemUser {
    pub id: Uuid,
    pub username: String,
    pub email: Option<String>,
    pub first_name: String,
    pub last_name: String,
    pub is_bot: bool,
}

/// Single agent-user row by the unique username (`workpad.py:53-61`),
/// or `None`. Django's `get()` pages with `LIMIT 21` to detect
/// multiplicity; the unique index precludes that, so this reads one
/// row.
pub async fn find_agent_user(
    db: &impl ConnectionTrait,
    username: &str,
) -> Result<Option<AgentSystemUser>, sqlx::Error> {
    users::Entity::find()
        .select_only()
        .columns([
            users::Column::Id,
            users::Column::Username,
            users::Column::Email,
            users::Column::FirstName,
            users::Column::LastName,
            users::Column::IsBot,
        ])
        .filter(users::Column::Username.eq(username))
        .into_model::<AgentSystemUser>()
        .one(db)
        .await
        .map_err(into_sqlx)
}

/// `make_password(None)`: `!` plus 40 `SystemRandom` alphanumerics
/// (`django/contrib/auth/hashers.py`). `Alphanumeric` draws from the
/// same 62-letter alphabet.
pub fn make_unusable_password() -> String {
    use rand::distr::{Alphanumeric, SampleString};
    format!("!{}", Alphanumeric.sample_string(&mut rand::rng(), 40))
}

/// `is_password_usable` (`django/contrib/auth/hashers.py`): anything
/// not starting with `!` counts as usable (including `""`).
pub fn is_password_usable(encoded: &str) -> bool {
    !encoded.starts_with('!')
}

/// Return (and create on first call) the dedicated bot user
/// (`workpad.py:44-71`). Returns the row plus whether this call
/// created it.
///
/// The `get_or_create` race seam mirrors Django: `SELECT`, then
/// `INSERT` under a savepoint (what `transaction.atomic()` nests to
/// inside a transaction — call inside one; sqlx's nested `begin` is
/// the savepoint), and on a unique violation roll back to the
/// savepoint and re-read the winner's row. A human-held name raises
/// [`AgentUserCollisionError`] instead of reusing the row.
pub async fn get_agent_system_user(
    conn: &mut sqlx::PgConnection,
    now: DateTime<Utc>,
) -> Result<(AgentSystemUser, bool), GetAgentUserError> {
    if let Some(user) = find_agent_user(&TxConn::new(&mut *conn), AGENT_USERNAME).await? {
        if !user.is_bot {
            return Err(AgentUserCollisionError.into());
        }
        return Ok((user, false));
    }
    let mut savepoint = sqlx::Connection::begin(&mut *conn).await?;
    let created = create_bot_user(
        &TxConn::new(&mut savepoint),
        &NewBotUser {
            id: None,
            username: AGENT_USERNAME,
            email: AGENT_USER_EMAIL,
            // Placeholder: the unusable hash lands right after the
            // insert (`workpad.py:62-65`).
            password_hash: "",
            display_name: AGENT_DISPLAY_NAME,
            first_name: AGENT_USER_FIRST_NAME,
            last_name: AGENT_USER_LAST_NAME,
            is_password_autoset: false,
            bot_type: None,
            now,
        },
    )
    .await;
    let id = match created {
        Ok(id) => {
            savepoint.commit().await?;
            id
        }
        Err(e) if is_unique_violation(&e) => {
            savepoint.rollback().await?;
            // Django re-runs `get()` here; when the conflict was
            // email-only the re-get misses and Django re-raises the
            // original `IntegrityError` — so return it, not
            // `RowNotFound` (which would mislead retry-on-missing
            // callers into a loop).
            let Some(user) = find_agent_user(&TxConn::new(&mut *conn), AGENT_USERNAME).await?
            else {
                return Err(e.into());
            };
            if !user.is_bot {
                return Err(AgentUserCollisionError.into());
            }
            return Ok((user, true));
        }
        Err(e) => return Err(e.into()),
    };
    stamp_bot_unusable_password(&TxConn::new(&mut *conn), id, &make_unusable_password()).await?;
    Ok((
        AgentSystemUser {
            id,
            username: AGENT_USERNAME.to_string(),
            email: Some(AGENT_USER_EMAIL.to_string()),
            first_name: AGENT_USER_FIRST_NAME.to_string(),
            last_name: AGENT_USER_LAST_NAME.to_string(),
            is_bot: true,
        },
        true,
    ))
}

/// [`get_agent_system_user`] (`orchestration/workpad.py:44-71`) on its
/// own transaction: the id other writers stamp as the ambient crum
/// user for pause/failure comments. Own transaction, since the flow
/// opens a savepoint on the insert path. Moved from the api ports
/// (`runner_runs::LivePorts::agent_system_user_id`, OVL-06
/// PIDASHCONV-1544); the ports method is now a thin caller.
/// Re-exported through `pidash-sdk::ops`.
pub async fn agent_system_user_id(pool: &sqlx::PgPool) -> Result<Uuid, GetAgentUserError> {
    let mut tx = pool.begin().await?;
    let (user, _) = get_agent_system_user(&mut tx, Utc::now()).await?;
    tx.commit().await?;
    Ok(user.id)
}

// ---------------------------------------------------------------------------
// save_user_data (login stamps; dynamic SET over the per-path columns)
// ---------------------------------------------------------------------------

/// How the stamps write `display_name`: bound (gitlab/github resolve
/// the backfill in Rust) or the atomic backfill `CASE` (gitea/google).
/// `None` on the whole field omits the column (admin).
pub enum DisplayNameUpdate<'a> {
    Bound(&'a str),
    BackfillCase,
}

/// How the stamps write `is_staff`: a bound `is_staff || is_superuser`
/// (gitlab/github) or the atomic `OR` expression (gitea/google).
/// `None` omits the column (admin).
pub enum StaffSync {
    Value(bool),
    OrSuperuser,
}

/// Resolved inputs for [`save_user_data`]: the `save_user_data`
/// (`adapter/base.py:220-234`) changed-column subset per caller.
/// Django runs one full-row `save()`; each port writes only the columns
/// its in-memory row dirtied, so the SET lists differ honestly:
///
/// * `medium`: the provider (`email`, `magic-code`, `gitlab`, `gitea`,
///   `google`, `github`), bound — the literal-vs-bind spelling is
///   normalized (no fixture pins it). `None` omits the column: the
///   license stamps never assign it (`admin.py:220-226`).
/// * `avatar` / `avatar_asset_id`: gitlab binds the URL, gitea the URL
///   plus the asset id; everyone else persists avatars through the
///   [`stamp_avatar_asset`]/[`stamp_avatar_url`] singles.
/// * `display_name` / `is_staff`: the `User.save()` backfill + sync
///   (`user.py:169-187`), in bound or `CASE`/expression form — or
///   omitted where the stored row already matches.
/// * `touch_updated_at`: bumps `updated_at` (`auto_now`); every login
///   path sets it, email included — `save_user_data` ends in a full
///   `user.save()` (NEWPIDASH-17).
/// * `login_ip`/`login_uagent` bind NULL when `None` (the NOT NULL
///   columns raise into the same 500 Python hits,
///   `adapter/base.py:225-226`), on every path including gitea.
///
/// `rotated_token` is caller-generated (`uuid4hex + uuid4hex`,
/// `user.py:174`); the activation-mail branch that precedes this write
/// stays with the callers (it is an enqueue, not a users write).
pub struct LoginStamps<'a> {
    pub medium: Option<&'a str>,
    pub login_ip: Option<&'a str>,
    pub login_uagent: Option<&'a str>,
    pub avatar: Option<&'a str>,
    pub avatar_asset_id: Option<Option<Uuid>>,
    pub display_name: Option<DisplayNameUpdate<'a>>,
    pub is_staff: Option<StaffSync>,
    pub touch_updated_at: bool,
    pub rotated_token: &'a str,
    pub now: DateTime<Utc>,
}

/// The stamps `UPDATE` for one input combination. Column order is
/// fixed (medium, stamps, token, active, avatar pair, display, staff,
/// updated_at). Pure, so the unit tests pin the shape per caller.
fn login_stamps_update(user_id: Uuid, stamps: &LoginStamps<'_>) -> UpdateMany<users::Entity> {
    use users::Column as C;
    let mut stmt = users::Entity::update_many();
    if let Some(medium) = stamps.medium {
        stmt = stmt.col_expr(C::LastLoginMedium, Expr::value(medium));
    }
    stmt = stmt
        .col_expr(C::LastActive, Expr::value(stamps.now))
        .col_expr(C::LastLoginTime, Expr::value(stamps.now))
        .col_expr(C::LastLoginIp, Expr::value(stamps.login_ip))
        .col_expr(C::LastLoginUagent, Expr::value(stamps.login_uagent))
        .col_expr(C::TokenUpdatedAt, Expr::value(stamps.now))
        .col_expr(C::Token, Expr::value(stamps.rotated_token))
        .col_expr(C::IsActive, Expr::value(true));
    if let Some(avatar) = stamps.avatar {
        stmt = stmt.col_expr(C::Avatar, Expr::value(avatar));
    }
    if let Some(asset) = stamps.avatar_asset_id {
        stmt = stmt.col_expr(C::AvatarAssetId, Expr::value(asset));
    }
    match &stamps.display_name {
        None => {}
        Some(DisplayNameUpdate::Bound(name)) => {
            stmt = stmt.col_expr(C::DisplayName, Expr::value(*name));
        }
        Some(DisplayNameUpdate::BackfillCase) => {
            stmt = stmt.col_expr(C::DisplayName, display_name_backfill_case());
        }
    }
    match &stamps.is_staff {
        None => {}
        Some(StaffSync::Value(staff)) => {
            stmt = stmt.col_expr(C::IsStaff, Expr::value(*staff));
        }
        Some(StaffSync::OrSuperuser) => {
            stmt = stmt.col_expr(C::IsStaff, col(C::IsStaff).or(col(C::IsSuperuser)));
        }
    }
    if stamps.touch_updated_at {
        stmt = stmt.col_expr(C::UpdatedAt, Expr::value(stamps.now));
    }
    stmt.filter(C::Id.eq(user_id))
}

/// `save_user_data` (`adapter/base.py:220-234`): the login stamps plus
/// the `is_active` flip in one `UPDATE`, with the `User.save()` column
/// effects (`user.py:169-187`) each caller dirtied. Runs on the
/// caller's connection — the signup/login flows never hold a
/// transaction here.
pub async fn save_user_data(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    stamps: &LoginStamps<'_>,
) -> Result<(), sqlx::Error> {
    exec_update(db, login_stamps_update(user_id, stamps)).await
}

// ---------------------------------------------------------------------------
// sync_user_data + avatar singles
// ---------------------------------------------------------------------------

/// Resolved inputs for [`sync_user_data`]: the IDP sync save
/// (`adapter/base.py:256-287`) per caller. gitlab writes names, the
/// avatar pair, the staff sync and `updated_at` in one statement;
/// gitea the same minus staff/`updated_at`; google only the names here
/// (its avatar pair lands through the singles below, after the avatar
/// round-trip); github names plus the asset id on upload success, or
/// names plus the provider URL on upload failure, always with
/// `updated_at`. `None` omits a column; `avatar_asset_id` is
/// doubly-optional (omit vs render, NULL-able on render).
pub struct SyncUpdate<'a> {
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub display_name: &'a str,
    pub avatar: Option<&'a str>,
    pub avatar_asset_id: Option<Option<Uuid>>,
    pub is_staff: Option<bool>,
    pub touch_updated_at: bool,
    pub now: DateTime<Utc>,
}

/// The sync `UPDATE` for one input combination (fixed column order:
/// names, avatar pair, staff, updated_at). Pure; unit-tested per
/// caller like [`login_stamps_update`].
fn sync_update(user_id: Uuid, sync: &SyncUpdate<'_>) -> UpdateMany<users::Entity> {
    use users::Column as C;
    let mut stmt = users::Entity::update_many()
        .col_expr(C::FirstName, Expr::value(sync.first_name))
        .col_expr(C::LastName, Expr::value(sync.last_name))
        .col_expr(C::DisplayName, Expr::value(sync.display_name));
    if let Some(avatar) = sync.avatar {
        stmt = stmt.col_expr(C::Avatar, Expr::value(avatar));
    }
    if let Some(asset) = sync.avatar_asset_id {
        stmt = stmt.col_expr(C::AvatarAssetId, Expr::value(asset));
    }
    if let Some(staff) = sync.is_staff {
        stmt = stmt.col_expr(C::IsStaff, Expr::value(staff));
    }
    if sync.touch_updated_at {
        stmt = stmt.col_expr(C::UpdatedAt, Expr::value(sync.now));
    }
    stmt.filter(C::Id.eq(user_id))
}

/// `sync_user_data` (`adapter/base.py:256-287`): the IDP sync save.
/// Runs on the caller's connection.
pub async fn sync_user_data(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    sync: &SyncUpdate<'_>,
) -> Result<(), sqlx::Error> {
    exec_update(db, sync_update(user_id, sync)).await
}

/// `UPDATE users SET avatar_asset_id = <asset> WHERE id = <user>`: the
/// signup / sync asset stamp, one statement on every path that emits
/// it (gitlab, google twice, github). Runs on the caller's connection.
pub async fn stamp_avatar_asset(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    asset_id: Uuid,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        users::Entity::update_many()
            .col_expr(users::Column::AvatarAssetId, Expr::value(asset_id))
            .filter(users::Column::Id.eq(user_id)),
    )
    .await
}

/// `UPDATE users SET avatar = <url> WHERE id = <user>`: the sync URL
/// stamp when the avatar upload fails (google twice) and the github
/// signup URL stamp after its `''` insert. Runs on the caller's
/// connection.
pub async fn stamp_avatar_url(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    avatar_url: &str,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        users::Entity::update_many()
            .col_expr(users::Column::Avatar, Expr::value(avatar_url))
            .filter(users::Column::Id.eq(user_id)),
    )
    .await
}

// ---------------------------------------------------------------------------
// OIDC sync login stamps (cloud re-login triple, last-login-time single)
// ---------------------------------------------------------------------------

/// The OIDC sync re-login save (`pi_dash_cloud/airepublic/sync.py:122-124`):
/// `update_fields = ["avatar", "last_login_medium", "is_email_verified"]`,
/// which Django re-sorts into `_meta` order — `avatar`,
/// `is_email_verified`, `last_login_medium` — and `updated_at` is NOT
/// stamped (`auto_now` only fires when `update_fields` is `None` or lists
/// it). The dirty check and the never-downgrade verified gate live with
/// the driver; when dirty, all three current values are written. A fixed
/// shape, unlike [`sync_user_data`] (always names) and [`save_user_data`]
/// (the full login-stamps statement).
fn relogin_update(
    user_id: Uuid,
    avatar: &str,
    is_email_verified: bool,
    last_login_medium: &str,
) -> UpdateMany<users::Entity> {
    users::Entity::update_many()
        .col_expr(users::Column::Avatar, Expr::value(avatar))
        .col_expr(
            users::Column::IsEmailVerified,
            Expr::value(is_email_verified),
        )
        .col_expr(
            users::Column::LastLoginMedium,
            Expr::value(last_login_medium),
        )
        .filter(users::Column::Id.eq(user_id))
}

/// See [`relogin_update`]: the fixed 3-column re-login write. Runs on
/// the caller's connection (the sync driver holds the upsert
/// transaction).
pub async fn update_user_relogin(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    avatar: &str,
    is_email_verified: bool,
    last_login_medium: &str,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        relogin_update(user_id, avatar, is_email_verified, last_login_medium),
    )
    .await
}

/// The OIDC sync every-login bump (`pi_dash_cloud/airepublic/sync.py:126-127`):
/// `update_fields = ["last_login_time"]`, a single column. Runs on the
/// caller's connection (the sync driver holds the upsert transaction).
pub async fn update_user_last_login_time(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        users::Entity::update_many()
            .col_expr(users::Column::LastLoginTime, Expr::value(at))
            .filter(users::Column::Id.eq(user_id)),
    )
    .await
}

// ---------------------------------------------------------------------------
// v2 asset link (the file-asset user writes)
// ---------------------------------------------------------------------------

/// The full `user.save()` column effects (`db/models/user.py:169-187`)
/// the asset link/unlink twins append after their own columns —
/// normalized email, filled display name, staff sync, token rotation,
/// `auto_now` `updated_at` — from the caller's
/// [`pidash_db::ops::users::UserSaveWrite`] computation.
fn with_user_save_effects(
    stmt: UpdateMany<users::Entity>,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> UpdateMany<users::Entity> {
    use users::Column as C;
    stmt.col_expr(C::Email, Expr::value(write.email.as_str()))
        .col_expr(C::DisplayName, Expr::value(write.display_name.as_str()))
        .col_expr(C::IsStaff, Expr::value(write.is_staff))
        .col_expr(C::Token, Expr::value(write.token.as_str()))
        .col_expr(C::TokenUpdatedAt, Expr::value(write.token_updated_at))
        .col_expr(C::UpdatedAt, Expr::value(now))
}

/// `UserAssetsV2Endpoint.entity_asset_save` (`v2.py:41-78`): the
/// avatar link (`avatar = ""`, the ported empty-string write) plus the
/// full `user.save()` column effects. The caller owns the user-row
/// read, the previous-asset delete and the `UserSaveWrite`
/// computation.
fn avatar_link_update(
    user_id: Uuid,
    asset_id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> UpdateMany<users::Entity> {
    with_user_save_effects(
        users::Entity::update_many()
            .col_expr(users::Column::Avatar, Expr::value(""))
            .col_expr(users::Column::AvatarAssetId, Expr::value(asset_id)),
        write,
        now,
    )
    .filter(users::Column::Id.eq(user_id))
}

/// Link an avatar asset onto one user row
/// (`avatar = ""`, the ported empty-string write).
pub async fn link_avatar_asset(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    asset_id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    exec_update(db, avatar_link_update(user_id, asset_id, write, now)).await
}

/// `UserAssetsV2Endpoint.entity_asset_save` (`v2.py:41-78`): the
/// cover link plus the full `user.save()` column effects.
/// `cover_image = NULL` here (asymmetry with avatar — ported).
fn cover_link_update(
    user_id: Uuid,
    asset_id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> UpdateMany<users::Entity> {
    with_user_save_effects(
        users::Entity::update_many()
            .col_expr(users::Column::CoverImage, Expr::value(None::<String>))
            .col_expr(users::Column::CoverImageAssetId, Expr::value(asset_id)),
        write,
        now,
    )
    .filter(users::Column::Id.eq(user_id))
}

/// Link a cover asset onto one user row.
pub async fn link_cover_asset(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    asset_id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    exec_update(db, cover_link_update(user_id, asset_id, write, now)).await
}

/// `UserAssetsV2Endpoint.entity_asset_delete` (`v2.py:80-107`):
/// clears the avatar link FK only (the `avatar` string is untouched
/// here), plus the full `user.save()` column effects.
fn avatar_unlink_update(
    user_id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> UpdateMany<users::Entity> {
    with_user_save_effects(
        users::Entity::update_many()
            .col_expr(users::Column::AvatarAssetId, Expr::value(None::<Uuid>)),
        write,
        now,
    )
    .filter(users::Column::Id.eq(user_id))
}

/// Clear one user row's avatar link FK.
pub async fn unlink_avatar_asset(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    exec_update(db, avatar_unlink_update(user_id, write, now)).await
}

/// `UserAssetsV2Endpoint.entity_asset_delete` (`v2.py:80-107`):
/// clears the cover link FK only, plus the full `user.save()`
/// column effects.
fn cover_unlink_update(
    user_id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> UpdateMany<users::Entity> {
    with_user_save_effects(
        users::Entity::update_many()
            .col_expr(users::Column::CoverImageAssetId, Expr::value(None::<Uuid>)),
        write,
        now,
    )
    .filter(users::Column::Id.eq(user_id))
}

/// Clear one user row's cover link FK.
pub async fn unlink_cover_asset(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    exec_update(db, cover_unlink_update(user_id, write, now)).await
}

// ---------------------------------------------------------------------------
// notification-preference PATCH (dynamic SET list)
// ---------------------------------------------------------------------------

/// One validated PATCH column write for
/// [`save_preference_columns`].
pub enum PreferenceColumn {
    Stamp(Option<DateTime<Utc>>),
    Id(Option<Uuid>),
    Flag(bool),
}

/// `serializer.save()` (`base.py:306`): the stamps first
/// (`updated_at`, `updated_by` — `BaseModel.save` stamps the updater
/// through crum on every update, `db/models/base.py:23-44`), then the
/// validated columns in the caller's push order (the handler's fixed
/// order), by name. Pure.
fn preference_columns_update(
    id: Uuid,
    now: DateTime<Utc>,
    user_id: Uuid,
    columns: &[(&str, PreferenceColumn)],
) -> UpdateStatement {
    use user_notification_preferences::Column as P;
    let mut stmt = Query::update();
    stmt.table(user_notification_preferences::Entity)
        .value(P::UpdatedAt, Expr::value(now))
        .value(P::UpdatedById, Expr::value(user_id));
    for (column, value) in columns {
        let value: SimpleExpr = match value {
            PreferenceColumn::Stamp(stamp) => Expr::value(*stamp),
            PreferenceColumn::Id(owner) => Expr::value(*owner),
            PreferenceColumn::Flag(flag) => Expr::value(*flag),
        };
        stmt.value(Alias::new(*column), value);
    }
    stmt.and_where(Expr::col(P::Id).eq(id)).to_owned()
}

/// Apply the validated preference PATCH columns plus the
/// `updated_at`/`updated_by` stamps. Runs on the caller's connection.
pub async fn save_preference_columns(
    db: &impl ConnectionTrait,
    id: Uuid,
    now: DateTime<Utc>,
    user_id: Uuid,
    columns: &[(&str, PreferenceColumn)],
) -> Result<(), sqlx::Error> {
    exec_stmt(db, &preference_columns_update(id, now, user_id, columns)).await
}

// ---------------------------------------------------------------------------
// get_or_create_profile (login redirection path)
// ---------------------------------------------------------------------------

/// `Profile.objects.get_or_create(user=user)` for `get_redirection_path`
/// (`redirection_path.py:11`): the live row's id, `is_onboarded` and
/// `last_workspace_id`, inserting the default row first when missing.
/// The `SELECT` projects the union of the per-path reads (`user_id` is
/// unique, so `LIMIT 1` is a no-op); the insert carries every model
/// default with `company_name ''` (only the license signup passes a
/// company, and it goes through [`create_user`]). `background_color`
/// is caller-generated (each path keeps its `'#' + six hexdigits`
/// draw). Returns the fresh `{id, false, None}` on insert — the values
/// the gitea path used to re-`SELECT`, minus the redundant read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfilePath {
    pub id: Uuid,
    pub is_onboarded: bool,
    pub last_workspace_id: Option<Uuid>,
}

async fn fetch_profile_path(
    db: &impl ConnectionTrait,
    user_id: Uuid,
) -> Result<Option<ProfilePath>, sqlx::Error> {
    let row: Option<(Uuid, bool, Option<Uuid>)> = profiles::Entity::find()
        .select_only()
        .columns([
            profiles::Column::Id,
            profiles::Column::IsOnboarded,
            profiles::Column::LastWorkspaceId,
        ])
        .filter(profiles::Column::UserId.eq(user_id))
        .into_tuple()
        .one(db)
        .await
        .map_err(into_sqlx)?;
    Ok(
        row.map(|(id, is_onboarded, last_workspace_id)| ProfilePath {
            id,
            is_onboarded,
            last_workspace_id,
        }),
    )
}

/// The `get_or_create` create leg on its own: the default profile row
/// for `user_id` ([`default_profile_row`] with `company_name ''`, the
/// same values [`get_or_create_profile`] inserts). Returns the new row
/// id.
///
/// Race contract: a lost create race surfaces SQLSTATE 23505 from the
/// `user_id` unique index. Callers outside an explicit transaction
/// re-probe on it (that is [`get_or_create_profile`]); callers inside
/// one must hold a savepoint across this call — Django's inner
/// `atomic()` nests to a savepoint there
/// (`db/models/query.py:get_or_create`) — roll back to it on 23505,
/// and re-probe. Any other error propagates untouched.
pub async fn insert_default_profile(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    now: DateTime<Utc>,
    background_color: &str,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    profiles::Entity::insert(default_profile_row(id, user_id, now, "", background_color))
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    Ok(id)
}

/// `Profile.objects.get_or_create(user=user)`, race-tolerant:
/// probe, else [`insert_default_profile`], else — on a 23505 lost
/// create race — re-probe and return the winner's row, exactly
/// Django's `except IntegrityError: return self.get(**kwargs)`
/// (`db/models/query.py:get_or_create`). When the re-probe still
/// misses, the original 23505 error is returned, never `RowNotFound`
/// (Django re-raises there); any non-23505 insert error propagates
/// without a re-probe.
///
/// Race-tolerant on connections outside an explicit transaction (the
/// pool, which is what every caller passes): a failed `INSERT` there
/// aborts no transaction, so the re-probe still runs. Callers inside
/// an explicit transaction must hold a savepoint across the whole
/// call, or compose the probe with [`insert_default_profile`] under
/// their own savepoint (its race contract) — otherwise the failed
/// insert aborts their transaction and the re-probe fails with it.
pub async fn get_or_create_profile(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    now: DateTime<Utc>,
    background_color: &str,
) -> Result<ProfilePath, sqlx::Error> {
    if let Some(path) = fetch_profile_path(db, user_id).await? {
        return Ok(path);
    }
    match insert_default_profile(db, user_id, now, background_color).await {
        Ok(id) => Ok(ProfilePath {
            id,
            is_onboarded: false,
            last_workspace_id: None,
        }),
        Err(insert) if is_unique_violation(&insert) => match fetch_profile_path(db, user_id).await?
        {
            Some(path) => Ok(path),
            None => Err(insert),
        },
        Err(insert) => Err(insert),
    }
}

/// `profile.save(update_fields=["last_workspace_id", "updated_at"])`:
/// point `last_workspace_id` at `workspace_id`, restamping
/// `updated_at` to `now`. The `SET` lists `updated_at` first —
/// `_meta` field order, where the `TimeAuditModel` `auto_now` stamp
/// (`db/mixins.py:20`) precedes every concrete `Profile` field
/// (`db/models/user.py:200-273`). Runs on the caller's connection.
pub async fn set_profile_last_workspace(
    db: &impl ConnectionTrait,
    profile_id: Uuid,
    workspace_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        profiles::Entity::update_many()
            .col_expr(profiles::Column::UpdatedAt, Expr::value(now))
            .col_expr(profiles::Column::LastWorkspaceId, Expr::value(workspace_id))
            .filter(profiles::Column::Id.eq(profile_id)),
    )
    .await
}

// ---------------------------------------------------------------------------
// accounts (moved from db::auth_oauth::queries::account)
// ---------------------------------------------------------------------------

/// Account upsert statements
/// (`adapter/oauth.py:105-136`; table `accounts`, `db/models/user.py:280-302`,
/// `unique_together = ["provider", "provider_account_id"]`,
/// `ordering = ("-created_at",)`).
///
/// Moved from `pidash_db::auth_oauth::queries::account`
/// (PIDASHCONV-1525, ORM-07) as `accounts` entity statements; the
/// update leg is named after the issue's `rotate_account_tokens`.
pub mod account {
    use super::*;

    /// Django table name (`Meta.db_table`, `user.py:298`).
    pub const TABLE: &str = "accounts";

    /// Columns in Django declaration order (audit prefix first), the order
    /// the lookup projects and the `INSERT`/`UPDATE` bind.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "user_id",
        "provider_account_id",
        "provider",
        "access_token",
        "access_token_expired_at",
        "refresh_token",
        "refresh_token_expired_at",
        "last_connected_at",
        "id_token",
        "metadata",
    ];

    /// Columns the update path writes, in bind order: the six token fields
    /// Django assigns (`oauth.py:114-119`) plus `updated_at`, which
    /// `auto_now` stamps on every `.save()`.
    pub const UPDATE_COLUMNS: &[&str] = &[
        "access_token",
        "refresh_token",
        "access_token_expired_at",
        "refresh_token_expired_at",
        "last_connected_at",
        "id_token",
        "updated_at",
    ];

    /// Lookup filter columns in bind order
    /// (`filter(user=, provider=, provider_account_id=)`, `oauth.py:108-112`).
    pub const LOOKUP_COLUMNS: &[&str] = &["user_id", "provider", "provider_account_id"];

    /// The five token fields every provider's `set_token_data` stores
    /// (fixture F4 `token_fields_consumed_downstream`).
    #[derive(Debug, Clone, PartialEq)]
    pub struct TokenFields {
        pub access_token: Option<String>,
        pub refresh_token: Option<String>,
        pub access_token_expired_at: Option<chrono::DateTime<chrono::Utc>>,
        pub refresh_token_expired_at: Option<chrono::DateTime<chrono::Utc>>,
        /// `token_data.get("id_token", "")` (`oauth.py:119,132`): `""`
        /// when the key is absent.
        pub id_token: Option<String>,
    }

    impl TokenFields {
        /// `token_data.get("id_token", "")` — absent maps to `""`.
        pub fn id_token_or_default(&self) -> String {
            self.id_token.clone().unwrap_or_default()
        }
    }

    /// `self.user_data.get("user", {}).get("provider_id")` — the tolerant
    /// create-side read (`oauth.py:126`). The lookup side
    /// (`.get("user").get(...)`, `:111`) raises `AttributeError` on a
    /// user-less payload instead; see the module docs.
    pub fn lookup_provider_id(user_data: &serde_json::Value) -> Option<&str> {
        user_data.get("user")?.get("provider_id")?.as_str()
    }

    /// Outcome of [`create_update_account`]. `ErrorSwallowed` is BUG-6,
    /// ported as-is (see module docs).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum UpsertOutcome {
        Updated,
        Created,
        ErrorSwallowed,
    }

    /// `Account.objects.filter(user_id, provider,
    /// provider_account_id).first()` — fixture F6 `emitted_sql.lookup`
    /// — projected to the update path's only need: the row id. No
    /// `ORDER BY` (the statement the port executed carried none).
    pub async fn find_account_id(
        db: &impl ConnectionTrait,
        user_id: uuid::Uuid,
        provider: &str,
        provider_account_id: &str,
    ) -> Result<Option<uuid::Uuid>, sqlx::Error> {
        accounts::Entity::find()
            .select_only()
            .column(accounts::Column::Id)
            .filter(accounts::Column::UserId.eq(user_id))
            .filter(accounts::Column::Provider.eq(provider))
            .filter(accounts::Column::ProviderAccountId.eq(provider_account_id))
            .into_tuple()
            .one(db)
            .await
            .map_err(into_sqlx)
    }

    /// Parameters for the insert side (`oauth.py:123-133`). `id` is
    /// caller-generated (`uuid4`); `now` stands in for `timezone.now()`;
    /// `metadata` defaults to `{}` when `None`.
    pub struct NewAccount<'a> {
        pub id: uuid::Uuid,
        pub user_id: uuid::Uuid,
        pub provider: &'a str,
        pub provider_account_id: &'a str,
        pub tokens: &'a TokenFields,
        pub now: chrono::DateTime<chrono::Utc>,
        pub metadata: Option<serde_json::Value>,
    }

    /// `account.save()` on the update path (`oauth.py:114-120`): Django
    /// writes every local field, so the `SET` lists all non-pk columns
    /// in field order (fixture F6 `emitted_sql.update` names the six
    /// assigned columns plus `updated_at`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn rotate_tokens_update(
        account_id: uuid::Uuid,
        created_at: chrono::DateTime<chrono::Utc>,
        user_id: uuid::Uuid,
        provider_account_id: &str,
        provider: &str,
        tokens: &TokenFields,
        now: chrono::DateTime<chrono::Utc>,
        metadata: serde_json::Value,
    ) -> UpdateMany<accounts::Entity> {
        use accounts::Column as A;
        accounts::Entity::update_many()
            .col_expr(A::CreatedAt, Expr::value(created_at))
            .col_expr(A::UpdatedAt, Expr::value(now))
            .col_expr(A::UserId, Expr::value(user_id))
            .col_expr(A::ProviderAccountId, Expr::value(provider_account_id))
            .col_expr(A::Provider, Expr::value(provider))
            .col_expr(A::AccessToken, Expr::value(tokens.access_token.clone()))
            .col_expr(
                A::AccessTokenExpiredAt,
                Expr::value(tokens.access_token_expired_at),
            )
            .col_expr(A::RefreshToken, Expr::value(tokens.refresh_token.clone()))
            .col_expr(
                A::RefreshTokenExpiredAt,
                Expr::value(tokens.refresh_token_expired_at),
            )
            .col_expr(A::LastConnectedAt, Expr::value(now))
            .col_expr(A::IdToken, Expr::value(tokens.id_token_or_default()))
            .col_expr(A::Metadata, Expr::value(metadata))
            .filter(A::Id.eq(account_id))
    }

    /// The update side (`oauth.py:113-120`): assign the six token fields
    /// plus `last_connected_at = now`, then `.save()`.
    #[allow(clippy::too_many_arguments)]
    pub async fn rotate_account_tokens(
        db: &impl ConnectionTrait,
        account_id: uuid::Uuid,
        created_at: chrono::DateTime<chrono::Utc>,
        user_id: uuid::Uuid,
        provider_account_id: &str,
        provider: &str,
        tokens: &TokenFields,
        now: chrono::DateTime<chrono::Utc>,
        metadata: serde_json::Value,
    ) -> Result<(), sqlx::Error> {
        exec_update(
            db,
            rotate_tokens_update(
                account_id,
                created_at,
                user_id,
                provider_account_id,
                provider,
                tokens,
                now,
                metadata,
            ),
        )
        .await
    }

    /// `Account.objects.create(...)` (`oauth.py:123-133`): a client-side
    /// `uuid4` id (the caller supplies it), `metadata` falling back to
    /// `{}`, timestamps `now`. The nullable-input `Option`s bind straight
    /// through (a missing access token is NULL into NOT NULL and raises,
    /// as before).
    pub(super) fn new_account_insert(
        params: &NewAccount<'_>,
    ) -> sea_orm::sea_query::InsertStatement {
        Query::insert()
            .into_table(accounts::Entity)
            .columns([
                accounts::Column::Id,
                accounts::Column::CreatedAt,
                accounts::Column::UpdatedAt,
                accounts::Column::UserId,
                accounts::Column::ProviderAccountId,
                accounts::Column::Provider,
                accounts::Column::AccessToken,
                accounts::Column::AccessTokenExpiredAt,
                accounts::Column::RefreshToken,
                accounts::Column::RefreshTokenExpiredAt,
                accounts::Column::LastConnectedAt,
                accounts::Column::IdToken,
                accounts::Column::Metadata,
            ])
            .values_panic([
                Expr::value(params.id),
                Expr::value(params.now),
                Expr::value(params.now),
                Expr::value(params.user_id),
                Expr::value(params.provider_account_id),
                Expr::value(params.provider),
                Expr::value(params.tokens.access_token.clone()),
                Expr::value(params.tokens.access_token_expired_at),
                Expr::value(params.tokens.refresh_token.clone()),
                Expr::value(params.tokens.refresh_token_expired_at),
                Expr::value(params.now),
                Expr::value(params.tokens.id_token_or_default()),
                Expr::value(
                    params
                        .metadata
                        .clone()
                        .unwrap_or_else(|| serde_json::json!({})),
                ),
            ])
            .to_owned()
    }

    /// The create side (`oauth.py:123-133`).
    pub async fn create_account(
        db: &impl ConnectionTrait,
        params: &NewAccount<'_>,
    ) -> Result<(), sqlx::Error> {
        db.execute(DbBackend::Postgres.build(&new_account_insert(params)))
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// `create_update_account` (`oauth.py:105-136`): filter-first, then
    /// update or create. `DatabaseError`/`IntegrityError` are swallowed via
    /// `log_exception` (BUG-6) — a database failure returns
    /// `Ok(UpsertOutcome::ErrorSwallowed)`, never `Err`.
    ///
    /// Runs on the caller's connection (an acquired connection through
    /// `TxConn`, or `TxConn::new(&mut tx)` inside a larger transaction —
    /// Python runs each statement in autocommit, so both are faithful).
    ///
    /// `created_at`/`metadata` for the update side come from the looked-up
    /// row; pass them back in (Django's `.save()` rewrites them unchanged).
    /// `provider_account_id` is `None` when the payload has no
    /// `user.provider_id` (the strict lookup side would have raised
    /// `AttributeError`; here the lookup is skipped and the create side
    /// runs with an empty provider id, matching `.get("user",
    /// {}).get("provider_id")` → `None`).
    #[allow(clippy::too_many_arguments)]
    pub async fn create_update_account(
        db: &impl ConnectionTrait,
        user_id: uuid::Uuid,
        provider: &str,
        user_data: &serde_json::Value,
        tokens: &TokenFields,
        now: chrono::DateTime<chrono::Utc>,
        new_id: uuid::Uuid,
        existing: Option<ExistingAccount>,
    ) -> Result<UpsertOutcome, sqlx::Error> {
        let provider_account_id = lookup_provider_id(user_data);
        let found = match provider_account_id {
            Some(pid) => find_account_id(db, user_id, provider, pid).await,
            None => Ok(None),
        };
        let found = match found {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("Error in create_update_account lookup: {e}");
                return Ok(UpsertOutcome::ErrorSwallowed);
            }
        };
        if let Some(account_id) = found {
            let prior = existing.unwrap_or(ExistingAccount {
                created_at: now,
                metadata: serde_json::json!({}),
            });
            let res = rotate_account_tokens(
                db,
                account_id,
                prior.created_at,
                user_id,
                provider_account_id.unwrap_or(""),
                provider,
                tokens,
                now,
                prior.metadata,
            )
            .await;
            return match res {
                Ok(()) => Ok(UpsertOutcome::Updated),
                Err(e) => {
                    tracing::warn!("Error in create_update_account update: {e}");
                    Ok(UpsertOutcome::ErrorSwallowed)
                }
            };
        }
        let params = NewAccount {
            id: new_id,
            user_id,
            provider,
            provider_account_id: provider_account_id.unwrap_or(""),
            tokens,
            now,
            metadata: None,
        };
        match create_account(db, &params).await {
            Ok(()) => Ok(UpsertOutcome::Created),
            Err(e) => {
                tracing::warn!("Error in create_update_account create: {e}");
                Ok(UpsertOutcome::ErrorSwallowed)
            }
        }
    }

    /// The unchanged columns the update side rewrites verbatim
    /// (`created_at`, `metadata`; `user_id`/`provider`/`provider_account_id`
    /// are untouched per fixture F6 `after_state.update.untouched`).
    pub struct ExistingAccount {
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub metadata: serde_json::Value,
    }

    // -----------------------------------------------------------------------
    // Cloud account update legs (design D6; the overlay calls these instead
    // of its cloud-db builders)
    // -----------------------------------------------------------------------

    /// Parameters for [`update_account_upsert`]: the row as fetched and
    /// mutated by the caller (Django's `obj.save()` shape), plus the row
    /// id. `created_at` re-binds the stored value, exactly like Django;
    /// `now` stamps `updated_at`. Field for field, this is what the
    /// overlay `update_account_upsert` builder reads off its
    /// `IdentityAccount`.
    pub struct AccountUpsert<'a> {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub user_id: uuid::Uuid,
        pub access_token: &'a str,
        pub access_token_expired_at: Option<chrono::DateTime<chrono::Utc>>,
        pub refresh_token: Option<&'a str>,
        pub refresh_token_expired_at: Option<chrono::DateTime<chrono::Utc>>,
        pub last_connected_at: chrono::DateTime<chrono::Utc>,
        pub id_token: &'a str,
        pub metadata: Option<&'a serde_json::Value>,
    }

    /// The cloud sync update side (`Account.objects.update_or_create`,
    /// `pi_dash_cloud/airepublic/sync.py:146-159`): the eight `defaults`
    /// columns plus `created_at` (re-set to its stored value) and
    /// `updated_at` (re-stamped), in Django `_meta` order. The lookup
    /// columns (`provider`, `provider_account_id`) are excluded —
    /// probe-verified Django behavior, and why the overlay cannot use
    /// [`rotate_account_tokens`]: that OSS `oauth.py` full save rewrites
    /// all 13 columns including the Tier-1 provider pair.
    pub(super) fn upsert_update(
        row: &AccountUpsert<'_>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> UpdateMany<accounts::Entity> {
        use accounts::Column as A;
        accounts::Entity::update_many()
            .col_expr(A::CreatedAt, Expr::value(row.created_at))
            .col_expr(A::UpdatedAt, Expr::value(now))
            .col_expr(A::UserId, Expr::value(row.user_id))
            .col_expr(A::AccessToken, Expr::value(row.access_token))
            .col_expr(
                A::AccessTokenExpiredAt,
                Expr::value(row.access_token_expired_at),
            )
            .col_expr(A::RefreshToken, Expr::value(row.refresh_token))
            .col_expr(
                A::RefreshTokenExpiredAt,
                Expr::value(row.refresh_token_expired_at),
            )
            .col_expr(A::LastConnectedAt, Expr::value(row.last_connected_at))
            .col_expr(A::IdToken, Expr::value(row.id_token))
            .col_expr(A::Metadata, Expr::value(row.metadata.cloned()))
            .filter(A::Id.eq(row.id))
    }

    /// The cloud sync update side (`sync.py:146-159`): ten columns, the
    /// lookup pair excluded. Runs on the caller's pool or transaction
    /// (the overlay sync holds one for the whole
    /// `update_or_create`).
    pub async fn update_account_upsert(
        db: &impl ConnectionTrait,
        row: &AccountUpsert<'_>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), sqlx::Error> {
        exec_update(db, upsert_update(row, now)).await
    }

    /// `persist_rotated_tokens`
    /// (`pi_dash_cloud/airepublic/token_store.py:139-150`, twin at
    /// `refresh.py:154-164`): `save(update_fields=["access_token",
    /// "refresh_token", "access_token_expired_at"])`, emitted in `_meta`
    /// order with no `updated_at` stamp (probe-verified).
    pub(super) fn rotated_tokens_update(
        account_id: uuid::Uuid,
        access_token: &str,
        access_token_expired_at: Option<chrono::DateTime<chrono::Utc>>,
        refresh_token: Option<&str>,
    ) -> UpdateMany<accounts::Entity> {
        use accounts::Column as A;
        accounts::Entity::update_many()
            .col_expr(A::AccessToken, Expr::value(access_token))
            .col_expr(
                A::AccessTokenExpiredAt,
                Expr::value(access_token_expired_at),
            )
            .col_expr(A::RefreshToken, Expr::value(refresh_token))
            .filter(A::Id.eq(account_id))
    }

    /// The token-rotation write (`token_store.py:139-150`): always all
    /// three columns, never `updated_at`. The keep-vs-replace value
    /// rules (empty grant token, zero `expires_in`) live with the
    /// caller, which passes the resolved values in.
    pub async fn update_account_rotated_tokens(
        db: &impl ConnectionTrait,
        account_id: uuid::Uuid,
        access_token: &str,
        access_token_expired_at: Option<chrono::DateTime<chrono::Utc>>,
        refresh_token: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        exec_update(
            db,
            rotated_tokens_update(
                account_id,
                access_token,
                access_token_expired_at,
                refresh_token,
            ),
        )
        .await
    }

    /// `_persist_plan_snapshot`
    /// (`pi_dash_cloud/airepublic/refresh.py:167-197`):
    /// `save(update_fields=["metadata"])`. The unchanged-skip check lives
    /// with the caller.
    pub async fn update_account_metadata(
        db: &impl ConnectionTrait,
        account_id: uuid::Uuid,
        metadata: &serde_json::Value,
    ) -> Result<(), sqlx::Error> {
        exec_update(
            db,
            accounts::Entity::update_many()
                .col_expr(accounts::Column::Metadata, Expr::value(metadata.clone()))
                .filter(accounts::Column::Id.eq(account_id)),
        )
        .await
    }

    /// Pure row transform mirroring the update side for fixture
    /// before/after assertions: assign the token fields + `last_connected_at`
    /// over a JSON row shaped like F6 `before_state.case_a_exists`.
    pub fn apply_update_row(
        before: &serde_json::Value,
        tokens: &TokenFields,
        now: &str,
    ) -> serde_json::Value {
        let mut after = before.clone();
        after["access_token"] = tokens
            .access_token
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::from);
        after["refresh_token"] = tokens
            .refresh_token
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::from);
        after["access_token_expired_at"] = tokens
            .access_token_expired_at
            .map(|dt| serde_json::Value::from(dt.to_rfc3339()))
            .unwrap_or(serde_json::Value::Null);
        after["refresh_token_expired_at"] = tokens
            .refresh_token_expired_at
            .map(|dt| serde_json::Value::from(dt.to_rfc3339()))
            .unwrap_or(serde_json::Value::Null);
        after["last_connected_at"] = serde_json::Value::from(now);
        after["id_token"] = serde_json::Value::from(tokens.id_token_or_default());
        after
    }
}

// ---------------------------------------------------------------------------
// app_workspace user templates (PIDASHCONV-1631; relocated verbatim from
// `crate::app_workspace::queries_user`, whose `pub use` shims keep the
// handler paths stable). The api handlers splice these `:placeholder`
// templates with `positional()` plus model-owned columns, so the writes
// execute handler-side until those call sites migrate to executing twins.
// ---------------------------------------------------------------------------

/// Email save (`:232-235`): `user.email = new_email`,
/// `user.is_email_verified = False`, `user.save()`.
///
/// Django emits a full-row `UPDATE`; per the D-24 queries convention
/// the builder lists the semantically-changed columns plus the
/// `auto_now` stamp. `User.save()` (`db/models/user.py:154-177`)
/// additionally lowercases the email (no-op — already normalized),
/// backfills an empty `display_name` from the address, and rotates
/// `token` iff `token_updated_at` is set (model-owned, PIDASHCONV-607).
pub fn email_save_sql() -> String {
    "UPDATE users SET email = :email, is_email_verified = FALSE, updated_at = :now WHERE id = :pk"
        .to_owned()
}

/// Profile reset (`:319-339`): `Profile.objects.get(user=user)` (see
/// [`crate::app_workspace::queries_user::profile_by_user_sql`]) then
/// `save(update_fields=["last_workspace_id", "is_tour_completed",
/// "is_onboarded", "onboarding_step", "updated_at"])`.
///
/// Instance `save()` addresses the row by pk. `last_workspace_id` is
/// a plain nullable `UUIDField`, not an FK (`user.py:236`).
pub fn deactivate_profile_reset_sql() -> String {
    "UPDATE profiles SET last_workspace_id = NULL, is_tour_completed = FALSE, is_onboarded = FALSE, onboarding_step = :step_json, updated_at = :now WHERE id = :pk".to_owned()
}

/// User deactivation save (`:342-349`): `is_password_autoset = True`,
/// `set_password(uuid4().hex)`, `is_active = False`,
/// `last_logout_ip/time` stamps, full `save()`.
///
/// Django emits a full-row `UPDATE`; per convention the builder lists
/// the semantically-changed columns plus `updated_at`. `:hash` is the
/// PBKDF2 rendering of a fresh `uuid4().hex` (Django `set_password`
/// format — the handler hashes and binds). `last_logout_ip`
/// comes from `user_ip(request)` (`:347`), `last_logout_time` from
/// `timezone.now()` (`:348`). (`user_deactivation_email.delay(...)`
/// at `:352` is owned by PIDASHCONV-614; `logout(request)` at `:355`
/// is [`crate::app_workspace::queries_user::session_flush_sql`].)
pub fn deactivate_user_save_sql() -> String {
    "UPDATE users SET password = :hash, is_password_autoset = TRUE, is_active = FALSE, last_logout_ip = :ip, last_logout_time = :now, updated_at = :now WHERE id = :pk".to_owned()
}

/// Account delete (`:419`): instance `.delete()` on a model WITHOUT
/// the soft-delete mixin — a HARD `DELETE` addressed by pk, with no
/// related-object task.
pub fn account_hard_delete_sql() -> String {
    "DELETE FROM accounts WHERE accounts.id = :pk".to_owned()
}

/// Onboard patch (`:375-380`): `Profile.objects.get(user_id=...)`
/// (see [`crate::app_workspace::queries_user::profile_by_user_sql`])
/// then `save(update_fields=["is_onboarded", "updated_at"])`.
///
/// Ported bug 6: the value is the raw
/// `request.data.get("is_onboarded", False)` (`:376`) — no serializer
/// validation or coercion runs; whatever JSON value the client sent
/// binds at `:value` (defaulting to `False` only when the key is
/// absent).
pub fn onboard_patch_sql() -> String {
    "UPDATE profiles SET is_onboarded = :value, updated_at = :now WHERE id = :pk".to_owned()
}

/// Tour patch (`:386-388`): same shape as [`onboard_patch_sql`] for
/// `is_tour_completed` (`request.data.get("is_tour_completed",
/// False)`, `:387` — likewise unvalidated, ported bug 6).
pub fn tour_patch_sql() -> String {
    "UPDATE profiles SET is_tour_completed = :value, updated_at = :now WHERE id = :pk".to_owned()
}

/// `profiles` concrete columns in `_meta` order minus the pk:
/// the full-row `UPDATE` SET set for [`profile_full_save_sql`]
/// (`user.py:223-271` declared order over the `TimeAuditModel` pair —
/// abstract parents first, then declared fields).
pub const PROFILE_SAVE_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "user_id",
    "theme",
    "is_app_rail_docked",
    "is_tour_completed",
    "onboarding_step",
    "use_case",
    "role",
    "is_onboarded",
    "last_workspace_id",
    "billing_address_country",
    "billing_address",
    "has_billing_address",
    "company_name",
    "notification_view_mode",
    "is_smooth_cursor_enabled",
    "is_mobile_onboarded",
    "mobile_onboarding_step",
    "mobile_timezone_auto_set",
    "language",
    "start_of_the_week",
    "goals",
    "background_color",
    "is_navigation_tour_completed",
    "has_marketing_email_consent",
    "is_subscribed_to_changelog",
    "product_tour",
    "settings",
];

/// Profile patch save (`:456-461`): `ProfileSerializer.save()` issues
/// a full-row `UPDATE` — `settings` included, since the instance
/// carries it whether or not the serializer may write it (`:448-449`).
///
/// The `settings` merge (`user_settings.merge_settings`, `:460`) runs
/// BEFORE this write and is handler-owned (PIDASHCONV-620); by the
/// time this statement executes, `:settings` already holds the merged
/// bag. Every column renders as `col = :col`; the row is addressed
/// by pk.
pub fn profile_full_save_sql() -> String {
    let mut set_clause = String::new();
    for (i, col) in PROFILE_SAVE_COLUMNS.iter().enumerate() {
        if i > 0 {
            set_clause.push_str(", ");
        }
        let _ = write!(set_clause, "{col} = :{col}");
    }
    format!("UPDATE profiles SET {set_clause} WHERE id = :pk")
}

// ---------------------------------------------------------------------------
// ops user saves (PIDASHCONV-1631; relocated from `pidash_db::ops::users`,
// which cannot own covered-table writes). Management commands run each
// statement in autocommit, so the twins run on the caller's connection
// with `Utc::now()` sampled inside, as before.
// ---------------------------------------------------------------------------

/// `user.is_active = True; user.save()` (`activate_user.py:35-36`):
/// `save()` rewrites the row with the normalized email, filled
/// display name, staff escalation, possible token rotation, and the
/// `auto_now` `updated_at`.
fn user_activation_update(
    id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> UpdateMany<users::Entity> {
    with_user_save_effects(
        users::Entity::update_many().col_expr(users::Column::IsActive, Expr::value(true)),
        write,
        now,
    )
    .filter(users::Column::Id.eq(id))
}

/// See [`user_activation_update`]. Runs on the caller's connection.
pub async fn save_user_activation(
    db: &impl ConnectionTrait,
    id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
) -> Result<(), sqlx::Error> {
    exec_update(db, user_activation_update(id, write, Utc::now())).await
}

/// `user.set_password(password); user.is_password_autoset = False;
/// user.save()` (`reset_password.py:62-64`).
fn user_password_update(
    id: Uuid,
    encoded_password: &str,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> UpdateMany<users::Entity> {
    with_user_save_effects(
        users::Entity::update_many()
            .col_expr(users::Column::Password, Expr::value(encoded_password))
            .col_expr(users::Column::IsPasswordAutoset, Expr::value(false)),
        write,
        now,
    )
    .filter(users::Column::Id.eq(id))
}

/// See [`user_password_update`]. Runs on the caller's connection.
pub async fn save_user_password(
    db: &impl ConnectionTrait,
    id: Uuid,
    encoded_password: &str,
    write: &pidash_db::ops::users::UserSaveWrite,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        user_password_update(id, encoded_password, write, Utc::now()),
    )
    .await
}

// ---------------------------------------------------------------------------
// v1 asset full-row user save (PIDASHCONV-1631; relocated verbatim from
// `pidash_db::v1_assets::asset_queries`). The template follows the
// db-owned [`pidash_db::v1_assets::asset_queries::USER_COLUMNS`] list, so
// a Django user-model sync cannot silently desync the SET.
// ---------------------------------------------------------------------------

/// `entity_asset_delete` user write (`asset.py:63-64,69-70`, server
/// twins `:261-262,:267-268`): `user.save()` with no `update_fields`
/// is a **full-row** save — every non-pk column in `_meta` order, pk
/// in `WHERE`. The template renders exactly that: `SET` over
/// [`pidash_db::v1_assets::asset_queries::USER_COLUMNS`] minus `id`
/// (`$1`–`$39`), `WHERE "users"."id" = ($40)`. Handlers bind the fetched
/// row with `AvatarAssetId` / `CoverImageAssetId` nulled (see
/// [`pidash_db::v1_assets::asset_queries::EntityClearTarget`]); the
/// `NoOp` arm binds the row unchanged (and the handler skips the
/// statement entirely — the view returns without touching the profile).
///
/// `User.save()` also lowercases/strips the email and rotates the
/// token when `token_updated_at` is set (`db/models/user.py:162-170`);
/// that value-shaping is handler-owned, like every other Python-side
/// assignment on this path. A `$n` text template like the
/// `:placeholder` ones above (the executing handler is an api caller):
/// the text is the former sea-query render, byte for byte.
pub fn entity_user_save_sql() -> String {
    use pidash_db::v1_assets::asset_queries::USER_COLUMNS;
    let mut set_clause = String::new();
    let mut bind = 1u32;
    for col in USER_COLUMNS {
        if *col == "id" {
            continue;
        }
        if bind > 1 {
            set_clause.push_str(", ");
        }
        let _ = write!(set_clause, "\"{col}\" = ${bind}");
        bind += 1;
    }
    format!("UPDATE \"users\" SET {set_clause} WHERE \"users\".\"id\" = (${bind})")
}

// login/logout signal stamps + password writes + profile pointers
// (PIDASHCONV-1628: moved from the api handlers)
// ---------------------------------------------------------------------------

/// `update_last_login` (`auth/models.py`): `login()` sets
/// `last_login = now` on every login. One canonical statement serves
/// the four login paths (email, magic, OAuth, license admin): the
/// email path's id-first bind order and the magic path's qualified
/// `WHERE "users"."id"` were spelling-only — same column, same row,
/// same value — and are normalized here (the `login_stamps` precedent:
/// no fixture pins the spelling). Runs on the caller's connection —
/// the login flows never hold a transaction here.
pub async fn stamp_last_login(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        users::Entity::update_many()
            .col_expr(users::Column::LastLogin, Expr::value(now))
            .filter(users::Column::Id.eq(user_id)),
    )
    .await
}

/// Django's `check_password` setter (`base_user.py:111-123`): a
/// correct password on an outdated hash re-encodes with the default
/// hasher at once. `save(update_fields=["password"])` writes only
/// that column (no `updated_at` bump). One statement on all three
/// paths (email sign-in, change-password, license admin sign-in).
/// Runs on the caller's connection.
pub async fn rehash_password(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    new_hash: &str,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        users::Entity::update_many()
            .col_expr(users::Column::Password, Expr::value(new_hash))
            .filter(users::Column::Id.eq(user_id)),
    )
    .await
}

/// Resolved inputs for [`write_password_with_save_effects`]:
/// `fresh_token` is caller-generated (`uuid4hex + uuid4hex`,
/// `user.py:174`); `rotated_at` is the caller's rotation timestamp
/// (bound only when a token is already in use).
pub struct PasswordSave<'a> {
    pub encoded: &'a str,
    pub fresh_token: &'a str,
    pub rotated_at: DateTime<Utc>,
    /// The change/set-password twin (the `user_logged_in` signal bumps
    /// `last_login`) over reset-password.
    pub stamp_last_login: bool,
}

/// Change/set/reset-password `UPDATE`: the password write with the
/// `User.save()` column effects folded in
/// (`db/models/user.py:169-187`) — `updated_at` (`auto_now`, the
/// database clock), the `token`/`token_updated_at` rotation when a
/// token is already in use (PB-1079), the empty-`display_name` fill
/// from the email prefix, and `is_staff` for superusers — plus, on the
/// change/set twin only, the `user_logged_in` signal's `last_login`
/// bump (the database clock too). Moved from
/// `api::auth_session::email` (PIDASHCONV-1628). The `email`
/// re-normalization in `save()` is a no-op on these paths (the
/// address never changes), so it stays out.
fn password_save_update(user_id: Uuid, save: &PasswordSave<'_>) -> UpdateMany<users::Entity> {
    use users::Column as C;
    let (token, token_updated_at) =
        token_rotation_cases(save.fresh_token, Expr::value(save.rotated_at));
    let mut stmt = users::Entity::update_many()
        .col_expr(C::Password, Expr::value(save.encoded))
        .col_expr(C::IsPasswordAutoset, Expr::value(false));
    if save.stamp_last_login {
        stmt = stmt.col_expr(C::LastLogin, Expr::current_timestamp().into());
    }
    stmt.col_expr(C::UpdatedAt, Expr::current_timestamp().into())
        .col_expr(C::Token, token)
        .col_expr(C::TokenUpdatedAt, token_updated_at)
        .col_expr(C::DisplayName, display_name_backfill_coalesce_case())
        .col_expr(C::IsStaff, staff_for_superuser_case())
        .filter(C::Id.eq(user_id))
}

/// Password write shared by change/set/reset-password: the encoded
/// password plus the `User.save()` column effects above. Runs on the
/// caller's connection.
pub async fn write_password_with_save_effects(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    save: &PasswordSave<'_>,
) -> Result<(), sqlx::Error> {
    exec_update(db, password_save_update(user_id, save)).await
}

/// The sign-out stamps: `last_logout_ip`/`last_logout_time` and the
/// full `user.save()` that follows them (`views/app/signout.py:19-23`,
/// `views/space/signout.py:51-54`, `license/api/views/admin.py:388-391`),
/// so every `User.save()` column effect applies (`user.py:169-187`,
/// NEWPIDASH-17): `updated_at` (`auto_now`), the `token`/`token_updated_at`
/// rotation when a token is already in use, the empty-`display_name` fill
/// from the email prefix, and `is_staff` for superusers — the same
/// effects as the password writes above. The `email` re-normalization is
/// a no-op (the address never changes here), so it stays out. One
/// canonical statement serves the app, space and license admin sign-outs.
/// Both callers bind `None` when no IP is present (NEWPIDASH-42 brought
/// the admin path in line; it used to bind `""`).
fn logout_update(
    user_id: Uuid,
    ip: Option<&str>,
    now: DateTime<Utc>,
    fresh_token: &str,
) -> UpdateMany<users::Entity> {
    use users::Column as C;
    let (token, token_updated_at) = token_rotation_cases(fresh_token, Expr::value(now));
    users::Entity::update_many()
        .col_expr(C::LastLogoutIp, Expr::value(ip))
        .col_expr(C::LastLogoutTime, Expr::value(now))
        .col_expr(C::UpdatedAt, Expr::value(now))
        .col_expr(C::Token, token)
        .col_expr(C::TokenUpdatedAt, token_updated_at)
        .col_expr(C::DisplayName, display_name_backfill_coalesce_case())
        .col_expr(C::IsStaff, staff_for_superuser_case())
        .filter(C::Id.eq(user_id))
}

/// Stamp the sign-out columns plus the `User.save()` effects for one
/// user. The rotated token is drawn here (`uuid4().hex + uuid4().hex`,
/// `user.py:174`). Runs on the caller's connection.
pub async fn stamp_logout(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    ip: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    exec_update(db, logout_update(user_id, ip, now, &token)).await
}

/// The invite-accept user touch (`invite.py:204-205`): `User` has no
/// `last_workspace_id` column (it lives on `Profile`,
/// `db/models/user.py:236`), so Django drops the transient attribute
/// and the save only stamps `updated_at` (ported bug 11,
/// [`crate::app_workspace::queries_membership::user_touch_sql`]).
/// Runs on the caller's connection.
pub async fn touch_user_updated_at(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        users::Entity::update_many()
            .col_expr(users::Column::UpdatedAt, Expr::value(now))
            .filter(users::Column::Id.eq(user_id)),
    )
    .await
}

/// The `PATCH users/me/` `User.save()`: the merged row (validated
/// patch over the current row) plus the save-derived values
/// (`save_email`, `save_display_name`, token regeneration,
/// `save_is_staff`, `user.py:169-187`).
///
/// Moved from `api::app_workspace::handlers_user` (PIDASHCONV-1628):
/// the fourteen merged columns, then the four derived ones, then the
/// `auto_now` stamp, in that order. The merge and the derivation stay
/// at the call site (pure computation, not SQL); every resolved value
/// rides this struct.
pub struct MeProfileSave<'a> {
    pub last_login: Option<DateTime<Utc>>,
    pub display_name: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_asset_id: Option<Uuid>,
    pub cover_image: Option<&'a str>,
    pub cover_image_asset_id: Option<Uuid>,
    pub is_password_expired: bool,
    pub is_password_reset_required: bool,
    pub bot_type: Option<&'a str>,
    pub user_timezone: &'a str,
    pub is_email_valid: bool,
    pub masked_at: Option<DateTime<Utc>>,
    pub email: &'a str,
    pub token: &'a str,
    pub token_updated_at: Option<DateTime<Utc>>,
    pub is_staff: bool,
    pub updated_at: DateTime<Utc>,
}

/// See [`MeProfileSave`]: the merged columns, the derived ones, the
/// stamp.
fn me_profile_update(user_id: Uuid, save: &MeProfileSave<'_>) -> UpdateMany<users::Entity> {
    use users::Column as C;
    users::Entity::update_many()
        .col_expr(C::LastLogin, Expr::value(save.last_login))
        .col_expr(C::DisplayName, Expr::value(save.display_name))
        .col_expr(C::FirstName, Expr::value(save.first_name))
        .col_expr(C::LastName, Expr::value(save.last_name))
        .col_expr(C::Avatar, Expr::value(save.avatar))
        .col_expr(C::AvatarAssetId, Expr::value(save.avatar_asset_id))
        .col_expr(C::CoverImage, Expr::value(save.cover_image))
        .col_expr(C::CoverImageAssetId, Expr::value(save.cover_image_asset_id))
        .col_expr(C::IsPasswordExpired, Expr::value(save.is_password_expired))
        .col_expr(
            C::IsPasswordResetRequired,
            Expr::value(save.is_password_reset_required),
        )
        .col_expr(C::BotType, Expr::value(save.bot_type))
        .col_expr(C::UserTimezone, Expr::value(save.user_timezone))
        .col_expr(C::IsEmailValid, Expr::value(save.is_email_valid))
        .col_expr(C::MaskedAt, Expr::value(save.masked_at))
        .col_expr(C::Email, Expr::value(save.email))
        .col_expr(C::Token, Expr::value(save.token))
        .col_expr(C::TokenUpdatedAt, Expr::value(save.token_updated_at))
        .col_expr(C::IsStaff, Expr::value(save.is_staff))
        .col_expr(C::UpdatedAt, Expr::value(save.updated_at))
        .filter(C::Id.eq(user_id))
}

/// Persist the merged `PATCH users/me/` row. Runs on the caller's
/// connection.
pub async fn save_me_profile(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    save: &MeProfileSave<'_>,
) -> Result<(), sqlx::Error> {
    exec_update(db, me_profile_update(user_id, save)).await
}

/// Shared `delete_old_avatar` user leg (`adapter/base.py:244-248`):
/// clear the FK plus the avatar string after the asset soft-delete
/// lands. Runs on the caller's connection.
pub async fn clear_avatar(db: &impl ConnectionTrait, user_id: Uuid) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        users::Entity::update_many()
            .col_expr(users::Column::AvatarAssetId, Expr::value(None::<Uuid>))
            .col_expr(users::Column::Avatar, Expr::value(""))
            .filter(users::Column::Id.eq(user_id)),
    )
    .await
}

/// `remove_last_workspace_ids_from_user_settings`: null every profile
/// pointer at a deleted workspace. Runs on the caller's connection.
pub async fn clear_last_workspace(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        profiles::Entity::update_many()
            .col_expr(profiles::Column::LastWorkspaceId, Expr::value(None::<Uuid>))
            .filter(profiles::Column::LastWorkspaceId.eq(workspace_id)),
    )
    .await
}

/// The join-request approve pointer write (`join_request.py:204`):
/// `Profile.last_workspace_id` follows the approved workspace. Runs
/// on the caller's connection — the approve path holds its own `sqlx`
/// transaction around the member write, this write and the status
/// write.
pub async fn set_last_workspace(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        profiles::Entity::update_many()
            .col_expr(profiles::Column::LastWorkspaceId, Expr::value(workspace_id))
            .filter(profiles::Column::UserId.eq(user_id)),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::orm_tx::TxConn;
    use sqlx::Row;

    fn orm(pool: &sqlx::PgPool) -> sea_orm::DatabaseConnection {
        pidash_db::pool::sea_orm_connection(pool)
    }

    fn fixture(name: &str) -> serde_json::Value {
        let body = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/auth_oauth")
                .join(name),
        )
        .unwrap_or_else(|e| panic!("read {name}: {e}"));
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("{name} is valid JSON: {e}"))
    }

    fn token_fields() -> account::TokenFields {
        account::TokenFields {
            access_token: Some("TOK".to_owned()),
            refresh_token: None,
            access_token_expired_at: None,
            refresh_token_expired_at: None,
            id_token: Some(String::new()),
        }
    }

    // -- moved F6 pins (code side; fixture-shape assertions stay in db) ---

    #[test]
    fn f6_update_row_transform_matches_before_after() {
        let f6 = fixture("F6_account_upsert.before_after.json");
        let before = &f6["before_state"]["case_a_exists"]["rows"][0];
        let after = account::apply_update_row(before, &token_fields(), "<now>");
        assert_eq!(after["access_token"], serde_json::json!("TOK"));
        assert_eq!(after["user_id"], before["user_id"], "user_id untouched");
        assert_eq!(after["provider"], before["provider"], "provider untouched");
        assert_eq!(
            after["provider_account_id"], before["provider_account_id"],
            "provider_account_id untouched"
        );
        assert_ne!(
            after["last_connected_at"], before["last_connected_at"],
            "last_connected_at restamped"
        );
        assert_eq!(after["last_connected_at"], serde_json::json!("<now>"));
        assert_eq!(after["id_token"], serde_json::json!(""));
        assert_eq!(after["refresh_token"], serde_json::Value::Null);
        assert_eq!(after["access_token_expired_at"], serde_json::Value::Null);
        assert_eq!(after["refresh_token_expired_at"], serde_json::Value::Null);
    }

    #[test]
    fn f6_lookup_provider_id_tolerant_read() {
        assert_eq!(
            account::lookup_provider_id(&serde_json::json!({"user": {"provider_id": "gp-1"}})),
            Some("gp-1")
        );
        assert_eq!(account::lookup_provider_id(&serde_json::json!({})), None);
        assert_eq!(
            account::lookup_provider_id(&serde_json::json!({"user": {}})),
            None
        );
    }

    #[test]
    fn f6_error_outcome_variant() {
        assert_eq!(
            format!("{:?}", account::UpsertOutcome::ErrorSwallowed),
            "ErrorSwallowed"
        );
    }

    // -- cloud account update legs (D6; SET lists mirror the overlay builders)

    // -- stamps builder vectors (one exact text per caller) ------------------

    fn stamps_fixture() -> LoginStamps<'static> {
        LoginStamps {
            medium: None,
            login_ip: None,
            login_uagent: None,
            avatar: None,
            avatar_asset_id: None,
            display_name: None,
            is_staff: None,
            touch_updated_at: false,
            rotated_token: "tok",
            now: DateTime::from_timestamp(0, 0).expect("epoch"),
        }
    }

    // -- sync builder vectors -------------------------------------------------

    fn sync_fixture() -> SyncUpdate<'static> {
        SyncUpdate {
            first_name: "a",
            last_name: "b",
            display_name: "c",
            avatar: None,
            avatar_asset_id: None,
            is_staff: None,
            touch_updated_at: false,
            now: DateTime::from_timestamp(0, 0).expect("epoch"),
        }
    }

    // -- OIDC sync login stamps (PIDASHCONV-1621) -------------------------------

    /// PIDASHCONV-1621: the two OIDC sync stamps write only their own
    /// columns — the re-login triple flips avatar/verified/medium while
    /// `updated_at` keeps its seed stamp (Django's `update_fields` save
    /// does not touch `auto_now`, `sync.py:122-124`), and the timestamp
    /// bump moves only `last_login_time` (`sync.py:126-127`). Names,
    /// display name, token and ip are untouched by both.
    #[tokio::test]
    async fn live_relogin_and_login_time_write_only_their_columns() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(USERS_DDL)
            .execute(&mut *tx)
            .await
            .expect("temp users");
        let seeded: DateTime<Utc> = "2026-10-27T12:00:00+00:00".parse().expect("T0");
        let login: DateTime<Utc> = "2026-10-28T12:00:00+00:00".parse().expect("T1");
        let user_id = live_uuid("000000006211");
        sqlx::query(
            "INSERT INTO users (password, id, username, email, display_name, first_name,
             last_name, avatar, date_joined, created_at, updated_at, last_location,
             created_location, is_superuser, is_managed, is_password_expired, is_active,
             is_staff, is_email_verified, is_password_autoset, is_password_reset_required,
             token, last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
             is_bot, user_timezone, is_email_valid)
             VALUES ('usable-hash', $1, 'stamp1621', 'stamp1621@example.test', 'stamp',
             'St', 'Amp', 'http://pic/old', $2, $2, $2, '', '', false, false, false, true,
             false, false, false, false, '', '', '', 'email', '', false, 'UTC', false)",
        )
        .bind(user_id)
        .bind(seeded)
        .execute(&mut *tx)
        .await
        .expect("seed user");
        update_user_relogin(
            &TxConn::new(&mut tx),
            user_id,
            "http://pic/new",
            true,
            "airepublic",
        )
        .await
        .expect("relogin triple runs");
        update_user_last_login_time(&TxConn::new(&mut tx), user_id, login)
            .await
            .expect("login-time single runs");
        let row: sqlx::postgres::PgRow = sqlx::query(
            "SELECT avatar, is_email_verified, last_login_medium, last_login_time,
             updated_at, first_name, last_name, display_name, token, last_login_ip
             FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .expect("read back");
        let avatar: String = row.try_get("avatar").expect("avatar");
        let verified: bool = row.try_get("is_email_verified").expect("verified");
        let medium: String = row.try_get("last_login_medium").expect("medium");
        let at: Option<DateTime<Utc>> = row.try_get("last_login_time").expect("login time");
        let touched: DateTime<Utc> = row.try_get("updated_at").expect("updated_at");
        let first: String = row.try_get("first_name").expect("first_name");
        let last: String = row.try_get("last_name").expect("last_name");
        let display: String = row.try_get("display_name").expect("display_name");
        let token: String = row.try_get("token").expect("token");
        let ip: String = row.try_get("last_login_ip").expect("last_login_ip");
        assert_eq!(avatar, "http://pic/new", "relogin avatar follows claims");
        assert!(verified, "relogin promotes verified");
        assert_eq!(medium, "airepublic", "relogin flips medium");
        assert_eq!(at, Some(login), "every-login timestamp bump");
        assert_eq!(touched, seeded, "relogin stamps no updated_at");
        assert_eq!(first, "St", "names stay user-controlled");
        assert_eq!(last, "Amp", "names stay user-controlled");
        assert_eq!(display, "stamp", "display name untouched");
        assert_eq!(token, "", "token untouched");
        assert_eq!(ip, "", "login ip untouched");
    }

    // -- statement-shape pins (the SQL-text pins of the sqlx era, over the
    //    rendered SeaORM statements) ---------------------------------------

    fn render<E: EntityTrait>(stmt: UpdateMany<E>) -> String {
        use sea_orm::QueryTrait;
        stmt.build(DbBackend::Postgres).to_string()
    }

    fn set_clause(sql: &str) -> &str {
        &sql[sql.find("SET ").expect("SET") + 4..sql.find(" WHERE").expect("WHERE")]
    }

    const ID1: &str = "'00000000-0000-0000-0000-000000000001'";
    const T0: &str = "'1970-01-01 00:00:00.000000 +00:00'";

    /// Every `users` column the bot and provisioned rows set, in `_meta`
    /// order: all 40 physical columns, none left to a database default.
    #[test]
    fn bot_and_provisioned_rows_set_all_40_columns() {
        use sea_orm::{ActiveModelTrait, Iden, Iterable};
        let now: DateTime<Utc> = DateTime::from_timestamp(0, 0).expect("epoch");
        let bot = bot_user_row(
            Uuid::nil(),
            &NewBotUser {
                id: None,
                username: AGENT_USERNAME,
                email: AGENT_USER_EMAIL,
                password_hash: "",
                display_name: AGENT_DISPLAY_NAME,
                first_name: AGENT_USER_FIRST_NAME,
                last_name: AGENT_USER_LAST_NAME,
                is_password_autoset: false,
                bot_type: Some("WORKSPACE_SEED"),
                now,
            },
        );
        let provisioned = provisioned_user_row(
            Uuid::nil(),
            &ProvisionedUser {
                id: None,
                email: "p@example.test",
                username: "p",
                password_hash: "!x",
                display_name: "P",
                first_name: "",
                last_name: "",
                avatar: "",
                is_managed: true,
                is_email_verified: true,
                is_password_autoset: false,
                last_login_medium: "email",
                user_timezone: "UTC",
                now,
            },
        );
        let columns: Vec<String> = users::Column::iter().map(|c| c.to_string()).collect();
        assert_eq!(columns.len(), 40);
        for name in USER_META_COLUMNS {
            assert!(
                columns.iter().any(|c| c == name),
                "{name} is an entity column"
            );
        }
        for column in users::Column::iter() {
            assert!(bot.get(column).is_set(), "bot row sets {column:?}");
            assert!(
                provisioned.get(column).is_set(),
                "provisioned row sets {column:?}"
            );
        }
        // The bot fields: is_bot, the type, the fixed defaults.
        assert_eq!(bot.is_bot.as_ref(), &true);
        assert_eq!(bot.bot_type.as_ref(), &Some("WORKSPACE_SEED".to_owned()));
        assert_eq!(bot.is_managed.as_ref(), &false);
        assert_eq!(bot.last_login_medium.as_ref(), "email");
        assert_eq!(bot.last_active.as_ref(), &Some(at(now)));
        assert_eq!(bot.avatar.as_ref(), "");
        // The provisioned fields ride the caller's values, never a bot.
        assert_eq!(provisioned.is_bot.as_ref(), &false);
        assert_eq!(provisioned.is_managed.as_ref(), &true);
        assert_eq!(provisioned.is_email_verified.as_ref(), &true);
        assert_eq!(provisioned.bot_type.as_ref(), &None);
    }

    /// The default profile row carries every Django model default
    /// (`db/models/user.py:200-273`) and the two caller inputs.
    #[test]
    fn default_profile_row_pins_model_defaults() {
        let now: DateTime<Utc> = DateTime::from_timestamp(0, 0).expect("epoch");
        let row = default_profile_row(Uuid::nil(), Uuid::max(), now, "Acme", "#abcdef");
        assert_eq!(row.company_name.as_ref(), "Acme");
        assert_eq!(row.background_color.as_ref(), "#abcdef");
        assert_eq!(row.billing_address_country.as_ref(), "INDIA");
        assert_eq!(row.notification_view_mode.as_ref(), "full");
        assert_eq!(row.language.as_ref(), "en");
        assert_eq!(row.start_of_the_week.as_ref(), &0);
        assert_eq!(row.is_app_rail_docked.as_ref(), &true);
        assert_eq!(row.is_onboarded.as_ref(), &false);
        assert_eq!(
            row.onboarding_step.as_ref(),
            &serde_json::json!({
                "profile_complete": false, "workspace_create": false,
                "workspace_invite": false, "workspace_join": false,
            })
        );
        assert_eq!(
            row.product_tour.as_ref(),
            &serde_json::json!({
                "work_items": false, "cycles": false, "modules": false,
                "intake": false, "pages": false,
            })
        );
        assert_eq!(row.settings.as_ref(), &serde_json::json!({}));
    }

    /// `user_fields_update`: dirty columns only, `_meta` order, then
    /// `updated_at`; nothing dirty renders no statement.
    #[test]
    fn user_fields_update_shapes() {
        let now: DateTime<Utc> = DateTime::from_timestamp(0, 0).expect("epoch");
        assert!(user_fields_update(Uuid::nil(), &field_updates_fixture(), now).is_none());
        let all = render(
            user_fields_update(
                Uuid::nil(),
                &UserFieldUpdates {
                    username: Some("u"),
                    email: Some("e"),
                    display_name: Some("d"),
                },
                now,
            )
            .expect("dirty"),
        );
        assert_eq!(
            set_clause(&all),
            format!("\"username\" = 'u', \"email\" = 'e', \"display_name\" = 'd', \"updated_at\" = {T0}")
        );
        let email_only = render(
            user_fields_update(
                Uuid::nil(),
                &UserFieldUpdates {
                    email: Some("e"),
                    ..field_updates_fixture()
                },
                now,
            )
            .expect("dirty"),
        );
        assert_eq!(
            set_clause(&email_only),
            format!("\"email\" = 'e', \"updated_at\" = {T0}")
        );
        let username_display = render(
            user_fields_update(
                Uuid::nil(),
                &UserFieldUpdates {
                    username: Some("u"),
                    display_name: Some("d"),
                    ..field_updates_fixture()
                },
                now,
            )
            .expect("dirty"),
        );
        assert_eq!(
            set_clause(&username_display),
            format!("\"username\" = 'u', \"display_name\" = 'd', \"updated_at\" = {T0}")
        );
        assert!(
            all.ends_with("WHERE \"users\".\"id\" = '00000000-0000-0000-0000-000000000000'"),
            "{all}"
        );
    }

    /// The stamps builder, one shape per caller: fixed stamp columns,
    /// the optional avatar pair, display/staff in bound or `CASE`/`OR`
    /// form, `updated_at` last.
    #[test]
    fn login_stamps_update_shapes() {
        let id = Uuid::from_u128(1);
        let fixed = format!(
            "\"last_active\" = {T0}, \"last_login_time\" = {T0}, \"last_login_ip\" = NULL, \
             \"last_login_uagent\" = NULL, \"token_updated_at\" = {T0}, \"token\" = 'tok', \
             \"is_active\" = TRUE"
        );
        let backfill = "\"display_name\" = (CASE WHEN (\"display_name\" = '') THEN split_part(\"email\", '@', 1) ELSE \"display_name\" END)";
        let or_super = "\"is_staff\" = \"is_staff\" OR \"is_superuser\"";

        // email: NEWPIDASH-17, the full `user.save()` effects ride along.
        let email = render(login_stamps_update(
            id,
            &LoginStamps {
                medium: Some("email"),
                login_ip: Some(""),
                login_uagent: Some(""),
                display_name: Some(DisplayNameUpdate::BackfillCase),
                is_staff: Some(StaffSync::OrSuperuser),
                touch_updated_at: true,
                ..stamps_fixture()
            },
        ));
        assert_eq!(
            set_clause(&email),
            format!(
                "\"last_login_medium\" = 'email', {}, {backfill}, {or_super}, \"updated_at\" = {T0}",
                fixed.replace("= NULL", "= ''")
            )
        );
        assert!(
            email.ends_with(&format!("WHERE \"users\".\"id\" = {ID1}")),
            "{email}"
        );

        // magic: NEWPIDASH-41, the full `user.save()` effects ride along
        // like email/gitea/google.
        let magic = render(login_stamps_update(
            id,
            &LoginStamps {
                medium: Some("magic-code"),
                display_name: Some(DisplayNameUpdate::BackfillCase),
                is_staff: Some(StaffSync::OrSuperuser),
                touch_updated_at: true,
                ..stamps_fixture()
            },
        ));
        assert_eq!(
            set_clause(&magic),
            format!(
                "\"last_login_medium\" = 'magic-code', {fixed}, {backfill}, {or_super}, \"updated_at\" = {T0}"
            )
        );

        // gitlab: bound avatar, bound display name, bound staff.
        let gitlab = render(login_stamps_update(
            id,
            &LoginStamps {
                medium: Some("gitlab"),
                avatar: Some("https://avatar.test/a"),
                display_name: Some(DisplayNameUpdate::Bound("ada")),
                is_staff: Some(StaffSync::Value(false)),
                touch_updated_at: true,
                ..stamps_fixture()
            },
        ));
        assert_eq!(
            set_clause(&gitlab),
            format!(
                "\"last_login_medium\" = 'gitlab', {fixed}, \"avatar\" = 'https://avatar.test/a', \
                 \"display_name\" = 'ada', \"is_staff\" = FALSE, \"updated_at\" = {T0}"
            )
        );

        // gitea: avatar pair (NULL asset), backfill CASE, OR staff.
        let gitea = render(login_stamps_update(
            id,
            &LoginStamps {
                medium: Some("gitea"),
                avatar: Some(""),
                avatar_asset_id: Some(None),
                display_name: Some(DisplayNameUpdate::BackfillCase),
                is_staff: Some(StaffSync::OrSuperuser),
                touch_updated_at: true,
                ..stamps_fixture()
            },
        ));
        assert_eq!(
            set_clause(&gitea),
            format!(
                "\"last_login_medium\" = 'gitea', {fixed}, \"avatar\" = '', \"avatar_asset_id\" = NULL, \
                 {backfill}, {or_super}, \"updated_at\" = {T0}"
            )
        );

        // github: bound display name + bound staff, no avatar here.
        let github = render(login_stamps_update(
            id,
            &LoginStamps {
                medium: Some("github"),
                display_name: Some(DisplayNameUpdate::Bound("ada")),
                is_staff: Some(StaffSync::Value(true)),
                touch_updated_at: true,
                ..stamps_fixture()
            },
        ));
        assert_eq!(
            set_clause(&github),
            format!(
                "\"last_login_medium\" = 'github', {fixed}, \"display_name\" = 'ada', \
                 \"is_staff\" = TRUE, \"updated_at\" = {T0}"
            )
        );

        // admin: no medium column (the license stamps never assign it).
        let admin = render(login_stamps_update(
            id,
            &LoginStamps {
                touch_updated_at: true,
                ..stamps_fixture()
            },
        ));
        assert_eq!(
            set_clause(&admin),
            format!("{fixed}, \"updated_at\" = {T0}")
        );
    }

    /// The sync builder, one shape per caller.
    #[test]
    fn sync_update_shapes() {
        let id = Uuid::from_u128(1);
        let names = "\"first_name\" = 'a', \"last_name\" = 'b', \"display_name\" = 'c'";
        let gitlab = render(sync_update(
            id,
            &SyncUpdate {
                avatar: Some("u"),
                avatar_asset_id: Some(None),
                is_staff: Some(true),
                touch_updated_at: true,
                ..sync_fixture()
            },
        ));
        assert_eq!(
            set_clause(&gitlab),
            format!("{names}, \"avatar\" = 'u', \"avatar_asset_id\" = NULL, \"is_staff\" = TRUE, \"updated_at\" = {T0}")
        );
        assert!(
            gitlab.ends_with(&format!("WHERE \"users\".\"id\" = {ID1}")),
            "{gitlab}"
        );
        let gitea = render(sync_update(
            id,
            &SyncUpdate {
                avatar: Some("u"),
                avatar_asset_id: Some(None),
                ..sync_fixture()
            },
        ));
        assert_eq!(
            set_clause(&gitea),
            format!("{names}, \"avatar\" = 'u', \"avatar_asset_id\" = NULL")
        );
        let google = render(sync_update(id, &sync_fixture()));
        assert_eq!(set_clause(&google), names);
        let github_asset = render(sync_update(
            id,
            &SyncUpdate {
                avatar_asset_id: Some(Some(Uuid::nil())),
                touch_updated_at: true,
                ..sync_fixture()
            },
        ));
        assert_eq!(
            set_clause(&github_asset),
            format!("{names}, \"avatar_asset_id\" = '00000000-0000-0000-0000-000000000000', \"updated_at\" = {T0}")
        );
        let github_url = render(sync_update(
            id,
            &SyncUpdate {
                avatar: Some("u"),
                touch_updated_at: true,
                ..sync_fixture()
            },
        ));
        assert_eq!(
            set_clause(&github_url),
            format!("{names}, \"avatar\" = 'u', \"updated_at\" = {T0}")
        );
    }

    /// The fixed-shape singles: the OIDC re-login triple (no
    /// `updated_at`), the asset link/unlink twins with the `User.save()`
    /// effects appended, the ops saves, and the `PATCH users/me/` row.
    #[test]
    fn fixed_statement_shapes() {
        let id = Uuid::from_u128(1);
        let now: DateTime<Utc> = DateTime::from_timestamp(0, 0).expect("epoch");
        let relogin = render(relogin_update(id, "pic", true, "airepublic"));
        assert_eq!(
            set_clause(&relogin),
            "\"avatar\" = 'pic', \"is_email_verified\" = TRUE, \"last_login_medium\" = 'airepublic'"
        );
        assert!(
            !relogin.contains("updated_at"),
            "relogin stamps nothing: {relogin}"
        );

        let write = pidash_db::ops::users::UserSaveWrite {
            email: "u@x".to_owned(),
            display_name: "U".to_owned(),
            is_staff: false,
            token: "tok".to_owned(),
            token_updated_at: None,
        };
        let effects = format!(
            "\"email\" = 'u@x', \"display_name\" = 'U', \"is_staff\" = FALSE, \"token\" = 'tok', \
             \"token_updated_at\" = NULL, \"updated_at\" = {T0}"
        );
        let avatar_link = render(avatar_link_update(id, Uuid::nil(), &write, now));
        assert_eq!(
            set_clause(&avatar_link),
            format!("\"avatar\" = '', \"avatar_asset_id\" = '00000000-0000-0000-0000-000000000000', {effects}")
        );
        let cover_link = render(cover_link_update(id, Uuid::nil(), &write, now));
        assert_eq!(
            set_clause(&cover_link),
            format!("\"cover_image\" = NULL, \"cover_image_asset_id\" = '00000000-0000-0000-0000-000000000000', {effects}")
        );
        let avatar_unlink = render(avatar_unlink_update(id, &write, now));
        assert_eq!(
            set_clause(&avatar_unlink),
            format!("\"avatar_asset_id\" = NULL, {effects}")
        );
        let cover_unlink = render(cover_unlink_update(id, &write, now));
        assert_eq!(
            set_clause(&cover_unlink),
            format!("\"cover_image_asset_id\" = NULL, {effects}")
        );
        for sql in [&avatar_link, &cover_link, &avatar_unlink, &cover_unlink] {
            assert!(
                sql.ends_with(&format!("WHERE \"users\".\"id\" = {ID1}")),
                "{sql}"
            );
        }

        // Ops saves: the flag / the hash + autoset, then the same effects.
        let activation = render(user_activation_update(id, &write, now));
        assert_eq!(
            set_clause(&activation),
            format!("\"is_active\" = TRUE, {effects}")
        );
        let password = render(user_password_update(id, "pbkdf2$new", &write, now));
        assert_eq!(
            set_clause(&password),
            format!("\"password\" = 'pbkdf2$new', \"is_password_autoset\" = FALSE, {effects}")
        );

        // `PATCH users/me/`: fourteen merged columns, four derived, the stamp.
        let me = render(me_profile_update(
            id,
            &MeProfileSave {
                last_login: None,
                display_name: "Ada",
                first_name: "Ada",
                last_name: "L",
                avatar: "img",
                avatar_asset_id: None,
                cover_image: None,
                cover_image_asset_id: None,
                is_password_expired: false,
                is_password_reset_required: false,
                bot_type: None,
                user_timezone: "UTC",
                is_email_valid: true,
                masked_at: None,
                email: "ada@x",
                token: "tok",
                token_updated_at: None,
                is_staff: false,
                updated_at: now,
            },
        ));
        assert!(me.starts_with("UPDATE \"users\" SET \"last_login\" = NULL, \"display_name\" = 'Ada', \"first_name\" = 'Ada'"), "{me}");
        assert!(
            me.contains("\"masked_at\" = NULL, \"email\" = 'ada@x', \"token\" = 'tok', \"token_updated_at\" = NULL, \"is_staff\" = FALSE, \"updated_at\" = "),
            "{me}"
        );
        assert_eq!(
            set_clause(&me).matches(" = ").count(),
            19,
            "nineteen SET entries"
        );
    }

    /// The password and logout twins: the `User.save()` effects as
    /// expressions — the rotation `CASE`s, the NULL-tolerant backfill,
    /// the superuser staff `CASE`, and the database clock where Django
    /// used `auto_now`; only the change/set twin bumps `last_login`.
    #[test]
    fn password_and_logout_update_shapes() {
        let id = Uuid::from_u128(1);
        let now: DateTime<Utc> = DateTime::from_timestamp(0, 0).expect("epoch");
        let rotation = format!(
            "\"token\" = (CASE WHEN (\"token_updated_at\" IS NOT NULL) THEN 'fresh' ELSE \"token\" END), \
             \"token_updated_at\" = (CASE WHEN (\"token_updated_at\" IS NOT NULL) THEN {T0} ELSE \"token_updated_at\" END)"
        );
        let backfill = "\"display_name\" = (CASE WHEN (COALESCE(\"display_name\", '') = '') THEN split_part(\"email\", '@', 1) ELSE \"display_name\" END)";
        let staff =
            "\"is_staff\" = (CASE WHEN (\"is_superuser\" = TRUE) THEN TRUE ELSE \"is_staff\" END)";
        let login_twin = render(password_save_update(
            id,
            &PasswordSave {
                encoded: "hash",
                fresh_token: "fresh",
                rotated_at: now,
                stamp_last_login: true,
            },
        ));
        assert_eq!(
            set_clause(&login_twin),
            format!(
                "\"password\" = 'hash', \"is_password_autoset\" = FALSE, \"last_login\" = CURRENT_TIMESTAMP, \
                 \"updated_at\" = CURRENT_TIMESTAMP, {rotation}, {backfill}, {staff}"
            )
        );
        let reset_twin = render(password_save_update(
            id,
            &PasswordSave {
                encoded: "hash",
                fresh_token: "fresh",
                rotated_at: now,
                stamp_last_login: false,
            },
        ));
        assert!(!reset_twin.contains("last_login"), "{reset_twin}");
        assert!(
            reset_twin.contains("\"updated_at\" = CURRENT_TIMESTAMP"),
            "{reset_twin}"
        );
        let logout = render(logout_update(id, Some("9.9.9.9"), now, "fresh"));
        assert_eq!(
            set_clause(&logout),
            format!(
                "\"last_logout_ip\" = '9.9.9.9', \"last_logout_time\" = {T0}, \"updated_at\" = {T0}, \
                 {rotation}, {backfill}, {staff}"
            )
        );
        let logout_none = render(logout_update(id, None, now, "fresh"));
        assert!(
            logout_none.contains("\"last_logout_ip\" = NULL"),
            "{logout_none}"
        );
    }

    /// The preference PATCH: stamps first, then the caller's columns
    /// by name in push order.
    #[test]
    fn preference_columns_update_shape() {
        let now: DateTime<Utc> = DateTime::from_timestamp(0, 0).expect("epoch");
        let columns = [
            ("comment", PreferenceColumn::Flag(false)),
            ("project_id", PreferenceColumn::Id(None)),
            ("deleted_at", PreferenceColumn::Stamp(Some(now))),
        ];
        let sql = preference_columns_update(Uuid::from_u128(1), now, Uuid::nil(), &columns)
            .to_string(sea_orm::sea_query::PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                "UPDATE \"user_notification_preferences\" SET \"updated_at\" = {T0}, \
                 \"updated_by_id\" = '00000000-0000-0000-0000-000000000000', \"comment\" = FALSE, \
                 \"project_id\" = NULL, \"deleted_at\" = {T0} WHERE \"id\" = {ID1}"
            )
        );
    }

    /// The account statements: the F6 full-save update (every non-pk
    /// column, field order), the insert (client-side id, `{}` metadata
    /// default, NULL tokens bound as given), and the three cloud legs.
    #[test]
    fn account_statement_shapes() {
        use sea_orm::sea_query::PostgresQueryBuilder;
        let id = Uuid::from_u128(1);
        let now: DateTime<Utc> = DateTime::from_timestamp(0, 0).expect("epoch");
        let tokens = token_fields();
        let full = render(account::rotate_tokens_update(
            id,
            now,
            Uuid::nil(),
            "pid",
            "google",
            &tokens,
            now,
            serde_json::json!({}),
        ));
        assert_eq!(
            set_clause(&full),
            format!(
                "\"created_at\" = {T0}, \"updated_at\" = {T0}, \"user_id\" = '00000000-0000-0000-0000-000000000000', \
                 \"provider_account_id\" = 'pid', \"provider\" = 'google', \"access_token\" = 'TOK', \
                 \"access_token_expired_at\" = NULL, \"refresh_token\" = NULL, \"refresh_token_expired_at\" = NULL, \
                 \"last_connected_at\" = {T0}, \"id_token\" = '', \"metadata\" = '{{}}'"
            )
        );
        assert!(
            full.ends_with(&format!("WHERE \"accounts\".\"id\" = {ID1}")),
            "{full}"
        );
        for col in account::UPDATE_COLUMNS {
            assert!(
                full.contains(&format!("\"{col}\" = ")),
                "update writes {col}"
            );
        }

        let insert = account::new_account_insert(&account::NewAccount {
            id,
            user_id: Uuid::nil(),
            provider: "google",
            provider_account_id: "pid",
            tokens: &tokens,
            now,
            metadata: None,
        })
        .to_string(PostgresQueryBuilder);
        assert!(
            insert.starts_with(
                "INSERT INTO \"accounts\" (\"id\", \"created_at\", \"updated_at\", \"user_id\", \
                 \"provider_account_id\", \"provider\", \"access_token\", \"access_token_expired_at\", \
                 \"refresh_token\", \"refresh_token_expired_at\", \"last_connected_at\", \"id_token\", \"metadata\") VALUES ("
            ),
            "{insert}"
        );
        assert!(
            insert.contains(&format!(
                "'pid', 'google', 'TOK', NULL, NULL, NULL, {T0}, '', "
            )) && insert.ends_with("'{}')"),
            "{insert}"
        );
        for key in account::COLUMNS {
            assert!(insert.contains(&format!("\"{key}\"")), "insert binds {key}");
        }

        let upsert = render(account::upsert_update(
            &account::AccountUpsert {
                id,
                created_at: now,
                user_id: Uuid::nil(),
                access_token: "at",
                access_token_expired_at: None,
                refresh_token: None,
                refresh_token_expired_at: None,
                last_connected_at: now,
                id_token: "idt",
                metadata: None,
            },
            now,
        ));
        assert_eq!(
            set_clause(&upsert),
            format!(
                "\"created_at\" = {T0}, \"updated_at\" = {T0}, \"user_id\" = '00000000-0000-0000-0000-000000000000', \
                 \"access_token\" = 'at', \"access_token_expired_at\" = NULL, \"refresh_token\" = NULL, \
                 \"refresh_token_expired_at\" = NULL, \"last_connected_at\" = {T0}, \"id_token\" = 'idt', \"metadata\" = NULL"
            )
        );
        assert!(
            !upsert.contains("provider"),
            "lookup columns excluded: {upsert}"
        );
        let rotated = render(account::rotated_tokens_update(id, "at", None, Some("rt")));
        assert_eq!(
            set_clause(&rotated),
            "\"access_token\" = 'at', \"access_token_expired_at\" = NULL, \"refresh_token\" = 'rt'"
        );
        assert!(
            !rotated.contains("updated_at"),
            "rotation stamps nothing: {rotated}"
        );
    }

    // -- moved agent-user pins (from db::orchestration::workpad) ---------------

    static AGENT_USER_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx02_reads/agent_system_user.golden.json");

    #[test]
    fn agent_constants_and_collision_message_match_fixture() {
        let parsed: serde_json::Value =
            serde_json::from_str(AGENT_USER_FIXTURE).expect("fixture parses");
        assert_eq!(AGENT_USERNAME, parsed["constants"]["AGENT_USERNAME"]);
        assert_eq!(AGENT_USER_EMAIL, parsed["constants"]["AGENT_USER_EMAIL"]);
        assert_eq!(
            AGENT_USER_FIRST_NAME,
            parsed["constants"]["AGENT_USER_FIRST_NAME"]
        );
        assert_eq!(
            AGENT_USER_LAST_NAME,
            parsed["constants"]["AGENT_USER_LAST_NAME"]
        );
        // `User.save()` derives the blank display name from the email
        // prefix (`user.py:176-182`).
        assert_eq!(AGENT_DISPLAY_NAME, "agent");
        assert_eq!(
            AGENT_USER_EMAIL.split('@').next().expect("prefix"),
            AGENT_DISPLAY_NAME
        );
        assert_eq!(
            AgentUserCollisionError.to_string(),
            parsed["collision_error"].as_str().expect("collision_error")
        );
        assert_eq!(
            parsed["collision_error_type"].as_str(),
            Some("AgentUserCollisionError")
        );
    }

    #[test]
    fn unusable_password_shape_matches_django_hashers() {
        for _ in 0..25 {
            let hash = make_unusable_password();
            assert_eq!(hash.len(), 41);
            assert!(hash.starts_with('!'));
            assert!(hash[1..].bytes().all(|byte| byte.is_ascii_alphanumeric()));
            assert!(!is_password_usable(&hash));
        }
        // Two draws differ (122+ bits of entropy would collide never).
        assert_ne!(make_unusable_password(), make_unusable_password());
        assert!(is_password_usable(""));
        assert!(is_password_usable("pbkdf2_sha256$1500000$salt$hash"));
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

    /// Temp `users`: the 40 insert columns with production nullability
    /// plus the `username` unique index the race seam relies on.
    const USERS_DDL: &str = "CREATE TEMPORARY TABLE users (
        password TEXT NOT NULL, last_login TIMESTAMPTZ, id UUID PRIMARY KEY,
        username TEXT NOT NULL UNIQUE, mobile_number TEXT, email TEXT UNIQUE,
        display_name TEXT NOT NULL, first_name TEXT NOT NULL, last_name TEXT NOT NULL,
        avatar TEXT NOT NULL, avatar_asset_id UUID, cover_image TEXT,
        cover_image_asset_id UUID, date_joined TIMESTAMPTZ NOT NULL,
        created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL,
        last_location TEXT NOT NULL, created_location TEXT NOT NULL,
        is_superuser BOOLEAN NOT NULL, is_managed BOOLEAN NOT NULL,
        is_password_expired BOOLEAN NOT NULL, is_active BOOLEAN NOT NULL,
        is_staff BOOLEAN NOT NULL, is_email_verified BOOLEAN NOT NULL,
        is_password_autoset BOOLEAN NOT NULL, is_password_reset_required BOOLEAN NOT NULL,
        token TEXT NOT NULL, last_active TIMESTAMPTZ, last_login_time TIMESTAMPTZ,
        last_logout_time TIMESTAMPTZ, last_login_ip TEXT NOT NULL,
        last_logout_ip TEXT NOT NULL, last_login_medium TEXT NOT NULL,
        last_login_uagent TEXT NOT NULL, token_updated_at TIMESTAMPTZ,
        is_bot BOOLEAN NOT NULL, bot_type TEXT, user_timezone TEXT NOT NULL,
        is_email_valid BOOLEAN NOT NULL, masked_at TIMESTAMPTZ)";

    fn live_uuid(tag: &str) -> Uuid {
        Uuid::parse_str(&format!("11111111-2222-3333-4444-{tag}")).expect("fixed uuid")
    }

    /// `agent_system_user.golden.json` replay: first call creates the
    /// bot row (unusable password), second call returns the same pk,
    /// a human-held name raises the verbatim collision error.
    #[tokio::test]
    async fn live_agent_user_create_exists_collision() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(USERS_DDL)
            .execute(&mut *tx)
            .await
            .expect("temp users");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let (created, was_created) = get_agent_system_user(&mut tx, now)
            .await
            .expect("create agent user");
        assert!(was_created);
        assert_eq!(created.username, AGENT_USERNAME);
        assert_eq!(created.email.as_deref(), Some(AGENT_USER_EMAIL));
        assert_eq!(created.first_name, AGENT_USER_FIRST_NAME);
        assert_eq!(created.last_name, AGENT_USER_LAST_NAME);
        assert!(created.is_bot);
        let row: sqlx::postgres::PgRow = sqlx::query(
            "SELECT password, display_name, is_active, user_timezone, last_active,
             date_joined, created_at, updated_at FROM users WHERE id = $1",
        )
        .bind(created.id)
        .fetch_one(&mut *tx)
        .await
        .expect("read back");
        let password: String = row.try_get("password").expect("password");
        assert!(!is_password_usable(&password));
        assert_eq!(password.len(), 41);
        let display: String = row.try_get("display_name").expect("display_name");
        assert_eq!(display, "agent");
        let active: bool = row.try_get("is_active").expect("is_active");
        assert!(active);
        let zone: String = row.try_get("user_timezone").expect("user_timezone");
        assert_eq!(zone, "UTC");
        let last_active: Option<DateTime<Utc>> = row.try_get("last_active").expect("last_active");
        assert_eq!(last_active, Some(now));
        let (existing, was_created) = get_agent_system_user(&mut tx, now)
            .await
            .expect("existing agent user");
        assert!(!was_created);
        assert_eq!(existing.id, created.id);

        // A racing loser's insert surfaces SQLSTATE 23505 — the
        // trigger the savepoint arm above keys on.
        sqlx::query("SAVEPOINT pidash_agent_user_probe")
            .execute(&mut *tx)
            .await
            .expect("probe savepoint");
        let raced = create_bot_user(
            &TxConn::new(&mut tx),
            &NewBotUser {
                id: Some(live_uuid("000000005002")),
                username: AGENT_USERNAME,
                email: AGENT_USER_EMAIL,
                password_hash: "",
                display_name: AGENT_DISPLAY_NAME,
                first_name: AGENT_USER_FIRST_NAME,
                last_name: AGENT_USER_LAST_NAME,
                is_password_autoset: false,
                bot_type: None,
                now,
            },
        )
        .await;
        sqlx::query("ROLLBACK TO SAVEPOINT pidash_agent_user_probe")
            .execute(&mut *tx)
            .await
            .expect("probe rollback");
        let err = raced.expect_err("duplicate username fails");
        assert!(is_unique_violation(&err), "got {err:?}");
    }

    /// A human row holding the reserved username refuses with the
    /// verbatim [`AgentUserCollisionError`].
    #[tokio::test]
    async fn live_agent_user_collision() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(USERS_DDL)
            .execute(&mut *tx)
            .await
            .expect("temp users");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let human = live_uuid("000000005001");
        sqlx::query(
            "INSERT INTO users (password, id, username, email, display_name, first_name,
             last_name, avatar, date_joined, created_at, updated_at, last_location,
             created_location, is_superuser, is_managed, is_password_expired, is_active,
             is_staff, is_email_verified, is_password_autoset, is_password_reset_required,
             token, last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
             is_bot, user_timezone, is_email_valid)
             VALUES ('usable-hash', $1, $2, 'human@example.com', 'human', 'Hu', 'Man', '',
             $3, $3, $3, '', '', false, false, false, true, false, false, false, false, '',
             '', '', 'email', '', false, 'UTC', false)",
        )
        .bind(human)
        .bind(AGENT_USERNAME)
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("seed human");
        let err = get_agent_system_user(&mut tx, now)
            .await
            .expect_err("collision refuses");
        match err {
            GetAgentUserError::Collision(collision) => assert_eq!(
                collision.to_string(),
                "User 'pi_dash_agent' exists but is not a bot; refusing to author agent activity under a human account."
            ),
            GetAgentUserError::Db(db) => panic!("wrong error: {db:?}"),
        }
    }

    /// Email-only conflict: another row holds the agent email under a
    /// different username. Django's `get_or_create` re-raises the
    /// original `IntegrityError` when its retry `get()` misses — the
    /// port returns the 23505 database error, never `RowNotFound`.
    #[tokio::test]
    async fn live_agent_user_email_conflict_reraises_unique_violation() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(USERS_DDL)
            .execute(&mut *tx)
            .await
            .expect("temp users");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        sqlx::query(
            "INSERT INTO users (password, id, username, email, display_name, first_name,
             last_name, avatar, date_joined, created_at, updated_at, last_location,
             created_location, is_superuser, is_managed, is_password_expired, is_active,
             is_staff, is_email_verified, is_password_autoset, is_password_reset_required,
             token, last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
             is_bot, user_timezone, is_email_valid)
             VALUES ('usable-hash', $1, 'some_human', $2, 'human', 'Hu', 'Man', '',
             $3, $3, $3, '', '', false, false, false, true, false, false, false, false, '',
             '', '', 'email', '', false, 'UTC', false)",
        )
        .bind(live_uuid("000000005003"))
        .bind(AGENT_USER_EMAIL)
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("seed email holder");
        let err = get_agent_system_user(&mut tx, now)
            .await
            .expect_err("email conflict fails");
        match err {
            GetAgentUserError::Db(db) => assert!(is_unique_violation(&db), "got {db:?}"),
            GetAgentUserError::Collision(_) => panic!("wrong error: collision"),
        }
    }

    /// Temp `profiles`: the columns [`create_user`] inserts (entity
    /// names); types accept the call-site values.
    const PROFILES_DDL: &str = "CREATE TEMPORARY TABLE profiles (
        id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL,
        updated_at TIMESTAMPTZ NOT NULL, user_id UUID NOT NULL,
        theme JSONB NOT NULL, is_app_rail_docked BOOLEAN NOT NULL,
        is_tour_completed BOOLEAN NOT NULL, onboarding_step JSONB NOT NULL,
        use_case TEXT, role TEXT, is_onboarded BOOLEAN NOT NULL,
        last_workspace_id UUID, billing_address_country TEXT NOT NULL,
        billing_address JSONB, has_billing_address BOOLEAN NOT NULL,
        company_name TEXT NOT NULL, notification_view_mode TEXT NOT NULL,
        is_smooth_cursor_enabled BOOLEAN NOT NULL,
        is_mobile_onboarded BOOLEAN NOT NULL,
        mobile_onboarding_step JSONB NOT NULL,
        mobile_timezone_auto_set BOOLEAN NOT NULL, language TEXT NOT NULL,
        start_of_the_week SMALLINT NOT NULL, goals JSONB NOT NULL,
        background_color TEXT NOT NULL,
        is_navigation_tour_completed BOOLEAN NOT NULL,
        has_marketing_email_consent BOOLEAN NOT NULL,
        is_subscribed_to_changelog BOOLEAN NOT NULL,
        product_tour JSONB NOT NULL, settings JSONB NOT NULL)";

    /// Temp `user_notification_preferences`: the columns the users
    /// `after_save` hook inserts.
    const PREFS_DDL: &str = "CREATE TEMPORARY TABLE user_notification_preferences (
        id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL,
        updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID,
        updated_by_id UUID, deleted_at TIMESTAMPTZ, user_id UUID NOT NULL,
        workspace_id UUID, project_id UUID,
        property_change BOOLEAN NOT NULL, state_change BOOLEAN NOT NULL,
        comment BOOLEAN NOT NULL, mention BOOLEAN NOT NULL,
        issue_completed BOOLEAN NOT NULL)";

    /// Single-connection scratch pool: temp tables live on the session,
    /// and the SeaORM handle shares it (no explicit transaction — the
    /// one connection serializes the sequential awaits).
    async fn live_single_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::postgres::PgPoolOptions::new()
                    .max_connections(1)
                    .connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    /// PIDASHCONV-1560: each signup path's exact `NewUser` inputs (no
    /// stamps save) store Django's insert-time row — `last_active = now`
    /// (`user.py:105`) and `avatar = ''` (`user.py:68`).
    #[tokio::test]
    async fn live_signup_insert_stores_django_last_active_and_avatar() {
        let Some(pool) = live_single_pool().await else {
            return;
        };
        for ddl in [USERS_DDL, PROFILES_DDL, PREFS_DDL] {
            sqlx::query(ddl)
                .execute(&pool)
                .await
                .expect("temp identity tables");
        }
        let db = pidash_db::sea_orm_connection(&pool);
        let now: DateTime<Utc> = "2026-10-27T12:00:00+00:00".parse().expect("T0");
        // `password_hash` is fixed, not PBKDF2: the value is irrelevant
        // to the pinned columns and 600k rounds would slow the test.
        let paths = [
            // `auth_session/email.rs` signup arm.
            NewUser {
                id: None,
                email: "insert1560-email@example.test",
                username: "email1560user".to_owned(),
                password_hash: "pbkdf2-fixed".to_owned(),
                first_name: "",
                last_name: "",
                display_name: "insert1560-email".to_owned(),
                avatar: "",
                is_email_verified: false,
                is_password_autoset: false,
                last_active: Some(now),
                login_ip: Some(""),
                login_uagent: Some(""),
                is_bot: false,
                now,
                profile: Some(NewProfile {
                    company_name: "",
                    background_color: "#abcdef".to_owned(),
                }),
            },
            // `auth_oauth/oauth_gitea.rs` signup arm.
            NewUser {
                id: Some(live_uuid("000000005601")),
                email: "insert1560-gitea@example.test",
                username: "gitea1560user".to_owned(),
                password_hash: "pbkdf2-fixed".to_owned(),
                first_name: "Gi",
                last_name: "Tea",
                display_name: "gitea-display".to_owned(),
                avatar: "",
                is_email_verified: true,
                is_password_autoset: true,
                last_active: Some(now),
                login_ip: Some(""),
                login_uagent: Some(""),
                is_bot: false,
                now,
                profile: Some(NewProfile {
                    company_name: "",
                    background_color: "#abcdef".to_owned(),
                }),
            },
            // `auth_oauth/oauth_github.rs` signup arm.
            NewUser {
                id: Some(live_uuid("000000005602")),
                email: "insert1560-github@example.test",
                username: "github1560user".to_owned(),
                password_hash: "pbkdf2-fixed".to_owned(),
                first_name: "Git",
                last_name: "Hub",
                display_name: "insert1560-github".to_owned(),
                avatar: "",
                is_email_verified: true,
                is_password_autoset: true,
                last_active: Some(now),
                login_ip: Some(""),
                login_uagent: Some(""),
                is_bot: false,
                now,
                profile: Some(NewProfile {
                    company_name: "",
                    background_color: "#abcdef".to_owned(),
                }),
            },
        ];
        for user in &paths {
            let id = create_user(&db, user).await.expect("insert runs");
            let row: sqlx::postgres::PgRow =
                sqlx::query("SELECT last_active, avatar FROM users WHERE id = $1")
                    .bind(id)
                    .fetch_one(&pool)
                    .await
                    .expect("read back");
            let last_active: Option<DateTime<Utc>> =
                row.try_get("last_active").expect("last_active");
            let avatar: String = row.try_get("avatar").expect("avatar");
            assert_eq!(last_active, Some(now), "insert-time last_active");
            assert_eq!(avatar, "", "insert-time avatar");
        }
    }

    /// The v2 asset link/unlink twins and the preference-columns
    /// save round-trip through the temp tables with the same binds
    /// the handlers pass.
    #[tokio::test]
    async fn live_asset_link_and_preference_columns() {
        let Some(pool) = live_single_pool().await else {
            return;
        };
        for ddl in [USERS_DDL, PREFS_DDL] {
            sqlx::query(ddl)
                .execute(&pool)
                .await
                .expect("temp identity tables");
        }
        let now: DateTime<Utc> = "2026-10-27T12:00:00+00:00".parse().expect("T0");
        let user = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (password, id, username, display_name, first_name, last_name, avatar, date_joined, created_at, updated_at, last_location, created_location, is_superuser, is_managed, is_password_expired, is_active, is_staff, is_email_verified, is_password_autoset, is_password_reset_required, token, last_login_ip, last_logout_ip, last_login_medium, last_login_uagent, is_bot, user_timezone, is_email_valid) VALUES ('x', $1, 'u', 'U', 'F', 'L', '', $2, $2, $2, '', '', FALSE, FALSE, FALSE, TRUE, FALSE, FALSE, FALSE, FALSE, 'tok', '', '', '', '', FALSE, 'UTC', TRUE)",
        )
        .bind(user)
        .bind(now)
        .execute(&pool)
        .await
        .expect("user row");
        let write = pidash_db::ops::users::UserSaveWrite {
            email: "u@example.test".to_owned(),
            display_name: "U".to_owned(),
            is_staff: false,
            token: "tok".to_owned(),
            token_updated_at: None,
        };
        let avatar_asset = Uuid::new_v4();
        link_avatar_asset(&orm(&pool), user, avatar_asset, &write, now)
            .await
            .expect("avatar link");
        let linked: (String, Option<Uuid>, String) =
            sqlx::query_as("SELECT avatar, avatar_asset_id, email FROM users WHERE id = $1")
                .bind(user)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(linked.0, "");
        assert_eq!(linked.1, Some(avatar_asset));
        assert_eq!(linked.2, "u@example.test");
        unlink_avatar_asset(&orm(&pool), user, &write, now)
            .await
            .expect("avatar unlink");
        let cover_asset = Uuid::new_v4();
        link_cover_asset(&orm(&pool), user, cover_asset, &write, now)
            .await
            .expect("cover link");
        let cover: (Option<String>, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
            "SELECT cover_image, cover_image_asset_id, avatar_asset_id FROM users WHERE id = $1",
        )
        .bind(user)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(cover, (None, Some(cover_asset), None));
        unlink_cover_asset(&orm(&pool), user, &write, now)
            .await
            .expect("cover unlink");
        let pref = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO user_notification_preferences (id, created_at, updated_at, user_id, property_change, state_change, comment, mention, issue_completed) VALUES ($1, $2, $2, $3, TRUE, TRUE, TRUE, TRUE, TRUE)",
        )
        .bind(pref)
        .bind(now)
        .bind(user)
        .execute(&pool)
        .await
        .expect("preference row");
        let columns = [
            ("comment", PreferenceColumn::Flag(false)),
            ("project_id", PreferenceColumn::Id(None)),
        ];
        save_preference_columns(&orm(&pool), pref, now, user, &columns)
            .await
            .expect("preference save");
        let saved: (bool, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
            "SELECT comment, project_id, updated_by_id FROM user_notification_preferences WHERE id = $1",
        )
        .bind(pref)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(saved, (false, None, Some(user)));
    }

    fn assert_not_null_violation(err: &sqlx::Error) {
        match err {
            sqlx::Error::Database(db) => {
                assert_eq!(db.code().as_deref(), Some("23502"), "not-null violation")
            }
            other => panic!("wrong error: {other:?}"),
        }
    }

    /// PIDASHCONV-1560: the gitea stamps with missing ip/user-agent bind
    /// NULL into the NOT NULL columns and raise 23502 — the same error
    /// the other paths' stamps raise (Django 500s the same way,
    /// `adapter/base.py:225-226`). A savepoint separates the two
    /// expected failures (each aborts to it, not the transaction).
    #[tokio::test]
    async fn live_gitea_stamps_none_ip_uagent_raise_not_null() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(USERS_DDL)
            .execute(&mut *tx)
            .await
            .expect("temp users");
        let now: DateTime<Utc> = "2026-10-27T12:00:00+00:00".parse().expect("T0");
        let user_id = live_uuid("000000005603");
        sqlx::query(
            "INSERT INTO users (password, id, username, email, display_name, first_name,
             last_name, avatar, date_joined, created_at, updated_at, last_location,
             created_location, is_superuser, is_managed, is_password_expired, is_active,
             is_staff, is_email_verified, is_password_autoset, is_password_reset_required,
             token, last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
             is_bot, user_timezone, is_email_valid)
             VALUES ('usable-hash', $1, 'stamp1560', 'stamp1560@example.test', 'stamp',
             'St', 'Amp', '', $2, $2, $2, '', '', false, false, false, true, false,
             false, false, false, '', '', '', 'email', '', false, 'UTC', false)",
        )
        .bind(user_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("seed user");
        // The gitea call site with missing inputs (`oauth_gitea.rs`
        // `save_user_data` passes its `Option`s straight through).
        let gitea = LoginStamps {
            medium: Some("gitea"),
            login_ip: None,
            login_uagent: None,
            avatar: Some(""),
            avatar_asset_id: Some(None),
            display_name: Some(DisplayNameUpdate::BackfillCase),
            is_staff: Some(StaffSync::OrSuperuser),
            touch_updated_at: true,
            rotated_token: "tok",
            now,
        };
        sqlx::query("SAVEPOINT sp_gitea")
            .execute(&mut *tx)
            .await
            .expect("savepoint");
        let err = save_user_data(&TxConn::new(&mut tx), user_id, &gitea)
            .await
            .expect_err("missing ip/uagent fails");
        assert_not_null_violation(&err);
        sqlx::query("ROLLBACK TO SAVEPOINT sp_gitea")
            .execute(&mut *tx)
            .await
            .expect("rollback to savepoint");
        // The same inputs through the email shape fail identically.
        let email = LoginStamps {
            medium: Some("email"),
            login_ip: None,
            login_uagent: None,
            avatar: None,
            avatar_asset_id: None,
            display_name: Some(DisplayNameUpdate::BackfillCase),
            is_staff: Some(StaffSync::OrSuperuser),
            touch_updated_at: true,
            rotated_token: "tok",
            now,
        };
        let err = save_user_data(&TxConn::new(&mut tx), user_id, &email)
            .await
            .expect_err("email shape fails identically");
        assert_not_null_violation(&err);
    }

    // -- PIDASHCONV-1615: race-tolerant profile get_or_create + setter ----

    /// The 30 `profiles` columns both race-table DDLs share: the
    /// `PROFILES_DDL` column set plus the production `user_id` unique
    /// index the get_or_create race arm keys on. One const so the
    /// temp-table tests and the permanent race table cannot drift.
    const PROFILES_UNIQUE_COLUMNS: &str = "(
        id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL,
        updated_at TIMESTAMPTZ NOT NULL, user_id UUID NOT NULL UNIQUE,
        theme JSONB NOT NULL, is_app_rail_docked BOOLEAN NOT NULL,
        is_tour_completed BOOLEAN NOT NULL, onboarding_step JSONB NOT NULL,
        use_case TEXT, role TEXT, is_onboarded BOOLEAN NOT NULL,
        last_workspace_id UUID, billing_address_country TEXT NOT NULL,
        billing_address JSONB, has_billing_address BOOLEAN NOT NULL,
        company_name TEXT NOT NULL, notification_view_mode TEXT NOT NULL,
        is_smooth_cursor_enabled BOOLEAN NOT NULL,
        is_mobile_onboarded BOOLEAN NOT NULL,
        mobile_onboarding_step JSONB NOT NULL,
        mobile_timezone_auto_set BOOLEAN NOT NULL, language TEXT NOT NULL,
        start_of_the_week SMALLINT NOT NULL, goals JSONB NOT NULL,
        background_color TEXT NOT NULL,
        is_navigation_tour_completed BOOLEAN NOT NULL,
        has_marketing_email_consent BOOLEAN NOT NULL,
        is_subscribed_to_changelog BOOLEAN NOT NULL,
        product_tour JSONB NOT NULL, settings JSONB NOT NULL)";

    /// Temp-table DDL over [`PROFILES_UNIQUE_COLUMNS`].
    fn temp_profiles_ddl() -> String {
        format!("CREATE TEMPORARY TABLE profiles {PROFILES_UNIQUE_COLUMNS}")
    }

    /// Permanent twin for the true-concurrency race test: temp tables
    /// are session-private, so racers on separate pool connections
    /// need a shared table. No FKs — the race inserts a parentless
    /// row, and the test skips on migrated scratch DBs (see below).
    fn race_profiles_ddl() -> String {
        format!("CREATE TABLE IF NOT EXISTS profiles {PROFILES_UNIQUE_COLUMNS}")
    }

    /// Create leg stores every Django model default
    /// (`db/models/user.py:200-273`), found leg returns the same row,
    /// and the JSONB columns land byte-identical to the same values
    /// spelled differently (compact, scrambled key order — the JSONB
    /// input spelling is not part of the stored bytes).
    #[tokio::test]
    async fn live_profile_get_or_create_roundtrip_pins_defaults() {
        let Some(pool) = live_single_pool().await else {
            return;
        };
        sqlx::query(&temp_profiles_ddl())
            .execute(&pool)
            .await
            .expect("temp profiles");
        let now: DateTime<Utc> = "2026-10-28T12:00:00+00:00".parse().expect("T0");
        let user_id = live_uuid("000000006151");
        let background = "#a1b2c3";
        let fresh = get_or_create_profile(&orm(&pool), user_id, now, background)
            .await
            .expect("create leg");
        assert!(!fresh.is_onboarded);
        assert_eq!(fresh.last_workspace_id, None);
        let row: sqlx::postgres::PgRow = sqlx::query(
            "SELECT id, created_at, updated_at, user_id, theme, is_app_rail_docked,
             is_tour_completed, onboarding_step, use_case, role, is_onboarded,
             last_workspace_id, billing_address_country, billing_address,
             has_billing_address, company_name, notification_view_mode,
             is_smooth_cursor_enabled, is_mobile_onboarded, mobile_onboarding_step,
             mobile_timezone_auto_set, language, start_of_the_week, goals,
             background_color, is_navigation_tour_completed,
             has_marketing_email_consent, is_subscribed_to_changelog,
             product_tour, settings FROM profiles WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        let id: Uuid = row.try_get("id").expect("id");
        assert_eq!(id, fresh.id);
        let created_at: DateTime<Utc> = row.try_get("created_at").expect("created_at");
        let updated_at: DateTime<Utc> = row.try_get("updated_at").expect("updated_at");
        assert_eq!(created_at, now);
        assert_eq!(updated_at, now);
        let theme: serde_json::Value = row.try_get("theme").expect("theme");
        assert_eq!(theme, serde_json::json!({}));
        let docked: bool = row.try_get("is_app_rail_docked").expect("docked");
        assert!(docked);
        let tour: bool = row.try_get("is_tour_completed").expect("tour");
        assert!(!tour);
        let onboarding: serde_json::Value = row.try_get("onboarding_step").expect("onboarding");
        assert_eq!(
            onboarding,
            serde_json::json!({
                "profile_complete": false,
                "workspace_create": false,
                "workspace_invite": false,
                "workspace_join": false,
            })
        );
        let use_case: Option<String> = row.try_get("use_case").expect("use_case");
        assert_eq!(use_case, None);
        let role: Option<String> = row.try_get("role").expect("role");
        assert_eq!(role, None);
        let onboarded: bool = row.try_get("is_onboarded").expect("onboarded");
        assert!(!onboarded);
        let country: String = row.try_get("billing_address_country").expect("country");
        assert_eq!(country, "INDIA");
        let address: Option<serde_json::Value> = row.try_get("billing_address").expect("address");
        assert_eq!(address, None);
        let has_address: bool = row.try_get("has_billing_address").expect("has_address");
        assert!(!has_address);
        let company: String = row.try_get("company_name").expect("company");
        assert_eq!(company, "");
        let view_mode: String = row.try_get("notification_view_mode").expect("view_mode");
        assert_eq!(view_mode, "full");
        let smooth: bool = row.try_get("is_smooth_cursor_enabled").expect("smooth");
        assert!(!smooth);
        let mobile: bool = row.try_get("is_mobile_onboarded").expect("mobile");
        assert!(!mobile);
        let mobile_step: serde_json::Value =
            row.try_get("mobile_onboarding_step").expect("mobile_step");
        assert_eq!(
            mobile_step,
            serde_json::json!({
                "profile_complete": false,
                "workspace_create": false,
                "workspace_join": false,
            })
        );
        let tz_auto: bool = row.try_get("mobile_timezone_auto_set").expect("tz_auto");
        assert!(!tz_auto);
        let language: String = row.try_get("language").expect("language");
        assert_eq!(language, "en");
        let week_start: i16 = row.try_get("start_of_the_week").expect("week_start");
        assert_eq!(week_start, 0);
        let goals: serde_json::Value = row.try_get("goals").expect("goals");
        assert_eq!(goals, serde_json::json!({}));
        let stored_bg: String = row.try_get("background_color").expect("bg");
        assert_eq!(stored_bg, background);
        let nav_tour: bool = row
            .try_get("is_navigation_tour_completed")
            .expect("nav_tour");
        assert!(!nav_tour);
        let marketing: bool = row
            .try_get("has_marketing_email_consent")
            .expect("marketing");
        assert!(!marketing);
        let changelog: bool = row
            .try_get("is_subscribed_to_changelog")
            .expect("changelog");
        assert!(!changelog);
        let tour_json: serde_json::Value = row.try_get("product_tour").expect("product_tour");
        assert_eq!(
            tour_json,
            serde_json::json!({
                "work_items": false,
                "cycles": false,
                "modules": false,
                "intake": false,
                "pages": false,
            })
        );
        let settings: serde_json::Value = row.try_get("settings").expect("settings");
        assert_eq!(settings, serde_json::json!({}));
        // The stored JSONB bytes equal the same values spelled
        // compactly with scrambled key order: literal spelling is not
        // part of the stored row.
        let canonical: (bool, bool, bool) = sqlx::query_as(
            "SELECT onboarding_step::text = '{\"workspace_join\": false, \
             \"profile_complete\": false, \"workspace_create\": false, \
             \"workspace_invite\": false}'::jsonb::text,
             mobile_onboarding_step::text = '{\"workspace_join\": false, \
             \"profile_complete\": false, \
             \"workspace_create\": false}'::jsonb::text,
             product_tour::text = '{\"pages\": false, \"work_items\": false, \
             \"cycles\": false, \"modules\": false, \
             \"intake\": false}'::jsonb::text FROM profiles WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("canonical bytes");
        assert!(canonical.0 && canonical.1 && canonical.2);
        // Found leg: same row, no second insert.
        let found = get_or_create_profile(&orm(&pool), user_id, now, "#ffffff")
            .await
            .expect("found leg");
        assert_eq!(found, fresh);
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM profiles WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count.0, 1);
    }

    /// A racing loser's insert surfaces SQLSTATE 23505 — the trigger
    /// the [`get_or_create_profile`] re-probe arm and the documented
    /// [`insert_default_profile`] savepoint contract key on.
    #[tokio::test]
    async fn live_profile_insert_duplicate_surfaces_unique_violation() {
        let Some(pool) = live_single_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(&temp_profiles_ddl())
            .execute(&mut *tx)
            .await
            .expect("temp profiles");
        let now: DateTime<Utc> = "2026-10-28T12:00:00+00:00".parse().expect("T0");
        let user_id = live_uuid("000000006152");
        insert_default_profile(&TxConn::new(&mut tx), user_id, now, "#a1b2c3")
            .await
            .expect("first insert");
        sqlx::query("SAVEPOINT sp_profile_race")
            .execute(&mut *tx)
            .await
            .expect("savepoint");
        let raced = insert_default_profile(&TxConn::new(&mut tx), user_id, now, "#d4e5f6").await;
        sqlx::query("ROLLBACK TO SAVEPOINT sp_profile_race")
            .execute(&mut *tx)
            .await
            .expect("rollback");
        let err = raced.expect_err("duplicate user_id fails");
        assert!(is_unique_violation(&err), "got {err:?}");
    }

    /// Sixteen racers on separate pool connections converge: every
    /// call returns the same row and exactly one row lands. Skipped on
    /// migrated scratch DBs, where `profiles` carries FKs to `users`
    /// and the parentless race row cannot land.
    #[tokio::test]
    async fn live_profile_get_or_create_race_converges_one_row() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };

        let Some(pool) = scratch_pool().await else {
            return;
        };
        let fk: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pg_constraint
             WHERE conrelid = to_regclass('profiles') AND contype = 'f'",
        )
        .fetch_one(&pool)
        .await
        .expect("fk probe");
        if fk.0 > 0 {
            eprintln!("skipping race test: scratch profiles carries FKs (migrated DB)");
            return;
        }
        sqlx::query(&race_profiles_ddl())
            .execute(&pool)
            .await
            .expect("race profiles table");
        let now: DateTime<Utc> = "2026-10-28T12:00:00+00:00".parse().expect("T0");
        let user_id = Uuid::new_v4();
        sqlx::query("DELETE FROM profiles WHERE user_id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("clean slate");
        const RACERS: usize = 16;
        let gate = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::with_capacity(RACERS);
        for _ in 0..RACERS {
            let (pool, gate) = (pool.clone(), gate.clone());
            tasks.push(tokio::spawn(async move {
                gate.fetch_add(1, Ordering::SeqCst);
                while gate.load(Ordering::SeqCst) < RACERS {
                    tokio::task::yield_now().await;
                }
                get_or_create_profile(&orm(&pool), user_id, now, "#1615a1").await
            }));
        }
        let mut winner: Option<Uuid> = None;
        for task in tasks {
            let path = task.await.expect("racer joins").expect("racer gets a row");
            assert!(!path.is_onboarded);
            assert_eq!(path.last_workspace_id, None);
            match winner {
                Some(id) => assert_eq!(path.id, id, "every racer sees the same row"),
                None => winner = Some(path.id),
            }
        }
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM profiles WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count.0, 1, "exactly one row wins");
        sqlx::query("DELETE FROM profiles WHERE user_id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("cleanup");
    }

    /// The setter restamps `updated_at` and repoints
    /// `last_workspace_id`, twice in a row; every other column of the
    /// row and every other row stays untouched.
    #[tokio::test]
    async fn live_set_profile_last_workspace_stamps_two_columns() {
        let Some(pool) = live_single_pool().await else {
            return;
        };
        sqlx::query(&temp_profiles_ddl())
            .execute(&pool)
            .await
            .expect("temp profiles");
        let t0: DateTime<Utc> = "2026-10-28T12:00:00+00:00".parse().expect("T0");
        let t1: DateTime<Utc> = "2026-10-28T12:00:01+00:00".parse().expect("T1");
        let t2: DateTime<Utc> = "2026-10-28T12:00:02+00:00".parse().expect("T2");
        let user_a = live_uuid("000000006153");
        let user_b = live_uuid("000000006154");
        let background = "#a1b2c3";
        let a = get_or_create_profile(&orm(&pool), user_a, t0, background)
            .await
            .expect("row a");
        let b = get_or_create_profile(&orm(&pool), user_b, t0, background)
            .await
            .expect("row b");
        let ws1 = live_uuid("000000006155");
        let ws2 = live_uuid("000000006156");
        set_profile_last_workspace(&orm(&pool), a.id, ws1, t1)
            .await
            .expect("first stamp");
        let row: sqlx::postgres::PgRow = sqlx::query(
            "SELECT updated_at, last_workspace_id, is_onboarded, company_name,
             background_color, created_at FROM profiles WHERE id = $1",
        )
        .bind(a.id)
        .fetch_one(&pool)
        .await
        .expect("read back a");
        let stamped: DateTime<Utc> = row.try_get("updated_at").expect("updated_at");
        assert_eq!(stamped, t1);
        let last: Option<Uuid> = row.try_get("last_workspace_id").expect("last");
        assert_eq!(last, Some(ws1));
        let onboarded: bool = row.try_get("is_onboarded").expect("onboarded");
        assert!(!onboarded);
        let company: String = row.try_get("company_name").expect("company");
        assert_eq!(company, "");
        let stored_bg: String = row.try_get("background_color").expect("bg");
        assert_eq!(stored_bg, background);
        let created: DateTime<Utc> = row.try_get("created_at").expect("created");
        assert_eq!(created, t0);
        let other: (DateTime<Utc>, Option<Uuid>) =
            sqlx::query_as("SELECT updated_at, last_workspace_id FROM profiles WHERE id = $1")
                .bind(b.id)
                .fetch_one(&pool)
                .await
                .expect("read back b");
        assert_eq!(other, (t0, None));
        set_profile_last_workspace(&orm(&pool), a.id, ws2, t2)
            .await
            .expect("second stamp");
        let moved: (DateTime<Utc>, Option<Uuid>) =
            sqlx::query_as("SELECT updated_at, last_workspace_id FROM profiles WHERE id = $1")
                .bind(a.id)
                .fetch_one(&pool)
                .await
                .expect("read back a again");
        assert_eq!(moved, (t2, Some(ws2)));
    }

    // -- provisioned-user pins (D6 gap PIDASHCONV-1616) -------------------------

    /// `_meta` column order for the provisioned-insert assertion
    /// (the bot test above carries its own copy for its statement).
    const USER_META_COLUMNS: [&str; 40] = [
        "password",
        "last_login",
        "id",
        "username",
        "mobile_number",
        "email",
        "display_name",
        "first_name",
        "last_name",
        "avatar",
        "avatar_asset_id",
        "cover_image",
        "cover_image_asset_id",
        "date_joined",
        "created_at",
        "updated_at",
        "last_location",
        "created_location",
        "is_superuser",
        "is_managed",
        "is_password_expired",
        "is_active",
        "is_staff",
        "is_email_verified",
        "is_password_autoset",
        "is_password_reset_required",
        "token",
        "last_active",
        "last_login_time",
        "last_logout_time",
        "last_login_ip",
        "last_logout_ip",
        "last_login_medium",
        "last_login_uagent",
        "token_updated_at",
        "is_bot",
        "bot_type",
        "user_timezone",
        "is_email_valid",
        "masked_at",
    ];

    fn field_updates_fixture() -> UserFieldUpdates<'static> {
        UserFieldUpdates {
            username: None,
            email: None,
            display_name: None,
        }
    }

    /// Whole-row read-back as JSON: one assertion pins all 40 stored
    /// values, so a swapped bind cannot hide.
    async fn user_json(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: Uuid,
    ) -> serde_json::Value {
        sqlx::query_scalar("SELECT to_jsonb(users) FROM users WHERE id = $1")
            .bind(id)
            .fetch_one(&mut **tx)
            .await
            .expect("user row reads back")
    }

    /// D6 gap PIDASHCONV-1616: both provisioning shapes store their
    /// callers' exact rows — platform (`service.py:255-265`) and OIDC
    /// sync (`sync.py:91-103`) — the prefs call stores the receiver
    /// row (`user.py:307-320`), and the dirty update writes only
    /// dirty columns plus `updated_at` (a no-op when empty).
    #[tokio::test]
    async fn live_provisioned_shapes_store_caller_rows() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        for ddl in [USERS_DDL, PREFS_DDL] {
            sqlx::query(ddl)
                .execute(&mut *tx)
                .await
                .expect("temp identity tables");
        }
        let now: DateTime<Utc> = "2026-10-28T12:00:00+00:00".parse().expect("T0");
        let later: DateTime<Utc> = "2026-10-28T12:05:00+00:00".parse().expect("T1");
        let stamp = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
        let stamp_later = later.to_rfc3339_opts(chrono::SecondsFormat::Secs, false);

        // The platform upsert's values (`service.py:255-265`).
        let platform_id = live_uuid("000000006101");
        let platform = ProvisionedUser {
            id: Some(platform_id),
            email: "plat1616@example.test",
            username: "plat1616user",
            password_hash: "!unusable1616hash0000000000000000000000",
            display_name: "plat1616",
            first_name: "",
            last_name: "",
            avatar: "",
            is_managed: true,
            is_email_verified: true,
            is_password_autoset: false,
            last_login_medium: "email",
            user_timezone: "UTC",
            now,
        };
        let got = insert_provisioned_user(&TxConn::new(&mut tx), &platform)
            .await
            .expect("platform insert");
        assert_eq!(got, platform_id);
        assert_eq!(
            user_json(&mut tx, platform_id).await,
            serde_json::json!({
                "password": "!unusable1616hash0000000000000000000000",
                "last_login": null,
                "id": platform_id.to_string(),
                "username": "plat1616user",
                "mobile_number": null,
                "email": "plat1616@example.test",
                "display_name": "plat1616",
                "first_name": "",
                "last_name": "",
                "avatar": "",
                "avatar_asset_id": null,
                "cover_image": null,
                "cover_image_asset_id": null,
                "date_joined": stamp,
                "created_at": stamp,
                "updated_at": stamp,
                "last_location": "",
                "created_location": "",
                "is_superuser": false,
                "is_managed": true,
                "is_password_expired": false,
                "is_active": true,
                "is_staff": false,
                "is_email_verified": true,
                "is_password_autoset": false,
                "is_password_reset_required": false,
                "token": "",
                "last_active": stamp,
                "last_login_time": null,
                "last_logout_time": null,
                "last_login_ip": "",
                "last_logout_ip": "",
                "last_login_medium": "email",
                "last_login_uagent": "",
                "token_updated_at": null,
                "is_bot": false,
                "bot_type": null,
                "user_timezone": "UTC",
                "is_email_valid": false,
                "masked_at": null,
            })
        );

        // The OIDC sync create's values (`sync.py:91-103`); verified
        // false here so the pin proves the flag is caller-resolved.
        let sync_id = live_uuid("000000006102");
        let sync = ProvisionedUser {
            id: Some(sync_id),
            email: "sync1616@example.test",
            username: "Sync1616@Example.TEST",
            password_hash: "",
            display_name: "Sync User",
            first_name: "Sync",
            last_name: "User",
            avatar: "https://picture.test/sync1616",
            is_managed: false,
            is_email_verified: false,
            is_password_autoset: true,
            last_login_medium: "airepublic",
            user_timezone: "UTC",
            now,
        };
        let got = insert_provisioned_user(&TxConn::new(&mut tx), &sync)
            .await
            .expect("sync insert");
        assert_eq!(got, sync_id);
        assert_eq!(
            user_json(&mut tx, sync_id).await,
            serde_json::json!({
                "password": "",
                "last_login": null,
                "id": sync_id.to_string(),
                "username": "Sync1616@Example.TEST",
                "mobile_number": null,
                "email": "sync1616@example.test",
                "display_name": "Sync User",
                "first_name": "Sync",
                "last_name": "User",
                "avatar": "https://picture.test/sync1616",
                "avatar_asset_id": null,
                "cover_image": null,
                "cover_image_asset_id": null,
                "date_joined": stamp,
                "created_at": stamp,
                "updated_at": stamp,
                "last_location": "",
                "created_location": "",
                "is_superuser": false,
                "is_managed": false,
                "is_password_expired": false,
                "is_active": true,
                "is_staff": false,
                "is_email_verified": false,
                "is_password_autoset": true,
                "is_password_reset_required": false,
                "token": "",
                "last_active": stamp,
                "last_login_time": null,
                "last_logout_time": null,
                "last_login_ip": "",
                "last_logout_ip": "",
                "last_login_medium": "airepublic",
                "last_login_uagent": "",
                "token_updated_at": null,
                "is_bot": false,
                "bot_type": null,
                "user_timezone": "UTC",
                "is_email_valid": false,
                "masked_at": null,
            })
        );

        // The prefs call stores the receiver row (`user.py:307-320`).
        let prefs_id = live_uuid("000000006103");
        insert_user_notification_preference(&TxConn::new(&mut tx), prefs_id, platform_id, now)
            .await
            .expect("prefs insert");
        let prefs: serde_json::Value = sqlx::query_scalar(
            "SELECT to_jsonb(user_notification_preferences) \
             FROM user_notification_preferences WHERE id = $1",
        )
        .bind(prefs_id)
        .fetch_one(&mut *tx)
        .await
        .expect("prefs row reads back");
        assert_eq!(
            prefs,
            serde_json::json!({
                "created_at": stamp,
                "updated_at": stamp,
                "created_by_id": null,
                "updated_by_id": null,
                "deleted_at": null,
                "id": prefs_id.to_string(),
                "user_id": platform_id.to_string(),
                "workspace_id": null,
                "project_id": null,
                "property_change": true,
                "state_change": true,
                "comment": true,
                "mention": true,
                "issue_completed": true,
            })
        );

        // The dirty update writes only dirty columns plus updated_at.
        update_user_fields(
            &TxConn::new(&mut tx),
            sync_id,
            &UserFieldUpdates {
                username: None,
                email: Some("sync1616-new@example.test"),
                display_name: Some("Sync Renamed"),
            },
            later,
        )
        .await
        .expect("dirty update");
        let row = user_json(&mut tx, sync_id).await;
        assert_eq!(row["email"], serde_json::json!("sync1616-new@example.test"));
        assert_eq!(row["display_name"], serde_json::json!("Sync Renamed"));
        assert_eq!(row["updated_at"], serde_json::json!(stamp_later));
        assert_eq!(row["username"], serde_json::json!("Sync1616@Example.TEST"));
        assert_eq!(row["created_at"], serde_json::json!(stamp));

        // Empty updates save nothing — not even updated_at.
        update_user_fields(
            &TxConn::new(&mut tx),
            sync_id,
            &field_updates_fixture(),
            later,
        )
        .await
        .expect("empty update no-ops");
        let row = user_json(&mut tx, sync_id).await;
        assert_eq!(row["updated_at"], serde_json::json!(stamp_later));
    }
    // -- cloud account update legs: live behavior ---------------------------

    /// Temp `accounts`: production nullability in `_meta` order.
    const ACCOUNTS_DDL: &str = "CREATE TEMPORARY TABLE accounts (
        id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL,
        updated_at TIMESTAMPTZ NOT NULL, user_id UUID NOT NULL,
        provider_account_id TEXT NOT NULL, provider TEXT NOT NULL,
        access_token TEXT NOT NULL, access_token_expired_at TIMESTAMPTZ,
        refresh_token TEXT, refresh_token_expired_at TIMESTAMPTZ,
        last_connected_at TIMESTAMPTZ NOT NULL, id_token TEXT NOT NULL,
        metadata JSONB NOT NULL)";

    fn cloud_ts(text: &str) -> DateTime<Utc> {
        text.parse().expect("fixed timestamp")
    }

    async fn seed_account(pool: &sqlx::PgPool, id: Uuid, now: DateTime<Utc>) {
        let tokens = account::TokenFields {
            access_token: Some("at-1".to_owned()),
            refresh_token: Some("rt-1".to_owned()),
            access_token_expired_at: Some(now),
            refresh_token_expired_at: Some(now),
            id_token: Some("idt-1".to_owned()),
        };
        account::create_account(
            &orm(pool),
            &account::NewAccount {
                id,
                user_id: live_uuid("aaaaaaaaaaaa"),
                provider: "airepublic",
                provider_account_id: "sub-1",
                tokens: &tokens,
                now,
                metadata: Some(serde_json::json!({"seeded": true})),
            },
        )
        .await
        .expect("seed account");
    }

    async fn read_account(pool: &sqlx::PgPool, id: Uuid) -> sqlx::postgres::PgRow {
        sqlx::query("SELECT * FROM accounts WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("read back account")
    }

    /// The upsert leg rewrites its ten columns (re-binding `created_at`,
    /// stamping `updated_at`) and leaves the lookup pair alone — on the
    /// pool and on the caller's transaction alike.
    #[tokio::test]
    async fn live_update_account_upsert_rewrites_ten_not_lookups() {
        let Some(pool) = live_single_pool().await else {
            return;
        };
        sqlx::query(ACCOUNTS_DDL)
            .execute(&pool)
            .await
            .expect("temp accounts");
        let t0 = cloud_ts("2026-10-03T12:00:00+00:00");
        let t1 = cloud_ts("2026-10-04T12:00:00+00:00");
        let id = live_uuid("bbbbbbbbbbbb");
        seed_account(&pool, id, t0).await;
        let meta = serde_json::json!({"x": 1});
        account::update_account_upsert(
            &orm(&pool),
            &account::AccountUpsert {
                id,
                created_at: t0,
                user_id: live_uuid("cccccccccccc"),
                access_token: "at-2",
                access_token_expired_at: Some(t1),
                refresh_token: Some("rt-2"),
                refresh_token_expired_at: None,
                last_connected_at: t1,
                id_token: "idt-2",
                metadata: Some(&meta),
            },
            t1,
        )
        .await
        .expect("upsert update");
        let row = read_account(&pool, id).await;
        assert_eq!(
            row.try_get::<DateTime<Utc>, _>("created_at").expect("c"),
            t0
        );
        assert_eq!(
            row.try_get::<DateTime<Utc>, _>("updated_at").expect("u"),
            t1
        );
        assert_eq!(
            row.try_get::<Uuid, _>("user_id").expect("uid"),
            live_uuid("cccccccccccc")
        );
        assert_eq!(
            row.try_get::<String, _>("access_token").expect("at"),
            "at-2"
        );
        assert_eq!(
            row.try_get::<Option<DateTime<Utc>>, _>("access_token_expired_at")
                .expect("aexp"),
            Some(t1)
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("refresh_token")
                .expect("rt"),
            Some("rt-2".to_owned())
        );
        assert_eq!(
            row.try_get::<Option<DateTime<Utc>>, _>("refresh_token_expired_at")
                .expect("rexp"),
            None
        );
        assert_eq!(
            row.try_get::<DateTime<Utc>, _>("last_connected_at")
                .expect("lca"),
            t1
        );
        assert_eq!(row.try_get::<String, _>("id_token").expect("idt"), "idt-2");
        assert_eq!(
            row.try_get::<serde_json::Value, _>("metadata").expect("md"),
            meta
        );
        assert_eq!(
            row.try_get::<String, _>("provider").expect("prov"),
            "airepublic"
        );
        assert_eq!(
            row.try_get::<String, _>("provider_account_id")
                .expect("pid"),
            "sub-1"
        );
        // The executor-generic shape: the same write on the caller's
        // transaction lands on commit.
        let t2 = cloud_ts("2026-10-05T12:00:00+00:00");
        let mut tx = pool.begin().await.expect("tx");
        let meta2 = serde_json::json!({"x": 2});
        account::update_account_upsert(
            &TxConn::new(&mut tx),
            &account::AccountUpsert {
                id,
                created_at: t0,
                user_id: live_uuid("cccccccccccc"),
                access_token: "at-3",
                access_token_expired_at: None,
                refresh_token: None,
                refresh_token_expired_at: Some(t2),
                last_connected_at: t2,
                id_token: "idt-3",
                metadata: Some(&meta2),
            },
            t2,
        )
        .await
        .expect("upsert update on tx");
        tx.commit().await.expect("commit");
        let row = read_account(&pool, id).await;
        assert_eq!(
            row.try_get::<String, _>("access_token").expect("at"),
            "at-3"
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("refresh_token")
                .expect("rt"),
            None
        );
        assert_eq!(
            row.try_get::<DateTime<Utc>, _>("updated_at").expect("u"),
            t2
        );
        assert_eq!(
            row.try_get::<serde_json::Value, _>("metadata").expect("md"),
            meta2
        );
    }

    /// The rotation leg writes its triple and stamps nothing else — not
    /// even `updated_at`.
    #[tokio::test]
    async fn live_update_account_rotated_tokens_writes_triple_only() {
        let Some(pool) = live_single_pool().await else {
            return;
        };
        sqlx::query(ACCOUNTS_DDL)
            .execute(&pool)
            .await
            .expect("temp accounts");
        let t0 = cloud_ts("2026-10-03T12:00:00+00:00");
        let t1 = cloud_ts("2026-10-04T12:00:00+00:00");
        let id = live_uuid("dddddddddddd");
        seed_account(&pool, id, t0).await;
        let mut tx = pool.begin().await.expect("tx");
        account::update_account_rotated_tokens(
            &TxConn::new(&mut tx),
            id,
            "at-9",
            Some(t1),
            Some("rt-9"),
        )
        .await
        .expect("rotate");
        tx.commit().await.expect("commit");
        let row = read_account(&pool, id).await;
        assert_eq!(
            row.try_get::<String, _>("access_token").expect("at"),
            "at-9"
        );
        assert_eq!(
            row.try_get::<Option<DateTime<Utc>>, _>("access_token_expired_at")
                .expect("aexp"),
            Some(t1)
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("refresh_token")
                .expect("rt"),
            Some("rt-9".to_owned())
        );
        assert_eq!(
            row.try_get::<DateTime<Utc>, _>("updated_at").expect("u"),
            t0,
            "rotation stamps nothing"
        );
        assert_eq!(
            row.try_get::<Option<DateTime<Utc>>, _>("refresh_token_expired_at")
                .expect("rexp"),
            Some(t0)
        );
        assert_eq!(row.try_get::<String, _>("id_token").expect("idt"), "idt-1");
        assert_eq!(
            row.try_get::<serde_json::Value, _>("metadata").expect("md"),
            serde_json::json!({"seeded": true})
        );
    }

    /// The snapshot leg writes `metadata` and nothing else.
    #[tokio::test]
    async fn live_update_account_metadata_writes_metadata_only() {
        let Some(pool) = live_single_pool().await else {
            return;
        };
        sqlx::query(ACCOUNTS_DDL)
            .execute(&pool)
            .await
            .expect("temp accounts");
        let t0 = cloud_ts("2026-10-03T12:00:00+00:00");
        let id = live_uuid("eeeeeeeeeeee");
        seed_account(&pool, id, t0).await;
        let snapshot = serde_json::json!({"user_type": 2});
        account::update_account_metadata(&orm(&pool), id, &snapshot)
            .await
            .expect("snapshot");
        let row = read_account(&pool, id).await;
        assert_eq!(
            row.try_get::<serde_json::Value, _>("metadata").expect("md"),
            snapshot
        );
        assert_eq!(
            row.try_get::<String, _>("access_token").expect("at"),
            "at-1"
        );
        assert_eq!(
            row.try_get::<DateTime<Utc>, _>("updated_at").expect("u"),
            t0
        );
        assert_eq!(
            row.try_get::<DateTime<Utc>, _>("last_connected_at")
                .expect("lca"),
            t0
        );
    }

    #[test]
    fn entity_user_save_is_full_row_with_pk_last() {
        let sql = entity_user_save_sql();
        assert!(sql.starts_with("UPDATE \"users\" SET "), "{sql}");
        // Every non-pk column bound in USER_COLUMNS order ($1-$39),
        // pk last ($40) — Django save() shape.
        assert!(sql.contains("\"password\" = $1"), "{sql}");
        assert!(sql.contains("\"masked_at\" = $39"), "{sql}");
        assert!(sql.contains("WHERE \"users\".\"id\" = ($40)"), "{sql}");
        assert!(!sql.contains("\"id\" = $"), "{sql}");
    }

    /// Ops-command saves round-trip: activation flips the flag plus the
    /// `User.save()` outputs; the password save stores the hash and
    /// clears autoset. Same temp-`users` shape as the agent-user tests.
    #[tokio::test]
    async fn live_ops_activation_and_password_saves() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(USERS_DDL)
            .execute(&mut *tx)
            .await
            .expect("temp users");
        let id = live_uuid("aaaaaaaaaaaa");
        sqlx::query(
            "INSERT INTO users (id, password, username, email, display_name, \
             first_name, last_name, avatar, date_joined, created_at, updated_at, \
             last_location, created_location, is_superuser, is_managed, \
             is_password_expired, is_active, is_staff, is_email_verified, \
             is_password_autoset, is_password_reset_required, token, last_login_ip, \
             last_logout_ip, last_login_medium, last_login_uagent, is_bot, \
             user_timezone, is_email_valid) \
             VALUES ($1, 'old-hash', 'op', 'old@x.io', 'Old', 'O', 'L', '', \
             now(), now(), now(), '', '', FALSE, FALSE, FALSE, FALSE, FALSE, \
             FALSE, TRUE, FALSE, 'old-tok', '', '', '', '', FALSE, 'UTC', TRUE)",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .expect("seed user");
        let write = pidash_db::ops::users::UserSaveWrite {
            email: "new@x.io".to_owned(),
            display_name: "New".to_owned(),
            is_staff: true,
            token: "tok2".to_owned(),
            token_updated_at: None,
        };
        save_user_activation(&TxConn::new(&mut tx), id, &write)
            .await
            .expect("activate");
        let row: (bool, String, String, bool, String) = sqlx::query_as(
            "SELECT is_active, email, display_name, is_staff, token \
             FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .expect("read back");
        assert!(row.0);
        assert_eq!(row.1, "new@x.io");
        assert_eq!(row.2, "New");
        assert!(row.3);
        assert_eq!(row.4, "tok2");
        save_user_password(&TxConn::new(&mut tx), id, "pbkdf2$new", &write)
            .await
            .expect("password");
        let row: (String, bool) =
            sqlx::query_as("SELECT password, is_password_autoset FROM users WHERE id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(row.0, "pbkdf2$new");
        assert!(!row.1);
    }

    /// Seed one temp `users` row carrying the columns the stamp tests
    /// read back.
    async fn seed_stamp_user(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: Uuid,
        now: DateTime<Utc>,
    ) {
        sqlx::query(
            "INSERT INTO users (password, id, username, email, display_name, first_name,
             last_name, avatar, date_joined, created_at, updated_at, last_location,
             created_location, is_superuser, is_managed, is_password_expired, is_active,
             is_staff, is_email_verified, is_password_autoset, is_password_reset_required,
             token, last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
             is_bot, user_timezone, is_email_valid)
             VALUES ('usable-hash', $1, 'stamp1628', 'stamp1628@example.test', 'stamp',
             'St', 'Amp', '', $2, $2, $2, '', '', false, false, false, true, false,
             false, false, false, '', '', '', 'email', '', false, 'UTC', false)",
        )
        .bind(user_id)
        .bind(now)
        .execute(&mut **tx)
        .await
        .expect("seed user");
    }

    /// PIDASHCONV-1628: each stamp single writes its columns and
    /// nothing else; the logout `None` arm raises the NOT NULL the app
    /// sign-out swallows.
    #[tokio::test]
    async fn live_moved_stamps_write_their_columns() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(USERS_DDL)
            .execute(&mut *tx)
            .await
            .expect("temp users");
        let now: DateTime<Utc> = "2026-10-27T12:00:00+00:00".parse().expect("T0");
        let later: DateTime<Utc> = "2026-10-28T12:00:00+00:00".parse().expect("T1");
        let user_id = live_uuid("000000001628");
        seed_stamp_user(&mut tx, user_id, now).await;
        stamp_last_login(&TxConn::new(&mut tx), user_id, later)
            .await
            .expect("stamp last_login");
        let stamped: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT last_login FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(stamped, Some(later));
        rehash_password(&TxConn::new(&mut tx), user_id, "new-hash")
            .await
            .expect("rehash");
        let row: (String, DateTime<Utc>) =
            sqlx::query_as("SELECT password, updated_at FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(row, ("new-hash".to_owned(), now));
        touch_user_updated_at(&TxConn::new(&mut tx), user_id, later)
            .await
            .expect("touch");
        let touched: DateTime<Utc> =
            sqlx::query_scalar("SELECT updated_at FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(touched, later);
        clear_avatar(&TxConn::new(&mut tx), user_id)
            .await
            .expect("clear avatar");
        let avatar: String = sqlx::query_scalar("SELECT avatar FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .expect("read back");
        assert_eq!(avatar, "");
        stamp_logout(&TxConn::new(&mut tx), user_id, Some("9.9.9.9"), later)
            .await
            .expect("stamp logout");
        let logged: (String, Option<DateTime<Utc>>) =
            sqlx::query_as("SELECT last_logout_ip, last_logout_time FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(logged, ("9.9.9.9".to_owned(), Some(later)));
        // No token in use: the `User.save()` rotation leaves it alone, the
        // named row keeps its name, and `updated_at` follows `auto_now`.
        let saved: (String, Option<DateTime<Utc>>, String, bool, DateTime<Utc>) = sqlx::query_as(
            "SELECT token, token_updated_at, display_name, is_staff, updated_at \
             FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .expect("read back");
        assert_eq!(
            saved,
            (String::new(), None, "stamp".to_owned(), false, later)
        );
        // NEWPIDASH-17: a token in use rotates, an empty name backfills
        // from the email prefix, and a superuser gains `is_staff`.
        sqlx::query(
            "UPDATE users SET token = 'old', token_updated_at = $2, display_name = '', \
             is_superuser = true WHERE id = $1",
        )
        .bind(user_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("arm rotation");
        let latest = later + chrono::Duration::hours(1);
        stamp_logout(&TxConn::new(&mut tx), user_id, Some("9.9.9.9"), latest)
            .await
            .expect("stamp logout");
        let rotated: (String, Option<DateTime<Utc>>, String, bool, DateTime<Utc>) = sqlx::query_as(
            "SELECT token, token_updated_at, display_name, is_staff, updated_at \
                 FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .expect("read back");
        assert_eq!(rotated.0.len(), 64, "fresh uuid4hex + uuid4hex token");
        assert_ne!(rotated.0, "old");
        assert_eq!(
            (rotated.1, rotated.2.as_str(), rotated.3, rotated.4),
            (Some(latest), "stamp1628", true, latest)
        );
        // The app sign-out binds `None` when no IP is present: the NOT
        // NULL column raises, and the caller swallows it (bare redirect).
        sqlx::query("SAVEPOINT sp_logout_none")
            .execute(&mut *tx)
            .await
            .expect("savepoint");
        let err = stamp_logout(&TxConn::new(&mut tx), user_id, None, later)
            .await
            .expect_err("None ip raises");
        assert_not_null_violation(&err);
        sqlx::query("ROLLBACK TO SAVEPOINT sp_logout_none")
            .execute(&mut *tx)
            .await
            .expect("rollback to savepoint");
    }

    /// PIDASHCONV-1628: the password twins write the hash plus the
    /// `User.save()` effects; rotation fires only when a token is
    /// already in use, and only the login twin bumps `last_login`.
    #[tokio::test]
    async fn live_password_twins_rotation_and_login_arms() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(USERS_DDL)
            .execute(&mut *tx)
            .await
            .expect("temp users");
        let now: DateTime<Utc> = "2026-10-27T12:00:00+00:00".parse().expect("T0");
        let user_id = live_uuid("000000001629");
        seed_stamp_user(&mut tx, user_id, now).await;
        // No token in use: the token columns stay put, `last_login`
        // bumps on the login twin.
        write_password_with_save_effects(
            &TxConn::new(&mut tx),
            user_id,
            &PasswordSave {
                encoded: "hash-1",
                fresh_token: "fresh-1",
                rotated_at: now,
                stamp_last_login: true,
            },
        )
        .await
        .expect("login twin");
        let row: (String, String, Option<DateTime<Utc>>, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT password, token, token_updated_at, last_login FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .expect("read back");
        assert_eq!(row.0, "hash-1");
        assert_eq!(row.1, "");
        assert_eq!(row.2, None);
        assert!(row.3.is_some(), "login twin bumps last_login");
        // A token in use rotates; the reset twin leaves `last_login`
        // alone.
        sqlx::query("UPDATE users SET token = 'old', token_updated_at = $1 WHERE id = $2")
            .bind(now)
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .expect("arm token");
        sqlx::query("UPDATE users SET last_login = NULL WHERE id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .expect("clear last_login");
        write_password_with_save_effects(
            &TxConn::new(&mut tx),
            user_id,
            &PasswordSave {
                encoded: "hash-2",
                fresh_token: "fresh-2",
                rotated_at: now,
                stamp_last_login: false,
            },
        )
        .await
        .expect("reset twin");
        let row: (String, Option<DateTime<Utc>>, Option<DateTime<Utc>>) =
            sqlx::query_as("SELECT token, token_updated_at, last_login FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(row.0, "fresh-2");
        assert_eq!(row.1, Some(now));
        assert_eq!(row.2, None);
    }

    /// PIDASHCONV-1628: the profile pointer follows the approved
    /// workspace and is nulled when that workspace is deleted.
    #[tokio::test]
    async fn live_last_workspace_set_and_clear() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(PROFILES_DDL)
            .execute(&mut *tx)
            .await
            .expect("temp profiles");
        let now: DateTime<Utc> = "2026-10-27T12:00:00+00:00".parse().expect("T0");
        let user_id = live_uuid("00000000162a");
        let ws_id = live_uuid("00000000162b");
        sqlx::query(
            "INSERT INTO profiles (id, created_at, updated_at, user_id, theme,
             is_app_rail_docked, is_tour_completed, onboarding_step, is_onboarded,
             billing_address_country, has_billing_address, company_name,
             notification_view_mode, is_smooth_cursor_enabled, is_mobile_onboarded,
             mobile_onboarding_step, mobile_timezone_auto_set, language,
             start_of_the_week, goals, background_color,
             is_navigation_tour_completed, has_marketing_email_consent,
             is_subscribed_to_changelog, product_tour, settings)
             VALUES (gen_random_uuid(), $1, $1, $2, '{}', TRUE, FALSE, '{}', FALSE,
             'INDIA', FALSE, '', 'full', FALSE, FALSE, '{}', FALSE, 'en', 0, '{}',
             '#ffffff', FALSE, FALSE, FALSE, '{}', '{}')",
        )
        .bind(now)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .expect("seed profile");
        set_last_workspace(&TxConn::new(&mut tx), user_id, ws_id)
            .await
            .expect("set pointer");
        let pointed: Option<Uuid> =
            sqlx::query_scalar("SELECT last_workspace_id FROM profiles WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(pointed, Some(ws_id));
        clear_last_workspace(&TxConn::new(&mut tx), ws_id)
            .await
            .expect("clear pointers");
        let pointed: Option<Uuid> =
            sqlx::query_scalar("SELECT last_workspace_id FROM profiles WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(pointed, None);
    }

    /// PIDASHCONV-1628: the `PATCH users/me/` save persists the merged
    /// row plus the derived values.
    #[tokio::test]
    async fn live_save_me_profile_persists_merged_row() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(USERS_DDL)
            .execute(&mut *tx)
            .await
            .expect("temp users");
        let now: DateTime<Utc> = "2026-10-27T12:00:00+00:00".parse().expect("T0");
        let later: DateTime<Utc> = "2026-10-28T12:00:00+00:00".parse().expect("T1");
        let user_id = live_uuid("00000000162c");
        seed_stamp_user(&mut tx, user_id, now).await;
        save_me_profile(
            &TxConn::new(&mut tx),
            user_id,
            &MeProfileSave {
                last_login: Some(later),
                display_name: "Ada",
                first_name: "Ada",
                last_name: "Lovelace",
                avatar: "img",
                avatar_asset_id: None,
                cover_image: Some("cover"),
                cover_image_asset_id: None,
                is_password_expired: true,
                is_password_reset_required: false,
                bot_type: Some("agent"),
                user_timezone: "UTC",
                is_email_valid: true,
                masked_at: None,
                email: "ada@example.test",
                token: "tok",
                token_updated_at: Some(now),
                is_staff: true,
                updated_at: later,
            },
        )
        .await
        .expect("save me profile");
        let row: (
            Option<DateTime<Utc>>,
            String,
            String,
            bool,
            bool,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT last_login, display_name, email, is_password_expired, is_staff, bot_type \
                 FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .expect("read back");
        assert_eq!(
            row,
            (
                Some(later),
                "Ada".to_owned(),
                "ada@example.test".to_owned(),
                true,
                true,
                Some("agent".to_owned())
            )
        );
    }
}
