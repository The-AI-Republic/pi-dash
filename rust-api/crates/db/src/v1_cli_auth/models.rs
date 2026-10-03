//! `runner` table read model (D-22, V1CLIAUTH-F2).
//!
//! Translation of the model slice PIDASHCONV-533 owns:
//!
//! * `apps/api/pi_dash/runner/models.py:382-498` (`Runner` fields
//!   `:394-476`, `Meta` `:478-498`: `db_table = "runner"` `:479`,
//!   `ordering` `:480`, `UniqueConstraint(pod, name)` `:484-489`,
//!   `indexes` `:490-498`).
//! * `runner/models.py:179-183` (`RunnerStatus`), `:186-187`
//!   (`Visibility`, `PRIVATE = 0`), `:190-204` (`RunnerProvisioning`).
//!
//! Fixture source of truth: `rust-api/fixtures/v1_cli_auth/`
//! `V1CLIAUTH-F2.runner.columns.json` (PIDASHCONV-526); the `#[cfg(test)]`
//! suite asserts these consts equal the fixture column list
//! field-for-field.
//!
//! # Column order
//!
//! [`runner::COLUMNS`] follows Django `_meta` field order (the order
//! recorded in the fixture): `id` first, then the declared fields in
//! source order with Django attnames (`owner` -> `owner_id`,
//! `workspace` -> `workspace_id`, `dev_machine` -> `dev_machine_id`,
//! `pod` -> `pod_id`).
//!
//! # Django-level defaults
//!
//! Every column default below is application-level (Django); the live
//! table carries no `column_default`, so Rust inserts must supply these
//! values explicitly — there is no DB fallback. This module is
//! read-only (the delete path only looks the row up), so defaults are
//! pinned as consts for the row's consumers, not applied here.
//!
//! # Not columns (translate, don't redesign)
//!
//! * `Runner.save()` auto-resolves `pod_id` when `None` (`:503-519`) —
//!   write-path only; the read port does not replicate it.
//! * `project` / `project_id` are `@property` accessors via `pod`
//!   (`:521-536`), not columns.
//! * `revoke()` (`:542+`) is context only — D-13 owns the delete
//!   internals.
//! * `MAX_PER_USER = 5` (`:392`) constrains enrollment, not this read.
//! * Plain `models.Model`: no `deleted_at` column, no soft-delete
//!   manager, no manager exclusions on the lookup path.

/// Django-level FK delete behavior (ORM-emulated where the live FK shows
/// `NO ACTION`, so Rust write paths replicate the behavior explicitly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
    /// `models.PROTECT` — deleting the parent raises instead.
    Protect,
}

