#![forbid(unsafe_code)]

//! Agent workpad accessors + agent system user (D-12 L2, stage 5).
//!
//! Ports `orchestration/workpad.py` whole:
//!
//! * [`fetch_workpad`] / [`set_workpad`] — `get_workpad` (`:74-76`)
//!   and `set_workpad` (`:79-84`, `@transaction.atomic`).
//! * [`get_agent_system_user`] — `get_agent_system_user` (`:44-71`)
//!   with [`AGENT_USERNAME`] / [`AGENT_USER_EMAIL`] /
//!   [`AGENT_USER_FIRST_NAME`] / [`AGENT_USER_LAST_NAME`] (`:29-32`)
//!   and [`AgentUserCollisionError`] (`:35-41`).
//!
//! The workpad functions take the issue id where Python takes the
//! instance; [`set_workpad`] writes `workpad` + `updated_at` exactly
//! like `update_fields=["workpad", "updated_at"]` (the `SET` order
//! follows Django's `_meta` order per the fixture).
//!
//! The agent-user `INSERT` spells out all 40 physical `users` columns
//! in `_meta` order with Django-side values, including the two
//! `User.save()` normalizations (`db/models/user.py:168-182`): the
//! email is already lowercase, and `display_name` defaults to the
//! email prefix (`agent`). The `post_save` notification-preference
//! receiver (`user.py:307-320`) skips bots, so creation has no side
//! effects beyond the row plus the unusable-password `UPDATE`.
//!
//! Fixture: `rust-api/fixtures/orchestration/fx02_reads/` (FX-ORCH-02:
//! `workpad.golden.json`, `agent_system_user.golden.json`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use sqlx::postgres::PgRow;
use sqlx::Row;

// ---------------------------------------------------------------------------
// Workpad accessors
// ---------------------------------------------------------------------------

/// `get_workpad` read (`workpad.py:74-76`): the issue's workpad body.
/// `$1` is the issue id.
pub const GET_WORKPAD_SQL: &str = "SELECT workpad FROM issues WHERE id = $1";

/// `set_workpad` write (`workpad.py:79-84`):
/// `update_fields=["workpad", "updated_at"]`, rendered in `_meta`
/// order per the fixture. `$1` is the stamp, `$2` the body, `$3` the
/// issue id.
pub const SET_WORKPAD_SQL: &str = "UPDATE issues SET updated_at = $1, workpad = $2 WHERE id = $3";

/// Return the issue's workpad body, `""` when never written
/// (`issue.workpad or ""`). A missing row is `RowNotFound` — Python
/// takes a live instance, so "missing" is unreachable there.
pub async fn fetch_workpad<'e, E>(ex: E, issue_id: uuid::Uuid) -> Result<String, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(GET_WORKPAD_SQL)
        .bind(issue_id)
        .fetch_optional(ex)
        .await?;
    match row {
        None => Err(sqlx::Error::RowNotFound),
        Some(row) => {
            let body: Option<String> = row.try_get("workpad")?;
            Ok(body.unwrap_or_default())
        }
    }
}

/// Overwrite the workpad body; `None`/empty clears it
/// (`body or ""`). `now` stands in for the `auto_now` stamp. A
/// missing row is a silent no-op (Python's `save()` would attempt an
/// `INSERT` fallback there — unreachable from live callers and
/// deliberately not recreated).
pub async fn set_workpad<'e, E>(
    ex: E,
    issue_id: uuid::Uuid,
    body: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(SET_WORKPAD_SQL)
        .bind(now)
        .bind(body.unwrap_or(""))
        .bind(issue_id)
        .execute(ex)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Agent system user
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
    pub id: uuid::Uuid,
    pub username: String,
    pub email: Option<String>,
    pub first_name: String,
    pub last_name: String,
    pub is_bot: bool,
}

/// Lookup by the unique username (`workpad.py:53-61`). Django's
/// `get()` pages with `LIMIT 21` to detect multiplicity; the unique
/// index precludes that, so this reads one row. `$1` is the username.
pub const AGENT_USER_SELECT_SQL: &str =
    "SELECT id, username, email, first_name, last_name, is_bot FROM users WHERE username = $1 LIMIT 1";

/// Map one agent-user row.
pub fn map_agent_system_user(row: &PgRow) -> Result<AgentSystemUser, sqlx::Error> {
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
    let row: Option<PgRow> = sqlx::query(AGENT_USER_SELECT_SQL)
        .bind(username)
        .fetch_optional(ex)
        .await?;
    row.map(|row| map_agent_system_user(&row)).transpose()
}

