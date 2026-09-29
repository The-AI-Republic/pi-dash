//! Session + User auth column semantics (D-16, stage 5).
//!
//! Port of `apps/api/pi_dash/db/models/session.py` (full file, `:1-56`) and
//! the auth-relevant columns of `apps/api/pi_dash/db/models/user.py`
//! (`:56-137`; `USERNAME_FIELD = "email"`, `Meta.db_table = "users"`).
//! Every function here is pure over injected inputs: this crate holds no
//! database handle, so `exists()` checks and randomness arrive as caller
//! arguments and the SQL itself lives with the queries layer
//! (PIDASHCONV-382). The foundation `pidash-db` crate is not touched.
//!
//! Vectors replay `rust-api/fixtures/auth_session/FX-AUTH-03.models.json`
//! (recorded by PIDASHCONV-279, traced in
//! `rust-api/fixtures/auth_session/TRACE.md`); the `#[cfg(test)]` suite
//! loads that file and asserts byte-level equality.
//!
//! Scope notes (what is deliberately NOT here):
//!
//! * `set_password` / `check_password` are Django `AbstractBaseUser` PBKDF2
//!   behaviour owned by F-05. Only the recorded algorithm name and iteration
//!   count are pinned here ([`PASSWORD_HASH_ALGORITHM`],
//!   [`PASSWORD_HASH_ITERATIONS`]); no hash is computed in this module.
//! * `User.save()` email lower-casing / token rotation / display-name
//!   backfill (`user.py:169-187`) and `get_display_name` (`:189-197`) are
//!   recorded in the fixture as reference but belong to the write path the
//!   queries layer ports; they are not re-declared here.
//! * `email` carries no Django validators beyond the field itself
//!   (`CharField(max_length=255, null=True, blank=True, unique=True)`):
//!   `user.py:56-137` declares none, so there is nothing to port. The
//!   uniqueness is a database constraint, not code.
//!
//! Ported bugs (translate, don't redesign): this slice's semantics carry no
//! recorded bug. The D-16 bug list (shared-mutable payload default, inverted
//! `is_signup`, `ProjectMember` bulk_create missing `project_id`, magic
//! 500s, redirect quirks, dead throttle view) lives in the other FX-AUTH
//! files and is ported by the layer that owns each unit.

use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Table identity
// ---------------------------------------------------------------------------

/// `Session.Meta.db_table` (`session.py:26-27`).
pub const SESSION_DB_TABLE: &str = "sessions";

/// Django base class supplying `session_data` / `expire_date` / the default
/// `session_key` (`session.py:17`, `AbstractBaseSession`).
pub const SESSION_DJANGO_BASE: &str = "django.contrib.sessions.base_session.AbstractBaseSession";

/// `Session.get_session_store_class` (`session.py:23-24`).
pub const SESSION_STORE_CLASS: &str = "pi_dash.db.models.session.SessionStore";

/// `User.Meta.db_table` (`user.py:133-137`).
pub const USER_DB_TABLE: &str = "users";

/// `User.USERNAME_FIELD` (`user.py:128`).
pub const USERNAME_FIELD: &str = "email";

// ---------------------------------------------------------------------------
// Column specs
// ---------------------------------------------------------------------------

/// A recorded default value: Django renders text defaults quoted
/// (`"''"`, `"'email'"`) and booleans bare, exactly as the fixture stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnDefault {
    /// A quoted text default, verbatim from the fixture (e.g. `"''"`).
    Text(&'static str),
    /// A boolean default.
    Bool(bool),
}

impl ColumnDefault {
    fn to_json(self) -> Value {
        match self {
            ColumnDefault::Text(s) => Value::String(s.to_owned()),
            ColumnDefault::Bool(b) => Value::Bool(b),
        }
    }
}

