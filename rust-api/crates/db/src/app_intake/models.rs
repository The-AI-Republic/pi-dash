//! Intake app table models (D-32, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/intake.py:1-84` (`Intake`,
//! `SourceType`, `IntakeIssueStatus`, `IntakeIssue`), adopting the
//! Django-owned schema column-for-column; migrations are not ported —
//! Django stays schema owner until switchover.
//!
//! Column order in each `*_COLUMNS` const follows the Django `_meta` field
//! order recorded in `rust-api/fixtures/app_intake/models/*.columns.json`
//! (FK entries use the Django attnames: `project_id`, `workspace_id`,
//! `intake_id`, …). Every application-level default below is Django-side
//! (the live tables carry no `column_default` in `information_schema`, as
//! established for D-01); Rust inserts must supply these values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! Both tables inherit the soft-delete marker (`deleted_at`, from
//! `SoftDeleteModel` in `pi_dash/db/mixins.py:57-69`) and the default
//! manager filters `deleted_at IS NULL` (`objects = SoftDeletionManager`,
//! `mixins.py:56-58`; `all_objects` is the plain unscoped manager,
//! `mixins.py:67`). Every read built from these tables must apply
//! [`crate::soft_delete::active_condition`]; the tests pin this by
//! rendering a scoped `SELECT` per table. The partial unique constraint
//! stays as it is (tombstones are excluded by the `deleted_at IS NULL`
//! condition, so a deleted name can be reused).
//!
//! # Writes backfill the workspace
//!
//! `ProjectBaseModel.save()` (`db/models/project.py:309-311`) sets
//! `workspace` from `project.workspace` on every save: Rust inserts/updates
//! must resolve `workspace_id` from the `project_id` row explicitly. There
//! is no other custom `save()` on either model (the `is_default`
//! exclusivity logic in `project.py:255-299` belongs to `Project.save`,
//! not to `Intake`).
//!
//! # No models-scope ported bugs
//!
//! The recorded intake bugs all live outside this layer: the
//! missing-`None`-check default-intake delete guard
//! (`app/views/intake/base.py:83-85`), the raw-`order_by`-into-`ORDER BY`
//! `FieldError`-as-500 (`base.py:198`), and the falsy-status-CSV skip
//! (`base.py:200-202`) are views-layer behavior owned by
//! PIDASHCONV-314/329/360/385/395. This module ports no behavior that
//! could carry them — struct + column/constraint mapping only.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Django-level FK delete behavior (ORM-emulated; same shape as the
/// D-03 `loop::models::OnDelete`, D-05 `integrations::OnDelete` and D-10
/// `tasks_ticker::models::OnDelete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// Intake source channel (`intake.py:38-39`).
///
/// A single-variant `TextChoices` today (`IN_APP` only); ported as-is so a
/// second channel later is a pure addition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceType {
    /// In-app intake (`IN_APP`).
    InApp,
}

impl SourceType {
    /// The stored string (`TextChoices` value, `intake.py:39`).
    pub fn as_str(self) -> &'static str {
        match self {
            SourceType::InApp => "IN_APP",
        }
    }

    /// All values in declaration order.
    pub const ALL: &[SourceType] = &[SourceType::InApp];
}

/// Error for unknown source-type strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSourceType(pub String);

impl std::fmt::Display for UnknownSourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown intake source type: {}", self.0)
    }
}

impl std::error::Error for UnknownSourceType {}

impl FromStr for SourceType {
    type Err = UnknownSourceType;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "IN_APP" => Ok(SourceType::InApp),
            other => Err(UnknownSourceType(other.to_string())),
        }
    }
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Intake-issue triage state (`intake.py:42-47`).
///
/// Stored as an `IntegerField` (`intake.py:53-62`); ported as-is including
/// the non-zero-based values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntakeIssueStatus {
    /// Awaiting triage (`-2`, the field default).
    Pending,
    /// Rejected (`-1`).
    Rejected,
    /// Snoozed (`0`).
    Snoozed,
    /// Accepted (`1`).
    Accepted,
    /// Duplicate (`2`).
    Duplicate,
}

impl IntakeIssueStatus {
    /// The stored integer (`IntegerChoices` value, `intake.py:43-47`).
    pub fn as_i32(self) -> i32 {
        match self {
            IntakeIssueStatus::Pending => -2,
            IntakeIssueStatus::Rejected => -1,
            IntakeIssueStatus::Snoozed => 0,
            IntakeIssueStatus::Accepted => 1,
            IntakeIssueStatus::Duplicate => 2,
        }
    }

