//! Loop (auto-PM) table models (D-03, stage 4).
//!
//! Ports `apps/api/pi_dash/db/models/loop.py:1-183` (`SkipReason`,
//! `LoopJob`, `LoopTarget`, `LoopUserPreference`), adopting the
//! Django-owned schema column-for-column; migrations are not ported —
//! Django stays schema owner until switchover.
//!
//! Column order in each `*_COLUMNS` const follows the Django `_meta` field
//! order recorded in `rust-api/fixtures/loop/models/*.columns.json`
//! (FK entries use the Django attnames: `job_id`, `workspace_id`, …).
//! Every application-level default below is Django-side (the live tables
//! carry no `column_default` in `information_schema`, as established for
//! D-01); Rust inserts must supply these values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! All three tables inherit the soft-delete marker (`deleted_at`, from
//! `SoftDeleteModel` in `pi_dash/db/mixins.py:57-69`) and the default
//! manager filters `deleted_at IS NULL`. Every read built from these
//! tables must apply [`crate::soft_delete::active_condition`] (or go
//! through the per-table read view); the tests pin this by rendering a
//! scoped `SELECT` per table. Partial unique constraints stay as they are
//! (tombstones are excluded by the `deleted_at IS NULL` condition, so
//! uninstall/reinstall does not collide).
//!
//! # No models-scope ported bugs
//!
//! The two known loop bugs both live outside this layer: BUG-LOOP-1
//! (`min_role` `ValueError` on a non-integer role) is raised by the guard
//! layer (`_validate_writes`), and BUG-LOOP-2 (dispatch returning the
//! exception object) lives in `loop/dispatch.py`. This module ports no
//! behavior that could carry them — struct + column/constraint mapping
//! only.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Django-level FK delete behavior (ORM-emulated; same shape as the
/// D-05 `integrations::OnDelete` and D-10 `tasks_ticker::models::OnDelete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// Why a due target was skipped instead of dispatched (`loop.py:25-39`).
///
/// Stored sparsely on [`loop_target::LoopTarget::last_skip_reason`]
/// (overwritten in place, never accreted as a log; design §6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkipReason {
    /// The user disabled this job.
    UserDisabled,
    /// The user paused all Auto PM (master switch).
    MasterPaused,
    /// Below the job's minimum role.
    MinRole,
    /// No usable LLM credentials.
    LlmConfigMissing,
    /// No active workspace membership.
    MembershipGone,
    /// Previous run still in flight.
    TurnActive,
    /// Unexpected error creating the turn.
    DispatchError,
}

impl SkipReason {
    /// The stored string (`TextChoices` value, `loop.py:31-37`).
    pub fn as_str(self) -> &'static str {
        match self {
            SkipReason::UserDisabled => "user_disabled",
            SkipReason::MasterPaused => "master_paused",
            SkipReason::MinRole => "min_role",
            SkipReason::LlmConfigMissing => "llm_config_missing",
            SkipReason::MembershipGone => "membership_gone",
            SkipReason::TurnActive => "turn_active",
            SkipReason::DispatchError => "dispatch_error",
        }
    }

    /// The human label (`TextChoices` label, `loop.py:31-37`).
    pub fn label(self) -> &'static str {
        match self {
            SkipReason::UserDisabled => "User disabled this job",
            SkipReason::MasterPaused => "User paused all Auto PM",
            SkipReason::MinRole => "Below the job's minimum role",
            SkipReason::LlmConfigMissing => "No usable LLM credentials",
            SkipReason::MembershipGone => "No active workspace membership",
            SkipReason::TurnActive => "Previous run still in flight",
            SkipReason::DispatchError => "Unexpected error creating the turn",
        }
    }

    /// All seven values in declaration order (matches `skip_reason.json`).
    pub const ALL: &[SkipReason] = &[
        SkipReason::UserDisabled,
        SkipReason::MasterPaused,
        SkipReason::MinRole,
        SkipReason::LlmConfigMissing,
        SkipReason::MembershipGone,
        SkipReason::TurnActive,
        SkipReason::DispatchError,
    ];
}

/// Error for unknown skip-reason strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSkipReason(pub String);

impl std::fmt::Display for UnknownSkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown loop skip reason: {}", self.0)
    }
}