/// `get_or_create` insert (`workpad.py:53-61`): all 40 physical
/// `users` columns in `_meta` order (per
/// `FX-AUTH-03.models.json`, which pins the columns + nullability),
/// so the statement holds every `NOT NULL` constraint with no
/// database default. `$1..$40` follow the column order; see
/// [`get_agent_system_user`] for the values.
pub const AGENT_USER_INSERT_SQL: &str = "INSERT INTO users (password, last_login, id, username,
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
/// (`workpad.py:62-65`). `$1` is the unusable hash, `$2` the user id.
pub const AGENT_USER_PASSWORD_SQL: &str = "UPDATE users SET password = $1 WHERE id = $2";

/// `make_password(None)`: `!` plus 40 `SystemRandom` alphanumerics
/// (`django/contrib/auth/hashers.py`). `rand::Alphanumeric` draws from
/// the same 62-letter alphabet.
pub fn make_unusable_password() -> String {
    use rand::{distributions::Alphanumeric, Rng};
    let suffix: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(40)
        .map(char::from)
        .collect();
    format!("!{suffix}")
}

/// `is_password_usable` (`django/contrib/auth/hashers.py`): anything
/// not starting with `!` counts as usable (including `""`).
pub fn is_password_usable(encoded: &str) -> bool {
    !encoded.starts_with('!')
}

/// `uuid.uuid4` for the created row (`UUIDField(default=uuid.uuid4)`).
/// The `uuid` dependency has no `v4` feature, so the version/variant
/// bits are set by hand over 122 random bits — the same distribution.
fn new_v4() -> uuid::Uuid {
    let mut bytes: [u8; 16] = rand::random();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}

