//! License / instance-console table models (D-01, stage 3).
//!
//! Ports `apps/api/pi_dash/license/models/instance.py:1-100`
//! (`Instance`, `InstanceAdmin`, `InstanceConfiguration`, `ChangeLog`) plus
//! `InstanceEdition` (`:18-19`) and `ROLE_CHOICES` (`:15`), adopting the
//! Django-owned schema column-for-column. Migrations are not ported; Django
//! stays schema owner until switchover.
//!
//! Column order in each `*_COLUMNS` const follows Django `_meta` field order
//! (the order recorded in
//! `rust-api/fixtures/license/models/*.columns.json`), **not** physical
//! `information_schema` ordinal order, which reflects migration history
//! (e.g. live `instances` starts `created_at, updated_at, id, ...` with
//! `domain`, `latest_version`, `edition` added later; verified 2026-09-27
//! against the `contract98` scratch database: all four tables match the
//! fixtures as column sets exactly). Order is cosmetic for query building;
//! membership is the contract.
//!
//! # ChangeLog: table only, no readers
//!
//! No view, serializer, or query under `apps/api/pi_dash/license/` touches
//! the `changelogs` table (the single `changelog` string in
//! `api/views/instance.py:166` is the `INSTANCE_CHANGELOG_URL` settings URL,
//! not the table). This module therefore ports the table shape only; there
//! is no read/write path for it here. The queries layer (PIDASHCONV-117)
//! must not add one without a new issue.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * `license/models/__init__.py:1-5` re-exports `Instance`,
//!   `InstanceAdmin`, `InstanceConfiguration`, and `InstanceEdition` only —
//!   `ChangeLog` is **not** importable via the `pi_dash.license.models`
//!   package (only via `pi_dash.license.models.instance`). Mirrored here:
//!   `license/mod.rs` re-exports everything except `ChangeLog`, which stays
//!   reachable only through the full `pidash_db::license::models` path.
//! * Every column default below is application-level (Django); the live
//!   tables carry **no** `column_default` in `information_schema`, so Rust
//!   inserts must supply these values explicitly — there is no DB fallback.
//! * `domain` (`blank=True`, **not** null) and ChangeLog `description`
//!   (`blank=True`, **not** null) store `""`, never `NULL`: they are
//!   `String`, not `Option<String>`. Conversely `value` (`null=True`,
//!   `blank=True`, `default=None`) keeps `NULL` and `""` as distinct stored
//!   states, and `whitelist_emails` / `latest_version` / `namespace` allow
//!   both.
//! * `role` is a plain `integer` at DB level with no `CHECK` constraint;
//!   `choices=((20, "Admin"),)` is enforced application-side only.
//! * `unique_together = ["instance", "user"]` is a plain unique index
//!   (`instance_admins_instance_id_user_id_2e80a466_uniq`), enforced
//!   soft-delete-unaware: soft-deleted rows still collide.
//! * FK `on_delete` (`SET_NULL` for `user`, `CASCADE` for `instance`) is
//!   ORM-emulated — the live FKs show `NO ACTION` — so Rust write paths
//!   (PIDASHCONV-117) must replicate the nulling/cascading explicitly.
//! * `tags` uses `JSONField(default=list)`: the callable yields a fresh `[]`
//!   per row. [`default_tags`] returns a new empty array on every call.
//!
//! Wiring note: the crate root declares `pub mod license;` (foundation
//! change, tracked separately); these files are new-files-only for this
//! issue.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Edition choices for [`Instance::edition`]
/// (`instance.py:18-19`, ported as-is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum InstanceEdition {
    /// The only edition Django knows today.
    #[default]
    PiDashCommunity,
}

impl InstanceEdition {
    /// The stored string (`InstanceEdition.PI_DASH_COMMUNITY.value`).
    pub const AS_STR: &'static str = "PI_DASH_COMMUNITY";

    /// Render the stored string.
    pub fn as_str(self) -> &'static str {
        Self::AS_STR
    }
}

impl std::fmt::Display for InstanceEdition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error for unknown edition strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownEdition(pub String);

impl std::fmt::Display for UnknownEdition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown instance edition: {}", self.0)
    }
}

impl std::error::Error for UnknownEdition {}

impl FromStr for InstanceEdition {
    type Err = UnknownEdition;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PI_DASH_COMMUNITY" => Ok(InstanceEdition::PiDashCommunity),
            other => Err(UnknownEdition(other.to_string())),
        }
    }
}

