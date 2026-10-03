//! Issue read-model tables (D-26, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/issue.py:514-548` (`IssueActivity`),
//! `:803-906` (`IssueVersion`), `:908-948` (`IssueDescriptionVersion`),
//! `db/models/cycle.py:104-128` (`CycleIssue`) and
//! `db/models/module.py:152-172` (`ModuleIssue`), adopting the
//! Django-owned schema column-for-column; migrations are not ported —
//! Django stays schema owner until switchover. Fixture source of truth:
//! `rust-api/fixtures/app_issues/models/FX-ISS-10.reads.json` (recorded by
//! PIDASHCONV-637); the `#[cfg(test)]` suite replays it section by
//! section.
//!
//! D-26 only READS these tables: the D-08/D-09 tasks own the writes (the
//! activity emit path, `IssueVersion.log_issue_version`,
//! `IssueDescriptionVersion.log_issue_description_version`). This module
//! therefore ports column lists, row structs and the read-visible halves
//! (defaults, `__str__`, the history filter); the `log_*` creation
//! orchestration is NOT ported here.
//!
//! Column order in each `*_COLUMNS` const follows the fixture: the 8
//! inherited audit/project columns first (`id`, `created_at`,
//! `updated_at`, `created_by_id`, `updated_by_id`, `deleted_at`, then
//! `project_id`, `workspace_id` from `ProjectBaseModel` at
//! `db/models/project.py:302-311`), then the model fields in declaration
//! order. FK entries use the Django attnames — except `IssueVersion`'s
//! snapshot references (`parent`, `state`, `estimate_point`, `type`,
//! `cycle`), which are plain UUID columns WITHOUT the `_id` suffix
//! (`issue.py:812-814,833-834`). Every application-level default below
//! is Django-side (the live tables carry no `column_default` in
//! `information_schema`, as established for D-01); Rust inserts must
//! supply these values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! All five tables inherit the soft-delete marker (`deleted_at`, from
//! `SoftDeleteModel` in `pi_dash/db/mixins.py:61-67`) and the default
//! manager filters `deleted_at IS NULL` (`objects = SoftDeletionManager`,
//! `mixins.py:66`; `all_objects` is the plain unscoped manager,
//! `mixins.py:67`). Every read built from these tables must apply
//! [`crate::soft_delete::active_condition`]; the tests pin this by
//! rendering a scoped `SELECT` per table.
//!
//! # Writes backfill the workspace
//!
//! `ProjectBaseModel.save()` (`db/models/project.py:309-311`) sets
//! `workspace` from `project.workspace` on every save: the D-08/D-09
//! Rust writers of all five tables must resolve `workspace_id` from the
//! `project_id` row explicitly.
//!
//! # No `delete()` override
//!
//! None of the five models defines `delete()`; all inherit
//! `SoftDeleteModel.delete` (`mixins.py:72-78`), which stamps
//! `deleted_at` and calls `save()` — so destroy DOES emit
//! `pre_save`/`post_save`. There is no code to port here (shared kernel
//! behavior); the write layers must not assume "no signal on destroy".
//!
//! # Write orchestration owned elsewhere
//!
//! `IssueVersion.log_issue_version` (`issue.py:862-906`) and
//! `IssueDescriptionVersion.log_issue_description_version` (`:926-948`)
//! are creation orchestration (the first-link `cycle` lookup, the
//! assignee/label `values_list` snapshots, the explicit `now()` stamps,
//! the swallow-all `except Exception: return False`); the D-08/D-09
//! tasks port them. This module ports the read-visible consequences:
//! the snapshot columns they fill (plain UUIDs, UUID arrays, `{}` JSON
//! objects) and their Django-side defaults.
//!
//! # Foreign reads: reuse, else pinned literals
//!
//! The columns D-26 reads on foreign tables (fixture `foreign_reads`)
//! reuse the owning domain's merged defs where present, else the SQL
//! literals pinned in [`foreign`] (query builders copy those literals;
//! the tests assert they equal the fixture):
//!
//! | Table | Columns D-26 reads | Merged def or pin |
//! | --- | --- | --- |
//! | `agent_run` | 18 run/status columns | [`foreign`] literals (merged `dispatch::agent_run` carries no `COLUMNS`); status values checked against merged `AgentRunStatus` |
//! | `cycles` | `id` | [`foreign`] literal; table pinned against D-27 `v1_cycles_modules::cycle::TABLE` |
//! | `intake_issues` | 8 columns | `app_intake::models::intake_issue` (D-32) — full reuse, no literals |
//! | `issue_agent_ticker` | all | `tasks_ticker::models::issue_agent_ticker` (D-10) — full reuse, no literals |
//! | `issue_mentions` | `id`, `issue_id`, `project_id`, `workspace_id` | [`foreign`] literals |
//! | `issue_sequences` | `id`, `issue_id`, `sequence`, `project_id`, `deleted` | [`foreign`] literals |
//! | `modules` | `id`, `archived_at` | [`foreign`] literal; table pinned against D-27 `v1_cycles_modules::module::TABLE` |
//! | `pod` | `id`, `project_id`, `is_default`, `deleted_at` | [`foreign`] literals |
//! | `runner_live_state` | 12 columns | [`foreign`] literals |
//! | `user_recent_visits` | 5 columns | [`foreign`] literals |
//!
//! # Ported quirks (translate as-is)
//!
//! `issue.py:514-548,803-948`, `cycle.py:104-128` and
//! `module.py:152-172` were read line by line for this layer. The
//! following behaviors mistranslate easily and are ported exactly:
//!
//! * `IssueActivity.issue` and `IssueActivity.issue_comment` are
//!   `DO_NOTHING`: deleting an issue or comment leaves the activity row
//!   pointing at a dead id (or the database raises, where constrained).
//!   The shared [`crate::app_issues::models_core::OnDelete`] has no
//!   `DoNothing` variant and this issue must not edit that file, so the
//!   two markers are `&str` consts
//!   ([`issue_activity::ISSUE_ON_DELETE`],
//!   [`issue_activity::ISSUE_COMMENT_ON_DELETE`]); reads must tolerate
//!   dangling ids. Ported as-is.
//! * `IssueVersion`'s snapshot references are plain UUIDs, not FKs: no
//!   integrity, and readers must not assume the referenced rows still
//!   exist. Ported as-is (columns without `_id`, struct fields plain
//!   `Option<Uuid>`).
//! * `IssueDescriptionVersion` declares no `Meta.ordering` (unlike its
//!   siblings): callers order explicitly. Ported as-is (no `ORDERING`).
//! * `comment` has `blank=True` but no `default=`: Django inserts `""`
//!   (empty strings allowed + `null=False`). Ported as
//!   [`issue_activity::COMMENT_DEFAULT`].
//! * `CycleIssue`/`ModuleIssue` duplicate D-27's merged defs table for
//!   table (this issue ports what D-26 reads); the tests pin both
//!   ports' `COLUMNS` equal so the two cannot drift apart.

use super::models_core::OnDelete;