impl std::error::Error for UnknownSkipReason {}

impl FromStr for SkipReason {
    type Err = UnknownSkipReason;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user_disabled" => Ok(SkipReason::UserDisabled),
            "master_paused" => Ok(SkipReason::MasterPaused),
            "min_role" => Ok(SkipReason::MinRole),
            "llm_config_missing" => Ok(SkipReason::LlmConfigMissing),
            "membership_gone" => Ok(SkipReason::MembershipGone),
            "turn_active" => Ok(SkipReason::TurnActive),
            "dispatch_error" => Ok(SkipReason::DispatchError),
            other => Err(UnknownSkipReason(other.to_string())),
        }
    }
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `loop_jobs` table (`loop.py:41-81`).
///
/// An instance catalog entry: a prompt + a recurrence. Instance-scoped (no
/// workspace FK), so the operator defines a job once and it auto-applies
/// to every membership edge via reconcile (design §7.2).
pub mod loop_job {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "loop_jobs";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/loop/models/loop_job.columns.json`).
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "slug",
        "name",
        "public_name",
        "public_description",
        "prompt",
        "min_role",
        "enabled",
        "is_builtin",
        "dtstart",
        "rrule",
        "tzid",
    ];

    /// Partial unique `slug` when active (`Meta.constraints`, `:72-78`;
    /// tombstones excluded so a deleted slug can be reused).
    pub const UNIQUE_WHEN_ACTIVE: &[&[&str]] = &[&["slug"]];
    /// Name of the partial unique constraint.
    pub const UNIQUE_WHEN_ACTIVE_NAME: &str = "loop_job_unique_slug_when_active";
    /// The partial condition, as Django spells it
    /// (`deleted_at__isnull=True`).
    pub const UNIQUE_WHEN_ACTIVE_CONDITION: &str = "deleted_at IS NULL";

    /// `slug` bound (`:51`, `max_length=64`).
    pub const SLUG_MAX_LENGTH: usize = 64;
    /// `name` bound (`:52`, `max_length=255`).
    pub const NAME_MAX_LENGTH: usize = 255;
    /// `public_name` bound (`:53`, `max_length=255`).
    pub const PUBLIC_NAME_MAX_LENGTH: usize = 255;
    /// `rrule` bound (`:64`, `max_length=255`).
    pub const RRULE_MAX_LENGTH: usize = 255;
    /// `tzid` bound (`:65`, `max_length=64`).
    pub const TZID_MAX_LENGTH: usize = 64;

    /// `public_description` default (`:54`, `blank=True`, `default=""`).
    pub const DEFAULT_PUBLIC_DESCRIPTION: &str = "";
    /// `min_role` default (`:56`): member. `ROLE_CHOICES` (from
    /// `db/models/workspace.py`): 20 admin / 15 member / 5 guest.
    pub const DEFAULT_MIN_ROLE: i16 = 15;
    /// `enabled` default (`:57`).
    pub const DEFAULT_ENABLED: bool = true;
    /// `is_builtin` default (`:58`).
    pub const DEFAULT_IS_BUILTIN: bool = true;
    /// `tzid` default (`:65`).
    pub const DEFAULT_TZID: &str = "UTC";

    /// `created_by` / `updated_by` audit FKs: `SET_NULL`, nullable
    /// (from `UserAuditModel`, `db/mixins.py:26-38`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// See [`CREATED_BY_ON_DELETE`].
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One loop job row. `public_description` / `rrule` / `tzid` store
    /// `""`-style values, never `NULL`; only the audit FKs and
    /// `deleted_at` are nullable besides them. `rrule` may not be empty
    /// (a single-shot loop job is meaningless; enforced by the guard
    /// layer, PIDASHCONV-155).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct LoopJob {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub slug: String,
        pub name: String,
        pub public_name: String,
        pub public_description: String,
        pub prompt: String,
        pub min_role: i16,
        pub enabled: bool,
        pub is_builtin: bool,
        pub dtstart: chrono::DateTime<chrono::Utc>,
        pub rrule: String,
        pub tzid: String,
    }

    impl std::fmt::Display for LoopJob {
        /// `__str__` (`:80-81`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "LoopJob({})", self.slug)
        }
    }
}

/// `loop_targets` table (`loop.py:84-140`).
///
/// The cursor for one (job × workspace × user) membership edge. There is
/// intentionally no `LoopRun` model — the run *is* the `AssistantTurn`
/// referenced by [`LoopTarget::last_run_id`].
pub mod loop_target {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "loop_targets";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/loop/models/loop_target.columns.json`). FK columns use
    /// the Django attnames (`job_id`, …).
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "job_id",
        "workspace_id",
        "user_id",
        "thread_id",
        "next_run_at",
        "last_run_id",
        "last_skipped_at",
        "last_skip_reason",
    ];

    /// Partial unique (job, workspace, user) when active
    /// (`Meta.constraints`, `:130-136`).
    pub const UNIQUE_WHEN_ACTIVE: &[&[&str]] = &[&["job_id", "workspace_id", "user_id"]];
    /// Name of the partial unique constraint.
    pub const UNIQUE_WHEN_ACTIVE_NAME: &str = "loop_target_unique_edge_when_active";
    /// The partial condition, as Django spells it
    /// (`deleted_at__isnull=True`).
    pub const UNIQUE_WHEN_ACTIVE_CONDITION: &str = "deleted_at IS NULL";

    /// Due-scan index (`Meta.indexes`, `:137`).
    pub const DUE_INDEX_NAME: &str = "loop_target_due_idx";
    /// Columns of [`DUE_INDEX_NAME`].
    pub const DUE_INDEX: &[&str] = &["next_run_at"];

    /// `last_skip_reason` default (`:115-117`, `blank=True`, `default=""`;
    /// empty means "never skipped").
    pub const DEFAULT_LAST_SKIP_REASON: &str = "";
    /// `last_skip_reason` bound (`:116`, `max_length=64`).
    pub const LAST_SKIP_REASON_MAX_LENGTH: usize = 64;

    /// `job` FK: `CASCADE`, non-nullable (`:92`).
    pub const JOB_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE`, non-nullable (`:93-95`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `user` FK: `CASCADE`, non-nullable (`:96-98`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `thread` FK: `SET_NULL`, nullable (`:103-109`) — deleting a thread
    /// can't kill the cursor.
    pub const THREAD_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `last_run` FK: `SET_NULL`, nullable (`:114-119`).
    pub const LAST_RUN_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One loop target row. `next_run_at = NULL` means newly created with
    /// stagger pending (treated as due by the scanner). `last_skip_reason`
    /// stores `""`, never `NULL`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct LoopTarget {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub job_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub user_id: uuid::Uuid,
        pub thread_id: Option<uuid::Uuid>,
        pub next_run_at: Option<chrono::DateTime<chrono::Utc>>,
        pub last_run_id: Option<uuid::Uuid>,
        pub last_skipped_at: Option<chrono::DateTime<chrono::Utc>>,
        pub last_skip_reason: String,
    }

    impl std::fmt::Display for LoopTarget {
        /// `__str__` (`:139-140`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "LoopTarget(job={}, ws={}, user={})",
                self.job_id, self.workspace_id, self.user_id
            )
        }
    }
}

/// `loop_user_preferences` table (`loop.py:143-182`).
///
/// A user's opt-out for a job (or the master "pause all" switch when
/// `job_id` is NULL). Absence of a row means enabled — only opt-outs are
/// stored, which is what makes new builtin jobs light up with zero
/// backfill (design §6.3).
pub mod loop_user_preference {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "loop_user_preferences";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/loop/models/loop_user_preference.columns.json`).
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "user_id",
        "job_id",
        "enabled",
    ];

    /// Partial unique (user, job) when active (`:168-173`). Postgres
    /// treats `NULL` as distinct, so this alone cannot guard the master
    /// (`NULL` job) rows — hence the second constraint below.
    pub const UNIQUE_USER_JOB_NAME: &str = "loop_pref_unique_user_job_when_active";
    /// Columns of [`UNIQUE_USER_JOB_NAME`].
    pub const UNIQUE_USER_JOB: &[&str] = &["user_id", "job_id"];
    /// Partial unique (user) for the master switch (`:174-178`):
    /// at most one live `NULL`-job row per user.
    pub const UNIQUE_USER_MASTER_NAME: &str = "loop_pref_unique_user_master_when_active";
    /// Columns of [`UNIQUE_USER_MASTER_NAME`].
    pub const UNIQUE_USER_MASTER: &[&str] = &["user_id"];
    /// The partial condition both constraints share, as Django spells it
    /// (`deleted_at__isnull=True`; the master one additionally requires
    /// `job__isnull=True`).
    pub const UNIQUE_WHEN_ACTIVE_CONDITION: &str = "deleted_at IS NULL";

    /// `enabled` default (`:161`).
    pub const DEFAULT_ENABLED: bool = true;

    /// `user` FK: `CASCADE`, non-nullable (`:150-152`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `job` FK: `CASCADE`, nullable (`:155-160`; `NULL` = master switch).
    pub const JOB_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// One preference row. `job_id = None` is the master "pause all Auto
    /// Project Management" switch.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct LoopUserPreference {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub user_id: uuid::Uuid,
        pub job_id: Option<uuid::Uuid>,
        pub enabled: bool,
    }

    impl std::fmt::Display for LoopUserPreference {
        /// `__str__` (`:181-182`; Django renders `None` for a null FK).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let scope = self
                .job_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "master".to_string());
            write!(
                f,
                "LoopUserPreference(user={}, job={}, enabled={})",
                self.user_id, scope, self.enabled
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soft_delete::active_condition;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/loop/models")
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

    /// Physical column names from a fixture `columns` array: the `column`
    /// key when present (FK attnames like `job_id`), else `name`.
    fn fixture_columns(value: &serde_json::Value) -> Vec<String> {
        value["columns"]
            .as_array()
            .expect("fixture has columns array")
            .iter()
            .map(|c| {
                c.get("column")
                    .and_then(|c| c.as_str())
                    .or_else(|| c.get("name").and_then(|n| n.as_str()))
                    .expect("column entry has column or name")
                    .to_string()
            })
            .collect()
    }

    /// Map a fixture field name to its physical column (FK `job` ->
    /// `job_id`, plain `slug` -> `slug`).
    fn physical_column(value: &serde_json::Value, field: &str) -> String {
        let columns = value["columns"]
            .as_array()
            .expect("fixture has columns array");
        let found = columns
            .iter()
            .find(|c| c.get("name").and_then(|n| n.as_str()) == Some(field))
            .unwrap_or_else(|| panic!("fixture has field {field}"));
        found
            .get("column")
            .and_then(|c| c.as_str())
            .or_else(|| found.get("name").and_then(|n| n.as_str()))
            .expect("column entry has column or name")
            .to_string()
    }

    fn entry<'a>(value: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        value["columns"]
            .as_array()
            .expect("fixture has columns array")
            .iter()
            .find(|c| c.get("name").and_then(|n| n.as_str()) == Some(name))
            .unwrap_or_else(|| panic!("fixture has column {name}"))
    }

    fn constraint<'a>(value: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        value["constraints"]
            .as_array()
            .expect("fixture has constraints array")
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("fixture has constraint {name}"))
    }

    fn constraint_fields(value: &serde_json::Value, name: &str) -> Vec<String> {
        constraint(value, name)["fields"]
            .as_array()
            .unwrap_or_else(|| panic!("constraint {name} has fields"))
            .iter()
            .map(|f| physical_column(value, f.as_str().unwrap()))
            .collect()
    }

    #[test]
    fn skip_reason_values_match_fixture() {
        let v = fixture("skip_reason.json");
        let values: Vec<String> = v["values"]
            .as_array()
            .expect("skip_reason has values")
            .iter()
            .map(|e| e["value"].as_str().unwrap().to_string())
            .collect();
        let actual: Vec<String> = SkipReason::ALL
            .iter()
            .map(|r| r.as_str().to_string())
            .collect();
        assert_eq!(actual, values);
        let labels: Vec<String> = v["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["label"].as_str().unwrap().to_string())
            .collect();
        let actual_labels: Vec<String> = SkipReason::ALL
            .iter()
            .map(|r| r.label().to_string())
            .collect();
        assert_eq!(actual_labels, labels);
        assert_eq!(SkipReason::ALL.len(), 7);
    }

    #[test]
    fn skip_reason_round_trip() {
        for reason in SkipReason::ALL {
            assert_eq!(reason.to_string(), reason.as_str());
            assert_eq!(reason.as_str().parse::<SkipReason>(), Ok(*reason));
        }
        let err = "paused".parse::<SkipReason>().unwrap_err();
        assert_eq!(err, UnknownSkipReason("paused".to_string()));
    }

    #[test]
    fn loop_job_columns_match_fixture() {
        let v = fixture("loop_job.columns.json");
        assert_eq!(owned(loop_job::COLUMNS), fixture_columns(&v));
        let table: &str = loop_job::TABLE;
        assert_eq!(table, v["db_table"].as_str().unwrap());
        assert_eq!(table, "loop_jobs");
        let ordering: &str = loop_job::ORDERING;
        assert_eq!(ordering, v["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        assert_eq!(v["model"].as_str().unwrap(), "LoopJob");
        let unique: Vec<Vec<String>> = loop_job::UNIQUE_WHEN_ACTIVE
            .iter()
            .map(|cols| owned(cols))
            .collect();
        assert_eq!(
            unique,
            vec![constraint_fields(&v, loop_job::UNIQUE_WHEN_ACTIVE_NAME)]
        );
        let name: &str = loop_job::UNIQUE_WHEN_ACTIVE_NAME;
        assert_eq!(name, "loop_job_unique_slug_when_active");
        assert!(v["indexes"].as_array().unwrap().is_empty());
    }

    #[test]
    fn loop_job_defaults_match_fixture() {
        let v = fixture("loop_job.columns.json");
        let default = |name: &str| entry(&v, name)["default"].clone();
        assert_eq!(default("public_description"), "");
        assert_eq!(default("min_role"), 15);
        let min_role: i16 = loop_job::DEFAULT_MIN_ROLE;
        assert_eq!(min_role, 15);
        assert_eq!(default("enabled"), true);
        let enabled: bool = loop_job::DEFAULT_ENABLED;
        assert!(enabled);
        assert_eq!(default("is_builtin"), true);
        let builtin: bool = loop_job::DEFAULT_IS_BUILTIN;
        assert!(builtin);
        assert_eq!(default("tzid"), "UTC");
        let tzid: &str = loop_job::DEFAULT_TZID;
        assert_eq!(tzid, "UTC");
        assert_eq!(entry(&v, "slug")["max_length"], 64);
        assert_eq!(entry(&v, "name")["max_length"], 255);
        assert_eq!(entry(&v, "public_name")["max_length"], 255);
        assert_eq!(entry(&v, "rrule")["max_length"], 255);
        assert_eq!(entry(&v, "tzid")["max_length"], 64);
        let (slug_len, name_len, rrule_len, tzid_len): (usize, usize, usize, usize) = (
            loop_job::SLUG_MAX_LENGTH,
            loop_job::NAME_MAX_LENGTH,
            loop_job::RRULE_MAX_LENGTH,
            loop_job::TZID_MAX_LENGTH,
        );
        assert_eq!(
            (slug_len, name_len, rrule_len, tzid_len),
            (64, 255, 255, 64)
        );
        let pub_len: usize = loop_job::PUBLIC_NAME_MAX_LENGTH;
        assert_eq!(pub_len, 255);
        // Nullability: audit FKs + deleted_at nullable; declared fields not.
        for name in ["created_by", "updated_by", "deleted_at"] {
            assert!(
                entry(&v, name)["nullable"].as_bool().unwrap(),
                "{name} nullable"
            );
        }
        for name in [
            "slug",
            "name",
            "prompt",
            "min_role",
            "enabled",
            "is_builtin",
            "dtstart",
            "rrule",
            "tzid",
        ] {
            assert!(
                !entry(&v, name)["nullable"].as_bool().unwrap(),
                "{name} not null"
            );
        }
        assert_eq!(entry(&v, "created_by")["on_delete"], "SET_NULL");
        assert_eq!(entry(&v, "updated_by")["on_delete"], "SET_NULL");
        assert_eq!(loop_job::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(loop_job::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
        // Partial-unique condition is the soft-delete gate.
        let cond = constraint(&v, loop_job::UNIQUE_WHEN_ACTIVE_NAME)["condition"]
            .as_str()
            .unwrap();
        assert!(cond.contains("deleted_at__isnull"), "condition: {cond}");
        let gate: &str = loop_job::UNIQUE_WHEN_ACTIVE_CONDITION;
        assert_eq!(gate, "deleted_at IS NULL");
    }

    #[test]
    fn loop_target_columns_match_fixture() {
        let v = fixture("loop_target.columns.json");
        assert_eq!(owned(loop_target::COLUMNS), fixture_columns(&v));
        let table: &str = loop_target::TABLE;
        assert_eq!(table, v["db_table"].as_str().unwrap());
        assert_eq!(table, "loop_targets");
        let ordering: &str = loop_target::ORDERING;
        assert_eq!(ordering, v["ordering"][0].as_str().unwrap());
        assert_eq!(v["model"].as_str().unwrap(), "LoopTarget");
        let unique: Vec<Vec<String>> = loop_target::UNIQUE_WHEN_ACTIVE
            .iter()
            .map(|cols| owned(cols))
            .collect();
        assert_eq!(
            unique,
            vec![constraint_fields(&v, loop_target::UNIQUE_WHEN_ACTIVE_NAME)]
        );
        let name: &str = loop_target::UNIQUE_WHEN_ACTIVE_NAME;
        assert_eq!(name, "loop_target_unique_edge_when_active");
        let index = &v["indexes"][0];
        let index_name: &str = loop_target::DUE_INDEX_NAME;
        assert_eq!(index_name, index["name"].as_str().unwrap());
        assert_eq!(index_name, "loop_target_due_idx");
        let index_fields: Vec<String> = index["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap().to_string())
            .collect();
        assert_eq!(owned(loop_target::DUE_INDEX), index_fields);
        // FK delete behavior.
        assert_eq!(entry(&v, "job")["on_delete"], "CASCADE");
        assert_eq!(entry(&v, "workspace")["on_delete"], "CASCADE");
        assert_eq!(entry(&v, "user")["on_delete"], "CASCADE");
        assert_eq!(entry(&v, "thread")["on_delete"], "SET_NULL");
        assert_eq!(entry(&v, "last_run")["on_delete"], "SET_NULL");
        assert_eq!(loop_target::JOB_ON_DELETE, OnDelete::Cascade);
        assert_eq!(loop_target::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(loop_target::USER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(loop_target::THREAD_ON_DELETE, OnDelete::SetNull);
        assert_eq!(loop_target::LAST_RUN_ON_DELETE, OnDelete::SetNull);
        // Nullability: thread / next_run_at / last_run / skips nullable.
        for name in ["thread", "next_run_at", "last_run", "last_skipped_at"] {
            assert!(
                entry(&v, name)["nullable"].as_bool().unwrap(),
                "{name} nullable"
            );
        }
        for name in ["job", "workspace", "user", "last_skip_reason"] {
            assert!(
                !entry(&v, name)["nullable"].as_bool().unwrap(),
                "{name} not null"
            );
        }
        assert_eq!(entry(&v, "last_skip_reason")["default"], "");
        let reason_default: &str = loop_target::DEFAULT_LAST_SKIP_REASON;
        assert_eq!(reason_default, "");
        assert_eq!(entry(&v, "last_skip_reason")["max_length"], 64);
        let reason_len: usize = loop_target::LAST_SKIP_REASON_MAX_LENGTH;
        assert_eq!(reason_len, 64);
    }

    #[test]
    fn loop_target_skip_choices_match_enum() {
        let v = fixture("loop_target.columns.json");
        let choices: Vec<String> = entry(&v, "last_skip_reason")["choices"]
            .as_array()
            .expect("last_skip_reason has choices")
            .iter()
            .map(|c| c[0].as_str().unwrap().to_string())
            .collect();
        let actual: Vec<String> = SkipReason::ALL
            .iter()
            .map(|r| r.as_str().to_string())
            .collect();
        assert_eq!(actual, choices);
    }

    #[test]
    fn loop_user_preference_columns_match_fixture() {
        let v = fixture("loop_user_preference.columns.json");
        assert_eq!(owned(loop_user_preference::COLUMNS), fixture_columns(&v));
        let table: &str = loop_user_preference::TABLE;
        assert_eq!(table, v["db_table"].as_str().unwrap());
        assert_eq!(table, "loop_user_preferences");
        let ordering: &str = loop_user_preference::ORDERING;
        assert_eq!(ordering, v["ordering"][0].as_str().unwrap());
        assert_eq!(v["model"].as_str().unwrap(), "LoopUserPreference");
        // Two partial uniques: (user, job) and the master (user).
        let job_name: &str = loop_user_preference::UNIQUE_USER_JOB_NAME;
        assert_eq!(job_name, "loop_pref_unique_user_job_when_active");
        assert_eq!(
            owned(loop_user_preference::UNIQUE_USER_JOB),
            constraint_fields(&v, job_name)
        );
        let master_name: &str = loop_user_preference::UNIQUE_USER_MASTER_NAME;
        assert_eq!(master_name, "loop_pref_unique_user_master_when_active");
        assert_eq!(
            owned(loop_user_preference::UNIQUE_USER_MASTER),
            constraint_fields(&v, master_name)
        );
        // The master constraint additionally requires job IS NULL.
        let master_cond = constraint(&v, master_name)["condition"].as_str().unwrap();
        assert!(
            master_cond.contains("deleted_at__isnull"),
            "master condition: {master_cond}"
        );
        assert!(
            master_cond.contains("job__isnull"),
            "master condition: {master_cond}"
        );
        let gate: &str = loop_user_preference::UNIQUE_WHEN_ACTIVE_CONDITION;
        assert_eq!(gate, "deleted_at IS NULL");
        assert!(v["indexes"].as_array().unwrap().is_empty());
        // user CASCADE non-null; job CASCADE nullable (NULL = master).
        assert_eq!(entry(&v, "user")["on_delete"], "CASCADE");
        assert_eq!(entry(&v, "job")["on_delete"], "CASCADE");
        assert!(!entry(&v, "user")["nullable"].as_bool().unwrap());
        assert!(entry(&v, "job")["nullable"].as_bool().unwrap());
        assert_eq!(loop_user_preference::USER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(loop_user_preference::JOB_ON_DELETE, OnDelete::Cascade);
        assert_eq!(entry(&v, "enabled")["default"], true);
        let enabled: bool = loop_user_preference::DEFAULT_ENABLED;
        assert!(enabled);
    }

    #[test]
    fn reads_are_soft_delete_scoped() {
        for table in [
            loop_job::TABLE,
            loop_target::TABLE,
            loop_user_preference::TABLE,
        ] {
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
        let job = loop_job::LoopJob {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            slug: "auto-close-merged".to_string(),
            name: "Auto-close merged".to_string(),
            public_name: "Merged PR follow-ups".to_string(),
            public_description: String::new(),
            prompt: "p".to_string(),
            min_role: loop_job::DEFAULT_MIN_ROLE,
            enabled: true,
            is_builtin: true,
            dtstart: epoch,
            rrule: "FREQ=DAILY;BYHOUR=3;BYMINUTE=0".to_string(),
            tzid: loop_job::DEFAULT_TZID.to_string(),
        };
        assert_eq!(job.to_string(), "LoopJob(auto-close-merged)");
        let target = loop_target::LoopTarget {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            job_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            user_id: uuid::Uuid::nil(),
            thread_id: None,
            next_run_at: None,
            last_run_id: None,
            last_skipped_at: None,
            last_skip_reason: String::new(),
        };
        assert_eq!(
            target.to_string(),
            format!(
                "LoopTarget(job={}, ws={}, user={})",
                uuid::Uuid::nil(),
                uuid::Uuid::nil(),
                uuid::Uuid::nil()
            )
        );
        let pref = loop_user_preference::LoopUserPreference {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            user_id: uuid::Uuid::nil(),
            job_id: None,
            enabled: true,
        };
        assert_eq!(
            pref.to_string(),
            format!(
                "LoopUserPreference(user={}, job=master, enabled=true)",
                uuid::Uuid::nil()
            )
        );
    }
}