/// Role choices for [`InstanceAdmin::role`] (`instance.py:15`, ported as-is).
pub const ROLE_CHOICES: &[(i32, &str)] = &[(20, "Admin")];

/// Default `role` (`instance.py:61`, `default=20`).
pub const DEFAULT_ROLE: i32 = 20;

/// The single known role value.
pub const ADMIN_ROLE: i32 = 20;

/// Django-level FK delete behavior (ORM-emulated; see module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// Fresh default for ChangeLog `tags` (`JSONField(default=list)`).
pub fn default_tags() -> serde_json::Value {
    serde_json::Value::Array(Vec::new())
}

/// `instances` table (`instance.py:22-50`, `db_table = "instances"`).
pub mod instance {
    use super::{InstanceEdition, OnDelete};
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "instances";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/license/models/instance.columns.json`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
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

    /// `instance_id` carries a column-level `UNIQUE` (`:26`).
    pub const UNIQUE_COLUMNS: &[&str] = &["instance_id"];

    /// Default `edition` (`:29`, `InstanceEdition.PI_DASH_COMMUNITY.value`).
    pub const DEFAULT_EDITION: InstanceEdition = InstanceEdition::PiDashCommunity;

    /// Boolean defaults (`:35-44`).
    pub const DEFAULT_IS_TELEMETRY_ENABLED: bool = true;
    pub const DEFAULT_IS_SUPPORT_REQUIRED: bool = true;
    pub const DEFAULT_IS_SETUP_DONE: bool = false;
    pub const DEFAULT_IS_SIGNUP_SCREEN_VISITED: bool = false;
    pub const DEFAULT_IS_VERIFIED: bool = false;
    pub const DEFAULT_IS_TEST: bool = false;
    pub const DEFAULT_IS_CURRENT_VERSION_DEPRECATED: bool = false;

    /// Audit FK delete behavior (`db/mixins.py:26-38`, `SET_NULL`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One `Instance` row. Timestamps are `timestamptz`; `domain` is
    /// `NOT NULL` (blank string when unset); `latest_version`,
    /// `whitelist_emails`, and `namespace` are nullable.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Instance {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub instance_name: String,
        pub whitelist_emails: Option<String>,
        pub instance_id: String,
        pub current_version: String,
        pub latest_version: Option<String>,
        pub edition: String,
        pub domain: String,
        pub last_checked_at: chrono::DateTime<chrono::Utc>,
        pub namespace: Option<String>,
        pub is_telemetry_enabled: bool,
        pub is_support_required: bool,
        pub is_setup_done: bool,
        pub is_signup_screen_visited: bool,
        pub is_verified: bool,
        pub is_test: bool,
        pub is_current_version_deprecated: bool,
    }
}

/// `instance_admins` table (`instance.py:53-69`).
pub mod instance_admin {
    use super::{OnDelete, DEFAULT_ROLE};
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "instance_admins";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/license/models/instance_admin.columns.json`).
    /// FK columns use the Django attnames (`user_id`, `instance_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "user_id",
        "instance_id",
        "role",
        "is_verified",
    ];

    /// `unique_together = ["instance", "user"]` (`:65`): the live unique
    /// index is `(instance_id, user_id)`, enforced soft-delete-unaware.
    pub const UNIQUE_TOGETHER: &[&[&str]] = &[&["instance_id", "user_id"]];

    /// Default `role` (`:61`).
    pub const DEFAULT_ROLE_VALUE: i32 = DEFAULT_ROLE;

    /// Default `is_verified` (`:62`).
    pub const DEFAULT_IS_VERIFIED: bool = false;

    /// `user` FK: `SET_NULL`, nullable, `related_name="instance_owner"`
    /// (`:54-59`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const USER_NULLABLE: bool = true;
    pub const USER_RELATED_NAME: &str = "instance_owner";

    /// `instance` FK: `CASCADE`, `related_name="admins"` (`:60`).
    pub const INSTANCE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const INSTANCE_RELATED_NAME: &str = "admins";

    /// One `InstanceAdmin` row. `user_id` is nullable (`SET_NULL`);
    /// `instance_id` is required (`CASCADE`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct InstanceAdmin {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub user_id: Option<uuid::Uuid>,
        pub instance_id: uuid::Uuid,
        pub role: i32,
        pub is_verified: bool,
    }
}

