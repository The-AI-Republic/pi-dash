//! OSS identity writes: `users`, `profiles`, `accounts`,
//! `user_notification_preferences` (ORM-07, PIDASHCONV-1525).
//!
//! Every OSS write to the four identity tables lives in this module; the
//! signup, admin, sync and bot-provisioning call sites call these
//! functions instead of executing SQL text. Reads stay where they are
//! (design D5).
//!
//! Two execution shapes, by writer kind:
//!
//! * [`create_user`] runs on any SeaORM connection (the `&DatabaseConnection`
//!   the binary carries, or a future `WriteTxn` handle): it inserts through
//!   the `users` entity, so the `after_save` hook
//!   (`db::entity::users_hooks`, the `create_user_notification` post_save
//!   receiver, `user.py:307-320`) fires and the preferences row lands in
//!   the same transaction. The profile row is service-level, not hook
//!   code: Django creates it explicitly in the adapter
//!   (`adapter/base.py:342`, defaults) and the license admin view
//!   (`admin.py:218`, with `company_name`), never in a receiver (D4).
//! * Everything else is generic over `sqlx::Executor`, like
//!   [`super::tokens`]: the login stamps, the IDP sync, the avatar
//!   singles and the account upsert all run on the caller's pool or on
//!   the caller's existing `sqlx` transaction with no restructuring.
//!   The bot insert ([`create_bot_user`]) is executor-generic too: the
//!   workpad `get_or_create` flow races under `SAVEPOINT`s on a
//!   `&mut PgConnection` Django's `transaction.atomic()` nests to, and
//!   an entity insert cannot join that transaction (D5). Bots get
//!   neither hook effect anyway (Django skips bots in the receiver and
//!   creates no profile for them), so nothing is lost.
//! * The provisioned insert ([`insert_provisioned_user`]) is
//!   executor-generic for the same D5 reason: the overlay's platform
//!   upsert and OIDC sync create users on a caller-owned sqlx
//!   transaction. Neither path writes a profile row here, and the
//!   prefs row is the separate explicit
//!   [`insert_user_notification_preference`] call (both paths create
//!   non-bots, so the receiver gate is statically satisfied; the sync
//!   path stamps it with a fresh `now`). The platform dirty update
//!   ([`update_user_fields`]) rides the same executor shape (D6 gap
//!   PIDASHCONV-1616).
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
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, EntityTrait};
use std::fmt::Write as _;
use uuid::Uuid;

use super::error::DomainError;

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
    C: sea_orm::ConnectionTrait,
{
    use pidash_db::entity::{profiles, users};
    // `None` ip/user-agent renders NULL into the NOT NULL columns, so the
    // request 500s — the 500 Python answers uncaught (`META.get` is None,
    // `user.py` has no null=True). Every caller maps any error here to its
    // 500 arm, exactly like today's NULL-violation arm.
    let (Some(login_ip), Some(login_uagent)) = (user.login_ip, user.login_uagent) else {
        return Err(DomainError::Work("login ip/user-agent is None".to_owned()));
    };
    let id = user.id.unwrap_or_else(Uuid::new_v4);
    let now = user.now.fixed_offset();
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
        last_active: Set(user.last_active.map(|dt| dt.fixed_offset())),
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
        let prow = profiles::ActiveModel {
            id: Set(Uuid::new_v4()),
            created_at: Set(now),
            updated_at: Set(now),
            user_id: Set(id),
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
            company_name: Set(profile.company_name.to_owned()),
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
            background_color: Set(profile.background_color.clone()),
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
        };
        profiles::Entity::insert(prow)
            .exec(db)
            .await
            .map_err(DomainError::Db)?;
    }
    Ok(id)
}

// ---------------------------------------------------------------------------
// bots (executor-generic; no hook effects by design)
// ---------------------------------------------------------------------------

/// The bot `users` insert, moved verbatim from
/// `db::orchestration::workpad::AGENT_USER_INSERT_SQL` (D8: that const is
/// now `pub(crate)`): all 40 physical columns in `_meta` order, so the
/// statement holds every `NOT NULL` constraint with no database
/// default. `$1..$40` follow the column order.
const BOT_USER_INSERT_SQL: &str = "INSERT INTO users (password, last_login, id, username,
    mobile_number, email, display_name, first_name, last_name, avatar, avatar_asset_id,
    cover_image, cover_image_asset_id, date_joined, created_at, updated_at, last_location,
    created_location, is_superuser, is_managed, is_password_expired, is_active, is_staff,
    is_email_verified, is_password_autoset, is_password_reset_required, token, last_active,
    last_login_time, last_logout_time, last_login_ip, last_logout_ip, last_login_medium,
    last_login_uagent, token_updated_at, is_bot, bot_type, user_timezone, is_email_valid,
    masked_at)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18,
    $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32, $33, $34, $35,
    $36, $37, $38, $39, $40)";

/// `set_unusable_password` + `save(update_fields=["password"])`
/// (`workpad.py:62-65`), moved verbatim from
/// `db::orchestration::workpad::AGENT_USER_PASSWORD_SQL` (D8). `$1` is
/// the unusable hash, `$2` the user id.
const BOT_USER_PASSWORD_SQL: &str = "UPDATE users SET password = $1 WHERE id = $2";

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

/// Bot `users` insert for the provisioning flows that cannot run an
/// entity insert: the workpad `get_or_create` races under savepoints on
/// a `&mut PgConnection`, and the seed bot follows the same function
/// for one bot path. Column order and values are the moved
/// `AGENT_USER_INSERT_SQL` verbatim; like the hook, bots get no
/// preferences row (the receiver skips `is_bot`) and no profile row
/// (neither Django flow creates one). Returns the row id.
pub async fn create_bot_user<'e, E>(ex: E, bot: &NewBotUser<'_>) -> Result<Uuid, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let id = bot.id.unwrap_or_else(Uuid::new_v4);
    sqlx::query(BOT_USER_INSERT_SQL)
        .bind(bot.password_hash)
        .bind(Option::<DateTime<Utc>>::None)
        .bind(id)
        .bind(bot.username)
        .bind(Option::<String>::None)
        .bind(bot.email)
        .bind(bot.display_name)
        .bind(bot.first_name)
        .bind(bot.last_name)
        .bind("")
        .bind(Option::<Uuid>::None)
        .bind(Option::<String>::None)
        .bind(Option::<Uuid>::None)
        .bind(bot.now)
        .bind(bot.now)
        .bind(bot.now)
        .bind("")
        .bind("")
        .bind(false)
        .bind(false)
        .bind(false)
        .bind(true)
        .bind(false)
        .bind(false)
        .bind(bot.is_password_autoset)
        .bind(false)
        .bind("")
        .bind(bot.now)
        .bind(Option::<DateTime<Utc>>::None)
        .bind(Option::<DateTime<Utc>>::None)
        .bind("")
        .bind("")
        .bind("email")
        .bind("")
        .bind(Option::<DateTime<Utc>>::None)
        .bind(true)
        .bind(bot.bot_type)
        .bind("UTC")
        .bind(false)
        .bind(Option::<DateTime<Utc>>::None)
        .execute(ex)
        .await?;
    Ok(id)
}

/// `set_unusable_password` + `save(update_fields=["password"])`
/// (`workpad.py:62-65`): the bot insert stores a placeholder password
/// and this stamps the unusable hash right after.
pub async fn stamp_bot_unusable_password<'e, E>(
    ex: E,
    user_id: Uuid,
    unusable_hash: &str,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(BOT_USER_PASSWORD_SQL)
        .bind(unusable_hash)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

// ---------------------------------------------------------------------------
// provisioned users (executor-generic; D6 gap PIDASHCONV-1616)
// ---------------------------------------------------------------------------

/// The provisioned `users` insert: all 40 physical columns in `_meta`
/// order, the same list [`BOT_USER_INSERT_SQL`] carries — one
/// full-row shape for the table. The platform path spells the columns
/// id-first; the stored rows are identical either way. `$1..$40`
/// follow the column order.
const PROVISIONED_USER_INSERT_SQL: &str = "INSERT INTO users (password, last_login, id, username,
    mobile_number, email, display_name, first_name, last_name, avatar, avatar_asset_id,
    cover_image, cover_image_asset_id, date_joined, created_at, updated_at, last_location,
    created_location, is_superuser, is_managed, is_password_expired, is_active, is_staff,
    is_email_verified, is_password_autoset, is_password_reset_required, token, last_active,
    last_login_time, last_logout_time, last_login_ip, last_logout_ip, last_login_medium,
    last_login_uagent, token_updated_at, is_bot, bot_type, user_timezone, is_email_valid,
    masked_at)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18,
    $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32, $33, $34, $35,
    $36, $37, $38, $39, $40)";

/// The `User` `post_save` effect as explicit SQL
/// (`db/models/user.py:307-320`): one `user_notification_preferences`
/// row — all five flags true, NULL audit and scope columns — in
/// `_meta` column order. `$1..$14` follow the column order.
const USER_NOTIFICATION_PREFERENCE_INSERT_SQL: &str = "INSERT INTO user_notification_preferences
    (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, user_id,
    workspace_id, project_id, property_change, state_change, comment, mention, issue_completed)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)";

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

/// `User(...)` + first `save()` for the tx-bound provisioning paths:
/// the platform upsert (`service.py:255-265`) and the OIDC sync
/// `get_or_create` create side (`sync.py:91-103`). Executor-generic —
/// both callers hold a sqlx transaction an entity insert cannot join
/// (D5) — and profile-free. The `post_save` prefs effect is the
/// caller's separate [`insert_user_notification_preference`] call.
/// Returns the row id (generated when `ProvisionedUser::id` is
/// `None`).
pub async fn insert_provisioned_user<'e, E>(
    ex: E,
    user: &ProvisionedUser<'_>,
) -> Result<Uuid, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let id = user.id.unwrap_or_else(Uuid::new_v4);
    sqlx::query(PROVISIONED_USER_INSERT_SQL)
        .bind(user.password_hash)
        .bind(Option::<DateTime<Utc>>::None)
        .bind(id)
        .bind(user.username)
        .bind(Option::<String>::None)
        .bind(user.email)
        .bind(user.display_name)
        .bind(user.first_name)
        .bind(user.last_name)
        .bind(user.avatar)
        .bind(Option::<Uuid>::None)
        .bind(Option::<String>::None)
        .bind(Option::<Uuid>::None)
        .bind(user.now)
        .bind(user.now)
        .bind(user.now)
        .bind("")
        .bind("")
        .bind(false)
        .bind(user.is_managed)
        .bind(false)
        .bind(true)
        .bind(false)
        .bind(user.is_email_verified)
        .bind(user.is_password_autoset)
        .bind(false)
        .bind("")
        .bind(user.now)
        .bind(Option::<DateTime<Utc>>::None)
        .bind(Option::<DateTime<Utc>>::None)
        .bind("")
        .bind("")
        .bind(user.last_login_medium)
        .bind("")
        .bind(Option::<DateTime<Utc>>::None)
        .bind(false)
        .bind(Option::<String>::None)
        .bind(user.user_timezone)
        .bind(false)
        .bind(Option::<DateTime<Utc>>::None)
        .execute(ex)
        .await?;
    Ok(id)
}