/// Run [`AGENT_USER_INSERT_SQL`] with the Django-side values.
/// Column order follows the statement: Django defaults everywhere
/// except the four `defaults={...}` entries, `is_bot=True`, the
/// `save()`-derived display name, and `now` for the auto stamps plus
/// `last_active` (whose default is `timezone.now`).
async fn insert_agent_row(
    conn: &mut sqlx::PgConnection,
    id: uuid::Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(AGENT_USER_INSERT_SQL)
        .bind("") // password: CharField with no default
        .bind(Option::<chrono::DateTime<chrono::Utc>>::None) // last_login
        .bind(id)
        .bind(AGENT_USERNAME)
        .bind(Option::<String>::None) // mobile_number
        .bind(AGENT_USER_EMAIL)
        .bind(AGENT_DISPLAY_NAME)
        .bind(AGENT_USER_FIRST_NAME)
        .bind(AGENT_USER_LAST_NAME)
        .bind("") // avatar
        .bind(Option::<uuid::Uuid>::None) // avatar_asset_id
        .bind(Option::<String>::None) // cover_image
        .bind(Option::<uuid::Uuid>::None) // cover_image_asset_id
        .bind(now) // date_joined
        .bind(now) // created_at
        .bind(now) // updated_at
        .bind("") // last_location
        .bind("") // created_location
        .bind(false) // is_superuser
        .bind(false) // is_managed
        .bind(false) // is_password_expired
        .bind(true) // is_active
        .bind(false) // is_staff
        .bind(false) // is_email_verified
        .bind(false) // is_password_autoset
        .bind(false) // is_password_reset_required
        .bind("") // token
        .bind(now) // last_active (default=timezone.now)
        .bind(Option::<chrono::DateTime<chrono::Utc>>::None) // last_login_time
        .bind(Option::<chrono::DateTime<chrono::Utc>>::None) // last_logout_time
        .bind("") // last_login_ip
        .bind("") // last_logout_ip
        .bind("email") // last_login_medium
        .bind("") // last_login_uagent
        .bind(Option::<chrono::DateTime<chrono::Utc>>::None) // token_updated_at
        .bind(true) // is_bot
        .bind(Option::<String>::None) // bot_type
        .bind("UTC") // user_timezone
        .bind(false) // is_email_valid
        .bind(Option::<chrono::DateTime<chrono::Utc>>::None) // masked_at
        .execute(&mut *conn)
        .await?;
    Ok(())
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
    now: chrono::DateTime<chrono::Utc>,
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
    let id = new_v4();
    let insert = insert_agent_row(&mut *conn, id, now).await;
    match insert {
        Ok(_) => {
            sqlx::query("RELEASE SAVEPOINT pidash_agent_user")
                .execute(&mut *conn)
                .await?;
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
    }
    sqlx::query(AGENT_USER_PASSWORD_SQL)
        .bind(make_unusable_password())
        .bind(id)
        .execute(&mut *conn)
        .await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    static WORKPAD_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx02_reads/workpad.golden.json");
    static AGENT_USER_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx02_reads/agent_system_user.golden.json");

    #[test]
    fn set_workpad_sql_sets_both_update_fields() {
        // Fixture `set_update_sql[0]` renders Django's `_meta` order
        // (`updated_at` first); the bind order follows it.
        let parsed: Value = serde_json::from_str(WORKPAD_FIXTURE).expect("fixture parses");
        let rendered = parsed["set_update_sql"][0]
            .as_str()
            .expect("set_update_sql");
        assert!(rendered.starts_with("UPDATE \"issues\" SET \"updated_at\" = "));
        assert!(rendered.contains(", \"workpad\" = "));
        assert_eq!(
            SET_WORKPAD_SQL,
            "UPDATE issues SET updated_at = $1, workpad = $2 WHERE id = $3"
        );
        assert_eq!(GET_WORKPAD_SQL, "SELECT workpad FROM issues WHERE id = $1");
    }

    #[test]
    fn agent_constants_and_collision_message_match_fixture() {
        let parsed: Value = serde_json::from_str(AGENT_USER_FIXTURE).expect("fixture parses");
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
        let head = AGENT_USER_INSERT_SQL;
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
            AGENT_USER_PASSWORD_SQL,
            "UPDATE users SET password = $1 WHERE id = $2"
        );
    }

    // -- live scratch-DB tests (env-gated) -------------------------------

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

    const ISSUES_DDL: &str = "CREATE TEMPORARY TABLE issues (
        id UUID PRIMARY KEY, workpad TEXT, updated_at TIMESTAMPTZ NOT NULL)";

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

    fn live_uuid(tag: &str) -> uuid::Uuid {
        uuid::Uuid::parse_str(&format!("11111111-2222-3333-4444-{tag}")).expect("fixed uuid")
    }

    /// `workpad.golden.json` replay: fresh issue reads `""`, set /
    /// overwrite / clear round-trip, `None` and `NULL` read as `""`.
    #[tokio::test]
    async fn live_workpad_roundtrip_replays_fixture() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(ISSUES_DDL)
            .execute(&mut *tx)
            .await
            .expect("temp issues");
        let issue = live_uuid("000000003020");
        let epoch: chrono::DateTime<chrono::Utc> =
            "2026-10-03T11:00:00+00:00".parse().expect("epoch");
        sqlx::query("INSERT INTO issues (id, workpad, updated_at) VALUES ($1, '', $2)")
            .bind(issue)
            .bind(epoch)
            .execute(&mut *tx)
            .await
            .expect("seed issue");
        assert_eq!(fetch_workpad(&mut *tx, issue).await.expect("get fresh"), "");
        let now: chrono::DateTime<chrono::Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        set_workpad(
            &mut *tx,
            issue,
            Some("## Agent Workpad\n\n- implementing\n"),
            now,
        )
        .await
        .expect("set");
        assert_eq!(
            fetch_workpad(&mut *tx, issue).await.expect("get set"),
            "## Agent Workpad\n\n- implementing\n"
        );
        let stamp: chrono::DateTime<chrono::Utc> =
            sqlx::query("SELECT updated_at FROM issues WHERE id = $1")
                .bind(issue)
                .fetch_one(&mut *tx)
                .await
                .expect("read back")
                .try_get("updated_at")
                .expect("updated_at");
        assert_eq!(stamp, now);
        set_workpad(&mut *tx, issue, Some("second"), now)
            .await
            .expect("overwrite");
        assert_eq!(
            fetch_workpad(&mut *tx, issue).await.expect("get overwrite"),
            "second"
        );
        set_workpad(&mut *tx, issue, Some(""), now)
            .await
            .expect("clear");
        assert_eq!(
            fetch_workpad(&mut *tx, issue).await.expect("get cleared"),
            ""
        );
        set_workpad(&mut *tx, issue, None, now)
            .await
            .expect("set none");
        assert_eq!(fetch_workpad(&mut *tx, issue).await.expect("get none"), "");
        // A `NULL` workpad (unreachable on the real `NOT NULL`
        // column) still reads as `""`, like `issue.workpad or ""`.
        sqlx::query("UPDATE issues SET workpad = NULL WHERE id = $1")
            .bind(issue)
            .execute(&mut *tx)
            .await
            .expect("null out");
        assert_eq!(fetch_workpad(&mut *tx, issue).await.expect("get null"), "");
        assert!(matches!(
            fetch_workpad(&mut *tx, live_uuid("000000009999")).await,
            Err(sqlx::Error::RowNotFound)
        ));
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
        let now: chrono::DateTime<chrono::Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let (created, was_created) = get_agent_system_user(&mut tx, now)
            .await
            .expect("create agent user");
        assert!(was_created);
        assert_eq!(created.username, AGENT_USERNAME);
        assert_eq!(created.email.as_deref(), Some(AGENT_USER_EMAIL));
        assert_eq!(created.first_name, AGENT_USER_FIRST_NAME);
        assert_eq!(created.last_name, AGENT_USER_LAST_NAME);
        assert!(created.is_bot);
        let row: PgRow = sqlx::query(
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
        let last_active: Option<chrono::DateTime<chrono::Utc>> =
            row.try_get("last_active").expect("last_active");
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
        let raced = insert_agent_row(&mut tx, live_uuid("000000005002"), now).await;
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
        let now: chrono::DateTime<chrono::Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
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
        let now: chrono::DateTime<chrono::Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
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