/// `issue_activities` table (`issue.py:514-548`).
pub mod issue_activity {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `issue.py:542`).
    pub const TABLE: &str = "issue_activities";
    /// Default ordering (`Meta.ordering`, `issue.py:543`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`issue.py:540`).
    pub const VERBOSE_NAME: &str = "Issue Activity";
    /// `verbose_name_plural` (`issue.py:541`).
    pub const VERBOSE_NAME_PLURAL: &str = "Issue Activities";

    /// Columns in fixture FX-ISS-10 order: 8 inherited audit/project
    /// columns, then `issue.py:515-537` in declaration order. FK columns
    /// use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "issue_id",
        "verb",
        "field",
        "old_value",
        "new_value",
        "comment",
        "attachments",
        "issue_comment_id",
        "actor_id",
        "old_identifier",
        "new_identifier",
        "epoch",
    ];

    /// `verb` bound (`issue.py:516`, `max_length=255`).
    pub const VERB_MAX_LENGTH: usize = 255;
    /// `field` bound (`issue.py:517`, `max_length=255`).
    pub const FIELD_MAX_LENGTH: usize = 255;
    /// `attachments` array bound (`issue.py:522`, `ArrayField(..., size=10)`).
    pub const ATTACHMENTS_SIZE: usize = 10;

    /// `verb` Django-side default (`issue.py:516`, `default="created"`).
    pub const VERB_DEFAULT: &str = "created";
    /// `comment` Django-side default (`issue.py:521`, `blank=True`
    /// without `default=`: empty strings allowed + `null=False` inserts
    /// `""`).
    pub const COMMENT_DEFAULT: &str = "";

    /// History-feed exclusion (`app/views/issue/activity.py:38`): the
    /// history endpoint filters `field NOT IN (comment, vote, reaction,
    /// draft)`. The queries layer (PIDASHCONV-649) renders the `WHERE`;
    /// this const pins the vocabulary.
    pub const HISTORY_EXCLUDED_FIELDS: &[&str] = &["comment", "vote", "reaction", "draft"];

    /// `issue` FK: `DO_NOTHING`, nullable (`issue.py:515`,
    /// `related_name="issue_activity"`).
    ///
    /// A `&str`, not [`OnDelete`]: the shared enum has no `DoNothing`
    /// variant and this issue must not edit `models_core.rs`. Deleting
    /// an issue leaves the activity row pointing at a dead id (or the
    /// database raises, where constrained); reads must tolerate
    /// dangling ids. Ported as-is.
    pub const ISSUE_ON_DELETE: &str = "DO_NOTHING";
    /// `issue_comment` FK: `DO_NOTHING`, nullable (`issue.py:523-528`,
    /// `related_name="issue_comment"`). A `&str` for the same reason as
    /// [`ISSUE_ON_DELETE`]; ported as-is.
    pub const ISSUE_COMMENT_ON_DELETE: &str = "DO_NOTHING";
    /// `actor` FK: `SET_NULL`, nullable (`issue.py:529-534`,
    /// `related_name="issue_activities"`).
    pub const ACTOR_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One issue-activity row. `attachments` is a URL array
    /// (`ArrayField(models.URLField(), size=10)`, `issue.py:522`) with
    /// the Django-side `default=list` (`[]`); `epoch` is a float
    /// (`FloatField`, `:537`); `old/new_identifier` are plain nullable
    /// UUIDs (`UUIDField(null=True)`, `:535-536`), not FKs.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IssueActivity {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub issue_id: Option<uuid::Uuid>,
        pub verb: String,
        pub field: Option<String>,
        pub old_value: Option<String>,
        pub new_value: Option<String>,
        pub comment: String,
        pub attachments: Vec<String>,
        pub issue_comment_id: Option<uuid::Uuid>,
        pub actor_id: Option<uuid::Uuid>,
        pub old_identifier: Option<uuid::Uuid>,
        pub new_identifier: Option<uuid::Uuid>,
        pub epoch: Option<f64>,
    }

    impl std::fmt::Display for IssueActivity {
        /// `__str__` (`issue.py:545-547`): `str(self.issue)`, i.e. the
        /// joined issue's own `__str__`. Rust holds only the issue FK,
        /// so the display renders the issue id (mirroring D-27's
        /// `CycleIssue` display, which renders the cycle id); the name
        /// join is owned by the queries layer. A row with no issue
        /// renders `"None"`, exactly as Python's `str(None)` does.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self.issue_id {
                Some(id) => write!(f, "{id}"),
                None => write!(f, "None"),
            }
        }
    }
}

/// `issue_versions` snapshot table (`issue.py:803-906`).
pub mod issue_version {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `issue.py:856`).
    pub const TABLE: &str = "issue_versions";
    /// Default ordering (`Meta.ordering`, `issue.py:857`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`issue.py:854`).
    pub const VERBOSE_NAME: &str = "Issue Version";
    /// `verbose_name_plural` (`issue.py:855`).
    pub const VERBOSE_NAME_PLURAL: &str = "Issue Versions";

    /// Columns in fixture FX-ISS-10 order: 8 inherited audit/project
    /// columns, then `issue.py:812-851` in declaration order. The
    /// snapshot references (`parent`, `state`, `estimate_point`,
    /// `type`, `cycle`) are plain UUID columns WITHOUT the `_id`
    /// suffix; only the true FKs (`issue_id`, `activity_id`,
    /// `owned_by_id`) use attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "parent",
        "state",
        "estimate_point",
        "name",
        "priority",
        "start_date",
        "target_date",
        "assignees",
        "sequence_id",
        "labels",
        "sort_order",
        "completed_at",
        "archived_at",
        "is_draft",
        "external_source",
        "external_id",
        "type",
        "cycle",
        "modules",
        "properties",
        "meta",
        "last_saved_at",
        "issue_id",
        "activity_id",
        "owned_by_id",
    ];

    /// `name` bound (`issue.py:815`, `max_length=255`).
    pub const NAME_MAX_LENGTH: usize = 255;
    /// `priority` bound (`issue.py:817`, `max_length=30`).
    pub const PRIORITY_MAX_LENGTH: usize = 30;
    /// `external_source` bound (`issue.py:831`, `max_length=255`).
    pub const EXTERNAL_SOURCE_MAX_LENGTH: usize = 255;
    /// `external_id` bound (`issue.py:832`, `max_length=255`).
    pub const EXTERNAL_ID_MAX_LENGTH: usize = 255;

    /// `priority` choices (`PRIORITY_CHOICES`, `issue.py:804-810`).
    pub const PRIORITY_CHOICES: &[(&str, &str)] = &[
        ("urgent", "Urgent"),
        ("high", "High"),
        ("medium", "Medium"),
        ("low", "Low"),
        ("none", "None"),
    ];

    /// `priority` Django-side default (`issue.py:820`,
    /// `default="none"`).
    pub const PRIORITY_DEFAULT: &str = "none";
    /// `sequence_id` Django-side default (`issue.py:825`, `default=1`).
    pub const SEQUENCE_ID_DEFAULT: i32 = 1;
    /// `sort_order` Django-side default (`issue.py:827`,
    /// `default=65535`).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;
    /// `is_draft` Django-side default (`issue.py:830`,
    /// `default=False`).
    pub const IS_DRAFT_DEFAULT: bool = false;
    /// `properties` Django-side default (`issue.py:836`,
    /// `default=dict`): empty JSON object, supplied explicitly on every
    /// Rust insert.
    pub const PROPERTIES_DEFAULT: &str = "{}";
    /// `meta` Django-side default (`issue.py:837`, `default=dict`):
    /// empty JSON object, supplied explicitly on every Rust insert.
    pub const META_DEFAULT: &str = "{}";

    /// `issue` FK: `CASCADE` (`issue.py:840`, `related_name="versions"`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `activity` FK: `SET_NULL`, nullable (`issue.py:841-846`,
    /// `related_name="versions"`).
    pub const ACTIVITY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `owned_by` FK: `CASCADE` (`issue.py:847-851`,
    /// `related_name="issue_versions"`).
    pub const OWNED_BY_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One issue-version snapshot row. `parent`/`state`/`estimate_point`
    /// /`type`/`cycle` are plain nullable UUIDs, not FKs
    /// (`issue.py:812-814,833-834`); `assignees`/`labels`/`modules` are
    /// UUID arrays with the Django-side `default=list` (`[]`);
    /// `start_date`/`target_date`/`archived_at` are calendar dates
    /// (`DateField`, `:822-823,829`) while `completed_at` and
    /// `last_saved_at` are timestamps (`DateTimeField`, `:828,838`;
    /// `last_saved_at` defaults to `timezone.now` Django-side).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IssueVersion {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub parent: Option<uuid::Uuid>,
        pub state: Option<uuid::Uuid>,
        pub estimate_point: Option<uuid::Uuid>,
        pub name: String,
        pub priority: String,
        pub start_date: Option<chrono::NaiveDate>,
        pub target_date: Option<chrono::NaiveDate>,
        pub assignees: Vec<uuid::Uuid>,
        pub sequence_id: i32,
        pub labels: Vec<uuid::Uuid>,
        pub sort_order: f64,
        pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
        pub archived_at: Option<chrono::NaiveDate>,
        pub is_draft: bool,
        pub external_source: Option<String>,
        pub external_id: Option<String>,
        pub r#type: Option<uuid::Uuid>,
        pub cycle: Option<uuid::Uuid>,
        pub modules: Vec<uuid::Uuid>,
        pub properties: serde_json::Value,
        pub meta: serde_json::Value,
        pub last_saved_at: chrono::DateTime<chrono::Utc>,
        pub issue_id: uuid::Uuid,
        pub activity_id: Option<uuid::Uuid>,
        pub owned_by_id: uuid::Uuid,
    }

    impl std::fmt::Display for IssueVersion {
        /// `__str__` (`issue.py:859-860`): `"{name} <{project.name}>"`.
        /// Rust holds only the project FK, so the display renders the
        /// project id (mirroring
        /// [`crate::app_issues::models_core::issue::Issue`]'s display);
        /// the name join is owned by the queries layer.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} <{}>", self.name, self.project_id)
        }
    }
}