/// The `User` `post_save` effect as an explicit call
/// (`db/models/user.py:307-320`, reached from `service.py:265` and
/// `sync.py:91`): one preferences row — all five flags true, NULL
/// audit and scope columns. Executor-generic: the tx-bound callers
/// cannot reach the entity `after_save` hook (D5). `row_id` is
/// caller-generated (`uuid4`, both callers); `now` stamps both
/// columns (the platform path reuses the user row's `now`, the sync
/// path a fresh stamp).
pub async fn insert_user_notification_preference<'e, E>(
    ex: E,
    row_id: Uuid,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(USER_NOTIFICATION_PREFERENCE_INSERT_SQL)
        .bind(now)
        .bind(now)
        .bind(Option::<Uuid>::None)
        .bind(Option::<Uuid>::None)
        .bind(Option::<DateTime<Utc>>::None)
        .bind(row_id)
        .bind(user_id)
        .bind(Option::<Uuid>::None)
        .bind(Option::<Uuid>::None)
        .bind(true)
        .bind(true)
        .bind(true)
        .bind(true)
        .bind(true)
        .execute(ex)
        .await
        .map(|_| ())
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

/// Render the dirty-fields `UPDATE` for one input combination (fixed
/// `_meta` order: username, email, display_name, updated_at — Django
/// 4.2 re-sorts `update_fields`, `base.py:952-957`). `None` when no
/// column is dirty: the save never runs. Pure, so the unit tests pin
/// the exact text per combination.
fn user_fields_sql(updates: &UserFieldUpdates<'_>) -> Option<String> {
    if updates.is_empty() {
        return None;
    }
    let mut set = Vec::new();
    let mut next = 0;
    let mut arg = || {
        next += 1;
        format!("${next}")
    };
    if updates.username.is_some() {
        set.push(format!("\"username\" = {}", arg()));
    }
    if updates.email.is_some() {
        set.push(format!("\"email\" = {}", arg()));
    }
    if updates.display_name.is_some() {
        set.push(format!("\"display_name\" = {}", arg()));
    }
    let updated_at = arg();
    set.push(format!("\"updated_at\" = {updated_at}"));
    let id = arg();
    Some(format!(
        "UPDATE \"users\" SET {} WHERE \"id\" = {id}",
        set.join(", ")
    ))
}

/// `user.save(update_fields=...)` (`service.py:267-280`): only dirty
/// columns plus `updated_at`, in `_meta` order. A no-op when nothing
/// is dirty (the `if user_updates:` guard). Runs on the caller's
/// executor.
pub async fn update_user_fields<'e, E>(
    ex: E,
    user_id: Uuid,
    updates: &UserFieldUpdates<'_>,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let Some(sql) = user_fields_sql(updates) else {
        return Ok(());
    };
    let mut query = sqlx::query(&sql);
    if let Some(username) = updates.username {
        query = query.bind(username);
    }
    if let Some(email) = updates.email {
        query = query.bind(email);
    }
    if let Some(display_name) = updates.display_name {
        query = query.bind(display_name);
    }
    query.bind(now).bind(user_id).execute(ex).await.map(|_| ())
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSystemUser {
    pub id: Uuid,
    pub username: String,
    pub email: Option<String>,
    pub first_name: String,
    pub last_name: String,
    pub is_bot: bool,
}

/// Lookup by the unique username (`workpad.py:53-61`). Django's
/// `get()` pages with `LIMIT 21` to detect multiplicity; the unique
/// index precludes that, so this reads one row. `$1` is the username.
const AGENT_USER_SELECT_SQL: &str =
    "SELECT id, username, email, first_name, last_name, is_bot FROM users WHERE username = $1 LIMIT 1";

/// Map one agent-user row.
pub fn map_agent_system_user(row: &sqlx::postgres::PgRow) -> Result<AgentSystemUser, sqlx::Error> {
    use sqlx::Row;
    Ok(AgentSystemUser {
        id: row.try_get("id")?,
        username: row.try_get("username")?,
        email: row.try_get("email")?,
        first_name: row.try_get("first_name")?,
        last_name: row.try_get("last_name")?,
        is_bot: row.try_get("is_bot")?,
    })
}

/// Single agent-user row by username, or `None`.
pub async fn find_agent_user<'e, E>(
    ex: E,
    username: &str,
) -> Result<Option<AgentSystemUser>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(AGENT_USER_SELECT_SQL)
        .bind(username)
        .fetch_optional(ex)
        .await?;
    row.map(|row| map_agent_system_user(&row)).transpose()
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
/// inside a transaction — call inside one), and on a unique
/// violation roll back to the savepoint and re-read the winner's
/// row. A human-held name raises [`AgentUserCollisionError`] instead
/// of reusing the row.
pub async fn get_agent_system_user(
    conn: &mut sqlx::PgConnection,
    now: DateTime<Utc>,
) -> Result<(AgentSystemUser, bool), GetAgentUserError> {
    if let Some(user) = find_agent_user(&mut *conn, AGENT_USERNAME).await? {
        if !user.is_bot {
            return Err(AgentUserCollisionError.into());
        }
        return Ok((user, false));
    }
    sqlx::query("SAVEPOINT pidash_agent_user")
        .execute(&mut *conn)
        .await?;
    let created = create_bot_user(
        &mut *conn,
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
            sqlx::query("RELEASE SAVEPOINT pidash_agent_user")
                .execute(&mut *conn)
                .await?;
            id
        }
        Err(e) if is_unique_violation(&e) => {
            sqlx::query("ROLLBACK TO SAVEPOINT pidash_agent_user")
                .execute(&mut *conn)
                .await?;
            sqlx::query("RELEASE SAVEPOINT pidash_agent_user")
                .execute(&mut *conn)
                .await?;
            // Django re-runs `get()` here; when the conflict was
            // email-only the re-get misses and Django re-raises the
            // original `IntegrityError` — so return it, not
            // `RowNotFound` (which would mislead retry-on-missing
            // callers into a loop).
            let Some(user) = find_agent_user(&mut *conn, AGENT_USERNAME).await? else {
                return Err(e.into());
            };
            if !user.is_bot {
                return Err(AgentUserCollisionError.into());
            }
            return Ok((user, true));
        }
        Err(e) => return Err(e.into()),
    };
    stamp_bot_unusable_password(&mut *conn, id, &make_unusable_password()).await?;
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
/// issues `SAVEPOINT`s on the insert path. Moved from the api ports
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
/// `None` on the whole field omits the column (email/magic/admin,
/// whose in-memory name already matches the stored row).
pub enum DisplayNameUpdate<'a> {
    Bound(&'a str),
    BackfillCase,
}

/// How the stamps write `is_staff`: a bound `is_staff || is_superuser`
/// (gitlab/github) or the atomic `OR` expression (gitea/google).
/// `None` omits the column (email/magic/admin).
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
/// * `touch_updated_at`: every path but email bumps `updated_at`
///   (`auto_now`); email's missing bump is preserved verbatim (see the
///   module docs).
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

/// Render the stamps `UPDATE` for one input combination. Column order is
/// fixed (medium, stamps, token, active, avatar pair, display, staff,
/// updated_at); binds follow in the same order. Pure, so the unit
/// tests pin the exact text per caller.
fn login_stamps_sql(stamps: &LoginStamps<'_>) -> String {
    let mut set = Vec::new();
    let mut next = 0;
    let mut arg = || {
        next += 1;
        format!("${next}")
    };
    if stamps.medium.is_some() {
        set.push(format!("\"last_login_medium\" = {}", arg()));
    }
    let active = arg();
    let login_time = arg();
    let ip = arg();
    let uagent = arg();
    let token_at = arg();
    let token = arg();
    set.push(format!("\"last_active\" = {active}"));
    set.push(format!("\"last_login_time\" = {login_time}"));
    set.push(format!("\"last_login_ip\" = {ip}"));
    set.push(format!("\"last_login_uagent\" = {uagent}"));
    set.push(format!("\"token_updated_at\" = {token_at}"));
    set.push(format!("\"token\" = {token}"));
    set.push("\"is_active\" = TRUE".to_owned());
    if stamps.avatar.is_some() {
        set.push(format!("\"avatar\" = {}", arg()));
    }
    if stamps.avatar_asset_id.is_some() {
        set.push(format!("\"avatar_asset_id\" = {}", arg()));
    }
    match &stamps.display_name {
        None => {}
        Some(DisplayNameUpdate::Bound(_)) => {
            set.push(format!("\"display_name\" = {}", arg()));
        }
        Some(DisplayNameUpdate::BackfillCase) => {
            set.push(
                "\"display_name\" = CASE WHEN \"display_name\" = '' \
                 THEN split_part(\"email\", '@', 1) ELSE \"display_name\" END"
                    .to_owned(),
            );
        }
    }
    match &stamps.is_staff {
        None => {}
        Some(StaffSync::Value(_)) => {
            set.push(format!("\"is_staff\" = {}", arg()));
        }
        Some(StaffSync::OrSuperuser) => {
            set.push("\"is_staff\" = \"is_staff\" OR \"is_superuser\"".to_owned());
        }
    }
    if stamps.touch_updated_at {
        set.push(format!("\"updated_at\" = {}", arg()));
    }
    let id = arg();
    format!(
        "UPDATE \"users\" SET {} WHERE \"id\" = {id}",
        set.join(", ")
    )
}

/// `save_user_data` (`adapter/base.py:220-234`): the login stamps plus
/// the `is_active` flip in one `UPDATE`, with the `User.save()` column
/// effects (`user.py:169-187`) each caller dirtied. Runs on the
/// caller's executor — the signup/login flows never hold a transaction
/// here.
pub async fn save_user_data<'e, E>(
    ex: E,
    user_id: Uuid,
    stamps: &LoginStamps<'_>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let sql = login_stamps_sql(stamps);
    let mut query = sqlx::query(&sql);
    if let Some(medium) = stamps.medium {
        query = query.bind(medium);
    }
    query = query
        .bind(stamps.now)
        .bind(stamps.now)
        .bind(stamps.login_ip)
        .bind(stamps.login_uagent)
        .bind(stamps.now)
        .bind(stamps.rotated_token);
    if let Some(avatar) = stamps.avatar {
        query = query.bind(avatar);
    }
    if let Some(asset) = stamps.avatar_asset_id {
        query = query.bind(asset);
    }
    if let Some(DisplayNameUpdate::Bound(name)) = &stamps.display_name {
        query = query.bind(name);
    }
    if let Some(StaffSync::Value(staff)) = &stamps.is_staff {
        query = query.bind(staff);
    }
    if stamps.touch_updated_at {
        query = query.bind(stamps.now);
    }
    query.bind(user_id).execute(ex).await.map(|_| ())
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

/// Render the sync `UPDATE` for one input combination (fixed column
/// order: names, avatar pair, staff, updated_at). Pure; unit-tested
/// per caller like [`login_stamps_sql`].
fn sync_update_sql(sync: &SyncUpdate<'_>) -> String {
    let mut set = Vec::new();
    let mut next = 0;
    let mut arg = || {
        next += 1;
        format!("${next}")
    };
    let first = arg();
    let last = arg();
    let display = arg();
    set.push(format!("\"first_name\" = {first}"));
    set.push(format!("\"last_name\" = {last}"));
    set.push(format!("\"display_name\" = {display}"));
    if sync.avatar.is_some() {
        set.push(format!("\"avatar\" = {}", arg()));
    }
    if sync.avatar_asset_id.is_some() {
        set.push(format!("\"avatar_asset_id\" = {}", arg()));
    }
    if sync.is_staff.is_some() {
        set.push(format!("\"is_staff\" = {}", arg()));
    }
    if sync.touch_updated_at {
        set.push(format!("\"updated_at\" = {}", arg()));
    }
    let id = arg();
    format!(
        "UPDATE \"users\" SET {} WHERE \"id\" = {id}",
        set.join(", ")
    )
}