/// `runner` table (`runner/models.py:382-498`).
pub mod runner {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`, `:479`).
    pub const TABLE: &str = "runner";

    /// Default `ORDER BY`
    /// (`Meta.ordering = ("-last_heartbeat_at", "-created_at")`, `:480`).
    /// The by-pk lookup port omits it (vacuous on a pk — see
    /// [`crate::v1_cli_auth::queries::runner_lookup`]).
    pub const ORDERING: &[&str] = &["-last_heartbeat_at", "-created_at"];

    /// Columns in Django `_meta` field order (matches V1CLIAUTH-F2).
    /// FK columns use the Django attnames (`owner_id`, `workspace_id`,
    /// `dev_machine_id`, `pod_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "owner_id",
        "workspace_id",
        "dev_machine_id",
        "pod_id",
        "name",
        "host_label",
        "provisioning",
        "visibility",
        "refresh_token_hash",
        "refresh_token_fingerprint",
        "refresh_token_generation",
        "previous_refresh_token_hash",
        "access_token_signing_key_version",
        "enrollment_token_hash",
        "enrollment_token_fingerprint",
        "enrolled_at",
        "capabilities",
        "status",
        "os",
        "arch",
        "runner_version",
        "dev_metadata",
        "protocol_version",
        "last_heartbeat_at",
        "free_worktrees",
        "created_at",
        "updated_at",
        "revoked_at",
        "revoked_reason",
    ];

    /// `id` (`:394`): UUID pk, `default=uuid.uuid4`, `editable=False`.
    /// The declared `db_index=True` on the pk is a no-op.
    pub const ID_IS_PK: bool = true;

    /// `owner` FK (`:395-399`): required `CASCADE` to the auth user model
    /// (`settings.AUTH_USER_MODEL` → `users(id)`),
    /// `related_name="runners"`.
    pub const OWNER_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const OWNER_NULLABLE: bool = false;
    pub const OWNER_REFERENCES: &str = "users(id)";
    pub const OWNER_RELATED_NAME: &str = "runners";

    /// `workspace` FK (`:400-404`): required `CASCADE` to
    /// `db.Workspace` (`workspaces(id)`), `related_name="runners"`.
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_NULLABLE: bool = false;
    pub const WORKSPACE_REFERENCES: &str = "workspaces(id)";
    pub const WORKSPACE_RELATED_NAME: &str = "runners";

    /// `dev_machine` FK (`:405-411`): nullable + blank `SET_NULL` to
    /// `runner.DevMachine` (`dev_machine(id)`),
    /// `related_name="runners"`.
    pub const DEV_MACHINE_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const DEV_MACHINE_NULLABLE: bool = true;
    pub const DEV_MACHINE_BLANK: bool = true;
    pub const DEV_MACHINE_REFERENCES: &str = "dev_machine(id)";
    pub const DEV_MACHINE_RELATED_NAME: &str = "runners";

    /// `pod` FK (`:414-418`): required `PROTECT` to `runner.Pod`
    /// (`pod(id)`), `related_name="runners"`. Pods are soft-deleted, not
    /// physically removed, so this FK is always valid.
    pub const POD_ON_DELETE: OnDelete = OnDelete::Protect;
    pub const POD_NULLABLE: bool = false;
    pub const POD_REFERENCES: &str = "pod(id)";
    pub const POD_RELATED_NAME: &str = "runners";

    /// `name` (`:419`): required, `max_length=128`.
    pub const NAME_MAX_LENGTH: usize = 128;
    pub const NAME_NULLABLE: bool = false;

    /// `host_label` (`:421`): blank-allowed, default `""`,
    /// `max_length=255`.
    pub const HOST_LABEL_MAX_LENGTH: usize = 255;
    pub const HOST_LABEL_BLANK: bool = true;
    pub const DEFAULT_HOST_LABEL: &str = "";

    /// `provisioning` (`:425-430`): `max_length=24`, choices
    /// [`PROVISIONING_CHOICES`], default `MANUAL`, `db_index=True`.
    pub const PROVISIONING_MAX_LENGTH: usize = 24;
    pub const PROVISIONING_CHOICES: &[&str] = &["manual", "desktop_bundled"];
    pub const DEFAULT_PROVISIONING: &str = "manual";
    pub const PROVISIONING_DB_INDEX: bool = true;

    /// `visibility` (`:431-435`): `PositiveSmallIntegerField`, choices
    /// [`VISIBILITY_CHOICES`], default `PRIVATE`, `db_index=True`.
    pub const VISIBILITY_CHOICES: &[(i16, &str)] = &[(0, "Private")];
    pub const VISIBILITY_PRIVATE: i16 = 0;
    pub const DEFAULT_VISIBILITY: i16 = 0;
    pub const VISIBILITY_DB_INDEX: bool = true;

    /// Legacy refresh-token columns (`:439-444`): blank-allowed strings
    /// defaulting to `""` except `refresh_token_generation`
    /// (`PositiveIntegerField`, default `0`); `refresh_token_hash` is
    /// `db_index=True`.
    pub const REFRESH_TOKEN_HASH_MAX_LENGTH: usize = 128;
    pub const REFRESH_TOKEN_HASH_DB_INDEX: bool = true;
    pub const REFRESH_TOKEN_FINGERPRINT_MAX_LENGTH: usize = 16;
    pub const DEFAULT_REFRESH_TOKEN_GENERATION: i32 = 0;
    pub const PREVIOUS_REFRESH_TOKEN_HASH_MAX_LENGTH: usize = 128;

    /// `access_token_signing_key_version` (`:446`):
    /// `PositiveIntegerField`, default `1`.
    pub const DEFAULT_ACCESS_TOKEN_SIGNING_KEY_VERSION: i32 = 1;

    /// One-time enrollment-token columns (`:449-450`): blank-allowed,
    /// default `""`.
    pub const ENROLLMENT_TOKEN_HASH_MAX_LENGTH: usize = 128;
    pub const ENROLLMENT_TOKEN_FINGERPRINT_MAX_LENGTH: usize = 16;

    /// `enrolled_at` (`:451`): nullable + blank `DateTimeField`, no
    /// default (Django default `None`).
    pub const ENROLLED_AT_NULLABLE: bool = true;

    /// `capabilities` (`:452`): `JSONField`, `default=list`, blank-allowed,
    /// `NOT NULL`.
    pub const CAPABILITIES_NULLABLE: bool = false;

    /// `status` (`:453-458`): `max_length=16`, choices
    /// [`STATUS_CHOICES`], default `OFFLINE`, `db_index=True`.
    pub const STATUS_MAX_LENGTH: usize = 16;
    pub const STATUS_CHOICES: &[&str] = &["online", "offline", "busy", "revoked"];
    pub const DEFAULT_STATUS: &str = "offline";
    pub const STATUS_DB_INDEX: bool = true;

    /// `os` / `arch` / `runner_version` (`:459-461`): blank-allowed,
    /// default `""`, `max_length=32`.
    pub const OS_MAX_LENGTH: usize = 32;
    pub const ARCH_MAX_LENGTH: usize = 32;
    pub const RUNNER_VERSION_MAX_LENGTH: usize = 32;

    /// `dev_metadata` (`:464`): `JSONField`, `default=dict`,
    /// blank-allowed, `NOT NULL`.
    pub const DEV_METADATA_NULLABLE: bool = false;

    /// `protocol_version` (`:465`): `PositiveIntegerField`, default `1`.
    pub const DEFAULT_PROTOCOL_VERSION: i32 = 1;

    /// `last_heartbeat_at` (`:466`): nullable + blank `DateTimeField`.
    pub const LAST_HEARTBEAT_AT_NULLABLE: bool = true;

    /// `free_worktrees` (`:472`): nullable + blank `IntegerField`
    /// capacity hint; `None` means the runner predates the feature.
    pub const FREE_WORKTREES_NULLABLE: bool = true;

    /// `created_at` (`:473`) / `updated_at` (`:474`): `auto_now_add` /
    /// `auto_now` — always stamped, never null.
    pub const CREATED_AT_NULLABLE: bool = false;
    pub const UPDATED_AT_NULLABLE: bool = false;

    /// `revoked_at` (`:475`): nullable + blank `DateTimeField`.
    pub const REVOKED_AT_NULLABLE: bool = true;

    /// `revoked_reason` (`:476`): blank-allowed, default `""`,
    /// `max_length=32`.
    pub const REVOKED_REASON_MAX_LENGTH: usize = 32;
    pub const DEFAULT_REVOKED_REASON: &str = "";

    /// Per-pod name uniqueness (`:484-489`): unique btree on
    /// (`pod_id`, `name`).
    pub const UNIQUE_CONSTRAINT_NAME: &str = "runner_unique_name_per_pod";
    pub const UNIQUE_CONSTRAINT_FIELDS: &[&str] = &["pod_id", "name"];

    /// `Meta.indexes` (`:491-497`) in declaration order:
    /// (`owner`, `status`), (`workspace`, `status`),
    /// (`pod`, `status`) named `runner_pod_status_idx`,
    /// (`dev_machine`, `status`) named
    /// `runner_dev_machine_status_idx`. (Field-level `db_index` /
    /// implicit FK btrees above are separate entries in the fixture.)
    pub const META_INDEXES: &[(&str, &[&str])] = &[
        ("<auto>", &["owner_id", "status"]),
        ("<auto>", &["workspace_id", "status"]),
        ("runner_pod_status_idx", &["pod_id", "status"]),
        (
            "runner_dev_machine_status_idx",
            &["dev_machine_id", "status"],
        ),
    ];

    /// Enrollment cap (`MAX_PER_USER`, `:392`): constrains enrollment,
    /// not this read. Pinned so the read model stays complete.
    pub const MAX_PER_USER: u32 = 5;

    /// One `runner` row. Timestamps are `timestamptz`;
    /// `capabilities` / `dev_metadata` are `NOT NULL` JSON columns, so
    /// plain `Value` (no `NULL` state to keep distinct).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Runner {
        pub id: uuid::Uuid,
        pub owner_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub dev_machine_id: Option<uuid::Uuid>,
        pub pod_id: uuid::Uuid,
        pub name: String,
        pub host_label: String,
        pub provisioning: String,
        pub visibility: i16,
        pub refresh_token_hash: String,
        pub refresh_token_fingerprint: String,
        pub refresh_token_generation: i32,
        pub previous_refresh_token_hash: String,
        pub access_token_signing_key_version: i32,
        pub enrollment_token_hash: String,
        pub enrollment_token_fingerprint: String,
        pub enrolled_at: Option<chrono::DateTime<chrono::Utc>>,
        pub capabilities: serde_json::Value,
        pub status: String,
        pub os: String,
        pub arch: String,
        pub runner_version: String,
        pub dev_metadata: serde_json::Value,
        pub protocol_version: i32,
        pub last_heartbeat_at: Option<chrono::DateTime<chrono::Utc>>,
        pub free_worktrees: Option<i32>,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
        pub revoked_reason: String,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support as ts;
    use super::*;

    fn fixture_f2() -> serde_json::Value {
        ts::fixture_file("V1CLIAUTH-F2.runner.columns.json")
    }

    fn column_doc<'a>(f2: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        f2["columns"]
            .as_array()
            .expect("F2 columns is an array")
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("F2 has column {name}"))
    }

    #[test]
    fn f2_table_and_ordering_match_fixture() {
        let f2 = fixture_f2();
        assert_eq!(f2["fixture"].as_str(), Some("V1CLIAUTH-F2"));
        assert_eq!(f2["meta"]["db_table"].as_str(), Some(runner::TABLE));
        assert_eq!(
            f2["meta"]["ordering"],
            serde_json::json!(["-last_heartbeat_at", "-created_at"])
        );
        assert_eq!(runner::ORDERING, &["-last_heartbeat_at", "-created_at"]);
        assert_eq!(f2["column_count"].as_u64(), Some(30));
        assert_eq!(runner::COLUMNS.len(), 30);
    }

    #[test]
    fn f2_columns_byte_exact_in_django_field_order() {
        let f2 = fixture_f2();
        assert_eq!(
            ts::owned_columns(runner::COLUMNS),
            ts::fixture_columns(&f2),
            "F2 columns byte-exact in Django field order",
        );
    }

    #[test]
    fn f2_types_and_nullability_match_fixture() {
        let f2 = fixture_f2();
        // Nullability per column: only the four nullable fields are true.
        for name in runner::COLUMNS {
            let doc = column_doc(&f2, name);
            let nullable = doc["nullable"].as_bool().expect("nullable is bool");
            let expected = matches!(
                *name,
                "dev_machine_id"
                    | "enrolled_at"
                    | "last_heartbeat_at"
                    | "free_worktrees"
                    | "revoked_at"
            );
            assert_eq!(nullable, expected, "nullability of {name}");
        }
        // Spot-check the recorded Django types.
        assert_eq!(
            column_doc(&f2, "id")["type"].as_str(),
            Some("UUIDField(primary_key, db_index)")
        );
        assert_eq!(
            column_doc(&f2, "visibility")["type"].as_str(),
            Some("PositiveSmallIntegerField(db_index)")
        );
        assert_eq!(
            column_doc(&f2, "capabilities")["type"].as_str(),
            Some("JSONField(blank)")
        );
        assert_eq!(
            column_doc(&f2, "created_at")["type"].as_str(),
            Some("DateTimeField(auto_now_add)")
        );
        // FK targets.
        assert_eq!(
            column_doc(&f2, "owner_id")["references"].as_str(),
            Some(runner::OWNER_REFERENCES)
        );
        assert_eq!(
            column_doc(&f2, "workspace_id")["references"].as_str(),
            Some(runner::WORKSPACE_REFERENCES)
        );
        assert_eq!(
            column_doc(&f2, "dev_machine_id")["references"].as_str(),
            Some(runner::DEV_MACHINE_REFERENCES)
        );
        assert_eq!(
            column_doc(&f2, "pod_id")["references"].as_str(),
            Some(runner::POD_REFERENCES)
        );
        assert_eq!(
            column_doc(&f2, "pod_id")["on_delete"].as_str(),
            Some("PROTECT")
        );
        assert_eq!(runner::POD_ON_DELETE, OnDelete::Protect);
        // No deleted_at column anywhere on this path.
        assert!(
            !runner::COLUMNS.contains(&"deleted_at"),
            "plain models.Model: no deleted_at"
        );
    }

    #[test]
    fn f2_defaults_and_choices_match_fixture() {
        let f2 = fixture_f2();
        assert_eq!(
            column_doc(&f2, "provisioning")["default"].as_str(),
            Some("\"manual\"")
        );
        assert_eq!(runner::DEFAULT_PROVISIONING, "manual");
        assert_eq!(column_doc(&f2, "visibility")["default"].as_i64(), Some(0));
        assert_eq!(runner::DEFAULT_VISIBILITY, 0);
        assert_eq!(
            column_doc(&f2, "access_token_signing_key_version")["default"].as_i64(),
            Some(1)
        );
        assert_eq!(runner::DEFAULT_ACCESS_TOKEN_SIGNING_KEY_VERSION, 1);
        assert_eq!(
            column_doc(&f2, "status")["default"].as_str(),
            Some("\"offline\"")
        );
        assert_eq!(runner::DEFAULT_STATUS, "offline");
        assert_eq!(
            column_doc(&f2, "protocol_version")["default"].as_i64(),
            Some(1)
        );
        assert_eq!(runner::DEFAULT_PROTOCOL_VERSION, 1);
        assert_eq!(
            column_doc(&f2, "capabilities")["default"].as_str(),
            Some("[]")
        );
        assert_eq!(
            column_doc(&f2, "dev_metadata")["default"].as_str(),
            Some("{}")
        );
        // Choices.
        assert_eq!(
            f2["choices"]["Visibility"]["values"]["PRIVATE"].as_i64(),
            Some(0)
        );
        assert_eq!(runner::VISIBILITY_PRIVATE, 0);
        assert_eq!(
            f2["choices"]["RunnerStatus"]["values"],
            serde_json::json!({
                "BUSY": "busy",
                "OFFLINE": "offline",
                "ONLINE": "online",
                "REVOKED": "revoked",
            })
        );
        assert_eq!(
            runner::STATUS_CHOICES,
            &["online", "offline", "busy", "revoked"]
        );
        assert_eq!(
            f2["choices"]["RunnerProvisioning"]["values"],
            serde_json::json!({
                "DESKTOP_BUNDLED": "desktop_bundled",
                "MANUAL": "manual",
            })
        );
        assert_eq!(runner::PROVISIONING_CHOICES, &["manual", "desktop_bundled"]);
    }

    #[test]
    fn f2_constraints_and_indexes_match_fixture() {
        let f2 = fixture_f2();
        let constraints = f2["constraints"].as_array().expect("constraints array");
        assert_eq!(constraints.len(), 1);
        assert_eq!(
            constraints[0]["name"].as_str(),
            Some(runner::UNIQUE_CONSTRAINT_NAME)
        );
        assert_eq!(
            constraints[0]["fields"],
            serde_json::json!(["pod_id", "name"])
        );
        assert_eq!(runner::UNIQUE_CONSTRAINT_FIELDS, &["pod_id", "name"]);
        // Meta.indexes entries are the last four fixture index rows.
        let indexes = f2["indexes"].as_array().expect("indexes array");
        assert_eq!(indexes.len(), 13);
        for (i, (name, cols)) in runner::META_INDEXES.iter().enumerate() {
            let row = &indexes[9 + i];
            assert_eq!(row["name"].as_str(), Some(*name), "meta index {i} name");
            let row_cols: Vec<&str> = row["columns"]
                .as_array()
                .expect("index columns array")
                .iter()
                .map(|v| v.as_str().expect("index column is a string"))
                .collect();
            assert_eq!(&row_cols, cols, "meta index {i} columns");
        }
    }
}