/// `issue_description_versions` snapshot table (`issue.py:908-948`).
pub mod issue_description_version {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `issue.py:924`).
    pub const TABLE: &str = "issue_description_versions";
    // NOTE: no `ORDERING` — `Meta` (`issue.py:921-924`) declares no
    // `ordering`, unlike every sibling table. Callers order explicitly.
    // Ported as-is.
    /// `verbose_name` (`issue.py:922`).
    pub const VERBOSE_NAME: &str = "Issue Description Version";
    /// `verbose_name_plural` (`issue.py:923`).
    pub const VERBOSE_NAME_PLURAL: &str = "Issue Description Versions";

    /// Columns in fixture FX-ISS-10 order: 8 inherited audit/project
    /// columns, then `issue.py:909-919` in declaration order. FK columns
    /// use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "issue_id",
        "description_binary",
        "description_html",
        "description_stripped",
        "description_json",
        "last_saved_at",
        "owned_by_id",
    ];

    /// `description_html` Django-side default (`issue.py:911`,
    /// `default="<p></p>"`).
    pub const DESCRIPTION_HTML_DEFAULT: &str = "<p></p>";
    /// `description_json` Django-side default (`issue.py:913`,
    /// `default=dict`): empty JSON object, supplied explicitly on every
    /// Rust insert.
    pub const DESCRIPTION_JSON_DEFAULT: &str = "{}";

    /// `issue` FK: `CASCADE` (`issue.py:909`,
    /// `related_name="description_versions"`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `owned_by` FK: `CASCADE` (`issue.py:915-919`,
    /// `related_name="issue_description_versions"`).
    pub const OWNED_BY_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One issue-description-version snapshot row. `description_binary`
    /// is a nullable byte string (`BinaryField(null=True)`, `issue.py:910`);
    /// `description_html` is never null (`TextField`, `:911`);
    /// `description_stripped` is nullable (`:912`); `description_json`
    /// is never null (`JSONField(default=dict)`, `:913`);
    /// `last_saved_at` defaults to `timezone.now` Django-side (`:914`).
    /// No `Display`: Python defines no `__str__` for this model
    /// (`issue.py:908-948`), so the default repr applies.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IssueDescriptionVersion {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
        pub description_binary: Option<Vec<u8>>,
        pub description_html: String,
        pub description_stripped: Option<String>,
        pub description_json: serde_json::Value,
        pub last_saved_at: chrono::DateTime<chrono::Utc>,
        pub owned_by_id: uuid::Uuid,
    }
}

/// `cycle_issues` link table (`db/models/cycle.py:104-128`).
///
/// D-26's port of the table D-27 owns (`v1_cycles_modules::cycle`):
/// same table, same columns, no D-27 query helpers (D-26's queries
/// layer builds its own). The tests pin both ports' `COLUMNS` equal.
pub mod cycle_issue {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `cycle.py:123`).
    pub const TABLE: &str = "cycle_issues";
    /// Default ordering (`Meta.ordering`, `cycle.py:124`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`cycle.py:121`).
    pub const VERBOSE_NAME: &str = "Cycle Issue";
    /// `verbose_name_plural` (`cycle.py:122`).
    pub const VERBOSE_NAME_PLURAL: &str = "Cycle Issues";

    /// Columns in fixture FX-ISS-10 order: 8 inherited audit/project
    /// columns, then `issue_id` (`cycle.py:109`) and `cycle_id`
    /// (`cycle.py:110`). FK columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "issue_id",
        "cycle_id",
    ];

    /// `Meta.unique_together` (`cycle.py:113`), Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["issue", "cycle", "deleted_at"];
    /// Partial unique constraint name (`cycle.py:118`).
    pub const UNIQUE_CYCLE_ISSUE_NAME: &str = "cycle_issue_when_deleted_at_null";
    /// Columns of [`UNIQUE_CYCLE_ISSUE_NAME`] (`cycle.py:116`), physical.
    pub const UNIQUE_CYCLE_ISSUE_COLUMNS: &[&str] = &["cycle_id", "issue_id"];
    /// `WHERE` of [`UNIQUE_CYCLE_ISSUE_NAME`] (`cycle.py:117`,
    /// `deleted_at__isnull=True`): the link is unique among live rows
    /// only, so a soft-deleted link can be re-created.
    pub const UNIQUE_CYCLE_ISSUE_WHERE: &str = "deleted_at IS NULL";

    /// `issue` FK: `CASCADE` (`cycle.py:109`, `related_name="issue_cycle"`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `cycle` FK: `CASCADE` (`cycle.py:110`, `related_name="issue_cycle"`).
    pub const CYCLE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One cycle-issue link row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct CycleIssue {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
        pub cycle_id: uuid::Uuid,
    }

    impl std::fmt::Display for CycleIssue {
        /// `__str__` (`cycle.py:126-127`): `f"{self.cycle}"`, i.e. the
        /// cycle's own `__str__`. Rust holds only the cycle FK, so the
        /// display renders the cycle id (verbatim the D-27 port's
        /// display); the name join is owned by the queries layer.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.cycle_id)
        }
    }
}