/// `sync_user_data` (`adapter/base.py:256-287`): the IDP sync save.
/// Runs on the caller's executor.
pub async fn sync_user_data<'e, E>(
    ex: E,
    user_id: Uuid,
    sync: &SyncUpdate<'_>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let sql = sync_update_sql(sync);
    let mut query = sqlx::query(&sql)
        .bind(sync.first_name)
        .bind(sync.last_name)
        .bind(sync.display_name);
    if let Some(avatar) = sync.avatar {
        query = query.bind(avatar);
    }
    if let Some(asset) = sync.avatar_asset_id {
        query = query.bind(asset);
    }
    if let Some(staff) = sync.is_staff {
        query = query.bind(staff);
    }
    if sync.touch_updated_at {
        query = query.bind(sync.now);
    }
    query.bind(user_id).execute(ex).await.map(|_| ())
}

/// `UPDATE users SET avatar_asset_id = $1 WHERE id = $2`: the signup /
/// sync asset stamp, one statement on every path that emits it
/// (gitlab, google twice, github). Runs on the caller's executor.
pub async fn stamp_avatar_asset<'e, E>(
    ex: E,
    user_id: Uuid,
    asset_id: Uuid,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query("UPDATE \"users\" SET \"avatar_asset_id\" = $1 WHERE \"id\" = $2")
        .bind(asset_id)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// `UPDATE users SET avatar = $1 WHERE id = $2`: the sync URL stamp
/// when the avatar upload fails (google twice) and the github signup
/// URL stamp after its `''` insert. Runs on the caller's executor.
pub async fn stamp_avatar_url<'e, E>(
    ex: E,
    user_id: Uuid,
    avatar_url: &str,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query("UPDATE \"users\" SET \"avatar\" = $1 WHERE \"id\" = $2")
        .bind(avatar_url)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
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
const USER_RELOGIN_UPDATE_SQL: &str = "UPDATE \"users\" SET \"avatar\" = $1, \"is_email_verified\" = $2, \"last_login_medium\" = $3 WHERE \"id\" = $4";

/// See [`USER_RELOGIN_UPDATE_SQL`]: the fixed 3-column re-login write.
/// Runs on the caller's executor (the sync driver holds the upsert
/// transaction).
pub async fn update_user_relogin<'e, E>(
    ex: E,
    user_id: Uuid,
    avatar: &str,
    is_email_verified: bool,
    last_login_medium: &str,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(USER_RELOGIN_UPDATE_SQL)
        .bind(avatar)
        .bind(is_email_verified)
        .bind(last_login_medium)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// The OIDC sync every-login bump (`pi_dash_cloud/airepublic/sync.py:126-127`):
/// `update_fields = ["last_login_time"]`, a single column.
const USER_LAST_LOGIN_TIME_UPDATE_SQL: &str =
    "UPDATE \"users\" SET \"last_login_time\" = $1 WHERE \"id\" = $2";

/// See [`USER_LAST_LOGIN_TIME_UPDATE_SQL`]: the single-column timestamp
/// bump. Runs on the caller's executor (the sync driver holds the upsert
/// transaction).
pub async fn update_user_last_login_time<'e, E>(
    ex: E,
    user_id: Uuid,
    at: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(USER_LAST_LOGIN_TIME_UPDATE_SQL)
        .bind(at)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

// ---------------------------------------------------------------------------
// v2 asset link (executor-generic; the file-asset user writes)
// ---------------------------------------------------------------------------

/// `UserAssetsV2Endpoint.entity_asset_save` (`v2.py:41-78`): the
/// avatar link plus the full `user.save()` column effects
/// (`db/models/user.py:169-187`) — normalized email, filled display
/// name, staff sync, token rotation, `auto_now` `updated_at`. The
/// caller owns the user-row read, the previous-asset delete and the
/// [`pidash_db::ops::users::UserSaveWrite`] computation.
const USER_AVATAR_LINK_SQL: &str = "UPDATE \"users\" SET \"avatar\" = $1, \"avatar_asset_id\" = $2, \"email\" = $3, \"display_name\" = $4, \"is_staff\" = $5, \"token\" = $6, \"token_updated_at\" = $7, \"updated_at\" = $8 WHERE \"id\" = $9";

/// Link an avatar asset onto one user row
/// (`avatar = ""`, the ported empty-string write).
pub async fn link_avatar_asset<'e, E>(
    ex: E,
    user_id: Uuid,
    asset_id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(USER_AVATAR_LINK_SQL)
        .bind("")
        .bind(asset_id)
        .bind(&write.email)
        .bind(&write.display_name)
        .bind(write.is_staff)
        .bind(&write.token)
        .bind(write.token_updated_at)
        .bind(now)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// `UserAssetsV2Endpoint.entity_asset_save` (`v2.py:41-78`): the
/// cover link plus the full `user.save()` column effects.
/// `cover_image = NULL` here (asymmetry with avatar — ported).
const USER_COVER_LINK_SQL: &str = "UPDATE \"users\" SET \"cover_image\" = NULL, \"cover_image_asset_id\" = $1, \"email\" = $2, \"display_name\" = $3, \"is_staff\" = $4, \"token\" = $5, \"token_updated_at\" = $6, \"updated_at\" = $7 WHERE \"id\" = $8";

/// Link a cover asset onto one user row.
pub async fn link_cover_asset<'e, E>(
    ex: E,
    user_id: Uuid,
    asset_id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(USER_COVER_LINK_SQL)
        .bind(asset_id)
        .bind(&write.email)
        .bind(&write.display_name)
        .bind(write.is_staff)
        .bind(&write.token)
        .bind(write.token_updated_at)
        .bind(now)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// `UserAssetsV2Endpoint.entity_asset_delete` (`v2.py:80-107`):
/// clears the avatar link FK only (the `avatar` string is untouched
/// here), plus the full `user.save()` column effects.
const USER_AVATAR_UNLINK_SQL: &str = "UPDATE \"users\" SET \"avatar_asset_id\" = NULL, \"email\" = $1, \"display_name\" = $2, \"is_staff\" = $3, \"token\" = $4, \"token_updated_at\" = $5, \"updated_at\" = $6 WHERE \"id\" = $7";

/// Clear one user row's avatar link FK.
pub async fn unlink_avatar_asset<'e, E>(
    ex: E,
    user_id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(USER_AVATAR_UNLINK_SQL)
        .bind(&write.email)
        .bind(&write.display_name)
        .bind(write.is_staff)
        .bind(&write.token)
        .bind(write.token_updated_at)
        .bind(now)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// `UserAssetsV2Endpoint.entity_asset_delete` (`v2.py:80-107`):
/// clears the cover link FK only, plus the full `user.save()`
/// column effects.
const USER_COVER_UNLINK_SQL: &str = "UPDATE \"users\" SET \"cover_image_asset_id\" = NULL, \"email\" = $1, \"display_name\" = $2, \"is_staff\" = $3, \"token\" = $4, \"token_updated_at\" = $5, \"updated_at\" = $6 WHERE \"id\" = $7";

/// Clear one user row's cover link FK.
pub async fn unlink_cover_asset<'e, E>(
    ex: E,
    user_id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(USER_COVER_UNLINK_SQL)
        .bind(&write.email)
        .bind(&write.display_name)
        .bind(write.is_staff)
        .bind(&write.token)
        .bind(write.token_updated_at)
        .bind(now)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

// ---------------------------------------------------------------------------
// notification-preference PATCH (executor-generic; dynamic SET list)
// ---------------------------------------------------------------------------

/// One validated PATCH column write for
/// [`save_preference_columns`].
pub enum PreferenceColumn {
    Stamp(Option<DateTime<Utc>>),
    Id(Option<Uuid>),
    Flag(bool),
}

/// `serializer.save()` (`base.py:306`): the validated columns plus
/// `updated_at`/`updated_by` (`BaseModel.save` stamps the updater
/// through crum on every update — `db/models/base.py:23-44`). The
/// SET list is caller-ordered (fixed push order at the call site);
/// `$1` is the row id, `$2`/`$3` the stamps, `$4+` the columns.
pub async fn save_preference_columns<'e, E>(
    ex: E,
    id: Uuid,
    now: DateTime<Utc>,
    user_id: Uuid,
    columns: &[(&str, PreferenceColumn)],
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let mut sql = String::from(
        "UPDATE \"user_notification_preferences\" SET \"updated_at\" = $2, \"updated_by_id\" = $3",
    );
    for (index, (column, _)) in columns.iter().enumerate() {
        sql.push_str(&format!(", \"{column}\" = ${}", index + 4));
    }
    sql.push_str(" WHERE \"id\" = $1");
    let mut query = sqlx::query(&sql).bind(id).bind(now).bind(user_id);
    for (_, value) in columns {
        query = match value {
            PreferenceColumn::Stamp(stamp) => query.bind(*stamp),
            PreferenceColumn::Id(owner) => query.bind(*owner),
            PreferenceColumn::Flag(flag) => query.bind(*flag),
        };
    }
    query.execute(ex).await.map(|_| ())
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

const PROFILE_PATH_SELECT_SQL: &str =
    "SELECT \"id\", \"is_onboarded\", \"last_workspace_id\" FROM \"profiles\" \
     WHERE \"user_id\" = $1 LIMIT 1";

const PROFILE_DEFAULT_INSERT_SQL: &str = "INSERT INTO \"profiles\"
    (\"id\", \"created_at\", \"updated_at\", \"user_id\", \"theme\", \"is_app_rail_docked\",
     \"is_tour_completed\", \"onboarding_step\", \"use_case\", \"role\", \"is_onboarded\",
     \"last_workspace_id\", \"billing_address_country\", \"billing_address\",
     \"has_billing_address\", \"company_name\", \"notification_view_mode\",
     \"is_smooth_cursor_enabled\", \"is_mobile_onboarded\", \"mobile_onboarding_step\",
     \"mobile_timezone_auto_set\", \"language\", \"start_of_the_week\", \"goals\",
     \"background_color\", \"is_navigation_tour_completed\", \"has_marketing_email_consent\",
     \"is_subscribed_to_changelog\", \"product_tour\", \"settings\")
    VALUES ($1, $2, $2, $3, '{}', TRUE, FALSE,
     '{\"profile_complete\": false, \"workspace_create\": false, \
       \"workspace_invite\": false, \"workspace_join\": false}',
     NULL, NULL, FALSE, NULL, 'INDIA', NULL, FALSE, '', 'full', FALSE, FALSE,
     '{\"profile_complete\": false, \"workspace_create\": false, \"workspace_join\": false}',
     FALSE, 'en', 0, '{}', $4, FALSE, FALSE, FALSE,
     '{\"work_items\": false, \"cycles\": false, \"modules\": false, \"intake\": false, \
       \"pages\": false}', '{}')";

const SET_PROFILE_LAST_WORKSPACE_SQL: &str =
    "UPDATE \"profiles\" SET \"updated_at\" = $1, \"last_workspace_id\" = $2 WHERE \"id\" = $3";

async fn fetch_profile_path<'e, E>(ex: E, user_id: Uuid) -> Result<Option<ProfilePath>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<(Uuid, bool, Option<Uuid>)> = sqlx::query_as(PROFILE_PATH_SELECT_SQL)
        .bind(user_id)
        .fetch_optional(ex)
        .await?;
    Ok(
        row.map(|(id, is_onboarded, last_workspace_id)| ProfilePath {
            id,
            is_onboarded,
            last_workspace_id,
        }),
    )
}