/// One Django field as recorded by the fixture: field name, backing column
/// (`None` for relations with no column, e.g. `ManyToManyField`), Django
/// field type, length bound, nullability, key flags and default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnSpec {
    /// Field name on the model.
    pub name: &'static str,
    /// Backing column; `None` when the field has none.
    pub column: Option<&'static str>,
    /// Django field class name (e.g. `"CharField"`).
    pub django_type: &'static str,
    /// Length bound, if the field has one.
    pub max_length: Option<u32>,
    /// Whether the column is nullable.
    pub null: bool,
    /// Whether the field is the primary key.
    pub primary_key: bool,
    /// Whether the field is indexed.
    pub db_index: bool,
    /// Whether the field is unique.
    pub unique: bool,
    /// Recorded default, if any.
    pub default: Option<ColumnDefault>,
}

impl ColumnSpec {
    /// Render back to the fixture's JSON object shape
    /// (`name`, `column`, `django_type`, `max_length`, `null`,
    /// `primary_key`, `db_index`, `unique`, plus `default` when present).
    pub fn to_json(self) -> Value {
        let mut obj = Map::new();
        obj.insert("name".to_owned(), Value::String(self.name.to_owned()));
        obj.insert(
            "column".to_owned(),
            self.column
                .map_or(Value::Null, |c| Value::String(c.to_owned())),
        );
        obj.insert(
            "django_type".to_owned(),
            Value::String(self.django_type.to_owned()),
        );
        obj.insert(
            "max_length".to_owned(),
            self.max_length
                .map_or(Value::Null, |m| Value::from(m as i64)),
        );
        obj.insert("null".to_owned(), Value::Bool(self.null));
        obj.insert("primary_key".to_owned(), Value::Bool(self.primary_key));
        obj.insert("db_index".to_owned(), Value::Bool(self.db_index));
        obj.insert("unique".to_owned(), Value::Bool(self.unique));
        if let Some(default) = self.default {
            obj.insert("default".to_owned(), default.to_json());
        }
        Value::Object(obj)
    }
}

/// Render a whole column list to the fixture's JSON array shape.
pub fn columns_json(columns: &[ColumnSpec]) -> Value {
    Value::Array(columns.iter().map(|c| c.to_json()).collect())
}

/// Session columns in fixture (`_meta`) order: the two
/// `AbstractBaseSession` base fields first, then the D-16 additions.
/// `session.py:17-27`; `Meta.db_table = "sessions"`.
pub const SESSION_COLUMNS: &[ColumnSpec] = &[
    ColumnSpec {
        name: "session_data",
        column: Some("session_data"),
        django_type: "TextField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "expire_date",
        column: Some("expire_date"),
        django_type: "DateTimeField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: true,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "device_info",
        column: Some("device_info"),
        django_type: "JSONField",
        max_length: None,
        null: true,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "session_key",
        column: Some("session_key"),
        django_type: "CharField",
        max_length: Some(128),
        null: false,
        primary_key: true,
        db_index: false,
        unique: true,
        default: None,
    },
    ColumnSpec {
        name: "user_id",
        column: Some("user_id"),
        django_type: "CharField",
        max_length: Some(50),
        null: true,
        primary_key: false,
        db_index: true,
        unique: false,
        default: None,
    },
];

/// Session column names in fixture order (`column_names`).
pub const SESSION_COLUMN_NAMES: &[&str] = &[
    "session_data",
    "expire_date",
    "device_info",
    "session_key",
    "user_id",
];

/// Names of [`SESSION_COLUMNS`], in order.
pub fn session_column_names() -> Vec<&'static str> {
    SESSION_COLUMNS.iter().map(|c| c.name).collect()
}