    /// The human label (`choices=` label, `intake.py:54-60`).
    pub fn label(self) -> &'static str {
        match self {
            IntakeIssueStatus::Pending => "Pending",
            IntakeIssueStatus::Rejected => "Rejected",
            IntakeIssueStatus::Snoozed => "Snoozed",
            IntakeIssueStatus::Accepted => "Accepted",
            IntakeIssueStatus::Duplicate => "Duplicate",
        }
    }

    /// Parse a stored integer; `None` for values Django never writes.
    pub fn from_i32(value: i32) -> Option<IntakeIssueStatus> {
        match value {
            -2 => Some(IntakeIssueStatus::Pending),
            -1 => Some(IntakeIssueStatus::Rejected),
            0 => Some(IntakeIssueStatus::Snoozed),
            1 => Some(IntakeIssueStatus::Accepted),
            2 => Some(IntakeIssueStatus::Duplicate),
            _ => None,
        }
    }

    /// All five values in declaration order.
    pub const ALL: &[IntakeIssueStatus] = &[
        IntakeIssueStatus::Pending,
        IntakeIssueStatus::Rejected,
        IntakeIssueStatus::Snoozed,
        IntakeIssueStatus::Accepted,
        IntakeIssueStatus::Duplicate,
    ];
}

/// `intakes` table (`intake.py:12-35`).
pub mod intake {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `intake.py:34`).
    pub const TABLE: &str = "intakes";

    /// Default `ORDER BY` (`Meta.ordering = ("name",)`, `intake.py:35`).
    pub const ORDERING: &str = "name";

    /// `Meta.unique_together` (`intake.py:24`): field names as Django
    /// spells them.
    pub const UNIQUE_TOGETHER: &[&str] = &["name", "project", "deleted_at"];
    /// Partial unique constraint backing the live-name scope
    /// (`intake.py:25-31`).
    pub const UNIQUE_NAME_PROJECT_NAME: &str = "intake_unique_name_project_when_deleted_at_null";
    /// Columns of [`UNIQUE_NAME_PROJECT_NAME`] (`intake.py:27`).
    pub const UNIQUE_NAME_PROJECT_COLUMNS: &[&str] = &["name", "project_id"];
    /// `WHERE` of the partial unique index (`intake.py:28`,
    /// `deleted_at__isnull=True`): the name is unique per project among
    /// live rows only.
    pub const UNIQUE_NAME_PROJECT_WHERE: &str = "deleted_at IS NULL";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/app_intake/models/intake.columns.json`): `BaseModel.id`
    /// (`db/models/base.py:18`), audit cols (`db/mixins.py:18-19,27-40`
    /// and `deleted_at`, `:60-73`), project/workspace FKs
    /// (`ProjectBaseModel`, `db/models/project.py:302-311`), then
    /// `intake.py:13-17`. FK entries use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "name",
        "description",
        "is_default",
        "view_props",
        "logo_props",
    ];

    /// `name` bound (`intake.py:13`, `max_length=255`).
    pub const NAME_MAX_LENGTH: usize = 255;

    /// `is_default` default (`intake.py:15`).
    pub const DEFAULT_IS_DEFAULT: bool = false;

    /// `project` FK: `CASCADE`, non-nullable (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE`, non-nullable (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` / `updated_by` audit FKs: `SET_NULL`, nullable
    /// (from `UserAuditModel`, `db/mixins.py:27-40`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// See [`CREATED_BY_ON_DELETE`].
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One intake row. `description` stores `""`, never `NULL`
    /// (`blank=True` without `null=True`, `intake.py:14`); `view_props`
    /// and `logo_props` store `{}`, never `NULL`
    /// (`JSONField(default=dict)`, `intake.py:16-17`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Intake {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub name: String,
        pub description: String,
        pub is_default: bool,
        pub view_props: serde_json::Value,
        pub logo_props: serde_json::Value,
    }

    impl std::fmt::Display for Intake {
        /// `__str__` (`intake.py:19-21`, `"{name} <{project.name}>"`).
        /// The row carries only `project_id`; the project name is a
        /// queries-layer join, so this renders the id in its place.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} <{}>", self.name, self.project_id)
        }
    }
}

/// `intake_issues` table (`intake.py:50-84`).
pub mod intake_issue {
    use super::{IntakeIssueStatus, OnDelete, SourceType};
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `intake.py:79`).
    pub const TABLE: &str = "intake_issues";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`,
    /// `intake.py:80`).
    pub const ORDERING: &str = "-created_at";

    /// `Meta.unique_together`: none (`intake.py:76-80` declares no
    /// `unique_together` and no `constraints`).
    pub const UNIQUE_TOGETHER: &[&str] = &[];

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/app_intake/models/intake_issue.columns.json`): same
    /// audit/project base as [`super::intake`], then `intake.py:51-74`.
    /// FK entries use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "intake_id",
        "issue_id",
        "status",
        "snoozed_till",
        "duplicate_to_id",
        "source",
        "source_email",
        "external_source",
        "external_id",
        "extra",
    ];

    /// `status` default (`intake.py:61`): pending.
    pub const DEFAULT_STATUS: i32 = -2;
    /// [`DEFAULT_STATUS`] as the enum value.
    pub const DEFAULT_STATUS_ENUM: IntakeIssueStatus = IntakeIssueStatus::Pending;
    /// `source` default (`intake.py:70`, `default="IN_APP"`).
    pub const DEFAULT_SOURCE: &str = "IN_APP";
    /// [`DEFAULT_SOURCE`] as the enum value.
    pub const DEFAULT_SOURCE_ENUM: SourceType = SourceType::InApp;

    /// `source` bound (`intake.py:70`, `max_length=255`).
    pub const SOURCE_MAX_LENGTH: usize = 255;
    /// `external_source` bound (`intake.py:72`, `max_length=255`).
    pub const EXTERNAL_SOURCE_MAX_LENGTH: usize = 255;
    /// `external_id` bound (`intake.py:73`, `max_length=255`).
    pub const EXTERNAL_ID_MAX_LENGTH: usize = 255;

    /// `intake` FK: `CASCADE`, non-nullable (`intake.py:51`).
    pub const INTAKE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `issue` FK: `CASCADE`, non-nullable (`intake.py:52`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `duplicate_to` FK: `SET_NULL`, nullable (`intake.py:64-69`).
    pub const DUPLICATE_TO_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `project` FK: `CASCADE`, non-nullable (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE`, non-nullable (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` / `updated_by` audit FKs: `SET_NULL`, nullable
    /// (from `UserAuditModel`, `db/mixins.py:27-40`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// See [`CREATED_BY_ON_DELETE`].
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One intake-issue row. `status` stores the
    /// [`IntakeIssueStatus`] integer; `source` is nullable with a
    /// Python-level default (`CharField(default="IN_APP", null=True,
    /// blank=True)`, `intake.py:70` — the DB column admits `NULL`);
    /// `source_email` (`TextField`, `:71`), `external_source` (`:72`)
    /// and `external_id` (`:73`) are nullable; `extra` stores `{}`, never
    /// `NULL` (`JSONField(default=dict)`, `:74`). `snoozed_till`
    /// (`DateTimeField(null=True)`, `:63`) declares no `blank=True`
    /// (form-level only, ported as-is).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IntakeIssue {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub intake_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
        pub status: i32,
        pub snoozed_till: Option<chrono::DateTime<chrono::Utc>>,
        pub duplicate_to_id: Option<uuid::Uuid>,
        pub source: Option<String>,
        pub source_email: Option<String>,
        pub external_source: Option<String>,
        pub external_id: Option<String>,
        pub extra: serde_json::Value,
    }

    impl IntakeIssue {
        /// The triage state as the enum; `None` for integers Django
        /// never writes (see [`IntakeIssueStatus::from_i32`]).
        pub fn status_enum(&self) -> Option<IntakeIssueStatus> {
            IntakeIssueStatus::from_i32(self.status)
        }
    }

    impl std::fmt::Display for IntakeIssue {
        /// `__str__` (`intake.py:82-84`,
        /// `"{issue.name} <{intake.name}>"`). The row carries only the FK
        /// ids; the names are queries-layer joins, so this renders the
        /// ids in their place.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} <{}>", self.issue_id, self.intake_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soft_delete::active_condition;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/app_intake/models")
    }

    fn fixture(name: &str) -> serde_json::Value {
        let path = fixtures_dir().join(name);
        let body =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {name}: {e}"));
        serde_json::from_str(&body).expect("fixture is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Physical column names from a fixture `fields` array: the `column`
    /// key when present (FK attnames like `project_id`), else `name`.
    fn fixture_columns(value: &serde_json::Value) -> Vec<String> {
        value["fields"]
            .as_array()
            .expect("fixture has fields array")
            .iter()
            .map(|c| {
                c.get("column")
                    .and_then(|c| c.as_str())
                    .or_else(|| c.get("name").and_then(|n| n.as_str()))
                    .expect("field entry has column or name")
                    .to_string()
            })
            .collect()
    }

    /// Find a fixture field by Django field name or physical column: the
    /// audit-FK entries (`created_by_id`, `updated_by_id`) carry only a
    /// `column` key, no `name`.
    fn entry<'a>(value: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        value["fields"]
            .as_array()
            .expect("fixture has fields array")
            .iter()
            .find(|c| {
                c.get("name").and_then(|n| n.as_str()) == Some(name)
                    || c.get("column").and_then(|n| n.as_str()) == Some(name)
            })
            .unwrap_or_else(|| panic!("fixture has field {name}"))
    }

    fn is_null(value: &serde_json::Value, name: &str) -> bool {
        entry(value, name)["null"]
            .as_bool()
            .unwrap_or_else(|| panic!("field {name} has null flag"))
    }

    fn constraint<'a>(value: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        value["constraints"]
            .as_array()
            .expect("fixture has constraints array")
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("fixture has constraint {name}"))
    }

    /// Map a fixture `unique_together`/constraint field name to its
    /// physical column (FK `project` -> `project_id`, plain `name` ->
    /// `name`).
    fn physical_column(value: &serde_json::Value, field: &str) -> String {
        let found = value["fields"]
            .as_array()
            .expect("fixture has fields array")
            .iter()
            .find(|c| c.get("name").and_then(|n| n.as_str()) == Some(field))
            .unwrap_or_else(|| panic!("fixture has field {field}"));
        found
            .get("column")
            .and_then(|c| c.as_str())
            .or_else(|| found.get("name").and_then(|n| n.as_str()))
            .expect("field entry has column or name")
            .to_string()
    }

    #[test]
    fn intake_columns_match_fixture() {
        let v = fixture("intake.columns.json");
        assert_eq!(owned(intake::COLUMNS), fixture_columns(&v));
        let table: &str = intake::TABLE;
        assert_eq!(table, v["db_table"].as_str().unwrap());
        assert_eq!(table, "intakes");
        let ordering: &str = intake::ORDERING;
        assert_eq!(ordering, v["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "name");
        assert_eq!(v["model"].as_str().unwrap(), "db.Intake");
        assert_eq!(v["verbose_name"].as_str().unwrap(), "Intake");
        assert_eq!(v["verbose_name_plural"].as_str().unwrap(), "Intakes");
        // unique_together in Django field names; constraint in physical cols.
        let together: Vec<String> = owned(intake::UNIQUE_TOGETHER);
        assert_eq!(
            together,
            v["unique_together"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f.as_str().unwrap().to_string())
                .collect::<Vec<String>>()
        );
        let name: &str = intake::UNIQUE_NAME_PROJECT_NAME;
        assert_eq!(name, "intake_unique_name_project_when_deleted_at_null");
        let c = constraint(&v, name);
        let cfields: Vec<String> = c["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| physical_column(&v, f.as_str().unwrap()))
            .collect();
        assert_eq!(owned(intake::UNIQUE_NAME_PROJECT_COLUMNS), cfields);
        assert_eq!(
            c["condition"].as_str().unwrap(),
            intake::UNIQUE_NAME_PROJECT_WHERE
        );
        assert_eq!(intake::UNIQUE_NAME_PROJECT_WHERE, "deleted_at IS NULL");
        assert_eq!(
            v["str"]["format"].as_str().unwrap(),
            "<name> <project.name>"
        );
    }

    #[test]
    fn intake_defaults_match_fixture() {
        let v = fixture("intake.columns.json");
        assert_eq!(entry(&v, "name")["max_length"], 255);
        let name_len: usize = intake::NAME_MAX_LENGTH;
        assert_eq!(name_len, 255);
        assert_eq!(entry(&v, "is_default")["default"], false);
        let is_default: bool = intake::DEFAULT_IS_DEFAULT;
        assert!(!is_default);
        assert_eq!(entry(&v, "view_props")["default"], serde_json::json!({}));
        assert_eq!(entry(&v, "logo_props")["default"], serde_json::json!({}));
        // description: blank but never NULL (no null=True on intake.py:14).
        assert!(entry(&v, "description")["blank"].as_bool().unwrap());
        assert!(!is_null(&v, "description"));
        // Nullability: audit FKs + deleted_at nullable; everything else not.
        for name in ["created_by_id", "updated_by_id", "deleted_at"] {
            assert!(is_null(&v, name), "{name} nullable");
        }
        for name in [
            "id",
            "created_at",
            "updated_at",
            "project",
            "workspace",
            "name",
            "description",
            "is_default",
            "view_props",
            "logo_props",
        ] {
            assert!(!is_null(&v, name), "{name} not null");
        }
        assert_eq!(entry(&v, "project")["on_delete"], "CASCADE");
        assert_eq!(entry(&v, "workspace")["on_delete"], "CASCADE");
        assert_eq!(entry(&v, "created_by_id")["on_delete"], "SET_NULL");
        assert_eq!(entry(&v, "updated_by_id")["on_delete"], "SET_NULL");
        assert_eq!(intake::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(intake::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(intake::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(intake::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn intake_issue_columns_match_fixture() {
        let v = fixture("intake_issue.columns.json");
        assert_eq!(owned(intake_issue::COLUMNS), fixture_columns(&v));
        let table: &str = intake_issue::TABLE;
        assert_eq!(table, v["db_table"].as_str().unwrap());
        assert_eq!(table, "intake_issues");
        let ordering: &str = intake_issue::ORDERING;
        assert_eq!(ordering, v["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        assert_eq!(v["model"].as_str().unwrap(), "db.IntakeIssue");
        assert_eq!(v["verbose_name"].as_str().unwrap(), "IntakeIssue");
        assert_eq!(v["verbose_name_plural"].as_str().unwrap(), "IntakeIssues");
        // No unique_together and no constraints block on this model.
        assert!(v.get("unique_together").is_none());
        assert!(v.get("constraints").is_none());
        let empty: &[&str] = intake_issue::UNIQUE_TOGETHER;
        assert!(empty.is_empty());
        assert_eq!(
            v["str"]["format"].as_str().unwrap(),
            "<issue.name> <intake.name>"
        );
    }

    #[test]
    fn intake_issue_defaults_match_fixture() {
        let v = fixture("intake_issue.columns.json");
        assert_eq!(entry(&v, "status")["default"], -2);
        let status: i32 = intake_issue::DEFAULT_STATUS;
        assert_eq!(status, -2);
        assert_eq!(status, intake_issue::DEFAULT_STATUS_ENUM.as_i32());
        assert_eq!(entry(&v, "source")["default"], "IN_APP");
        let source: &str = intake_issue::DEFAULT_SOURCE;
        assert_eq!(source, "IN_APP");
        assert_eq!(source, intake_issue::DEFAULT_SOURCE_ENUM.as_str());
        assert_eq!(entry(&v, "extra")["default"], serde_json::json!({}));
        assert_eq!(entry(&v, "source")["max_length"], 255);
        assert_eq!(entry(&v, "external_source")["max_length"], 255);
        assert_eq!(entry(&v, "external_id")["max_length"], 255);
        let (s_len, es_len, ei_len): (usize, usize, usize) = (
            intake_issue::SOURCE_MAX_LENGTH,
            intake_issue::EXTERNAL_SOURCE_MAX_LENGTH,
            intake_issue::EXTERNAL_ID_MAX_LENGTH,
        );
        assert_eq!((s_len, es_len, ei_len), (255, 255, 255));
        // status choices enumerate the enum values with Django labels.
        let choices: Vec<(i32, String)> = entry(&v, "status")["choices"]
            .as_array()
            .expect("status has choices")
            .iter()
            .map(|c| {
                (
                    c[0].as_i64().unwrap() as i32,
                    c[1].as_str().unwrap().to_string(),
                )
            })
            .collect();
        let actual: Vec<(i32, String)> = IntakeIssueStatus::ALL
            .iter()
            .map(|s| (s.as_i32(), s.label().to_string()))
            .collect();
        assert_eq!(actual, choices);
        // FK delete behavior.
        assert_eq!(entry(&v, "intake")["on_delete"], "CASCADE");
        assert_eq!(entry(&v, "issue")["on_delete"], "CASCADE");
        assert_eq!(entry(&v, "duplicate_to")["on_delete"], "SET_NULL");
        assert_eq!(entry(&v, "project")["on_delete"], "CASCADE");
        assert_eq!(entry(&v, "workspace")["on_delete"], "CASCADE");
        assert_eq!(intake_issue::INTAKE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(intake_issue::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(intake_issue::DUPLICATE_TO_ON_DELETE, OnDelete::SetNull);
        assert_eq!(intake_issue::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(intake_issue::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(intake_issue::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(intake_issue::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
        // Nullability: audit FKs, deleted_at, snoozed_till, duplicate_to,
        // source family nullable; links and payloads not.
        for name in [
            "created_by_id",
            "updated_by_id",
            "deleted_at",
            "snoozed_till",
            "duplicate_to",
            "source",
            "source_email",
            "external_source",
            "external_id",
        ] {
            assert!(is_null(&v, name), "{name} nullable");
        }
        for name in [
            "id",
            "created_at",
            "updated_at",
            "project",
            "workspace",
            "intake",
            "issue",
            "status",
            "extra",
        ] {
            assert!(!is_null(&v, name), "{name} not null");
        }
    }

    #[test]
    fn enums_match_fixture() {
        let v = fixture("intake_issue.columns.json");
        let status_enum = &v["enums"]["IntakeIssueStatus"];
        for s in IntakeIssueStatus::ALL {
            let key = match s {
                IntakeIssueStatus::Pending => "PENDING",
                IntakeIssueStatus::Rejected => "REJECTED",
                IntakeIssueStatus::Snoozed => "SNOOZED",
                IntakeIssueStatus::Accepted => "ACCEPTED",
                IntakeIssueStatus::Duplicate => "DUPLICATE",
            };
            assert_eq!(status_enum[key].as_i64().unwrap() as i32, s.as_i32());
        }
        assert_eq!(IntakeIssueStatus::ALL.len(), 5);
        let source_enum = &v["enums"]["SourceType"];
        assert_eq!(source_enum["IN_APP"].as_str().unwrap(), "IN_APP");
        assert_eq!(SourceType::ALL.len(), 1);
        assert_eq!(SourceType::ALL[0].as_str(), "IN_APP");
    }

    #[test]
    fn enum_round_trips() {
        for s in IntakeIssueStatus::ALL {
            assert_eq!(IntakeIssueStatus::from_i32(s.as_i32()), Some(*s));
        }
        assert_eq!(IntakeIssueStatus::from_i32(-3), None);
        assert_eq!(IntakeIssueStatus::from_i32(3), None);
        for t in SourceType::ALL {
            assert_eq!(t.to_string(), t.as_str());
            assert_eq!(t.as_str().parse::<SourceType>(), Ok(*t));
        }
        let err = "EMAIL".parse::<SourceType>().unwrap_err();
        assert_eq!(err, UnknownSourceType("EMAIL".to_string()));
    }

    #[test]
    fn reads_are_soft_delete_scoped() {
        for table in [intake::TABLE, intake_issue::TABLE] {
            let mut select = Query::select();
            select
                .column(Alias::new("id"))
                .from(Alias::new(table))
                .cond_where(active_condition());
            let sql = select.to_string(PostgresQueryBuilder);
            assert_eq!(
                sql,
                format!(r#"SELECT "id" FROM "{table}" WHERE "deleted_at" IS NULL"#)
            );
        }
    }

    #[test]
    fn display_matches_python_str() {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap();
        let row = intake::Intake {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            name: "Requests".to_string(),
            description: String::new(),
            is_default: false,
            view_props: serde_json::json!({}),
            logo_props: serde_json::json!({}),
        };
        assert_eq!(row.to_string(), format!("Requests <{}>", uuid::Uuid::nil()));
        let link = intake_issue::IntakeIssue {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            intake_id: uuid::Uuid::nil(),
            issue_id: uuid::Uuid::nil(),
            status: intake_issue::DEFAULT_STATUS,
            snoozed_till: None,
            duplicate_to_id: None,
            source: Some(intake_issue::DEFAULT_SOURCE.to_string()),
            source_email: None,
            external_source: None,
            external_id: None,
            extra: serde_json::json!({}),
        };
        assert_eq!(link.status_enum(), Some(IntakeIssueStatus::Pending));
        assert_eq!(
            link.to_string(),
            format!("{} <{}>", uuid::Uuid::nil(), uuid::Uuid::nil())
        );
    }
}