/// The `get_or_create` create leg on its own: the default profile row
/// for `user_id` ([`PROFILE_DEFAULT_INSERT_SQL`], same values
/// [`get_or_create_profile`] inserts). Returns the new row id.
///
/// Race contract: a lost create race surfaces SQLSTATE 23505 from the
/// `user_id` unique index. Callers outside an explicit transaction
/// re-probe on it (that is [`get_or_create_profile`]); callers inside
/// one must hold a savepoint across this call — Django's inner
/// `atomic()` nests to a savepoint there
/// (`db/models/query.py:get_or_create`) — roll back to it on 23505,
/// and re-probe. Any other error propagates untouched.
pub async fn insert_default_profile<'e, E>(
    ex: E,
    user_id: Uuid,
    now: DateTime<Utc>,
    background_color: &str,
) -> Result<Uuid, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let id = Uuid::new_v4();
    sqlx::query(PROFILE_DEFAULT_INSERT_SQL)
        .bind(id)
        .bind(now)
        .bind(user_id)
        .bind(background_color)
        .execute(ex)
        .await?;
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
/// Race-tolerant on executors outside an explicit transaction (the
/// pool, which is what every caller passes): a failed `INSERT` there
/// aborts no transaction, so the re-probe still runs. Callers inside
/// an explicit transaction must hold a savepoint across the whole
/// call, or compose the probe with [`insert_default_profile`] under
/// their own savepoint (its race contract) — otherwise the failed
/// insert aborts their transaction and the re-probe fails with it.
pub async fn get_or_create_profile<'e, E>(
    ex: E,
    user_id: Uuid,
    now: DateTime<Utc>,
    background_color: &str,
) -> Result<ProfilePath, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
{
    if let Some(path) = fetch_profile_path(ex, user_id).await? {
        return Ok(path);
    }
    match insert_default_profile(ex, user_id, now, background_color).await {
        Ok(id) => Ok(ProfilePath {
            id,
            is_onboarded: false,
            last_workspace_id: None,
        }),
        Err(insert) if is_unique_violation(&insert) => match fetch_profile_path(ex, user_id).await?
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
/// (`db/models/user.py:200-273`). Runs on the caller's executor.
pub async fn set_profile_last_workspace<'e, E>(
    ex: E,
    profile_id: Uuid,
    workspace_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(SET_PROFILE_LAST_WORKSPACE_SQL)
        .bind(now)
        .bind(workspace_id)
        .bind(profile_id)
        .execute(ex)
        .await
        .map(|_| ())
}

// ---------------------------------------------------------------------------
// accounts (moved from db::auth_oauth::queries::account)
// ---------------------------------------------------------------------------

/// Account upsert statements
/// (`adapter/oauth.py:105-136`; table `accounts`, `db/models/user.py:280-302`,
/// `unique_together = ["provider", "provider_account_id"]`,
/// `ordering = ("-created_at",)`).
///
/// Moved verbatim from `pidash_db::auth_oauth::queries::account`
/// (PIDASHCONV-1525, ORM-07); the SQL constants are `pub(crate)` (D8)
/// and the update leg is renamed to the issue's `rotate_account_tokens`.
pub mod account {
    use sqlx::postgres::{PgConnection, PgRow};
    use sqlx::Row;

    /// Django table name (`Meta.db_table`, `user.py:298`).
    pub const TABLE: &str = "accounts";

    /// Columns in Django declaration order (audit prefix first), the order
    /// the `SELECT` projects and the `INSERT`/`UPDATE` bind.
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

    /// `Account.objects.filter(user_id=$1 AND provider=$2 AND
    /// provider_account_id=$3).first()` — fixture F6 `emitted_sql.lookup`
    /// (Django `%s` → `$N`).
    pub(crate) const LOOKUP_SQL: &str = "SELECT \"accounts\".\"id\", \"accounts\".\"created_at\", \"accounts\".\"updated_at\", \"accounts\".\"user_id\", \"accounts\".\"provider_account_id\", \"accounts\".\"provider\", \"accounts\".\"access_token\", \"accounts\".\"access_token_expired_at\", \"accounts\".\"refresh_token\", \"accounts\".\"refresh_token_expired_at\", \"accounts\".\"last_connected_at\", \"accounts\".\"id_token\", \"accounts\".\"metadata\" FROM \"accounts\" WHERE \"accounts\".\"user_id\" = $1 AND \"accounts\".\"provider\" = $2 AND \"accounts\".\"provider_account_id\" = $3 LIMIT 1";

    /// `account.save()` on the update path: Django writes every local field,
    /// so the `SET` lists all non-pk columns in field order
    /// (`oauth.py:114-120`; fixture F6 `emitted_sql.update` names the six
    /// assigned columns plus `updated_at`).
    pub(crate) const UPDATE_SQL: &str = "UPDATE \"accounts\" SET \"created_at\" = $1, \"updated_at\" = $2, \"user_id\" = $3, \"provider_account_id\" = $4, \"provider\" = $5, \"access_token\" = $6, \"access_token_expired_at\" = $7, \"refresh_token\" = $8, \"refresh_token_expired_at\" = $9, \"last_connected_at\" = $10, \"id_token\" = $11, \"metadata\" = $12 WHERE \"accounts\".\"id\" = $13";

    /// `Account.objects.create(...)` (`oauth.py:123-133`): `id` is a
    /// client-side `uuid4` (the caller supplies it — this crate's `uuid`
    /// has no `v4` feature, per the `license/queries` precedent),
    /// `metadata` falls back to `{}`, timestamps to `now`.
    pub(crate) const INSERT_SQL: &str = "INSERT INTO \"accounts\" (\"id\", \"created_at\", \"updated_at\", \"user_id\", \"provider_account_id\", \"provider\", \"access_token\", \"access_token_expired_at\", \"refresh_token\", \"refresh_token_expired_at\", \"last_connected_at\", \"id_token\", \"metadata\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)";

    /// The cloud sync update side (`Account.objects.update_or_create`,
    /// `pi_dash_cloud/airepublic/sync.py:146-159`): the eight `defaults`
    /// columns plus `created_at` (re-set to its stored value) and
    /// `updated_at` (re-stamped), in Django `_meta` order. The lookup
    /// columns (`provider`, `provider_account_id`) are excluded —
    /// probe-verified Django behavior, and why the overlay cannot use
    /// [`rotate_account_tokens`]: that OSS `oauth.py` full save rewrites
    /// all 13 columns including the Tier-1 provider pair. The `SET`
    /// list matches the overlay `update_account_upsert` builder column
    /// for column; only the sea-query `CAST(...)` wrappers are gone
    /// (Django emits none, and they are identity for typed binds).
    pub(crate) const UPDATE_ACCOUNT_UPSERT_SQL: &str = "UPDATE \"accounts\" SET \"created_at\" = $1, \"updated_at\" = $2, \"user_id\" = $3, \"access_token\" = $4, \"access_token_expired_at\" = $5, \"refresh_token\" = $6, \"refresh_token_expired_at\" = $7, \"last_connected_at\" = $8, \"id_token\" = $9, \"metadata\" = $10 WHERE \"accounts\".\"id\" = $11";

    /// `persist_rotated_tokens`
    /// (`pi_dash_cloud/airepublic/token_store.py:139-150`, twin at
    /// `refresh.py:154-164`): `save(update_fields=["access_token",
    /// "refresh_token", "access_token_expired_at"])`, emitted in `_meta`
    /// order with no `updated_at` stamp (probe-verified). The `SET`
    /// list matches the overlay `update_account_rotated_tokens`
    /// builder column for column.
    pub(crate) const UPDATE_ACCOUNT_ROTATED_SQL: &str = "UPDATE \"accounts\" SET \"access_token\" = $1, \"access_token_expired_at\" = $2, \"refresh_token\" = $3 WHERE \"accounts\".\"id\" = $4";

    /// `_persist_plan_snapshot`
    /// (`pi_dash_cloud/airepublic/refresh.py:167-197`):
    /// `save(update_fields=["metadata"])`. Matches the overlay
    /// `update_account_metadata` builder.
    pub(crate) const UPDATE_ACCOUNT_METADATA_SQL: &str =
        "UPDATE \"accounts\" SET \"metadata\" = $1 WHERE \"accounts\".\"id\" = $2";

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

    /// `Account.objects.filter(...).first()` projected to the update path's
    /// only need: the row id.
    pub async fn find_account_id<'e, E>(
        ex: E,
        user_id: uuid::Uuid,
        provider: &str,
        provider_account_id: &str,
    ) -> Result<Option<uuid::Uuid>, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row: Option<PgRow> = sqlx::query(LOOKUP_SQL)
            .bind(user_id)
            .bind(provider)
            .bind(provider_account_id)
            .fetch_optional(ex)
            .await?;
        row.map(|r| r.try_get("id")).transpose()
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

    /// The update side (`oauth.py:113-120`): assign the six token fields
    /// plus `last_connected_at = now`, then `.save()`.
    #[allow(clippy::too_many_arguments)]
    pub async fn rotate_account_tokens<'e, E>(
        ex: E,
        account_id: uuid::Uuid,
        created_at: chrono::DateTime<chrono::Utc>,
        user_id: uuid::Uuid,
        provider_account_id: &str,
        provider: &str,
        tokens: &TokenFields,
        now: chrono::DateTime<chrono::Utc>,
        metadata: serde_json::Value,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(UPDATE_SQL)
            .bind(created_at)
            .bind(now)
            .bind(user_id)
            .bind(provider_account_id)
            .bind(provider)
            .bind(tokens.access_token.clone())
            .bind(tokens.access_token_expired_at)
            .bind(tokens.refresh_token.clone())
            .bind(tokens.refresh_token_expired_at)
            .bind(now)
            .bind(tokens.id_token_or_default())
            .bind(metadata)
            .bind(account_id)
            .execute(ex)
            .await?;
        Ok(())
    }

    /// The create side (`oauth.py:123-133`).
    pub async fn create_account<'e, E>(ex: E, params: &NewAccount<'_>) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let metadata = params
            .metadata
            .clone()
            .unwrap_or_else(|| serde_json::json!({}));
        sqlx::query(INSERT_SQL)
            .bind(params.id)
            .bind(params.now)
            .bind(params.now)
            .bind(params.user_id)
            .bind(params.provider_account_id)
            .bind(params.provider)
            .bind(params.tokens.access_token.clone())
            .bind(params.tokens.access_token_expired_at)
            .bind(params.tokens.refresh_token.clone())
            .bind(params.tokens.refresh_token_expired_at)
            .bind(params.now)
            .bind(params.tokens.id_token_or_default())
            .bind(metadata)
            .execute(ex)
            .await?;
        Ok(())
    }

    /// `create_update_account` (`oauth.py:105-136`): filter-first, then
    /// update or create. `DatabaseError`/`IntegrityError` are swallowed via
    /// `log_exception` (BUG-6) — a database failure returns
    /// `Ok(UpsertOutcome::ErrorSwallowed)`, never `Err`.
    ///
    /// Takes `&mut PgConnection` (an acquired connection, or `&mut *tx`
    /// inside a larger transaction — Python runs each statement in
    /// autocommit, so both are faithful). Single-statement steps stay
    /// generic over `sqlx::Executor` for reuse.
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
        conn: &mut PgConnection,
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
            Some(pid) => find_account_id(&mut *conn, user_id, provider, pid).await,
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
                &mut *conn,
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
        match create_account(&mut *conn, &params).await {
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

    /// The cloud sync update side (`sync.py:146-159`): ten columns, the
    /// lookup pair excluded. Runs on the caller's pool or transaction
    /// (the overlay sync holds one for the whole
    /// `update_or_create`).
    pub async fn update_account_upsert<'e, E>(
        ex: E,
        row: &AccountUpsert<'_>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(UPDATE_ACCOUNT_UPSERT_SQL)
            .bind(row.created_at)
            .bind(now)
            .bind(row.user_id)
            .bind(row.access_token)
            .bind(row.access_token_expired_at)
            .bind(row.refresh_token)
            .bind(row.refresh_token_expired_at)
            .bind(row.last_connected_at)
            .bind(row.id_token)
            .bind(row.metadata)
            .bind(row.id)
            .execute(ex)
            .await?;
        Ok(())
    }

    /// The token-rotation write (`token_store.py:139-150`): always all
    /// three columns, never `updated_at`. The keep-vs-replace value
    /// rules (empty grant token, zero `expires_in`) live with the
    /// caller, which passes the resolved values in.
    pub async fn update_account_rotated_tokens<'e, E>(
        ex: E,
        account_id: uuid::Uuid,
        access_token: &str,
        access_token_expired_at: Option<chrono::DateTime<chrono::Utc>>,
        refresh_token: Option<&str>,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(UPDATE_ACCOUNT_ROTATED_SQL)
            .bind(access_token)
            .bind(access_token_expired_at)
            .bind(refresh_token)
            .bind(account_id)
            .execute(ex)
            .await?;
        Ok(())
    }

    /// The plan-snapshot write (`refresh.py:167-197`): `metadata` only.
    /// The unchanged-skip check lives with the caller.
    pub async fn update_account_metadata<'e, E>(
        ex: E,
        account_id: uuid::Uuid,
        metadata: &serde_json::Value,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(UPDATE_ACCOUNT_METADATA_SQL)
            .bind(metadata)
            .bind(account_id)
            .execute(ex)
            .await?;
        Ok(())
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
// ops user saves (PIDASHCONV-1631; relocated verbatim from
// `pidash_db::ops::users`, which cannot own covered-table writes).
// Management commands run each statement in autocommit, so the twins
// stay executor-generic with `Utc::now()` sampled inside, as before.
// ---------------------------------------------------------------------------

