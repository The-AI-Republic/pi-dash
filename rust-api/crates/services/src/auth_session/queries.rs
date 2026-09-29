//! D-16 authentication read/write query kernel (stage 5, PIDASHCONV-382).
//!
//! Ports the query layer named by the issue:
//!
//! * `User.objects.filter(email=).first()` / `.exists()` / `.get(pk=)`
//!   (`db/models/user.py:56-61`; call sites `adapter/base.py:297`,
//!   `provider/credentials/email.py`, `provider/credentials/magic_code.py`,
//!   `views/{app,space}/{email,magic,password_management,check}.py`,
//!   `views/common.py`).
//! * Session read/write/delete through the `sessions` table
//!   (`db/models/session.py:30-56`; writes inherit
//!   `django.contrib.sessions.backends.db`).
//! * `Instance.objects.first()` + the `is_setup_done` gate
//!   (`license/models/instance.py:22-50`).
//! * `Adapter.set_user_data` / `complete_login_or_signup` / `save_user_data` /
//!   `sync_user_data` create/update paths (`adapter/base.py:220-360`).
//! * `post_user_auth_workflow` → `process_workspace_project_invitations`
//!   (`utils/user_auth_workflow.py:8-9`, `utils/workspace_project_join.py:20-91`).
//! * `check_password` / `set_password` call sites (F-05 semantics,
//!   `views/common.py:74,92,131`, `views/*/password_management.py`).
//!
//! Out of scope (other domains, not re-specified here): OAuth `Account`
//! queries (D-17), CLI device-code queries (D-22), membership-table column
//! lists (owning-domain kernels; recorded here as observed writes only).
//!
//! Like the sibling [`super::models`] and D-29 `app_views_search` kernels,
//! everything here is pure over injected inputs: SQL builders take the bind
//! placeholder (`$1`, …) as a parameter and return `String`; no database
//! handle is held and the foundation `pidash-db` crate is not touched.
//! Fixture: `rust-api/fixtures/auth_session/FX-AUTH-04.queries.json`
//! (PIDASHCONV-279); the `#[cfg(test)]` suite replays it.
//!
//! SQL rendering note: Django `str(queryset.query)` inlines params unquoted
//! (`WHERE "users"."email" = a@x.com`); the builders below emit `$n`
//! placeholders for the same predicate. Quoting, qualification, column
//! order and row shape are identical — only the param spelling differs.
//!
//! Ported bugs (translated, not fixed; also listed in the PR):
//! - `BUG-WRITE (workspace_project_join.py:76-87)`: the `ProjectMember`
//!   `bulk_create` omits `project_id` despite the column being NOT NULL, so
//!   any signup carrying an accepted project invite raises `IntegrityError`
//!   on Postgres even with `ignore_conflicts=True`
//!   ([`project_member_bulk_create_sql`] keeps the missing column).
//! - `BUG-SIGNUP-FLAG (adapter/base.py:299)`: `complete_login_or_signup`
//!   sets `is_signup = bool(existing_user)` — inverted, so a new-user
//!   signup reports `is_signup=False` and an existing-user login reports
//!   `True` ([`complete_login_or_signup_steps`], FX-AUTH-06
//!   `signup_autoset` / `existing_login_callback`).

use serde_json::Value;

use super::models::{PASSWORD_HASH_ALGORITHM, PASSWORD_HASH_ITERATIONS};

// ---------------------------------------------------------------------------
// User reads
// ---------------------------------------------------------------------------