/// `module_issues` link table (`db/models/module.py:152-172`).
///
/// D-26's port of the table D-27 owns (`v1_cycles_modules::module`):
/// same table, same columns, no D-27 query helpers (D-26's queries
/// layer builds its own). The tests pin both ports' `COLUMNS` equal.
pub mod module_issue {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `module.py:167`).
    pub const TABLE: &str = "module_issues";
    /// Default ordering (`Meta.ordering`, `module.py:168`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`module.py:165`).
    pub const VERBOSE_NAME: &str = "Module Issue";
    /// `verbose_name_plural` (`module.py:166`).
    pub const VERBOSE_NAME_PLURAL: &str = "Module Issues";

    /// Columns in fixture FX-ISS-10 order: 8 inherited audit/project
    /// columns, then `module_id` (`module.py:153`) and `issue_id`
    /// (`module.py:154`). FK columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "module_id",
        "issue_id",
    ];

    /// `Meta.unique_together` (`module.py:157`), Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["issue", "module", "deleted_at"];
    /// Partial unique constraint name (`module.py:162`).
    pub const UNIQUE_MODULE_ISSUE_NAME: &str =
        "module_issue_unique_issue_module_when_deleted_at_null";
    /// Columns of [`UNIQUE_MODULE_ISSUE_NAME`] (`module.py:160`), physical.
    pub const UNIQUE_MODULE_ISSUE_COLUMNS: &[&str] = &["issue_id", "module_id"];
    /// `WHERE` of [`UNIQUE_MODULE_ISSUE_NAME`] (`module.py:161`,
    /// `deleted_at__isnull=True`): the link is unique among live rows
    /// only, so a soft-deleted link can be re-created.
    pub const UNIQUE_MODULE_ISSUE_WHERE: &str = "deleted_at IS NULL";

    /// `module` FK: `CASCADE` (`module.py:153`,
    /// `related_name="issue_module"`).
    pub const MODULE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `issue` FK: `CASCADE` (`module.py:154`,
    /// `related_name="issue_module"`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One module-issue link row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ModuleIssue {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub module_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
    }

    impl ModuleIssue {
        /// `__str__` (`module.py:170-171`):
        /// `f"{self.module.name} {self.issue.name}"`. Rust holds only
        /// the FKs, so the label takes both joined values (verbatim the
        /// D-27 port's label); the join itself is owned by the queries
        /// layer.
        pub fn label(module_name: &str, issue_name: &str) -> String {
            format!("{module_name} {issue_name}")
        }
    }
}

/// Foreign-table columns D-26 reads that this issue pins as SQL literals
/// (fixture FX-ISS-10 `foreign_reads`).
///
/// Three entries reuse merged defs instead and pin NOTHING here (the
/// tests assert the coverage): `intake_issues`
/// (`crate::app_intake::models::intake_issue`, D-32),
/// `issue_agent_ticker`
/// (`crate::tasks_ticker::models::issue_agent_ticker`, D-10) and the
/// `AgentRunStatus` enum (`crate::dispatch::status`, cited, no fixture).
/// Query builders copy the literals below verbatim.
pub mod foreign {
    /// Physical `agent_run` table (`runner/models.py`); D-26 reads the
    /// live-state/run block columns (`app/serializers/issue.py:1320-1371`).
    /// Equals the merged `dispatch::agent_run::TABLE`.
    pub const AGENT_RUN_TABLE: &str = "agent_run";
    /// Columns D-26 reads on [`AGENT_RUN_TABLE`], fixture order. Pinned
    /// (not reused): the merged `dispatch::agent_run` read shape carries
    /// no `COLUMNS` const.
    pub const AGENT_RUN_COLUMNS_READ: &[&str] = &[
        "id",
        "work_item_id",
        "status",
        "runner_id",
        "executor_kind",
        "queue_position",
        "created_at",
        "assigned_at",
        "started_at",
        "ended_at",
        "done_payload",
        "error",
        "error_code",
        "llm_model",
        "input_tokens",
        "output_tokens",
        "total_tokens",
        "pod_id",
        "run_config",
    ];
    /// `AgentRunStatus` values in the fixture's read set, in fixture
    /// order. Each resolves via the merged
    /// `dispatch::status::AgentRunStatus::from_value`; the merged enum
    /// (and Python, `runner/models.py:207-235`) additionally carry
    /// `refused`, which the fixture's read set does not list.
    pub const AGENT_RUN_STATUS_VALUES: &[&str] = &[
        "queued",
        "assigned",
        "waiting_for_worktree",
        "running",
        "cancel_requested",
        "awaiting_approval",
        "awaiting_reauth",
        "paused_awaiting_input",
        "blocked",
        "completed",
        "failed",
        "cancelled",
    ];

    /// Physical `cycles` table (`db/models/cycle.py:85`); D-26 only
    /// carries `cycle_id` through `CycleIssue` (owning domain D-27).
    /// Equals D-27's `v1_cycles_modules::cycle::TABLE`.
    pub const CYCLE_TABLE: &str = "cycles";
    /// Columns D-26 reads on [`CYCLE_TABLE`], fixture order.
    pub const CYCLE_COLUMNS_READ: &[&str] = &["id"];

    /// Physical `issue_mentions` table (`db/models/issue.py:423-442`);
    /// the move path re-points mentions to the target project
    /// (`utils/issue_move.py:341`).
    pub const ISSUE_MENTION_TABLE: &str = "issue_mentions";
    /// Columns D-26 reads on [`ISSUE_MENTION_TABLE`], fixture order.
    pub const ISSUE_MENTION_COLUMNS_READ: &[&str] =
        &["id", "issue_id", "project_id", "workspace_id"];

    /// Physical `issue_sequences` table (`db/models/issue.py:683-697`);
    /// the create path takes `max(sequence)+1` under the project
    /// advisory lock, the move path nulls out old rows and creates a new
    /// one (`utils/issue_move.py:310-311`).
    pub const ISSUE_SEQUENCE_TABLE: &str = "issue_sequences";
    /// Columns D-26 reads on [`ISSUE_SEQUENCE_TABLE`], fixture order.
    pub const ISSUE_SEQUENCE_COLUMNS_READ: &[&str] =
        &["id", "issue_id", "sequence", "project_id", "deleted"];

    /// Physical `modules` table (`db/models/module.py:112`); every
    /// `module_ids` annotation guards on `module__archived_at__isnull`
    /// (owning domain D-27). Equals D-27's
    /// `v1_cycles_modules::module::TABLE`.
    pub const MODULE_TABLE: &str = "modules";
    /// Columns D-26 reads on [`MODULE_TABLE`], fixture order.
    pub const MODULE_COLUMNS_READ: &[&str] = &["id", "archived_at"];

    /// Physical `pod` table (singular; `runner/models.py:99`,
    /// `db_table`). `Pod.default_for_project_id(pid)` is
    /// `objects.filter(project_id, is_default=True).first()`
    /// (`runner/models.py:174-176`), owned by D-13 (PIDASHCONV-581).
    pub const POD_TABLE: &str = "pod";
    /// Columns D-26 reads on [`POD_TABLE`], fixture order.
    pub const POD_COLUMNS_READ: &[&str] = &["id", "project_id", "is_default", "deleted_at"];

