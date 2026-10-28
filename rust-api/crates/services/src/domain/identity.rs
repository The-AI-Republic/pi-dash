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
//!
//! Per-caller deltas (design D6) ride on the input structs, each citing
//! the Python file:line that has the same difference. Three Rust-side
//! divergences the survey found are preserved verbatim and tracked by
//! PIDASHCONV-1560 (fix separately; this consolidation changes nothing):
//!
//! * email/gitea signup inserts store `last_active = NULL`; Django's
//!   field default is `timezone.now` (observable only when the stamps
//!   save never runs — a mid-signup database failure).
//! * the github signup stores the provider avatar URL at insert; Django
//!   stores `''` and persists the URL in `save_user_data` (same
//!   failure-arm-only window).
//! * the gitea stamps bind `''` for a missing ip/user-agent where Django
//!   (and every other path) stores NULL into the NOT NULL columns and
//!   the request 500s.
//!
//! Statement order moves one step on the gitlab path: its prefs insert
//! used to run after the profile insert; the hook fires at the user
//! save on every path now, which is Django's order (`post_save` runs
//! inside `save()`). Final rows are identical.

use chrono::{DateTime, Utc};
use pidash_compat::db::is_unique_violation;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, EntityTrait};
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
/// display). `avatar` is `''` everywhere but the github signup, which
/// stores the provider URL at insert (see the module docs).
/// `last_active` is `Some(now)` but `None` on the email/gitea paths
/// (see the module docs). `login_ip`/`login_uagent` are `Some("")` but
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
///   plus the asset id; everyone else persists avatars elsewhere
///   (insert-time, or the [`stamp_avatar_asset`]/[`stamp_avatar_url`]
///   singles).
/// * `display_name` / `is_staff`: the `User.save()` backfill + sync
///   (`user.py:169-187`), in bound or `CASE`/expression form — or
///   omitted where the stored row already matches.
/// * `touch_updated_at`: every path but email bumps `updated_at`
///   (`auto_now`); email's missing bump is preserved verbatim (see the
///   module docs).
/// * `login_ip`/`login_uagent` bind NULL when `None` (the NOT NULL
///   columns raise into the same 500 Python hits) — except gitea, which
///   pre-renders `''` (see the module docs).
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
/// when the avatar upload fails (google twice). Runs on the caller's
/// executor.
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

pub async fn get_or_create_profile<'e, E>(
    ex: E,
    user_id: Uuid,
    now: DateTime<Utc>,
    background_color: &str,
) -> Result<ProfilePath, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
{
    let row: Option<(Uuid, bool, Option<Uuid>)> = sqlx::query_as(PROFILE_PATH_SELECT_SQL)
        .bind(user_id)
        .fetch_optional(ex)
        .await?;
    if let Some((id, is_onboarded, last_workspace_id)) = row {
        return Ok(ProfilePath {
            id,
            is_onboarded,
            last_workspace_id,
        });
    }
    let id = Uuid::new_v4();
    sqlx::query(PROFILE_DEFAULT_INSERT_SQL)
        .bind(id)
        .bind(now)
        .bind(user_id)
        .bind(background_color)
        .execute(ex)
        .await?;
    Ok(ProfilePath {
        id,
        is_onboarded: false,
        last_workspace_id: None,
    })
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
}