const SAVE_USER_ACTIVATION_SQL: &str = r#"UPDATE "users" SET "is_active" = TRUE, "email" = $2, "display_name" = $3,
                  "is_staff" = $4, "token" = $5, "token_updated_at" = $6, "updated_at" = $7
           WHERE "id" = $1"#;

/// `user.is_active = True; user.save()` (`activate_user.py:35-36`):
/// `save()` rewrites the row with the normalized email, filled
/// display name, staff escalation, possible token rotation, and the
/// `auto_now` `updated_at`.
pub async fn save_user_activation<'e, E>(
    ex: E,
    id: Uuid,
    write: &pidash_db::ops::users::UserSaveWrite,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let now = Utc::now();
    sqlx::query(SAVE_USER_ACTIVATION_SQL)
        .bind(id)
        .bind(&write.email)
        .bind(&write.display_name)
        .bind(write.is_staff)
        .bind(&write.token)
        .bind(write.token_updated_at)
        .bind(now)
        .execute(ex)
        .await?;
    Ok(())
}

const SAVE_USER_PASSWORD_SQL: &str = r#"UPDATE "users" SET "password" = $2, "is_password_autoset" = FALSE,
                  "email" = $3, "display_name" = $4, "is_staff" = $5,
                  "token" = $6, "token_updated_at" = $7, "updated_at" = $8
           WHERE "id" = $1"#;

/// `user.set_password(password); user.is_password_autoset = False;
/// user.save()` (`reset_password.py:62-64`).
pub async fn save_user_password<'e, E>(
    ex: E,
    id: Uuid,
    encoded_password: &str,
    write: &pidash_db::ops::users::UserSaveWrite,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let now = Utc::now();
    sqlx::query(SAVE_USER_PASSWORD_SQL)
        .bind(id)
        .bind(encoded_password)
        .bind(&write.email)
        .bind(&write.display_name)
        .bind(write.is_staff)
        .bind(&write.token)
        .bind(write.token_updated_at)
        .bind(now)
        .execute(ex)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// v1 asset full-row user save (PIDASHCONV-1631; relocated verbatim from
// `pidash_db::v1_assets::asset_queries`). The sea-query builder follows
// the db-owned [`pidash_db::v1_assets::asset_queries::USER_COLUMNS`]
// list, so a Django user-model sync cannot silently desync the SET.
// ---------------------------------------------------------------------------

/// `users` table for [`entity_user_save_sql`] (the db-layer literal is
/// private to `asset_queries`).
const ENTITY_USER_TABLE: &str = "users";

/// `entity_asset_delete` user write (`asset.py:63-64,69-70`, server
/// twins `:261-262,:267-268`): `user.save()` with no `update_fields`
/// is a **full-row** save — every non-pk column in `_meta` order, pk
/// in `WHERE`. The builder emits exactly that: `SET` over
/// [`pidash_db::v1_assets::asset_queries::USER_COLUMNS`] minus `id`
/// (`$1`–`$39`), `WHERE "users"."id" = $40`. Handlers bind the fetched
/// row with `AvatarAssetId` / `CoverImageAssetId` nulled (see
/// [`pidash_db::v1_assets::asset_queries::EntityClearTarget`]); the
/// `NoOp` arm binds the row unchanged (and the handler skips the
/// statement entirely — the view returns without touching the profile).
///
/// `User.save()` also lowercases/strips the email and rotates the
/// token when `token_updated_at` is set (`db/models/user.py:162-170`);
/// that value-shaping is handler-owned, like every other Python-side
/// assignment on this path.
pub fn entity_user_save_sql() -> String {
    use pidash_db::v1_assets::asset_queries::USER_COLUMNS;
    use sea_query::PostgresQueryBuilder;
    use sea_query::{Alias, Expr, Query};
    let mut stmt = Query::update();
    stmt.table(Alias::new(ENTITY_USER_TABLE.to_owned()));
    let mut bind = 1u32;
    for col in USER_COLUMNS {
        if *col == "id" {
            continue;
        }
        stmt.value(
            Alias::new((*col).to_owned()),
            Expr::cust(format!("${bind}")),
        );
        bind += 1;
    }
    stmt.and_where(
        Expr::col((Alias::new(ENTITY_USER_TABLE), Alias::new("id")))
            .eq(Expr::cust(format!("${bind}"))),
    );
    stmt.to_string(PostgresQueryBuilder)
}

// login/logout signal stamps + password writes + profile pointers
// (PIDASHCONV-1628: moved from the api handlers; executor-generic)
// ---------------------------------------------------------------------------

/// `update_last_login` (`auth/models.py`): `login()` sets
/// `last_login = now` on every login. One canonical statement serves
/// the four login paths (email, magic, OAuth, license admin): the
/// email path's id-first bind order (`SET "last_login" = $2 WHERE
/// "id" = $1`) and the magic path's qualified `WHERE "users"."id"`
/// are spelling-only — same column, same row, same value — and are
/// normalized here (the `login_stamps_sql` precedent: no fixture pins
/// the spelling).
const STAMP_LAST_LOGIN_SQL: &str = "UPDATE \"users\" SET \"last_login\" = $1 WHERE \"id\" = $2";

/// Stamp `users.last_login` for one user. Runs on the caller's
/// executor — the login flows never hold a transaction here.
pub async fn stamp_last_login<'e, E>(
    ex: E,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(STAMP_LAST_LOGIN_SQL)
        .bind(now)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Django's `check_password` setter (`base_user.py:111-123`): a
/// correct password on an outdated hash re-encodes with the default
/// hasher at once. `save(update_fields=["password"])` writes only
/// that column (no `updated_at` bump). One statement on all three
/// paths (email sign-in, change-password, license admin sign-in).
const REHASH_PASSWORD_SQL: &str = "UPDATE \"users\" SET \"password\" = $1 WHERE \"id\" = $2";

/// Persist a re-encoded password hash, nothing else. Runs on the
/// caller's executor.
pub async fn rehash_password<'e, E>(ex: E, user_id: Uuid, new_hash: &str) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(REHASH_PASSWORD_SQL)
        .bind(new_hash)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Change/set-password `UPDATE`: the password write with the
/// `User.save()` column effects folded in
/// (`db/models/user.py:169-187`) — `updated_at` (`auto_now`), the
/// `token`/`token_updated_at` rotation when a token is already in use
/// (PB-1079), the empty-`display_name` fill from the email prefix, and
/// `is_staff` for superusers — plus the `user_logged_in` signal's
/// `last_login` bump.
///
/// Moved byte-for-byte from `api::auth_session::email`
/// (PIDASHCONV-1628); `$1` is the encoded password, `$2` the user id,
/// `$3` a fresh token, `$4` the rotation timestamp. The `email`
/// re-normalization in `save()` is a no-op on these paths (the address
/// never changes), so it stays out.
const PASSWORD_WRITE_WITH_LOGIN_SQL: &str = r#"UPDATE users SET password = $1,
    is_password_autoset = false, last_login = now(), updated_at = now(),
    token = CASE WHEN token_updated_at IS NOT NULL THEN $3 ELSE token END,
    token_updated_at = CASE WHEN token_updated_at IS NOT NULL THEN $4 ELSE token_updated_at END,
    display_name = CASE WHEN COALESCE(display_name, '') = '' THEN split_part(email, '@', 1) ELSE display_name END,
    is_staff = CASE WHEN is_superuser THEN true ELSE is_staff END
    WHERE id = $2"#;

/// Reset-password `UPDATE`: same `User.save()` effects as
/// [`PASSWORD_WRITE_WITH_LOGIN_SQL`] but no `login()` call follows,
/// so no `last_login` bump. Same binds. Moved byte-for-byte from
/// `api::auth_session::email` (PIDASHCONV-1628).
const PASSWORD_WRITE_SQL: &str = r#"UPDATE users SET password = $1,
    is_password_autoset = false, updated_at = now(),
    token = CASE WHEN token_updated_at IS NOT NULL THEN $3 ELSE token END,
    token_updated_at = CASE WHEN token_updated_at IS NOT NULL THEN $4 ELSE token_updated_at END,
    display_name = CASE WHEN COALESCE(display_name, '') = '' THEN split_part(email, '@', 1) ELSE display_name END,
    is_staff = CASE WHEN is_superuser THEN true ELSE is_staff END
    WHERE id = $2"#;

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

/// Password write shared by change/set/reset-password: the encoded
/// password plus the `User.save()` column effects above. Runs on the
/// caller's executor.
pub async fn write_password_with_save_effects<'e, E>(
    ex: E,
    user_id: Uuid,
    save: &PasswordSave<'_>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let sql = if save.stamp_last_login {
        PASSWORD_WRITE_WITH_LOGIN_SQL
    } else {
        PASSWORD_WRITE_SQL
    };
    sqlx::query(sql)
        .bind(save.encoded)
        .bind(user_id)
        .bind(save.fresh_token)
        .bind(save.rotated_at)
        .execute(ex)
        .await
        .map(|_| ())
}

/// The sign-out stamps: `last_logout_ip`/`last_logout_time` plus
/// `updated_at` (`auto_now`). One canonical statement serves the app
/// sign-out and the license admin sign-out; the admin path's unquoted
/// spelling and the app path's id-first bind order are normalized
/// (same columns, same row — no fixture pins the spelling). The bind
/// values stay per caller: the app path binds `None` when no IP is
/// present, the admin path `""`.
const STAMP_LOGOUT_SQL: &str = "UPDATE \"users\" SET \"last_logout_ip\" = $1, \
    \"last_logout_time\" = $2, \"updated_at\" = $2 WHERE \"id\" = $3";