/// `instance_configurations` table (`instance.py:72-83`).
pub mod instance_configuration {
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "instance_configurations";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/license/models/instance_configuration.columns.json`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "key",
        "value",
        "category",
        "is_encrypted",
    ];

    /// `key` is globally unique (`:74`, `max_length=100`).
    pub const UNIQUE_COLUMNS: &[&str] = &["key"];
    pub const KEY_MAX_LENGTH: usize = 100;

    /// `value` default (`:75`, `null=True, blank=True, default=None`).
    pub const DEFAULT_VALUE: Option<String> = None;

    /// Default `is_encrypted` (`:77`).
    pub const DEFAULT_IS_ENCRYPTED: bool = false;

    /// One `InstanceConfiguration` row. `value` keeps `NULL` and `""` as
    /// distinct stored states.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct InstanceConfiguration {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub key: String,
        pub value: Option<String>,
        pub category: String,
        pub is_encrypted: bool,
    }
}

/// `changelogs` table (`instance.py:86-100`).
///
/// Table shape only: no view, serializer, or query under
/// `apps/api/pi_dash/license/` reads this table (see module docs).
pub mod changelog {
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "changelogs";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/license/models/changelog.columns.json`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "title",
        "description",
        "version",
        "tags",
        "release_date",
        "is_release_candidate",
    ];

    /// Default `is_release_candidate` (`:94`).
    pub const DEFAULT_IS_RELEASE_CANDIDATE: bool = false;

    /// One `ChangeLog` row. `description` is `NOT NULL` (blank string when
    /// unset); `tags` is `jsonb NOT NULL` defaulting to `[]` (see
    /// [`super::default_tags`]); `release_date` is nullable.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ChangeLog {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub title: String,
        pub description: String,
        pub version: String,
        pub tags: serde_json::Value,
        pub release_date: Option<chrono::DateTime<chrono::Utc>>,
        pub is_release_candidate: bool,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_column_names(path: &str) -> Vec<String> {
        let body =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read fixture {path}: {e}"));
        let v: serde_json::Value = serde_json::from_str(&body).expect("fixture is valid JSON");
        v["columns"]
            .as_array()
            .expect("fixture has columns array")
            .iter()
            .map(|c| c["name"].as_str().expect("column has a name").to_string())
            .collect()
    }

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/license/models")
    }

    /// Copy a column const into an owned vec so assertions compare two
    /// runtime values (clippy denies asserting on constants directly).
    fn owned_columns(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    fn fixture_columns(name: &str) -> Vec<String> {
        let path = fixtures_dir().join(name);
        fixture_column_names(path.to_str().unwrap())
    }

    #[test]
    fn instance_columns_match_fixture() {
        let expected = fixture_columns("instance.columns.json");
        let actual = owned_columns(instance::COLUMNS);
        assert_eq!(actual, expected);
        let table: &str = instance::TABLE;
        assert_eq!(table, "instances");
        let ordering: &str = instance::ORDERING;
        assert_eq!(ordering, "-created_at");
        let unique = owned_columns(instance::UNIQUE_COLUMNS);
        assert_eq!(unique, vec!["instance_id".to_string()]);
    }

    #[test]
    fn instance_admin_columns_match_fixture() {
        let expected = fixture_columns("instance_admin.columns.json");
        let actual = owned_columns(instance_admin::COLUMNS);
        assert_eq!(actual, expected);
        let table: &str = instance_admin::TABLE;
        assert_eq!(table, "instance_admins");
        let ordering: &str = instance_admin::ORDERING;
        assert_eq!(ordering, "-created_at");
        let unique: Vec<Vec<String>> = instance_admin::UNIQUE_TOGETHER
            .iter()
            .map(|cols| owned_columns(cols))
            .collect();
        assert_eq!(unique, vec![vec!["instance_id", "user_id"]]);
    }

    #[test]
    fn instance_configuration_columns_match_fixture() {
        let expected = fixture_columns("instance_configuration.columns.json");
        let actual = owned_columns(instance_configuration::COLUMNS);
        assert_eq!(actual, expected);
        let table: &str = instance_configuration::TABLE;
        assert_eq!(table, "instance_configurations");
        let ordering: &str = instance_configuration::ORDERING;
        assert_eq!(ordering, "-created_at");
        let unique = owned_columns(instance_configuration::UNIQUE_COLUMNS);
        assert_eq!(unique, vec!["key".to_string()]);
        let max_len: usize = instance_configuration::KEY_MAX_LENGTH;
        assert_eq!(max_len, 100);
    }

    #[test]
    fn changelog_columns_match_fixture() {
        let expected = fixture_columns("changelog.columns.json");
        let actual = owned_columns(changelog::COLUMNS);
        assert_eq!(actual, expected);
        let table: &str = changelog::TABLE;
        assert_eq!(table, "changelogs");
        let ordering: &str = changelog::ORDERING;
        assert_eq!(ordering, "-created_at");
    }

    #[test]
    fn edition_round_trips() {
        assert_eq!(InstanceEdition::default(), InstanceEdition::PiDashCommunity);
        assert_eq!(InstanceEdition::default().as_str(), "PI_DASH_COMMUNITY");
        assert_eq!(
            "PI_DASH_COMMUNITY".parse::<InstanceEdition>().unwrap(),
            InstanceEdition::PiDashCommunity
        );
        assert!("ENTERPRISE".parse::<InstanceEdition>().is_err());
        let default_edition = instance::DEFAULT_EDITION;
        assert_eq!(default_edition.as_str(), "PI_DASH_COMMUNITY");
    }

    #[test]
    fn role_choices_ported_as_is() {
        let choices: &[(i32, &str)] = ROLE_CHOICES;
        assert_eq!(choices, &[(20, "Admin")]);
        let default_role: i32 = DEFAULT_ROLE;
        assert_eq!(default_role, 20);
        let admin_role: i32 = ADMIN_ROLE;
        assert_eq!(admin_role, 20);
        let admin_default: i32 = instance_admin::DEFAULT_ROLE_VALUE;
        assert_eq!(admin_default, 20);
        let verified: bool = instance_admin::DEFAULT_IS_VERIFIED;
        assert!(!verified);
    }

    #[test]
    fn django_defaults_match_python() {
        let telemetry: bool = instance::DEFAULT_IS_TELEMETRY_ENABLED;
        assert!(telemetry);
        let support: bool = instance::DEFAULT_IS_SUPPORT_REQUIRED;
        assert!(support);
        let setup: bool = instance::DEFAULT_IS_SETUP_DONE;
        assert!(!setup);
        let visited: bool = instance::DEFAULT_IS_SIGNUP_SCREEN_VISITED;
        assert!(!visited);
        let verified: bool = instance::DEFAULT_IS_VERIFIED;
        assert!(!verified);
        let test: bool = instance::DEFAULT_IS_TEST;
        assert!(!test);
        let deprecated: bool = instance::DEFAULT_IS_CURRENT_VERSION_DEPRECATED;
        assert!(!deprecated);
        let value: Option<String> = instance_configuration::DEFAULT_VALUE;
        assert_eq!(value, None);
        let encrypted: bool = instance_configuration::DEFAULT_IS_ENCRYPTED;
        assert!(!encrypted);
        let rc: bool = changelog::DEFAULT_IS_RELEASE_CANDIDATE;
        assert!(!rc);
    }

    #[test]
    fn fk_semantics_match_python() {
        let user_delete: OnDelete = instance_admin::USER_ON_DELETE;
        assert_eq!(user_delete, OnDelete::SetNull);
        let user_nullable: bool = instance_admin::USER_NULLABLE;
        assert!(user_nullable);
        let user_related: &str = instance_admin::USER_RELATED_NAME;
        assert_eq!(user_related, "instance_owner");
        let instance_delete: OnDelete = instance_admin::INSTANCE_ON_DELETE;
        assert_eq!(instance_delete, OnDelete::Cascade);
        let instance_related: &str = instance_admin::INSTANCE_RELATED_NAME;
        assert_eq!(instance_related, "admins");
        let created_by: OnDelete = instance::CREATED_BY_ON_DELETE;
        assert_eq!(created_by, OnDelete::SetNull);
        let updated_by: OnDelete = instance::UPDATED_BY_ON_DELETE;
        assert_eq!(updated_by, OnDelete::SetNull);
    }

    #[test]
    fn tags_default_is_a_fresh_empty_array() {
        assert_eq!(default_tags(), serde_json::Value::Array(vec![]));
    }

    #[test]
    fn changelog_row_serde_round_trip() {
        let row = changelog::ChangeLog {
            id: uuid::Uuid::nil(),
            created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            updated_at: chrono::DateTime::from_timestamp(1_700_000_001, 0).unwrap(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            title: "v1".to_string(),
            description: String::new(),
            version: "1.0.0".to_string(),
            tags: default_tags(),
            release_date: None,
            is_release_candidate: false,
        };
        let v = serde_json::to_value(&row).unwrap();
        assert_eq!(v["tags"], serde_json::Value::Array(vec![]));
        assert_eq!(v["release_date"], serde_json::Value::Null);
        assert_eq!(v["description"], serde_json::Value::String(String::new()));
        let back: changelog::ChangeLog = serde_json::from_value(v).unwrap();
        assert_eq!(back, row);
    }
}