/// Full `User` column list as recorded under `auth_columns`
/// (`user.py:56-137` plus the audit/flag/tracking columns the probe read
/// from the real `_meta`). The auth slice this issue owns is `email`
/// (unique, no extra validators), `password` (hash field, F-05 owned) and
/// `is_password_autoset`; the rest is carried so the replay is whole.
pub const USER_AUTH_COLUMNS: &[ColumnSpec] = &[
    ColumnSpec {
        name: "password",
        column: Some("password"),
        django_type: "CharField",
        max_length: Some(128),
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "last_login",
        column: Some("last_login"),
        django_type: "DateTimeField",
        max_length: None,
        null: true,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "id",
        column: Some("id"),
        django_type: "UUIDField",
        max_length: Some(32),
        null: false,
        primary_key: true,
        db_index: true,
        unique: true,
        default: None,
    },
    ColumnSpec {
        name: "username",
        column: Some("username"),
        django_type: "CharField",
        max_length: Some(128),
        null: false,
        primary_key: false,
        db_index: false,
        unique: true,
        default: None,
    },
    ColumnSpec {
        name: "mobile_number",
        column: Some("mobile_number"),
        django_type: "CharField",
        max_length: Some(255),
        null: true,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "email",
        column: Some("email"),
        django_type: "CharField",
        max_length: Some(255),
        null: true,
        primary_key: false,
        db_index: false,
        unique: true,
        default: None,
    },
    ColumnSpec {
        name: "display_name",
        column: Some("display_name"),
        django_type: "CharField",
        max_length: Some(255),
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: Some(ColumnDefault::Text("''")),
    },
    ColumnSpec {
        name: "first_name",
        column: Some("first_name"),
        django_type: "CharField",
        max_length: Some(255),
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "last_name",
        column: Some("last_name"),
        django_type: "CharField",
        max_length: Some(255),
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "avatar",
        column: Some("avatar"),
        django_type: "TextField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "avatar_asset",
        column: Some("avatar_asset_id"),
        django_type: "ForeignKey",
        max_length: None,
        null: true,
        primary_key: false,
        db_index: true,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "cover_image",
        column: Some("cover_image"),
        django_type: "URLField",
        max_length: Some(800),
        null: true,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "cover_image_asset",
        column: Some("cover_image_asset_id"),
        django_type: "ForeignKey",
        max_length: None,
        null: true,
        primary_key: false,
        db_index: true,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "date_joined",
        column: Some("date_joined"),
        django_type: "DateTimeField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "created_at",
        column: Some("created_at"),
        django_type: "DateTimeField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "updated_at",
        column: Some("updated_at"),
        django_type: "DateTimeField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "last_location",
        column: Some("last_location"),
        django_type: "CharField",
        max_length: Some(255),
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "created_location",
        column: Some("created_location"),
        django_type: "CharField",
        max_length: Some(255),
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "is_superuser",
        column: Some("is_superuser"),
        django_type: "BooleanField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: Some(ColumnDefault::Bool(false)),
    },
    ColumnSpec {
        name: "is_managed",
        column: Some("is_managed"),
        django_type: "BooleanField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: Some(ColumnDefault::Bool(false)),
    },
    ColumnSpec {
        name: "is_password_expired",
        column: Some("is_password_expired"),
        django_type: "BooleanField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: Some(ColumnDefault::Bool(false)),
    },
    ColumnSpec {
        name: "is_active",
        column: Some("is_active"),
        django_type: "BooleanField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: Some(ColumnDefault::Bool(true)),
    },
    ColumnSpec {
        name: "is_staff",
        column: Some("is_staff"),
        django_type: "BooleanField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: Some(ColumnDefault::Bool(false)),
    },
    ColumnSpec {
        name: "is_email_verified",
        column: Some("is_email_verified"),
        django_type: "BooleanField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: Some(ColumnDefault::Bool(false)),
    },
    ColumnSpec {
        name: "is_password_autoset",
        column: Some("is_password_autoset"),
        django_type: "BooleanField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: Some(ColumnDefault::Bool(false)),
    },
    ColumnSpec {
        name: "is_password_reset_required",
        column: Some("is_password_reset_required"),
        django_type: "BooleanField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: Some(ColumnDefault::Bool(false)),
    },
    ColumnSpec {
        name: "token",
        column: Some("token"),
        django_type: "CharField",
        max_length: Some(64),
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "last_active",
        column: Some("last_active"),
        django_type: "DateTimeField",
        max_length: None,
        null: true,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "last_login_time",
        column: Some("last_login_time"),
        django_type: "DateTimeField",
        max_length: None,
        null: true,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "last_logout_time",
        column: Some("last_logout_time"),
        django_type: "DateTimeField",
        max_length: None,
        null: true,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "last_login_ip",
        column: Some("last_login_ip"),
        django_type: "CharField",
        max_length: Some(255),
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "last_logout_ip",
        column: Some("last_logout_ip"),
        django_type: "CharField",
        max_length: Some(255),
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "last_login_medium",
        column: Some("last_login_medium"),
        django_type: "CharField",
        max_length: Some(20),
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: Some(ColumnDefault::Text("'email'")),
    },
    ColumnSpec {
        name: "last_login_uagent",
        column: Some("last_login_uagent"),
        django_type: "TextField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "token_updated_at",
        column: Some("token_updated_at"),
        django_type: "DateTimeField",
        max_length: None,
        null: true,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "is_bot",
        column: Some("is_bot"),
        django_type: "BooleanField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: Some(ColumnDefault::Bool(false)),
    },
    ColumnSpec {
        name: "bot_type",
        column: Some("bot_type"),
        django_type: "CharField",
        max_length: Some(30),
        null: true,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "user_timezone",
        column: Some("user_timezone"),
        django_type: "CharField",
        max_length: Some(255),
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: Some(ColumnDefault::Text("'UTC'")),
    },
    ColumnSpec {
        name: "is_email_valid",
        column: Some("is_email_valid"),
        django_type: "BooleanField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: Some(ColumnDefault::Bool(false)),
    },
    ColumnSpec {
        name: "masked_at",
        column: Some("masked_at"),
        django_type: "DateTimeField",
        max_length: None,
        null: true,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "groups",
        column: None,
        django_type: "ManyToManyField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
    ColumnSpec {
        name: "user_permissions",
        column: None,
        django_type: "ManyToManyField",
        max_length: None,
        null: false,
        primary_key: false,
        db_index: false,
        unique: false,
        default: None,
    },
];

// ---------------------------------------------------------------------------
// Session key generation
// ---------------------------------------------------------------------------

/// `VALID_KEY_CHARS` (`session.py:14`): `string.ascii_lowercase +
/// string.digits`, 36 characters.
pub const VALID_KEY_CHARS: &str = "abcdefghijklmnopqrstuvwxyz0123456789";

/// Key length passed to `get_random_string` (`session.py:51`).
pub const SESSION_KEY_LENGTH: usize = 128;

/// Largest byte value admitted by the rejection sampler: 252 is the
/// greatest multiple of 36 not above 256, so `byte % 36` is uniform.
const ADMITTED_BYTE_CEIL: u8 = 252;

/// Draw one [`SESSION_KEY_LENGTH`]-character candidate from
/// [`VALID_KEY_CHARS`], consuming caller-supplied random bytes.
///
/// Python (`session.py:50-53`) picks each character with
/// `SystemRandom().choice`, i.e. uniformly. Bytes map residue-free: values
/// `>= 252` are skipped so `byte % 36` stays uniform, matching that pick.
/// Randomness is a caller argument (this crate has no RNG dependency and no
/// I/O); the queries layer feeds OS bytes and the [`find_new_session_key`]
/// loop.
pub fn generate_session_key(random_bytes: &mut impl Iterator<Item = u8>) -> String {
    let alphabet = VALID_KEY_CHARS.as_bytes();
    let mut key = String::with_capacity(SESSION_KEY_LENGTH);
    for byte in random_bytes {
        if key.len() == SESSION_KEY_LENGTH {
            break;
        }
        if byte < ADMITTED_BYTE_CEIL {
            key.push(alphabet[(byte % 36) as usize] as char);
        }
    }
    key
}

/// `SessionStore._get_new_session_key` (`session.py:46-53`): loop drawing
/// candidates until `exists()` denies one, then return it. The Django
/// `self.exists(session_key)` query becomes the injected predicate; the DB
/// read itself stays with the caller.
pub fn find_new_session_key(
    mut exists: impl FnMut(&str) -> bool,
    random_bytes: &mut impl Iterator<Item = u8>,
) -> String {
    loop {
        let candidate = generate_session_key(random_bytes);
        if !exists(&candidate) {
            return candidate;
        }
    }
}

// ---------------------------------------------------------------------------
// SessionStore.create_model_instance
// ---------------------------------------------------------------------------

/// The unsaved per-request extraction `create_model_instance` computes
/// (`session.py:55-70`): the raw `_auth_user_id` passthrough plus the
/// device-info dict-or-None rule. The caller attaches these to its row.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionInstance {
    /// `data.get("_auth_user_id")`: missing (or JSON null) yields `None`;
    /// any present value passes through unconverted — including numbers
    /// (`user.py` fixture note: an int `5` stays `5`; the CharField
    /// coercion, if any, happens at save, owned by the queries layer).
    /// `ValueError`/`TypeError` from `.get()` likewise yield `None`; with a
    /// real dict input that branch is unreachable, and a non-dict `data`
    /// raises uncaught `AttributeError` in Python — here it is a
    /// compile-time type error, since `data` is `&Map`.
    pub user_id: Option<Value>,
    /// `data.get("device_info")` kept only when it is a dict, else `None`
    /// (`session.py:68-69`).
    pub device_info: Option<Value>,
}