/// Stamp the sign-out columns for one user. Runs on the caller's
/// executor.
pub async fn stamp_logout<'e, E>(
    ex: E,
    user_id: Uuid,
    ip: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(STAMP_LOGOUT_SQL)
        .bind(ip)
        .bind(now)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// The invite-accept user touch (`invite.py:204-205`): `User` has no
/// `last_workspace_id` column (it lives on `Profile`,
/// `db/models/user.py:236`), so Django drops the transient attribute
/// and the save only stamps `updated_at` (ported bug 11,
/// [`crate::app_workspace::queries_membership::user_touch_sql`]).
const TOUCH_USER_UPDATED_AT_SQL: &str = "UPDATE users SET updated_at = $1 WHERE id = $2";

/// Touch `users.updated_at` for one user. Runs on the caller's
/// executor.
pub async fn touch_user_updated_at<'e, E>(
    ex: E,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(TOUCH_USER_UPDATED_AT_SQL)
        .bind(now)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// The `PATCH users/me/` `User.save()`: the merged row (validated
/// patch over the current row) plus the save-derived values
/// (`save_email`, `save_display_name`, token regeneration,
/// `save_is_staff`, `user.py:169-187`).
///
/// Moved byte-for-byte from `api::app_workspace::handlers_user`
/// (PIDASHCONV-1628): `$1`-`$14` are the merged columns, `$15`-`$18`
/// the derived ones, `$19` the `auto_now` stamp, `$20` the id. The
/// merge and the derivation stay at the call site (pure computation,
/// not SQL); every resolved value rides this struct.
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

const ME_PROFILE_SAVE_SQL: &str =
    "UPDATE users SET last_login = $1, display_name = $2, first_name = $3, last_name = $4, \
    avatar = $5, avatar_asset_id = $6, cover_image = $7, cover_image_asset_id = $8, \
    is_password_expired = $9, is_password_reset_required = $10, bot_type = $11, \
    user_timezone = $12, is_email_valid = $13, masked_at = $14, email = $15, token = $16, \
    token_updated_at = $17, is_staff = $18, updated_at = $19 WHERE id = $20";

/// Persist the merged `PATCH users/me/` row. Runs on the caller's
/// executor.
pub async fn save_me_profile<'e, E>(
    ex: E,
    user_id: Uuid,
    save: &MeProfileSave<'_>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(ME_PROFILE_SAVE_SQL)
        .bind(save.last_login)
        .bind(save.display_name)
        .bind(save.first_name)
        .bind(save.last_name)
        .bind(save.avatar)
        .bind(save.avatar_asset_id)
        .bind(save.cover_image)
        .bind(save.cover_image_asset_id)
        .bind(save.is_password_expired)
        .bind(save.is_password_reset_required)
        .bind(save.bot_type)
        .bind(save.user_timezone)
        .bind(save.is_email_valid)
        .bind(save.masked_at)
        .bind(save.email)
        .bind(save.token)
        .bind(save.token_updated_at)
        .bind(save.is_staff)
        .bind(save.updated_at)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Shared `delete_old_avatar` user leg (`adapter/base.py:244-248`):
/// clear the FK plus the avatar string after the asset soft-delete
/// lands. Runs on the caller's executor.
const CLEAR_AVATAR_SQL: &str =
    "UPDATE \"users\" SET \"avatar_asset_id\" = NULL, \"avatar\" = '' WHERE \"id\" = $1";

/// Clear one user's avatar FK + string. Runs on the caller's
/// executor.
pub async fn clear_avatar<'e, E>(ex: E, user_id: Uuid) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(CLEAR_AVATAR_SQL)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// `remove_last_workspace_ids_from_user_settings`: null every profile
/// pointer at a deleted workspace. Runs on the caller's executor.
const CLEAR_LAST_WORKSPACE_SQL: &str =
    "UPDATE profiles SET last_workspace_id = NULL WHERE last_workspace_id = $1";