    /// Physical `runner_live_state` table
    /// (`runner/models.py:1454-1516`); read via the `runner__live_state`
    /// `select_related`, and the live-state block is `None` unless
    /// `observed_run_id` is `None` or equals the run id
    /// (`app/serializers/issue.py:1343-1350`).
    pub const RUNNER_LIVE_STATE_TABLE: &str = "runner_live_state";
    /// Columns D-26 reads on [`RUNNER_LIVE_STATE_TABLE`], fixture
    /// order. `usage` is JSONB: the token counts are PROPERTIES over
    /// it, not columns.
    pub const RUNNER_LIVE_STATE_COLUMNS_READ: &[&str] = &[
        "runner_id",
        "observed_run_id",
        "last_event_at",
        "last_event_kind",
        "last_event_summary",
        "agent_pid",
        "agent_subprocess_alive",
        "approvals_pending",
        "usage",
        "llm_model",
        "turn_count",
        "updated_at",
    ];

    /// Physical `user_recent_visits` table
    /// (`db/models/recent_visit.py:22-36`); retrieve/identifier enqueue
    /// `recent_visited_task` (write path owned by D-07) and destroy
    /// deletes rows (`app/views/issue/base.py:735-740`).
    pub const USER_RECENT_VISIT_TABLE: &str = "user_recent_visits";
    /// Columns D-26 reads on [`USER_RECENT_VISIT_TABLE`], fixture order.
    pub const USER_RECENT_VISIT_COLUMNS_READ: &[&str] = &[
        "id",
        "project_id",
        "workspace_id",
        "entity_identifier",
        "entity_name",
    ];
}