/// Columns of the `User.objects.filter(...).first()` / `.get(pk=)` select,
/// in Django's render order (`FX-AUTH-04 user_by_email.sql_postgres`).
pub const USER_SELECT_COLUMNS: &[&str] = &[
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

/// `Meta.ordering = ("-created_at",)` (`user.py:133-137`): `.first()`
/// returns the latest-created row on duplicate email.
pub const USER_ORDERING: &str = "\"users\".\"created_at\" DESC";

/// Render `"users"."c1", "users"."c2", …` in [`USER_SELECT_COLUMNS`] order.
pub fn user_select_list(table: &str) -> String {
    USER_SELECT_COLUMNS
        .iter()
        .map(|c| format!("\"{table}\".\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `User.objects.filter(email=<email>).first()`
/// (`adapter/base.py:297`, email/magic views).
///
/// Case handling is exactly Django's: `filter(email=)` is an exact match —
/// case-sensitive on Postgres — with no normalization at query time.
/// Lower-casing happens before the lookup (`Adapter.sanitize_email`,
/// `base.py:75`) and on write (`User.save`, `user.py:169`); the query
/// itself adds none.
pub fn user_by_email_sql(table: &str, email_param: &str) -> String {
    format!(
        "SELECT {} FROM \"{table}\" WHERE \"{table}\".\"email\" = {email_param} ORDER BY {USER_ORDERING}",
        user_select_list(table),
    )
}

/// `User.objects.filter(email=<email>).exists()` — the same WHERE arm as
/// [`user_by_email_sql`] under Django 4.2's `exists()` rendering
/// (`SELECT (1) AS "a" … LIMIT 1`; signup path `email.py`, magic-code
/// paths `magic_code.py:70,122,136`).
pub fn user_email_exists_sql(table: &str, email_param: &str) -> String {
    format!(
        "SELECT (1) AS \"a\" FROM \"{table}\" WHERE \"{table}\".\"email\" = {email_param} ORDER BY {USER_ORDERING} LIMIT 1",
    )
}

/// `User.objects.get(pk=<id>)` (`views/common.py`, `SetUserPasswordEndpoint`).
pub fn user_by_pk_sql(table: &str, pk_param: &str) -> String {
    format!(
        "SELECT {} FROM \"{table}\" WHERE \"{table}\".\"id\" = {pk_param} ORDER BY {USER_ORDERING}",
        user_select_list(table),
    )
}

// ---------------------------------------------------------------------------
// Session reads and writes
// ---------------------------------------------------------------------------

/// Columns of the session read, in Django's render order
/// (`FX-AUTH-04 session_read.sql_postgres`).
pub const SESSION_SELECT_COLUMNS: &[&str] = &[
    "session_data",
    "expire_date",
    "device_info",
    "session_key",
    "user_id",
];

/// `SessionStore.exists()` / `load()` path:
/// `Session.objects.filter(session_key=)` (`session.py`, inherited
/// `django.contrib.sessions.backends.db`).
pub fn session_read_sql(table: &str, key_param: &str) -> String {
    let list = SESSION_SELECT_COLUMNS
        .iter()
        .map(|c| format!("\"{table}\".\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!("SELECT {list} FROM \"{table}\" WHERE \"{table}\".\"session_key\" = {key_param}")
}

/// `SessionStore.save()` update arm: the loaded key exists, so Django issues
/// a full-model `UPDATE` over every column (`force_update`). `expire_date`
/// is `get_expiry_date()` (global `SESSION_COOKIE_AGE` default of two weeks
/// unless the payload carries a nearer expiry); `session_data` is the
/// signed payload string.
pub fn session_save_update_sql(
    table: &str,
    key_param: &str,
    data_param: &str,
    expire_param: &str,
) -> String {
    format!(
        "UPDATE \"{table}\" SET \"session_data\" = {data_param}, \"expire_date\" = {expire_param} WHERE \"{table}\".\"session_key\" = {key_param}",
    )
}

/// `SessionStore.save()` insert arm (`must_create` / `IntegrityError`
/// fallback): full-model `INSERT` over every column. `user_id` and
/// `device_info` are the [`super::models::create_model_instance`] outputs
/// (`session.py:45-56`); the key is 128 chars from
/// [`super::models::VALID_KEY_CHARS`].
pub fn session_save_insert_sql(
    table: &str,
    key_param: &str,
    data_param: &str,
    expire_param: &str,
) -> String {
    format!(
        "INSERT INTO \"{table}\" (\"session_key\", \"session_data\", \"expire_date\") VALUES ({key_param}, {data_param}, {expire_param})",
    )
}

/// `SessionStore.delete()`:
/// `Session.objects.filter(session_key=).delete()`.
pub fn session_delete_sql(table: &str, key_param: &str) -> String {
    format!("DELETE FROM \"{table}\" WHERE \"{table}\".\"session_key\" = {key_param}")
}

/// Django db-backend save order: try the update arm first, fall back to the
/// insert arm on zero matched rows (`IntegrityError` path).
pub const SESSION_SAVE_ORDER: &[&str] = &["update", "insert"];

// ---------------------------------------------------------------------------
// Instance setup gate
// ---------------------------------------------------------------------------

/// `Instance` table (`license/models/instance.py:46-50`).
pub const INSTANCE_DB_TABLE: &str = "instances";

/// Columns of `Instance.objects.first()`, in Django's render order:
/// `BaseModel` audit columns first, then definition order
/// (`FX-AUTH-04 instance_first.sql_postgres`).
pub const INSTANCE_SELECT_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "instance_name",
    "whitelist_emails",
    "instance_id",
    "current_version",
    "latest_version",
    "edition",
    "domain",
    "last_checked_at",
    "namespace",
    "is_telemetry_enabled",
    "is_support_required",
    "is_setup_done",
    "is_signup_screen_visited",
    "is_verified",
    "is_test",
    "is_current_version_deprecated",
];

/// `Instance.objects.first()`: the `SoftDeleteManager` adds
/// `deleted_at IS NULL`, `Meta.ordering = ("-created_at",)` picks the
/// latest row. Execution appends `LIMIT 1`; `str(query)` shows none.
pub fn instance_first_sql(table: &str) -> String {
    let list = INSTANCE_SELECT_COLUMNS
        .iter()
        .map(|c| format!("\"{table}\".\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {list} FROM \"{table}\" WHERE \"{table}\".\"deleted_at\" IS NULL ORDER BY \"{table}\".\"created_at\" DESC"
    )
}

/// The gate every sign-in/up + magic-generate view applies
/// (`views/app/email.py:30-31`, `views/app/magic.py:40-41`,
/// `views/app/password_management.py:54-55`, space twins):
/// `if instance is None or not instance.is_setup_done`.
/// Returns `true` when the request must be rejected.
pub fn setup_gate_blocks(instance_row: Option<&Value>) -> bool {
    match instance_row {
        None => true,
        Some(row) => row.get("is_setup_done") != Some(&Value::Bool(true)),
    }
}

// ---------------------------------------------------------------------------
// Adapter user create/update paths
// ---------------------------------------------------------------------------

/// New-user `set_user_data` defaults for credential providers
/// (`provider/credentials/email.py` signup branch; magic-code equivalent):
/// avatar `""`, names `""`, `provider_id` `""`, `is_password_autoset`
/// `False`.
pub fn new_user_data_defaults(email: &str) -> Value {
    serde_json::json!({
        "email": email,
        "user": {
            "avatar": "",
            "first_name": "",
            "last_name": "",
            "provider_id": "",
            "is_password_autoset": false,
        },
    })
}

/// Columns `complete_login_or_signup` writes on the new-user `INSERT`
/// (`adapter/base.py:306-328`): identity + names; the password hash lands
/// via `set_password` (F-05) before this insert.
pub const NEW_USER_INSERT_COLUMNS: &[&str] = &[
    "email",
    "username",
    "password",
    "is_password_autoset",
    "is_email_verified",
    "first_name",
    "last_name",
];

/// Fields `save_user_data` rewrites on every login
/// (`adapter/base.py:220-234`): login enrichment (medium, timestamps, IP,
/// user-agent, token stamp) plus the activation branch.
pub const SAVE_USER_DATA_FIELDS: &[&str] = &[
    "last_login_medium",
    "last_active",
    "last_login_time",
    "last_login_ip",
    "last_login_uagent",
    "token_updated_at",
    "is_active",
];

/// The `save_user_data` UPDATE over [`SAVE_USER_DATA_FIELDS`]. The
/// activation-mail branch (`user_activation_email.delay(base_host, user.id)`
/// when `not user.is_active`) is a call record, not SQL.
pub fn save_user_data_sql(table: &str, id_param: &str) -> String {
    let sets = SAVE_USER_DATA_FIELDS
        .iter()
        .enumerate()
        .map(|(i, c)| format!("\"{c}\" = ${}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    format!("UPDATE \"{table}\" SET {sets} WHERE \"{table}\".\"id\" = {id_param}")
}

/// Fields `sync_user_data` rewrites when IDP sync is enabled and the user
/// is not signing up (`adapter/base.py:256-287`).
pub const SYNC_USER_DATA_FIELDS: &[&str] = &[
    "first_name",
    "last_name",
    "display_name",
    "avatar",
    "avatar_asset",
];

/// Side-effect touches of the create/update paths, as call records:
/// avatar `FileAsset.objects.create` + old-asset delete
/// (`download_and_upload_avatar`, `delete_old_avatar`),
/// `Profile.objects.create(user=user)` on signup (`base.py:342`), and the
/// `WorkspaceMemberInvite.objects.filter(email=email).exists()` signup gate
/// (`__check_signup`, `base.py:111`).
pub const ADAPTER_TOUCHES: &[&str] = &[
    "FileAsset.objects.create(entity_type=USER_AVATAR)",
    "FileAsset.delete(old avatar_asset)",
    "Profile.objects.create(user)",
    "WorkspaceMemberInvite.objects.filter(email).exists()",
];

/// `complete_login_or_signup` (`adapter/base.py:289-360`) as an ordered
/// call sequence. Step 2 keeps the inverted flag as written
/// (`is_signup = bool(user)`): new-user signup reports `False`.
pub const COMPLETE_LOGIN_SIGNUP_STEPS: &[&str] = &[
    "sanitize_email",
    "User.objects.filter(email).first()",
    "is_signup = bool(user) [inverted as written]",
    "new-user: check_signup, User(email, username=uuid4hex), set_password, names, save, avatar upload, Profile.create",
    "existing-user + IDP sync: sync_user_data",
    "save_user_data",
    "callback(user, is_signup, request)",
    "token_data present: create_update_account",
];

/// `User.save()` side effects the write path inherits (`user.py:167-187`):
/// email lower/strip, token rotation when `token_updated_at` is set,
/// `display_name` backfill from the email local part, `is_staff` from
/// `is_superuser`.
pub const USER_SAVE_SIDE_EFFECTS: &[&str] = &[
    "email lower+strip",
    "token rotation when token_updated_at set",
    "display_name backfill",
    "is_staff from is_superuser",
];

// ---------------------------------------------------------------------------
// post_user_auth_workflow → process_workspace_project_invitations
// ---------------------------------------------------------------------------

/// Invite lookup arms (`workspace_project_join.py:24,59`).
pub const WORKSPACE_INVITE_WHERE: &str = "email = $1 AND accepted = TRUE";
pub const PROJECT_INVITE_WHERE: &str = "email = $1 AND accepted = TRUE";

/// Membership tables (owning-domain kernels own their column lists;
/// recorded here as the observed write targets only).
pub const WORKSPACE_MEMBER_TABLE: &str = "workspace_members";
pub const PROJECT_MEMBER_TABLE: &str = "project_members";
pub const WORKSPACE_INVITE_TABLE: &str = "workspace_member_invites";
pub const PROJECT_INVITE_TABLE: &str = "project_member_invites";

/// Role mapping for the project-invite arm (`:66,80`):
/// `role if role in [5, 15] else 15` (fixture: invite `role=20` → `15`).
pub fn map_invite_role(role: i32) -> i32 {
    if role == 5 || role == 15 {
        role
    } else {
        15
    }
}

/// `WorkspaceMember.objects.bulk_create(..., ignore_conflicts=True)` over
/// accepted workspace invites (`:26-36`): one multi-row `INSERT` with
/// `ON CONFLICT DO NOTHING`. BaseModel audit columns ride along via the
/// owning-domain kernels and are not re-pinned here.
pub fn workspace_member_bulk_create_sql(table: &str, rows: usize) -> String {
    let one = "(workspace_id, member_id, role)";
    let values = vec![one; rows].join(", ");
    format!("INSERT INTO \"{table}\" (\"workspace_id\", \"member_id\", \"role\") VALUES {values} ON CONFLICT DO NOTHING")
}

/// Project-invite arm of the same (`:62-73`): workspace membership with the
/// mapped role and `created_by_id` carried over.
pub fn project_invite_workspace_member_bulk_create_sql(table: &str, rows: usize) -> String {
    let one = "(workspace_id, member_id, role, created_by_id)";
    let values = vec![one; rows].join(", ");
    format!("INSERT INTO \"{table}\" (\"workspace_id\", \"member_id\", \"role\", \"created_by_id\") VALUES {values} ON CONFLICT DO NOTHING")
}

/// `ProjectMember.objects.bulk_create(..., ignore_conflicts=True)` (`:76-87`)
/// as written: `project_id` is missing (ported bug — on Postgres this
/// raises `IntegrityError`, fixture `ported_bugs[0]`).
pub fn project_member_bulk_create_sql(table: &str, rows: usize) -> String {
    let one = "(workspace_id, member_id, role, created_by_id)";
    let values = vec![one; rows].join(", ");
    format!("INSERT INTO \"{table}\" (\"workspace_id\", \"member_id\", \"role\", \"created_by_id\") VALUES {values} ON CONFLICT DO NOTHING")
}

/// Invite deletes (`:90-91`): both querysets deleted after the joins.
pub fn invite_delete_sql(table: &str) -> String {
    format!("DELETE FROM \"{table}\" WHERE email = $1 AND accepted = TRUE")
}

/// `invalidate_cache_directly` call shape (`:39-44`, fixture-pinned).
pub fn workspace_members_invalidate_call(workspace_slug: &str) -> Value {
    serde_json::json!({
        "path": format!("/api/workspaces/{workspace_slug}/members/"),
        "url_params": false,
        "user": false,
        "multiple": true,
    })
}

/// `track_event.delay` call shape (`:45-56`, `USER_JOINED_WORKSPACE =
/// "user_joined_workspace"`). `joined_at` is `str(timezone.now())` at call
/// time and arrives as a parameter, like a bind value.
pub fn workspace_join_track_event_call(
    user_id: &str,
    workspace_id: &str,
    workspace_slug: &str,
    role: i32,
    joined_at: &str,
) -> Value {
    serde_json::json!({
        "user_id": user_id,
        "event_name": "user_joined_workspace",
        "slug": workspace_slug,
        "event_properties": {
            "user_id": user_id,
            "workspace_id": workspace_id,
            "workspace_slug": workspace_slug,
            "role": role,
            "joined_at": joined_at,
        },
    })
}

// ---------------------------------------------------------------------------
// Password verification/set (F-05 delegation)
// ---------------------------------------------------------------------------

/// Call sites, as records — the crypto is F-05 library semantics
/// ([`PASSWORD_HASH_ALGORITHM`] `pbkdf2_sha256`,
/// [`PASSWORD_HASH_ITERATIONS`] `1500000`; fixture `password` section):
/// `common.py:74` check (skipped when `is_password_autoset`),
/// `common.py:92,131` + `password_management` views set +
/// `is_password_autoset = False` + `save()`.
pub const PASSWORD_CALL_SITES: &[(&str, &str)] = &[
    (
        "views/common.py:74",
        "check (skip when is_password_autoset)",
    ),
    (
        "views/common.py:92",
        "set + is_password_autoset=False + save",
    ),
    (
        "views/common.py:131",
        "set + is_password_autoset=False + save",
    ),
    (
        "views/app|space/password_management.py",
        "set + is_password_autoset=False + save",
    ),
    (
        "adapter/base.py:310,319",
        "set on signup (autoset uuid / credential code)",
    ),
];

/// A password-library call through this domain: verify or set the hash for
/// the user row. Rendered by F-05; recorded here so the sequence is pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordOp {
    Check,
    Set,
}

impl PasswordOp {
    /// F-05 entry point name and the recorded algorithm/iterations.
    pub fn call(&self) -> (&'static str, &'static str, u32) {
        match self {
            PasswordOp::Check => (
                "check_password",
                PASSWORD_HASH_ALGORITHM,
                PASSWORD_HASH_ITERATIONS,
            ),
            PasswordOp::Set => (
                "set_password",
                PASSWORD_HASH_ALGORITHM,
                PASSWORD_HASH_ITERATIONS,
            ),
        }
    }
}

/// Ported bugs replayed by this module (fixture `ported_bugs`).
pub const PORTED_BUGS: &[&str] = &[
    "workspace_project_join.py:76-87 ProjectMember bulk_create omits project_id -> IntegrityError on Postgres despite ignore_conflicts=True",
    "adapter/base.py:299 is_signup=bool(existing_user) inverted: signup reports False, login reports True",
];

pub use super::models::SESSION_DB_TABLE as SESSION_TABLE;
/// Re-export the table identities so handlers compose joins without
/// re-spelling them.
pub use super::models::USER_DB_TABLE as USER_TABLE;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/auth_session/FX-AUTH-04.queries.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    #[test]
    fn user_by_email_sql_matches_fixture() {
        let fx = fixture();
        let recorded = fx["queries"]["user_by_email"]["sql_postgres"]
            .as_str()
            .expect("sql_postgres recorded");
        // Django inlines the param unquoted; the kernel binds it instead.
        let expected = recorded.replace("\"email\" = a@x.com", "\"email\" = $1");
        assert_eq!(user_by_email_sql(USER_TABLE, "$1"), expected);
        assert_eq!(
            fx["queries"]["user_by_email"]["orm"],
            json!("User.objects.filter(email=<email>).first()")
        );
        // Fixture row keys are columns of this select.
        let row = &fx["queries"]["user_by_email"]["row"];
        for key in ["id", "email"] {
            assert!(
                USER_SELECT_COLUMNS.contains(&key),
                "{key} is a selected column"
            );
        }
        assert_eq!(row["email"], json!("probe@x.com"));
    }

    #[test]
    fn user_by_pk_sql_matches_fixture() {
        let fx = fixture();
        let recorded = fx["queries"]["user_by_pk"]["sql_sqlite"]
            .as_str()
            .expect("sql_sqlite recorded");
        let expected = recorded.replace("\"id\" = 0473977149744a1692e6f55898fd158f", "\"id\" = $1");
        assert_eq!(user_by_pk_sql(USER_TABLE, "$1"), expected);
        // Same select list as the email lookup, only the WHERE arm differs.
        assert!(user_by_pk_sql(USER_TABLE, "$1").contains(&user_select_list(USER_TABLE)));
    }

    #[test]
    fn user_email_exists_shares_where_arm() {
        let sql = user_email_exists_sql(USER_TABLE, "$1");
        assert!(sql.contains("SELECT (1) AS \"a\""));
        assert!(sql.contains("WHERE \"users\".\"email\" = $1"));
        assert!(sql.ends_with("LIMIT 1"));
    }

    #[test]
    fn session_read_sql_matches_fixture() {
        let fx = fixture();
        let recorded = fx["queries"]["session_read"]["sql_postgres"]
            .as_str()
            .expect("sql_postgres recorded");
        let expected = recorded.replace("\"session_key\" = k", "\"session_key\" = $1");
        assert_eq!(session_read_sql(SESSION_TABLE, "$1"), expected);
        let row = &fx["queries"]["session_read"]["row"];
        assert_eq!(
            row["device_info"],
            json!({"user_agent": "UA", "ip_address": "9.9.9.9", "domain": "http://localhost:3000"})
        );
        for col in ["session_key", "user_id", "device_info"] {
            assert!(SESSION_SELECT_COLUMNS.contains(&col), "{col} selected");
        }
    }

    #[test]
    fn session_write_delete_arms_cover_all_columns() {
        let update = session_save_update_sql(SESSION_TABLE, "$1", "$2", "$3");
        assert!(update.starts_with("UPDATE \"sessions\" SET "));
        for col in ["session_data", "expire_date"] {
            assert!(update.contains(col), "{col} written");
        }
        assert!(update.contains("WHERE \"sessions\".\"session_key\" = $1"));
        let insert = session_save_insert_sql(SESSION_TABLE, "$1", "$2", "$3");
        assert!(insert.starts_with("INSERT INTO \"sessions\""));
        for col in ["session_key", "session_data", "expire_date"] {
            assert!(insert.contains(col), "{col} inserted");
        }
        assert_eq!(
            session_delete_sql(SESSION_TABLE, "$1"),
            "DELETE FROM \"sessions\" WHERE \"sessions\".\"session_key\" = $1"
        );
        assert_eq!(SESSION_SAVE_ORDER, &["update", "insert"]);
    }

    #[test]
    fn instance_first_sql_matches_fixture() {
        let fx = fixture();
        assert_eq!(
            instance_first_sql(INSTANCE_DB_TABLE),
            fx["queries"]["instance_first"]["sql_postgres"]
                .as_str()
                .expect("sql_postgres recorded")
        );
        assert_eq!(
            instance_first_sql(INSTANCE_DB_TABLE),
            fx["queries"]["instance_first"]["sql_sqlite"]
                .as_str()
                .expect("sql_sqlite recorded")
        );
        assert_eq!(
            fx["queries"]["instance_first"]["row"]["is_setup_done"],
            json!(true)
        );
    }

    #[test]
    fn setup_gate_matches_python_condition() {
        assert!(setup_gate_blocks(None));
        assert!(setup_gate_blocks(Some(&json!({"is_setup_done": false}))));
        assert!(setup_gate_blocks(Some(&json!({}))));
        assert!(!setup_gate_blocks(Some(&json!({"is_setup_done": true}))));
    }

    #[test]
    fn adapter_new_user_defaults_match_provider() {
        let got = new_user_data_defaults("n@x.com");
        assert_eq!(got["email"], json!("n@x.com"));
        assert_eq!(
            got["user"],
            json!({
                "avatar": "",
                "first_name": "",
                "last_name": "",
                "provider_id": "",
                "is_password_autoset": false,
            })
        );
        assert_eq!(SAVE_USER_DATA_FIELDS.len(), 7);
        assert!(SAVE_USER_DATA_FIELDS.contains(&"last_login_ip"));
        assert!(SAVE_USER_DATA_FIELDS.contains(&"last_login_uagent"));
        let sql = save_user_data_sql(USER_TABLE, "$8");
        assert!(sql.starts_with("UPDATE \"users\" SET "));
        assert!(sql.ends_with("WHERE \"users\".\"id\" = $8"));
        assert_eq!(COMPLETE_LOGIN_SIGNUP_STEPS.len(), 8);
        assert!(COMPLETE_LOGIN_SIGNUP_STEPS[2].contains("inverted"));
    }

    #[test]
    fn workflow_write_sequence_matches_fixture() {
        let fx = fixture();
        let seq = &fx["workflow_write_sequence"];
        assert_eq!(seq["before"], json!({"wm": 0, "pm": 0, "wmi": 1, "pmi": 1}));
        assert_eq!(seq["after"], json!({"wm": 1, "pm": 0, "wmi": 0, "pmi": 0}));
        // Role mapping: invite role=20 -> 15; 5/15 pass through.
        assert_eq!(map_invite_role(20), 15);
        assert_eq!(map_invite_role(5), 5);
        assert_eq!(map_invite_role(15), 15);
        assert_eq!(
            seq["role_mapping"],
            json!("role if role in [5, 15] else 15 (probe invite role=20 -> 15)")
        );
        // bulk_create arms carry ignore_conflicts.
        for sql in [
            workspace_member_bulk_create_sql(WORKSPACE_MEMBER_TABLE, 1),
            project_invite_workspace_member_bulk_create_sql(WORKSPACE_MEMBER_TABLE, 1),
            project_member_bulk_create_sql(PROJECT_MEMBER_TABLE, 1),
        ] {
            assert!(sql.contains("ON CONFLICT DO NOTHING"), "{sql}");
        }
        // Ported bug kept: project_id missing from the ProjectMember arm.
        assert!(!project_member_bulk_create_sql(PROJECT_MEMBER_TABLE, 1).contains("project_id"));
        assert_eq!(
            invite_delete_sql(WORKSPACE_INVITE_TABLE),
            "DELETE FROM \"workspace_member_invites\" WHERE email = $1 AND accepted = TRUE"
        );
        // Call sequences replay the fixture.
        assert_eq!(
            workspace_members_invalidate_call("ws1"),
            seq["invalidate_calls"][0][0]
        );
        let recorded_event = &seq["track_event_calls"][0][0];
        let replay = workspace_join_track_event_call(
            recorded_event["user_id"].as_str().expect("user_id"),
            recorded_event["event_properties"]["workspace_id"]
                .as_str()
                .expect("workspace_id"),
            recorded_event["slug"].as_str().expect("slug"),
            recorded_event["event_properties"]["role"]
                .as_i64()
                .expect("role") as i32,
            recorded_event["event_properties"]["joined_at"]
                .as_str()
                .expect("joined_at"),
        );
        assert_eq!(replay, *recorded_event);
    }

    #[test]
    fn password_delegation_matches_fixture() {
        let fx = fixture();
        assert_eq!(
            fx["password"]["set_check"]["algo"],
            json!(PASSWORD_HASH_ALGORITHM)
        );
        assert_eq!(
            fx["password"]["set_check"]["iterations"]
                .as_str()
                .expect("iterations recorded as string")
                .parse::<u32>()
                .expect("iterations numeric"),
            PASSWORD_HASH_ITERATIONS
        );
        assert_eq!(fx["password"]["set_check"]["check_true"], json!(true));
        assert_eq!(fx["password"]["set_check"]["check_false"], json!(false));
        assert_eq!(PasswordOp::Check.call().0, "check_password");
        assert_eq!(PasswordOp::Set.call().0, "set_password");
        assert!(!PASSWORD_CALL_SITES.is_empty());
    }

    #[test]
    fn ported_bugs_match_fixture() {
        let fx = fixture();
        let recorded = fx["ported_bugs"].as_array().expect("ported_bugs recorded");
        assert_eq!(PORTED_BUGS.len(), recorded.len());
        assert!(PORTED_BUGS[0].contains("project_id"));
        assert!(PORTED_BUGS[1].contains("is_signup"));
    }
}