/// `SessionStore.create_model_instance` (`session.py:55-70`) as a pure
/// function over the session-data map.
pub fn create_model_instance(data: &Map<String, Value>) -> SessionInstance {
    let user_id = match data.get("_auth_user_id") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.clone()),
    };
    let device_info = match data.get("device_info") {
        Some(Value::Object(_)) => data.get("device_info").cloned(),
        _ => None,
    };
    SessionInstance {
        user_id,
        device_info,
    }
}

// ---------------------------------------------------------------------------
// Password boundary (F-05 owned)
// ---------------------------------------------------------------------------

/// Hash algorithm recorded for `User.password`
/// (`AbstractBaseUser` + fixture `password.algo`): PBKDF2-SHA256 via
/// Django's hasher, owned by F-05. Pinned here so the replay covers it;
/// never implement hashing in this module.
pub const PASSWORD_HASH_ALGORITHM: &str = "pbkdf2_sha256";

/// Iteration count recorded in the fixture (`password.iterations`).
/// Owned by F-05 / Django settings; pinned, not used, here.
pub const PASSWORD_HASH_ITERATIONS: u32 = 1_500_000;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/auth_session/FX-AUTH-03.models.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    #[test]
    fn table_identity_matches_fixture() {
        let fx = fixture();
        assert_eq!(fx["session"]["db_table"], json!(SESSION_DB_TABLE));
        assert_eq!(fx["user"]["db_table"], json!(USER_DB_TABLE));
        assert_eq!(fx["session"]["django_base"], json!(SESSION_DJANGO_BASE));
        assert_eq!(fx["session"]["store_class"], json!(SESSION_STORE_CLASS));
        assert_eq!(USERNAME_FIELD, "email");
    }

    #[test]
    fn session_columns_match_fixture() {
        let fx = fixture();
        assert_eq!(fx["session"]["columns"], columns_json(SESSION_COLUMNS));
        assert_eq!(SESSION_COLUMNS.len(), 5);
        assert_eq!(fx["session"]["column_names"], json!(SESSION_COLUMN_NAMES));
        assert_eq!(session_column_names(), SESSION_COLUMN_NAMES);
    }

    #[test]
    fn user_auth_columns_match_fixture() {
        let fx = fixture();
        assert_eq!(
            fx["user"]["auth_columns"]["columns"],
            columns_json(USER_AUTH_COLUMNS)
        );
        assert_eq!(USER_AUTH_COLUMNS.len(), 42);
        assert_eq!(fx["user"]["auth_columns"]["db_table"], json!(USER_DB_TABLE));
    }

    #[test]
    fn user_auth_slice_fields() {
        let by_name = |name: &str| {
            USER_AUTH_COLUMNS
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("auth column {name} recorded"))
        };
        // email: unique CharField(255), nullable, no extra validators.
        let email = by_name("email");
        assert_eq!(email.django_type, "CharField");
        assert_eq!(email.max_length, Some(255));
        assert!(email.null && email.unique);
        // password: Django AbstractBaseUser hash field, CharField(128).
        let password = by_name("password");
        assert_eq!(password.django_type, "CharField");
        assert_eq!(password.max_length, Some(128));
        assert!(!password.null && !password.unique);
        // is_password_autoset flag, default false.
        let autoset = by_name("is_password_autoset");
        assert_eq!(autoset.django_type, "BooleanField");
        assert_eq!(autoset.default, Some(ColumnDefault::Bool(false)));
    }

    #[test]
    fn keygen_matches_fixture() {
        let fx = fixture();
        assert_eq!(fx["valid_key_chars"], json!(VALID_KEY_CHARS));
        assert_eq!(VALID_KEY_CHARS.len(), 36);
        assert_eq!(fx["keygen"]["length"], json!(SESSION_KEY_LENGTH));
        assert_eq!(fx["keygen"]["charset_ok"], json!(true));
        // Property replay over a deterministic stream: every char admitted.
        let mut stream = (0u8..=255).cycle();
        let key = generate_session_key(&mut stream);
        assert_eq!(key.len(), SESSION_KEY_LENGTH);
        assert!(key.chars().all(|c| VALID_KEY_CHARS.contains(c)));
    }

    #[test]
    fn keygen_uniqueness_loop_retries() {
        // First candidate collides, second lands: mirrors
        // `if not self.exists(session_key): return session_key`.
        let mut stream = (0u8..=255).cycle();
        let first = generate_session_key(&mut stream);
        let mut calls = 0;
        let key = find_new_session_key(
            |candidate| {
                calls += 1;
                calls == 1 && candidate == first
            },
            &mut (0u8..=255).cycle(),
        );
        assert_eq!(calls, 2);
        assert_eq!(key.len(), SESSION_KEY_LENGTH);
    }

    #[test]
    fn create_model_instance_full_matches_fixture() {
        let fx = fixture();
        // Reconstructed input: the fixture records outputs only (see
        // `create_model_instance_note`); this is the input producing them.
        let mut data = Map::new();
        data.insert("_auth_user_id".to_owned(), json!("abc"));
        data.insert(
            "device_info".to_owned(),
            json!({"user_agent": "UA", "ip_address": "1.1.1.1", "domain": "http://x"}),
        );
        let got = create_model_instance(&data);
        assert_eq!(got.user_id, Some(json!("abc")));
        assert_eq!(
            got.device_info,
            Some(json!({"user_agent": "UA", "ip_address": "1.1.1.1", "domain": "http://x"}))
        );
        assert_eq!(
            got.user_id,
            Some(fx["create_model_instance"]["full"]["user_id"].clone())
        );
        assert_eq!(
            got.device_info,
            Some(fx["create_model_instance"]["full"]["device_info"].clone())
        );
    }

    #[test]
    fn create_model_instance_empty_matches_fixture() {
        let fx = fixture();
        let got = create_model_instance(&Map::new());
        assert_eq!(got.user_id, None);
        assert_eq!(got.device_info, None);
        assert_eq!(
            json!({"user_id": got.user_id, "device_info": got.device_info}),
            fx["create_model_instance"]["empty"]
        );
    }

    #[test]
    fn create_model_instance_nondict_device_info_matches_fixture() {
        let fx = fixture();
        // An int user_id passes through unconverted; a non-dict
        // device_info (here a string; a list behaves the same) is dropped.
        let mut data = Map::new();
        data.insert("_auth_user_id".to_owned(), json!(5));
        data.insert("device_info".to_owned(), json!("not-a-dict"));
        let got = create_model_instance(&data);
        assert_eq!(got.user_id, Some(json!(5)));
        assert_eq!(got.device_info, None);
        assert_eq!(
            json!({"user_id": got.user_id, "device_info": got.device_info}),
            fx["create_model_instance"]["nondict_device_info"]
        );
        let mut listed = Map::new();
        listed.insert("device_info".to_owned(), json!([1, 2]));
        assert_eq!(create_model_instance(&listed).device_info, None);
    }

    #[test]
    fn password_boundary_matches_fixture() {
        let fx = fixture();
        assert_eq!(fx["password"]["algo"], json!(PASSWORD_HASH_ALGORITHM));
        assert_eq!(
            fx["password"]["iterations"]
                .as_str()
                .expect("iterations recorded as string")
                .parse::<u32>()
                .expect("iterations numeric"),
            PASSWORD_HASH_ITERATIONS
        );
        assert_eq!(fx["password"]["check_true"], json!(true));
        assert_eq!(fx["password"]["check_false"], json!(false));
    }
}