#[cfg(test)]
mod tests {
    use super::cycle_issue;
    use super::foreign;
    use super::issue_activity;
    use super::issue_description_version;
    use super::issue_version;
    use super::module_issue;
    use super::OnDelete;
    use crate::app_intake::models::{intake_issue, IntakeIssueStatus};
    use crate::dispatch::{agent_run, status::AgentRunStatus};
    use crate::soft_delete::active_condition;
    use crate::tasks_ticker::models::issue_agent_ticker;
    use crate::v1_cycles_modules::{cycle as d27_cycle, module as d27_module};
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/app_issues/models")
    }

    fn fixture() -> serde_json::Value {
        let path = fixtures_dir().join("FX-ISS-10.reads.json");
        let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read FX-ISS-10: {e}"));
        serde_json::from_str(&body).expect("FX-ISS-10.reads.json is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Expand the compact string-list entries (`"<a>/<b> <type> …"`)
    /// into column names in order.
    fn string_column_names(value: &serde_json::Value, model: &str) -> Vec<String> {
        value[model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has columns array"))
            .iter()
            .flat_map(|c| {
                let s = c
                    .as_str()
                    .unwrap_or_else(|| panic!("{model} column entry is str"));
                let head = s.split_whitespace().next().expect("entry has head");
                head.split('/').map(str::to_string).collect::<Vec<_>>()
            })
            .collect()
    }

    /// Nullable column names from the compact string-list entries: an
    /// entry marks `NULL` (vs `NOT NULL` or the bare `PK`).
    fn string_nullable(value: &serde_json::Value, model: &str) -> Vec<String> {
        value[model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has columns array"))
            .iter()
            .flat_map(|c| {
                let s = c.as_str().expect("column entry is str");
                let nullable = s.contains("NULL") && !s.contains("NOT NULL");
                let head = s.split_whitespace().next().expect("entry has head");
                if nullable {
                    head.split('/').map(str::to_string).collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            })
            .collect()
    }

    /// The full compact entry for one column head (to pin its recorded
    /// type, nullability, default and `on_delete` in one place).
    fn entry<'a>(value: &'a serde_json::Value, model: &str, head: &str) -> &'a str {
        value[model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has columns array"))
            .iter()
            .map(|c| c.as_str().expect("column entry is str"))
            .find(|s| {
                s.split_whitespace()
                    .next()
                    .expect("entry has head")
                    .split('/')
                    .any(|h| h == head)
            })
            .unwrap_or_else(|| panic!("{model} fixture has column {head}"))
    }

    fn constraint_strings(value: &serde_json::Value, model: &str) -> Vec<String> {
        value[model]["constraints"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has constraints"))
            .iter()
            .map(|c| c.as_str().expect("constraint is str").to_string())
            .collect()
    }

    fn foreign_columns_read(value: &serde_json::Value, table: &str) -> Vec<String> {
        value["foreign_reads"][table]["columns_read"]
            .as_array()
            .unwrap_or_else(|| panic!("foreign_reads has {table}"))
            .iter()
            .map(|c| c.as_str().expect("columns_read entry is str").to_string())
            .collect()
    }

    fn foreign_note(value: &serde_json::Value, table: &str, key: &str) -> String {
        value["foreign_reads"][table][key]
            .as_str()
            .unwrap_or_else(|| panic!("foreign_reads.{table} has {key}"))
            .to_string()
    }

    #[test]
    fn fixture_loads() {
        let v = fixture();
        assert_eq!(v["_fixture"].as_str().unwrap(), "FX-ISS-10");
        let consumers: Vec<&str> = v["_source"]["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap())
            .collect();
        assert!(
            consumers.iter().any(|c| c.contains("PIDASHCONV-647")),
            "{consumers:?}"
        );
        assert!(v["_source"]["note"]
            .as_str()
            .unwrap()
            .contains("D-08/D-09 tasks write them"));
    }

    #[test]
    fn issue_activity_columns_match_fixture() {
        let v = fixture();
        assert_eq!(
            owned(issue_activity::COLUMNS),
            string_column_names(&v, "issue_activity")
        );
        assert_eq!(issue_activity::COLUMNS.len(), 20);
        let table: &str = issue_activity::TABLE;
        assert_eq!(table, v["issue_activity"]["table"].as_str().unwrap());
        assert_eq!(table, "issue_activities");
        assert_eq!(issue_activity::ORDERING, "-created_at");
        assert_eq!(
            string_nullable(&v, "issue_activity"),
            vec![
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "issue_id",
                "field",
                "old_value",
                "new_value",
                "issue_comment_id",
                "actor_id",
                "old_identifier",
                "new_identifier",
                "epoch",
            ]
        );
        // Recorded types, defaults and bounds, pinned per entry.
        assert!(entry(&v, "issue_activity", "verb").contains("varchar(255)"));
        assert!(entry(&v, "issue_activity", "verb").contains("default 'created'"));
        assert_eq!(issue_activity::VERB_DEFAULT, "created");
        assert_eq!(issue_activity::VERB_MAX_LENGTH, 255);
        assert!(entry(&v, "issue_activity", "field").contains("varchar(255)"));
        assert_eq!(issue_activity::FIELD_MAX_LENGTH, 255);
        assert!(entry(&v, "issue_activity", "old_value").contains("text NULL"));
        assert!(entry(&v, "issue_activity", "new_value").contains("text NULL"));
        assert!(entry(&v, "issue_activity", "comment").contains("NOT NULL"));
        assert!(entry(&v, "issue_activity", "comment").contains("default ''"));
        assert_eq!(issue_activity::COMMENT_DEFAULT, "");
        assert!(entry(&v, "issue_activity", "attachments").contains("varchar[] NOT NULL"));
        assert!(entry(&v, "issue_activity", "attachments").contains("default []"));
        assert!(entry(&v, "issue_activity", "attachments").contains("(size 10)"));
        assert_eq!(issue_activity::ATTACHMENTS_SIZE, 10);
        assert!(entry(&v, "issue_activity", "old_identifier").contains("uuid NULL"));
        assert!(entry(&v, "issue_activity", "new_identifier").contains("uuid NULL"));
        assert!(entry(&v, "issue_activity", "epoch").contains("float NULL"));
        // FK delete behavior, fixture `on_delete` per entry.
        assert!(entry(&v, "issue_activity", "issue_id").contains("DO_NOTHING"));
        assert!(entry(&v, "issue_activity", "issue_comment_id").contains("DO_NOTHING"));
        let issue_on_delete: &str = issue_activity::ISSUE_ON_DELETE;
        assert_eq!(issue_on_delete, "DO_NOTHING");
        let comment_on_delete: &str = issue_activity::ISSUE_COMMENT_ON_DELETE;
        assert_eq!(comment_on_delete, "DO_NOTHING");
        assert!(entry(&v, "issue_activity", "actor_id").contains("SET_NULL"));
        assert_eq!(issue_activity::ACTOR_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue_activity::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_activity::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_activity::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue_activity::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
        // History filter vocabulary (activity.py:38).
        assert_eq!(
            v["issue_activity"]["history_filter"].as_str().unwrap(),
            "field NOT IN (comment, vote, reaction, draft) (activity.py:38)"
        );
        assert_eq!(
            issue_activity::HISTORY_EXCLUDED_FIELDS,
            &["comment", "vote", "reaction", "draft"]
        );
    }

    #[test]
    fn issue_version_columns_match_fixture() {
        let v = fixture();
        assert_eq!(
            owned(issue_version::COLUMNS),
            string_column_names(&v, "issue_version")
        );
        assert_eq!(issue_version::COLUMNS.len(), 33);
        let table: &str = issue_version::TABLE;
        assert_eq!(table, v["issue_version"]["table"].as_str().unwrap());
        assert_eq!(table, "issue_versions");
        assert_eq!(issue_version::ORDERING, "-created_at");
        // Snapshot references carry NO `_id` suffix — plain UUIDs, not FKs.
        for bare in ["parent", "state", "estimate_point", "type", "cycle"] {
            assert!(issue_version::COLUMNS.contains(&bare), "{bare}");
            assert!(
                !issue_version::COLUMNS.contains(&format!("{bare}_id").as_str()),
                "{bare}_id must not exist"
            );
        }
        assert_eq!(
            string_nullable(&v, "issue_version"),
            vec![
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "parent",
                "state",
                "estimate_point",
                "start_date",
                "target_date",
                "completed_at",
                "archived_at",
                "external_source",
                "external_id",
                "type",
                "cycle",
                "activity_id",
            ]
        );
        assert!(entry(&v, "issue_version", "name").contains("varchar(255)"));
        assert_eq!(issue_version::NAME_MAX_LENGTH, 255);
        assert!(entry(&v, "issue_version", "priority").contains("varchar(30)"));
        assert!(entry(&v, "issue_version", "priority").contains("default 'none'"));
        assert_eq!(issue_version::PRIORITY_DEFAULT, "none");
        assert_eq!(issue_version::PRIORITY_MAX_LENGTH, 30);
        assert_eq!(issue_version::EXTERNAL_SOURCE_MAX_LENGTH, 255);
        assert_eq!(issue_version::EXTERNAL_ID_MAX_LENGTH, 255);
        assert!(entry(&v, "issue_version", "assignees").contains("uuid[] NOT NULL"));
        assert!(entry(&v, "issue_version", "assignees").contains("default []"));
        assert!(entry(&v, "issue_version", "labels").contains("default []"));
        assert!(entry(&v, "issue_version", "modules").contains("default []"));
        assert!(entry(&v, "issue_version", "sequence_id").contains("default 1"));
        assert_eq!(issue_version::SEQUENCE_ID_DEFAULT, 1);
        assert!(entry(&v, "issue_version", "sort_order").contains("default 65535"));
        assert_eq!(issue_version::DEFAULT_SORT_ORDER, 65535.0);
        assert!(entry(&v, "issue_version", "is_draft").contains("default false"));
        let is_draft_default: bool = issue_version::IS_DRAFT_DEFAULT;
        assert!(!is_draft_default);
        assert!(entry(&v, "issue_version", "properties").contains("default {}"));
        assert_eq!(issue_version::PROPERTIES_DEFAULT, "{}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(issue_version::PROPERTIES_DEFAULT).unwrap(),
            serde_json::json!({})
        );
        assert!(entry(&v, "issue_version", "meta").contains("default {}"));
        assert_eq!(issue_version::META_DEFAULT, "{}");
        assert!(entry(&v, "issue_version", "last_saved_at").contains("default now"));
        assert_eq!(
            issue_version::PRIORITY_CHOICES,
            &[
                ("urgent", "Urgent"),
                ("high", "High"),
                ("medium", "Medium"),
                ("low", "Low"),
                ("none", "None"),
            ]
        );
        assert!(entry(&v, "issue_version", "issue_id").contains("CASCADE"));
        assert_eq!(issue_version::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert!(entry(&v, "issue_version", "activity_id").contains("SET_NULL"));
        assert_eq!(issue_version::ACTIVITY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue_version::OWNED_BY_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_version::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_version::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_version::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue_version::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn issue_description_version_columns_match_fixture() {
        let v = fixture();
        assert_eq!(
            owned(issue_description_version::COLUMNS),
            string_column_names(&v, "issue_description_version")
        );
        assert_eq!(issue_description_version::COLUMNS.len(), 15);
        let table: &str = issue_description_version::TABLE;
        assert_eq!(
            table,
            v["issue_description_version"]["table"].as_str().unwrap()
        );
        assert_eq!(table, "issue_description_versions");
        assert_eq!(
            string_nullable(&v, "issue_description_version"),
            vec![
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "description_binary",
                "description_stripped",
            ]
        );
        assert!(entry(&v, "issue_description_version", "description_binary").contains("bytea NULL"));
        assert!(entry(&v, "issue_description_version", "description_html")
            .contains("default '<p></p>'"));
        assert_eq!(
            issue_description_version::DESCRIPTION_HTML_DEFAULT,
            "<p></p>"
        );
        assert!(entry(&v, "issue_description_version", "description_json").contains("default {}"));
        assert_eq!(issue_description_version::DESCRIPTION_JSON_DEFAULT, "{}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                issue_description_version::DESCRIPTION_JSON_DEFAULT
            )
            .unwrap(),
            serde_json::json!({})
        );
        assert!(entry(&v, "issue_description_version", "last_saved_at").contains("default now"));
        assert!(entry(&v, "issue_description_version", "issue_id").contains("CASCADE"));
        assert_eq!(
            issue_description_version::ISSUE_ON_DELETE,
            OnDelete::Cascade
        );
        assert_eq!(
            issue_description_version::OWNED_BY_ON_DELETE,
            OnDelete::Cascade
        );
        assert_eq!(
            issue_description_version::PROJECT_ON_DELETE,
            OnDelete::Cascade
        );
        assert_eq!(
            issue_description_version::WORKSPACE_ON_DELETE,
            OnDelete::Cascade
        );
        assert_eq!(
            issue_description_version::CREATED_BY_ON_DELETE,
            OnDelete::SetNull
        );
        assert_eq!(
            issue_description_version::UPDATED_BY_ON_DELETE,
            OnDelete::SetNull
        );
    }

    #[test]
    fn link_tables_columns_match_fixture() {
        let v = fixture();
        for (model, cols) in [
            ("cycle_issue", cycle_issue::COLUMNS),
            ("module_issue", module_issue::COLUMNS),
        ] {
            // Same ProjectBaseModel inheritance as the other three tables.
            let inherited: &[&str] = &cols[..8];
            assert_eq!(owned(inherited), owned(&issue_activity::COLUMNS[..8]));
            assert_eq!(cols.len(), 10);
            assert_eq!(owned(cols), string_column_names(&v, model));
        }
        assert_eq!(
            owned(&cycle_issue::COLUMNS[8..]),
            vec!["issue_id", "cycle_id"]
        );
        assert_eq!(
            owned(&module_issue::COLUMNS[8..]),
            vec!["module_id", "issue_id"]
        );
        let cycle_table: &str = cycle_issue::TABLE;
        assert_eq!(cycle_table, v["cycle_issue"]["table"].as_str().unwrap());
        assert_eq!(cycle_table, "cycle_issues");
        let module_table: &str = module_issue::TABLE;
        assert_eq!(module_table, v["module_issue"]["table"].as_str().unwrap());
        assert_eq!(module_table, "module_issues");
        assert_eq!(cycle_issue::ORDERING, "-created_at");
        assert_eq!(module_issue::ORDERING, "-created_at");
        assert_eq!(
            string_nullable(&v, "cycle_issue"),
            vec!["created_by_id", "updated_by_id", "deleted_at"]
        );
        assert_eq!(
            string_nullable(&v, "module_issue"),
            vec!["created_by_id", "updated_by_id", "deleted_at"]
        );
        // Partial unique constraints, recorded verbatim.
        let constraints = constraint_strings(&v, "cycle_issue");
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains("(cycle,issue)"));
        assert!(constraints[0].contains("WHERE deleted_at IS NULL"));
        assert_eq!(
            cycle_issue::UNIQUE_CYCLE_ISSUE_NAME,
            "cycle_issue_when_deleted_at_null"
        );
        assert_eq!(
            cycle_issue::UNIQUE_CYCLE_ISSUE_COLUMNS,
            &["cycle_id", "issue_id"]
        );
        assert_eq!(cycle_issue::UNIQUE_CYCLE_ISSUE_WHERE, "deleted_at IS NULL");
        assert_eq!(
            cycle_issue::UNIQUE_TOGETHER,
            &["issue", "cycle", "deleted_at"]
        );
        assert_eq!(cycle_issue::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(cycle_issue::CYCLE_ON_DELETE, OnDelete::Cascade);
        let constraints = constraint_strings(&v, "module_issue");
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains("(issue,module)"));
        assert!(constraints[0].contains("WHERE deleted_at IS NULL"));
        assert_eq!(
            module_issue::UNIQUE_MODULE_ISSUE_NAME,
            "module_issue_unique_issue_module_when_deleted_at_null"
        );
        assert_eq!(
            module_issue::UNIQUE_MODULE_ISSUE_COLUMNS,
            &["issue_id", "module_id"]
        );
        assert_eq!(
            module_issue::UNIQUE_MODULE_ISSUE_WHERE,
            "deleted_at IS NULL"
        );
        assert_eq!(
            module_issue::UNIQUE_TOGETHER,
            &["issue", "module", "deleted_at"]
        );
        assert_eq!(module_issue::MODULE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module_issue::ISSUE_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn d27_link_ports_match() {
        // Same tables, two ports: D-26's link columns must stay identical
        // to D-27's merged defs (drift guard; D-26 builds its own query
        // helpers and must not depend on D-27's).
        assert_eq!(
            owned(cycle_issue::COLUMNS),
            owned(d27_cycle::cycle_issue::COLUMNS)
        );
        let d27_table: &str = d27_cycle::cycle_issue::TABLE;
        assert_eq!(d27_table, cycle_issue::TABLE);
        assert_eq!(
            owned(module_issue::COLUMNS),
            owned(d27_module::module_issue::COLUMNS)
        );
        let d27_table: &str = d27_module::module_issue::TABLE;
        assert_eq!(d27_table, module_issue::TABLE);
    }

    #[test]
    fn foreign_reads_replay_fixture() {
        let v = fixture();
        // Pinned literals equal the fixture, table by table.
        assert_eq!(
            owned(foreign::AGENT_RUN_COLUMNS_READ),
            foreign_columns_read(&v, "agent_run")
        );
        assert_eq!(foreign::AGENT_RUN_COLUMNS_READ.len(), 19);
        let agent_run_table: &str = foreign::AGENT_RUN_TABLE;
        assert_eq!(agent_run_table, "agent_run");
        let merged_table: &str = agent_run::TABLE;
        assert_eq!(merged_table, agent_run_table);
        assert_eq!(
            owned(foreign::CYCLE_COLUMNS_READ),
            foreign_columns_read(&v, "cycle")
        );
        assert_eq!(foreign::CYCLE_COLUMNS_READ, &["id"]);
        let cycle_table: &str = foreign::CYCLE_TABLE;
        assert_eq!(cycle_table, "cycles");
        let merged_table: &str = d27_cycle::TABLE;
        assert_eq!(merged_table, cycle_table);
        assert_eq!(
            owned(foreign::ISSUE_MENTION_COLUMNS_READ),
            foreign_columns_read(&v, "issue_mention")
        );
        assert_eq!(
            foreign::ISSUE_MENTION_COLUMNS_READ,
            &["id", "issue_id", "project_id", "workspace_id"]
        );
        let mention_table: &str = foreign::ISSUE_MENTION_TABLE;
        assert_eq!(mention_table, "issue_mentions");
        assert!(foreign_note(&v, "issue_mention", "note").contains("move"));
        assert_eq!(
            owned(foreign::ISSUE_SEQUENCE_COLUMNS_READ),
            foreign_columns_read(&v, "issue_sequence")
        );
        assert_eq!(
            foreign::ISSUE_SEQUENCE_COLUMNS_READ,
            &["id", "issue_id", "sequence", "project_id", "deleted"]
        );
        let sequence_table: &str = foreign::ISSUE_SEQUENCE_TABLE;
        assert_eq!(sequence_table, "issue_sequences");
        assert!(foreign_note(&v, "issue_sequence", "note").contains("advisory lock"));
        assert_eq!(
            owned(foreign::MODULE_COLUMNS_READ),
            foreign_columns_read(&v, "module")
        );
        assert_eq!(foreign::MODULE_COLUMNS_READ, &["id", "archived_at"]);
        let module_table: &str = foreign::MODULE_TABLE;
        assert_eq!(module_table, "modules");
        let merged_table: &str = d27_module::TABLE;
        assert_eq!(merged_table, module_table);
        assert!(foreign_note(&v, "module", "note").contains("archived_at"));
        assert_eq!(
            owned(foreign::POD_COLUMNS_READ),
            foreign_columns_read(&v, "pod")
        );
        assert_eq!(
            foreign::POD_COLUMNS_READ,
            &["id", "project_id", "is_default", "deleted_at"]
        );
        let pod_table: &str = foreign::POD_TABLE;
        assert_eq!(pod_table, "pod");
        assert!(foreign_note(&v, "pod", "default_rule").contains("default_for_project_id"));
        // Two entries carry parenthetical annotations (PK note, JSONB
        // note); the pinned literal keeps the bare column heads, as with
        // the annotated `project` entries in models_core.
        let live_read: Vec<String> = foreign_columns_read(&v, "runner_live_state")
            .iter()
            .map(|c| c.split(" (").next().unwrap().to_string())
            .collect();
        assert_eq!(owned(foreign::RUNNER_LIVE_STATE_COLUMNS_READ), live_read);
        assert_eq!(foreign::RUNNER_LIVE_STATE_COLUMNS_READ.len(), 12);
        let live_raw = foreign_columns_read(&v, "runner_live_state");
        assert!(live_raw[0].contains("(PK, OneToOne)"));
        assert!(live_raw[8].contains("PROPERTIES"));
        let live_table: &str = foreign::RUNNER_LIVE_STATE_TABLE;
        assert_eq!(live_table, "runner_live_state");
        assert!(foreign_note(&v, "runner_live_state", "join").contains("observed_run_id"));
        assert_eq!(
            owned(foreign::USER_RECENT_VISIT_COLUMNS_READ),
            foreign_columns_read(&v, "user_recent_visit")
        );
        assert_eq!(
            foreign::USER_RECENT_VISIT_COLUMNS_READ,
            &[
                "id",
                "project_id",
                "workspace_id",
                "entity_identifier",
                "entity_name",
            ]
        );
        let visit_table: &str = foreign::USER_RECENT_VISIT_TABLE;
        assert_eq!(visit_table, "user_recent_visits");
        assert!(foreign_note(&v, "user_recent_visit", "note").contains("recent_visited_task"));
        // Fixture status values resolve through the merged enum (cite, no
        // fixture of its own).
        let status_values: Vec<String> = v["foreign_reads"]["agent_run"]["status_values"]
            .as_array()
            .expect("agent_run has status_values")
            .iter()
            .map(|c| c.as_str().expect("status value is str").to_string())
            .collect();
        assert_eq!(owned(foreign::AGENT_RUN_STATUS_VALUES), status_values);
        assert_eq!(foreign::AGENT_RUN_STATUS_VALUES.len(), 12);
        for value in foreign::AGENT_RUN_STATUS_VALUES {
            let parsed =
                AgentRunStatus::from_value(value).unwrap_or_else(|| panic!("merged {value}"));
            assert_eq!(parsed.value(), *value);
        }
        assert!(v["foreign_reads"]["agent_run_status_enum"]["cite"]
            .as_str()
            .unwrap()
            .contains("db/src/dispatch"));
        // Reused tables: every literal column the fixture names is covered
        // by the owning domain's merged def (reuse, not re-port).
        let merged_table: &str = intake_issue::TABLE;
        assert_eq!(merged_table, "intake_issues");
        for col in foreign_columns_read(&v, "intake_issue") {
            assert!(
                intake_issue::COLUMNS.contains(&col.as_str()),
                "intake_issue.{col}"
            );
        }
        assert_eq!(
            foreign_columns_read(&v, "intake_issue"),
            vec![
                "id",
                "issue_id",
                "status",
                "workspace_id",
                "project_id",
                "source",
                "source_email",
                "extra",
            ]
        );
        // Status vocabulary behind the archive/identifier filters.
        assert_eq!(IntakeIssueStatus::Pending.as_i32(), -2);
        assert_eq!(IntakeIssueStatus::Rejected.as_i32(), -1);
        assert_eq!(IntakeIssueStatus::Snoozed.as_i32(), 0);
        assert_eq!(IntakeIssueStatus::Accepted.as_i32(), 1);
        assert_eq!(IntakeIssueStatus::Duplicate.as_i32(), 2);
        let default_status: i32 = intake_issue::DEFAULT_STATUS;
        assert_eq!(default_status, IntakeIssueStatus::Pending.as_i32());
        assert!(foreign_note(&v, "intake_issue", "reuse").contains("D-32"));
        let merged_table: &str = issue_agent_ticker::TABLE;
        assert_eq!(merged_table, "issue_agent_ticker");
        let ticker_entry = foreign_columns_read(&v, "issue_agent_ticker");
        assert_eq!(ticker_entry.len(), 1);
        assert!(ticker_entry[0].starts_with("all: "));
        let head = ticker_entry[0]
            .split("(+")
            .next()
            .unwrap()
            .trim_start_matches("all: ");
        for col in head.split(", ") {
            let col = col.trim();
            assert!(
                issue_agent_ticker::COLUMNS.contains(&col),
                "issue_agent_ticker.{col}"
            );
        }
        assert!(foreign_note(&v, "issue_agent_ticker", "reuse").contains("MERGED"));
    }

    #[test]
    fn reads_are_soft_delete_scoped() {
        for table in [
            issue_activity::TABLE,
            issue_version::TABLE,
            issue_description_version::TABLE,
            cycle_issue::TABLE,
            module_issue::TABLE,
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
        let issue_id = uuid::Uuid::nil();
        let project_id = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let activity = issue_activity::IssueActivity {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id,
            workspace_id: uuid::Uuid::nil(),
            issue_id: Some(issue_id),
            verb: issue_activity::VERB_DEFAULT.to_string(),
            field: None,
            old_value: None,
            new_value: None,
            comment: issue_activity::COMMENT_DEFAULT.to_string(),
            attachments: Vec::new(),
            issue_comment_id: None,
            actor_id: None,
            old_identifier: None,
            new_identifier: None,
            epoch: None,
        };
        assert_eq!(activity.to_string(), issue_id.to_string());
        let orphan = issue_activity::IssueActivity {
            issue_id: None,
            ..activity.clone()
        };
        // `str(None)` in Python is `"None"` — ported as-is.
        assert_eq!(orphan.to_string(), "None");
        let version = issue_version::IssueVersion {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id,
            workspace_id: uuid::Uuid::nil(),
            parent: None,
            state: None,
            estimate_point: None,
            name: "Ship it".to_string(),
            priority: issue_version::PRIORITY_DEFAULT.to_string(),
            start_date: None,
            target_date: None,
            assignees: Vec::new(),
            sequence_id: issue_version::SEQUENCE_ID_DEFAULT,
            labels: Vec::new(),
            sort_order: issue_version::DEFAULT_SORT_ORDER,
            completed_at: None,
            archived_at: None,
            is_draft: issue_version::IS_DRAFT_DEFAULT,
            external_source: None,
            external_id: None,
            r#type: None,
            cycle: None,
            modules: Vec::new(),
            properties: serde_json::json!({}),
            meta: serde_json::json!({}),
            last_saved_at: epoch,
            issue_id,
            activity_id: None,
            owned_by_id: uuid::Uuid::nil(),
        };
        assert_eq!(version.to_string(), format!("Ship it <{project_id}>"));
        let link = cycle_issue::CycleIssue {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id,
            workspace_id: uuid::Uuid::nil(),
            issue_id,
            cycle_id: uuid::Uuid::nil(),
        };
        assert_eq!(link.to_string(), uuid::Uuid::nil().to_string());
        assert_eq!(
            module_issue::ModuleIssue::label("Sprint 1", "Ship it"),
            "Sprint 1 Ship it"
        );
    }
}