/// Null the `last_workspace_id` pointers at one workspace. Runs on
/// the caller's executor.
pub async fn clear_last_workspace<'e, E>(ex: E, workspace_id: Uuid) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(CLEAR_LAST_WORKSPACE_SQL)
        .bind(workspace_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// The join-request approve pointer write (`join_request.py:204`):
/// `Profile.last_workspace_id` follows the approved workspace. Runs
/// on the caller's executor — the approve path holds its own `sqlx`
/// transaction around the member write, this write and the status
/// write.
const SET_LAST_WORKSPACE_SQL: &str =
    "UPDATE profiles SET last_workspace_id = $1 WHERE user_id = $2";

/// Point one user's profile at a workspace. Runs on the caller's
/// executor.
pub async fn set_last_workspace<'e, E>(
    ex: E,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(SET_LAST_WORKSPACE_SQL)
        .bind(workspace_id)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;

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
    fn f6_lookup_sql_pins() {
        assert_eq!(account::TABLE, "accounts");
        assert_eq!(
            account::LOOKUP_COLUMNS,
            &["user_id", "provider", "provider_account_id"]
        );
        let sql = account::LOOKUP_SQL;
        assert!(sql.contains("FROM \"accounts\""), "lookup table");
        let (user_pos, prov_pos, pid_pos) = (
            sql.find("user_id\" = $1").expect("user_id $1"),
            sql.find("provider\" = $2").expect("provider $2"),
            sql.find("provider_account_id\" = $3")
                .expect("provider_account_id $3"),
        );
        assert!(
            user_pos < prov_pos && prov_pos < pid_pos,
            "predicate order user, provider, provider_account_id"
        );
        assert!(sql.ends_with("LIMIT 1"), ".first() limit");
    }

    #[test]
    fn f6_update_sql_pins() {
        for col in [
            "access_token",
            "refresh_token",
            "access_token_expired_at",
            "refresh_token_expired_at",
            "last_connected_at",
            "id_token",
        ] {
            assert!(
                account::UPDATE_COLUMNS.contains(&col),
                "update writes {col}"
            );
        }
        for col in ["user_id", "provider", "provider_account_id"] {
            assert!(
                !account::UPDATE_COLUMNS.contains(&col),
                "update leaves {col} alone"
            );
        }
        assert!(
            account::UPDATE_SQL.contains("\"updated_at\" = $"),
            "save() stamps updated_at"
        );
        assert!(
            account::UPDATE_SQL.contains("WHERE \"accounts\".\"id\" = $13"),
            "update targets the row id"
        );
    }

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
    fn f6_insert_sql_pins() {
        for key in [
            "access_token",
            "refresh_token",
            "access_token_expired_at",
            "refresh_token_expired_at",
            "last_connected_at",
            "id_token",
            "provider",
            "provider_account_id",
            "user_id",
        ] {
            assert!(account::COLUMNS.contains(&key), "insert binds {key}");
        }
        assert!(
            account::INSERT_SQL.contains("(\"id\","),
            "client-side uuid pk"
        );
        assert!(
            account::INSERT_SQL.contains("\"metadata\""),
            "metadata default {{}}"
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

    #[test]
    fn cloud_upsert_sql_pins_provider_excluding_shape() {
        // Column for column, the overlay `update_account_upsert`
        // render minus the sea-query `CAST(...)` wrappers: ten `SET`
        // entries in `_meta` order, the lookup pair excluded, the row
        // id last.
        assert_eq!(
            account::UPDATE_ACCOUNT_UPSERT_SQL,
            "UPDATE \"accounts\" SET \"created_at\" = $1, \"updated_at\" = $2, \
             \"user_id\" = $3, \"access_token\" = $4, \"access_token_expired_at\" = $5, \
             \"refresh_token\" = $6, \"refresh_token_expired_at\" = $7, \
             \"last_connected_at\" = $8, \"id_token\" = $9, \"metadata\" = $10 \
             WHERE \"accounts\".\"id\" = $11"
        );
        assert!(
            !account::UPDATE_ACCOUNT_UPSERT_SQL.contains("provider"),
            "lookup columns excluded"
        );
    }

    #[test]
    fn cloud_rotated_sql_pins_triple_without_stamp() {
        // The overlay `update_account_rotated_tokens` render minus the
        // `CAST(...)` wrapper: three `SET` entries, no `updated_at`.
        assert_eq!(
            account::UPDATE_ACCOUNT_ROTATED_SQL,
            "UPDATE \"accounts\" SET \"access_token\" = $1, \
             \"access_token_expired_at\" = $2, \"refresh_token\" = $3 \
             WHERE \"accounts\".\"id\" = $4"
        );
        assert!(
            !account::UPDATE_ACCOUNT_ROTATED_SQL.contains("updated_at"),
            "rotation stamps nothing"
        );
    }

    #[test]
    fn cloud_metadata_sql_pins_single_column() {
        // The overlay `update_account_metadata` render minus the
        // `CAST(... AS jsonb)` wrapper.
        assert_eq!(
            account::UPDATE_ACCOUNT_METADATA_SQL,
            "UPDATE \"accounts\" SET \"metadata\" = $1 WHERE \"accounts\".\"id\" = $2"
        );
    }

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

    #[test]
    fn stamps_sql_email_shape() {
        let stamps = LoginStamps {
            medium: Some("email"),
            login_ip: Some(""),
            login_uagent: Some(""),
            ..stamps_fixture()
        };
        assert_eq!(
            login_stamps_sql(&stamps),
            "UPDATE \"users\" SET \"last_login_medium\" = $1, \"last_active\" = $2, \
             \"last_login_time\" = $3, \"last_login_ip\" = $4, \"last_login_uagent\" = $5, \
             \"token_updated_at\" = $6, \"token\" = $7, \"is_active\" = TRUE \
             WHERE \"id\" = $8"
        );
    }

    #[test]
    fn stamps_sql_magic_shape() {
        let stamps = LoginStamps {
            medium: Some("magic-code"),
            touch_updated_at: true,
            ..stamps_fixture()
        };
        assert_eq!(
            login_stamps_sql(&stamps),
            "UPDATE \"users\" SET \"last_login_medium\" = $1, \"last_active\" = $2, \
             \"last_login_time\" = $3, \"last_login_ip\" = $4, \"last_login_uagent\" = $5, \
             \"token_updated_at\" = $6, \"token\" = $7, \"is_active\" = TRUE, \
             \"updated_at\" = $8 WHERE \"id\" = $9"
        );
    }

    #[test]
    fn stamps_sql_gitlab_shape() {
        let stamps = LoginStamps {
            medium: Some("gitlab"),
            avatar: Some("https://avatar.test/a"),
            display_name: Some(DisplayNameUpdate::Bound("ada")),
            is_staff: Some(StaffSync::Value(false)),
            touch_updated_at: true,
            ..stamps_fixture()
        };
        assert_eq!(
            login_stamps_sql(&stamps),
            "UPDATE \"users\" SET \"last_login_medium\" = $1, \"last_active\" = $2, \
             \"last_login_time\" = $3, \"last_login_ip\" = $4, \"last_login_uagent\" = $5, \
             \"token_updated_at\" = $6, \"token\" = $7, \"is_active\" = TRUE, \
             \"avatar\" = $8, \"display_name\" = $9, \"is_staff\" = $10, \
             \"updated_at\" = $11 WHERE \"id\" = $12"
        );
    }

    #[test]
    fn stamps_sql_gitea_shape() {
        let stamps = LoginStamps {
            medium: Some("gitea"),
            login_ip: Some(""),
            login_uagent: Some(""),
            avatar: Some(""),
            avatar_asset_id: Some(None),
            display_name: Some(DisplayNameUpdate::BackfillCase),
            is_staff: Some(StaffSync::OrSuperuser),
            touch_updated_at: true,
            ..stamps_fixture()
        };
        assert_eq!(
            login_stamps_sql(&stamps),
            "UPDATE \"users\" SET \"last_login_medium\" = $1, \"last_active\" = $2, \
             \"last_login_time\" = $3, \"last_login_ip\" = $4, \"last_login_uagent\" = $5, \
             \"token_updated_at\" = $6, \"token\" = $7, \"is_active\" = TRUE, \
             \"avatar\" = $8, \"avatar_asset_id\" = $9, \
             \"display_name\" = CASE WHEN \"display_name\" = '' \
             THEN split_part(\"email\", '@', 1) ELSE \"display_name\" END, \
             \"is_staff\" = \"is_staff\" OR \"is_superuser\", \
             \"updated_at\" = $10 WHERE \"id\" = $11"
        );
    }

    #[test]
    fn stamps_sql_google_shape() {
        let stamps = LoginStamps {
            medium: Some("google"),
            display_name: Some(DisplayNameUpdate::BackfillCase),
            is_staff: Some(StaffSync::OrSuperuser),
            touch_updated_at: true,
            ..stamps_fixture()
        };
        assert_eq!(
            login_stamps_sql(&stamps),
            "UPDATE \"users\" SET \"last_login_medium\" = $1, \"last_active\" = $2, \
             \"last_login_time\" = $3, \"last_login_ip\" = $4, \"last_login_uagent\" = $5, \
             \"token_updated_at\" = $6, \"token\" = $7, \"is_active\" = TRUE, \
             \"display_name\" = CASE WHEN \"display_name\" = '' \
             THEN split_part(\"email\", '@', 1) ELSE \"display_name\" END, \
             \"is_staff\" = \"is_staff\" OR \"is_superuser\", \
             \"updated_at\" = $8 WHERE \"id\" = $9"
        );
    }

    #[test]
    fn stamps_sql_github_shape() {
        let stamps = LoginStamps {
            medium: Some("github"),
            display_name: Some(DisplayNameUpdate::Bound("ada")),
            is_staff: Some(StaffSync::Value(true)),
            touch_updated_at: true,
            ..stamps_fixture()
        };
        assert_eq!(
            login_stamps_sql(&stamps),
            "UPDATE \"users\" SET \"last_login_medium\" = $1, \"last_active\" = $2, \
             \"last_login_time\" = $3, \"last_login_ip\" = $4, \"last_login_uagent\" = $5, \
             \"token_updated_at\" = $6, \"token\" = $7, \"is_active\" = TRUE, \
             \"display_name\" = $8, \"is_staff\" = $9, \
             \"updated_at\" = $10 WHERE \"id\" = $11"
        );
    }

    #[test]
    fn stamps_sql_admin_shape() {
        // No medium column: the license stamps never assign it.
        let stamps = LoginStamps {
            touch_updated_at: true,
            ..stamps_fixture()
        };
        assert_eq!(
            login_stamps_sql(&stamps),
            "UPDATE \"users\" SET \"last_active\" = $1, \
             \"last_login_time\" = $2, \"last_login_ip\" = $3, \"last_login_uagent\" = $4, \
             \"token_updated_at\" = $5, \"token\" = $6, \"is_active\" = TRUE, \
             \"updated_at\" = $7 WHERE \"id\" = $8"
        );
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

    #[test]
    fn sync_sql_gitlab_shape() {
        let sync = SyncUpdate {
            avatar: Some("u"),
            avatar_asset_id: Some(None),
            is_staff: Some(true),
            touch_updated_at: true,
            ..sync_fixture()
        };
        assert_eq!(
            sync_update_sql(&sync),
            "UPDATE \"users\" SET \"first_name\" = $1, \"last_name\" = $2, \
             \"display_name\" = $3, \"avatar\" = $4, \"avatar_asset_id\" = $5, \
             \"is_staff\" = $6, \"updated_at\" = $7 WHERE \"id\" = $8"
        );
    }

    #[test]
    fn sync_sql_gitea_shape() {
        let sync = SyncUpdate {
            avatar: Some("u"),
            avatar_asset_id: Some(None),
            ..sync_fixture()
        };
        assert_eq!(
            sync_update_sql(&sync),
            "UPDATE \"users\" SET \"first_name\" = $1, \"last_name\" = $2, \
             \"display_name\" = $3, \"avatar\" = $4, \"avatar_asset_id\" = $5 \
             WHERE \"id\" = $6"
        );
    }

    #[test]
    fn sync_sql_google_names_shape() {
        assert_eq!(
            sync_update_sql(&sync_fixture()),
            "UPDATE \"users\" SET \"first_name\" = $1, \"last_name\" = $2, \
             \"display_name\" = $3 WHERE \"id\" = $4"
        );
    }

    #[test]
    fn sync_sql_github_asset_arm_shape() {
        let sync = SyncUpdate {
            avatar_asset_id: Some(Some(Uuid::nil())),
            touch_updated_at: true,
            ..sync_fixture()
        };
        assert_eq!(
            sync_update_sql(&sync),
            "UPDATE \"users\" SET \"first_name\" = $1, \"last_name\" = $2, \
             \"display_name\" = $3, \"avatar_asset_id\" = $4, \"updated_at\" = $5 \
             WHERE \"id\" = $6"
        );
    }

    #[test]
    fn sync_sql_github_url_arm_shape() {
        let sync = SyncUpdate {
            avatar: Some("u"),
            touch_updated_at: true,
            ..sync_fixture()
        };
        assert_eq!(
            sync_update_sql(&sync),
            "UPDATE \"users\" SET \"first_name\" = $1, \"last_name\" = $2, \
             \"display_name\" = $3, \"avatar\" = $4, \"updated_at\" = $5 \
             WHERE \"id\" = $6"
        );
    }

    // -- OIDC sync login stamps (PIDASHCONV-1621) -------------------------------

    #[test]
    fn relogin_update_sql_is_byte_identical() {
        // The overlay re-login builder, verbatim
        // (`cloud-db/src/auth/queries.rs`, `update_user_relogin`): the SET
        // list is identical — columns, order, quoting and `$n` positions.
        // Only the `WHERE` differs: the sea-query builder qualifies it
        // (`WHERE "users"."id" = $4`) while OSS spells it unqualified.
        assert_eq!(
            USER_RELOGIN_UPDATE_SQL,
            "UPDATE \"users\" SET \"avatar\" = $1, \"is_email_verified\" = $2, \
             \"last_login_medium\" = $3 WHERE \"id\" = $4"
        );
    }

    #[test]
    fn last_login_time_update_sql_is_byte_identical() {
        // The overlay timestamp builder, verbatim
        // (`cloud-db/src/auth/queries.rs`, `update_user_last_login_time`):
        // same single SET column. The builder renders the bind as
        // `CAST($1 AS timestamptz)` (sea-query renders every datetime bind
        // that way) while OSS binds the `DateTime<Utc>` as a plain `$1`
        // like every other stamps fn — the same stored value either way —
        // and the `WHERE` is unqualified here (`WHERE "id" = $2`).
        assert_eq!(
            USER_LAST_LOGIN_TIME_UPDATE_SQL,
            "UPDATE \"users\" SET \"last_login_time\" = $1 WHERE \"id\" = $2"
        );
    }

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
        update_user_relogin(&mut *tx, user_id, "http://pic/new", true, "airepublic")
            .await
            .expect("relogin triple runs");
        update_user_last_login_time(&mut *tx, user_id, login)
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

    #[test]
    fn asset_link_sql_matches_handler_text() {
        assert_eq!(
            USER_AVATAR_LINK_SQL,
            "UPDATE \"users\" SET \"avatar\" = $1, \"avatar_asset_id\" = $2, \"email\" = $3, \"display_name\" = $4, \"is_staff\" = $5, \"token\" = $6, \"token_updated_at\" = $7, \"updated_at\" = $8 WHERE \"id\" = $9"
        );
        assert_eq!(
            USER_COVER_LINK_SQL,
            "UPDATE \"users\" SET \"cover_image\" = NULL, \"cover_image_asset_id\" = $1, \"email\" = $2, \"display_name\" = $3, \"is_staff\" = $4, \"token\" = $5, \"token_updated_at\" = $6, \"updated_at\" = $7 WHERE \"id\" = $8"
        );
        assert_eq!(
            USER_AVATAR_UNLINK_SQL,
            "UPDATE \"users\" SET \"avatar_asset_id\" = NULL, \"email\" = $1, \"display_name\" = $2, \"is_staff\" = $3, \"token\" = $4, \"token_updated_at\" = $5, \"updated_at\" = $6 WHERE \"id\" = $7"
        );
        assert_eq!(
            USER_COVER_UNLINK_SQL,
            "UPDATE \"users\" SET \"cover_image_asset_id\" = NULL, \"email\" = $1, \"display_name\" = $2, \"is_staff\" = $3, \"token\" = $4, \"token_updated_at\" = $5, \"updated_at\" = $6 WHERE \"id\" = $7"
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

    #[test]
    fn agent_insert_lists_all_40_physical_columns() {
        // `_meta` order per FX-AUTH-03.models.json (the two trailing
        // ManyToMany entries are relations, not columns).
        let columns = [
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
        assert_eq!(columns.len(), 40);
        let head = BOT_USER_INSERT_SQL;
        let mut cursor = 0;
        for column in columns {
            let found = head[cursor..]
                .find(column)
                .unwrap_or_else(|| panic!("{column} listed"));
            cursor += found + column.len();
        }
        for bind in 1..=40 {
            assert!(head.contains(&format!("${bind}")), "bind ${bind} present");
        }
        assert_eq!(
            BOT_USER_PASSWORD_SQL,
            "UPDATE users SET password = $1 WHERE id = $2"
        );
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
            &mut *tx,
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
        link_avatar_asset(&pool, user, avatar_asset, &write, now)
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
        unlink_avatar_asset(&pool, user, &write, now)
            .await
            .expect("avatar unlink");
        let cover_asset = Uuid::new_v4();
        link_cover_asset(&pool, user, cover_asset, &write, now)
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
        unlink_cover_asset(&pool, user, &write, now)
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
        save_preference_columns(&pool, pref, now, user, &columns)
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
        let err = save_user_data(&mut *tx, user_id, &gitea)
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
            display_name: None,
            is_staff: None,
            touch_updated_at: false,
            rotated_token: "tok",
            now,
        };
        let err = save_user_data(&mut *tx, user_id, &email)
            .await
            .expect_err("email shape fails identically");
        assert_not_null_violation(&err);
    }

    // -- PIDASHCONV-1615: race-tolerant profile get_or_create + setter ----

    #[test]
    fn profile_path_select_sql_pins() {
        assert_eq!(
            PROFILE_PATH_SELECT_SQL,
            "SELECT \"id\", \"is_onboarded\", \"last_workspace_id\" FROM \"profiles\" \
             WHERE \"user_id\" = $1 LIMIT 1"
        );
    }

    #[test]
    fn profile_default_insert_sql_pins() {
        let sql = PROFILE_DEFAULT_INSERT_SQL;
        assert!(
            sql.starts_with("INSERT INTO \"profiles\""),
            "profiles insert"
        );
        assert!(
            sql.contains("VALUES ($1, $2, $2, $3,"),
            "id, now, now, user binds"
        );
        for literal in ["'INDIA'", "'full'", "'en'", "'{}'", "$4"] {
            assert!(sql.contains(literal), "insert carries {literal}");
        }
        assert!(sql.contains(", '', '"), "company_name ''");
        assert!(
            sql.contains("\"workspace_invite\": false"),
            "full onboarding json"
        );
        assert!(sql.contains("\"work_items\": false"), "product tour json");
        assert!(sql.ends_with("'{}')"), "settings {{}} tail");
    }

    #[test]
    fn set_profile_last_workspace_sql_pins() {
        assert_eq!(
            SET_PROFILE_LAST_WORKSPACE_SQL,
            "UPDATE \"profiles\" SET \"updated_at\" = $1, \"last_workspace_id\" = $2 WHERE \"id\" = $3"
        );
    }

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
        let fresh = get_or_create_profile(&pool, user_id, now, background)
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
        let found = get_or_create_profile(&pool, user_id, now, "#ffffff")
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
        insert_default_profile(&mut *tx, user_id, now, "#a1b2c3")
            .await
            .expect("first insert");
        sqlx::query("SAVEPOINT sp_profile_race")
            .execute(&mut *tx)
            .await
            .expect("savepoint");
        let raced = insert_default_profile(&mut *tx, user_id, now, "#d4e5f6").await;
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
                get_or_create_profile(&pool, user_id, now, "#1615a1").await
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
        let a = get_or_create_profile(&pool, user_a, t0, background)
            .await
            .expect("row a");
        let b = get_or_create_profile(&pool, user_b, t0, background)
            .await
            .expect("row b");
        let ws1 = live_uuid("000000006155");
        let ws2 = live_uuid("000000006156");
        set_profile_last_workspace(&pool, a.id, ws1, t1)
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
        set_profile_last_workspace(&pool, a.id, ws2, t2)
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

    #[test]
    fn provisioned_insert_lists_all_40_columns_in_meta_order() {
        let head = PROVISIONED_USER_INSERT_SQL;
        let mut cursor = 0;
        for column in USER_META_COLUMNS {
            let found = head[cursor..]
                .find(column)
                .unwrap_or_else(|| panic!("{column} listed"));
            cursor += found + column.len();
        }
        for bind in 1..=40 {
            assert!(head.contains(&format!("${bind}")), "bind ${bind} present");
        }
    }

    #[test]
    fn prefs_insert_lists_all_14_columns_in_meta_order() {
        let columns = [
            "created_at",
            "updated_at",
            "created_by_id",
            "updated_by_id",
            "deleted_at",
            "id",
            "user_id",
            "workspace_id",
            "project_id",
            "property_change",
            "state_change",
            "comment",
            "mention",
            "issue_completed",
        ];
        let head = USER_NOTIFICATION_PREFERENCE_INSERT_SQL;
        let mut cursor = 0;
        for column in columns {
            let found = head[cursor..]
                .find(column)
                .unwrap_or_else(|| panic!("{column} listed"));
            cursor += found + column.len();
        }
        for bind in 1..=14 {
            assert!(head.contains(&format!("${bind}")), "bind ${bind} present");
        }
    }

    fn field_updates_fixture() -> UserFieldUpdates<'static> {
        UserFieldUpdates {
            username: None,
            email: None,
            display_name: None,
        }
    }

    #[test]
    fn user_fields_sql_empty_is_no_save() {
        let updates = field_updates_fixture();
        assert!(updates.is_empty());
        assert_eq!(user_fields_sql(&updates), None);
    }

    #[test]
    fn user_fields_sql_all_dirty_shape() {
        let updates = UserFieldUpdates {
            username: Some("u"),
            email: Some("e"),
            display_name: Some("d"),
        };
        assert!(!updates.is_empty());
        assert_eq!(
            user_fields_sql(&updates).as_deref(),
            Some(
                "UPDATE \"users\" SET \"username\" = $1, \"email\" = $2, \
                 \"display_name\" = $3, \"updated_at\" = $4 WHERE \"id\" = $5"
            )
        );
    }

    #[test]
    fn user_fields_sql_email_only_shape() {
        let updates = UserFieldUpdates {
            email: Some("e"),
            ..field_updates_fixture()
        };
        assert_eq!(
            user_fields_sql(&updates).as_deref(),
            Some("UPDATE \"users\" SET \"email\" = $1, \"updated_at\" = $2 WHERE \"id\" = $3")
        );
    }

    #[test]
    fn user_fields_sql_username_display_shape() {
        let updates = UserFieldUpdates {
            username: Some("u"),
            display_name: Some("d"),
            ..field_updates_fixture()
        };
        assert_eq!(
            user_fields_sql(&updates).as_deref(),
            Some(
                "UPDATE \"users\" SET \"username\" = $1, \"display_name\" = $2, \
                 \"updated_at\" = $3 WHERE \"id\" = $4"
            )
        );
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
        let got = insert_provisioned_user(&mut *tx, &platform)
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
        let got = insert_provisioned_user(&mut *tx, &sync)
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
        insert_user_notification_preference(&mut *tx, prefs_id, platform_id, now)
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
            &mut *tx,
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
        update_user_fields(&mut *tx, sync_id, &field_updates_fixture(), later)
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
            pool,
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
            &pool,
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
            &mut *tx,
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
        account::update_account_rotated_tokens(&mut *tx, id, "at-9", Some(t1), Some("rt-9"))
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
        account::update_account_metadata(&pool, id, &snapshot)
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

    #[test]
    fn ops_save_sql_pins() {
        // `save_user_activation` (`activate_user.py:35-36`): the active
        // flag, the `User.save()` outputs, the stamp — pk in WHERE.
        let act = SAVE_USER_ACTIVATION_SQL;
        let mut cursor = 0;
        for frag in [
            "\"is_active\" = TRUE",
            "\"email\" = $2",
            "\"display_name\" = $3",
            "\"is_staff\" = $4",
            "\"token\" = $5",
            "\"token_updated_at\" = $6",
            "\"updated_at\" = $7",
            "WHERE \"id\" = $1",
        ] {
            let found = act[cursor..]
                .find(frag)
                .unwrap_or_else(|| panic!("{frag} present"));
            cursor += found + frag.len();
        }
        // `save_user_password` (`reset_password.py:62-64`): the hash plus
        // the same save outputs, autoset cleared — pk in WHERE.
        let pwd = SAVE_USER_PASSWORD_SQL;
        let mut cursor = 0;
        for frag in [
            "\"password\" = $2",
            "\"is_password_autoset\" = FALSE",
            "\"email\" = $3",
            "\"display_name\" = $4",
            "\"is_staff\" = $5",
            "\"token\" = $6",
            "\"token_updated_at\" = $7",
            "\"updated_at\" = $8",
            "WHERE \"id\" = $1",
        ] {
            let found = pwd[cursor..]
                .find(frag)
                .unwrap_or_else(|| panic!("{frag} present"));
            cursor += found + frag.len();
        }
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
        save_user_activation(&mut *tx, id, &write)
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
        save_user_password(&mut *tx, id, "pbkdf2$new", &write)
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

    /// PIDASHCONV-1628: the moved stamp singles pin their exact
    /// statements; the password twins differ only in the `last_login`
    /// bump.
    #[test]
    fn moved_stamp_statements_pin() {
        assert_eq!(
            STAMP_LAST_LOGIN_SQL,
            "UPDATE \"users\" SET \"last_login\" = $1 WHERE \"id\" = $2"
        );
        assert_eq!(
            REHASH_PASSWORD_SQL,
            "UPDATE \"users\" SET \"password\" = $1 WHERE \"id\" = $2"
        );
        assert_eq!(
            STAMP_LOGOUT_SQL,
            "UPDATE \"users\" SET \"last_logout_ip\" = $1, \"last_logout_time\" = $2, \
             \"updated_at\" = $2 WHERE \"id\" = $3"
        );
        assert_eq!(
            TOUCH_USER_UPDATED_AT_SQL,
            "UPDATE users SET updated_at = $1 WHERE id = $2"
        );
        assert_eq!(
            CLEAR_AVATAR_SQL,
            "UPDATE \"users\" SET \"avatar_asset_id\" = NULL, \"avatar\" = '' WHERE \"id\" = $1"
        );
        assert_eq!(
            CLEAR_LAST_WORKSPACE_SQL,
            "UPDATE profiles SET last_workspace_id = NULL WHERE last_workspace_id = $1"
        );
        assert_eq!(
            SET_LAST_WORKSPACE_SQL,
            "UPDATE profiles SET last_workspace_id = $1 WHERE user_id = $2"
        );
        assert!(PASSWORD_WRITE_WITH_LOGIN_SQL.contains("last_login = now()"));
        assert!(!PASSWORD_WRITE_SQL.contains("last_login"));
        for sql in [PASSWORD_WRITE_WITH_LOGIN_SQL, PASSWORD_WRITE_SQL] {
            for clause in [
                "password = $1",
                "is_password_autoset = false",
                "updated_at = now()",
                "token = CASE WHEN token_updated_at IS NOT NULL THEN $3 ELSE token END",
                "display_name = CASE WHEN COALESCE(display_name, '') = ''",
                "is_staff = CASE WHEN is_superuser THEN true ELSE is_staff END",
                "WHERE id = $2",
            ] {
                assert!(sql.contains(clause), "{clause} present");
            }
        }
        assert!(
            ME_PROFILE_SAVE_SQL.starts_with("UPDATE users SET last_login = $1, display_name = $2")
        );
        assert!(ME_PROFILE_SAVE_SQL
            .ends_with("token_updated_at = $17, is_staff = $18, updated_at = $19 WHERE id = $20"));
        for bind in 1..=20 {
            assert!(
                ME_PROFILE_SAVE_SQL.contains(&format!("${bind}")),
                "bind ${bind} present"
            );
        }
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
        stamp_last_login(&mut *tx, user_id, later)
            .await
            .expect("stamp last_login");
        let stamped: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT last_login FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(stamped, Some(later));
        rehash_password(&mut *tx, user_id, "new-hash")
            .await
            .expect("rehash");
        let row: (String, DateTime<Utc>) =
            sqlx::query_as("SELECT password, updated_at FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(row, ("new-hash".to_owned(), now));
        touch_user_updated_at(&mut *tx, user_id, later)
            .await
            .expect("touch");
        let touched: DateTime<Utc> =
            sqlx::query_scalar("SELECT updated_at FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(touched, later);
        clear_avatar(&mut *tx, user_id).await.expect("clear avatar");
        let avatar: String = sqlx::query_scalar("SELECT avatar FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .expect("read back");
        assert_eq!(avatar, "");
        stamp_logout(&mut *tx, user_id, Some("9.9.9.9"), later)
            .await
            .expect("stamp logout");
        let logged: (String, Option<DateTime<Utc>>) =
            sqlx::query_as("SELECT last_logout_ip, last_logout_time FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(logged, ("9.9.9.9".to_owned(), Some(later)));
        // The app sign-out binds `None` when no IP is present: the NOT
        // NULL column raises, and the caller swallows it (bare redirect).
        sqlx::query("SAVEPOINT sp_logout_none")
            .execute(&mut *tx)
            .await
            .expect("savepoint");
        let err = stamp_logout(&mut *tx, user_id, None, later)
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
            &mut *tx,
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
            &mut *tx,
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
        set_last_workspace(&mut *tx, user_id, ws_id)
            .await
            .expect("set pointer");
        let pointed: Option<Uuid> =
            sqlx::query_scalar("SELECT last_workspace_id FROM profiles WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .expect("read back");
        assert_eq!(pointed, Some(ws_id));
        clear_last_workspace(&mut *tx, ws_id)
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
            &mut *tx,
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
