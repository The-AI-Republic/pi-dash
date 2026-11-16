#![forbid(unsafe_code)]

//! Module + ModuleMember table models (D-28, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/module.py:58-151` (`ModuleStatus`,
//! `Module`, `ModuleMember`), adopting the Django-owned schema
//! column-for-column; migrations are not ported — Django stays schema
//! owner. Sibling issue PIDASHCONV-363 ports `:152-217`
//! (`ModuleIssue`, `ModuleLink`, `ModuleUserProperties`) into this same
//! file's style, in its own sections; nothing below moves or renames.
//!
//! Column order in each `COLUMNS` const follows the FX-MOD-01 fixtures
//! (`rust-api/fixtures/app_modules/models/module.columns.json`,
//! `module_member.columns.json`, recorded by PIDASHCONV-294): the 8
//! inherited audit/project columns first (`id`, `created_at`,
//! `updated_at`, `created_by_id`, `updated_by_id`, `deleted_at`,
//! `project_id`, `workspace_id`, from `BaseModel` at
//! `db/models/base.py:17-21`, `TimeAuditModel`/`UserAuditModel` at
//! `db/mixins.py:19-44`, `SoftDeleteModel` at `db/mixins.py:61`, and
//! `ProjectBaseModel` at `db/models/project.py:302-311`), then the model
//! fields in declaration order. FK entries use the Django attnames
//! (`project_id`, `workspace_id`, `lead_id`, `module_id`, `member_id`).
//! Every application-level default below is Django-side (the live tables
//! carry no `column_default` in `information_schema`, as established for
//! D-01); Rust inserts must supply these values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! Both tables inherit the soft-delete marker (`deleted_at`, from
//! `SoftDeleteModel` in `pi_dash/db/mixins.py:57-69`) and the default
//! manager filters `deleted_at IS NULL` (`objects =
//! SoftDeletionManager`, `mixins.py:51-58`; `all_objects` is the plain
//! unscoped manager). Every read built from these tables must add
//! `deleted_at IS NULL` ([`module::LIVE_SCOPE_WHERE`],
//! [`module_member::LIVE_SCOPE_WHERE`]); the SQL itself lives with the
//! queries layer (PIDASHCONV-374). The partial unique constraints stay
//! as they are (tombstones are excluded by the scope, so a deleted
//! module name or membership can be re-created).
//!
//! # Writes backfill the workspace
//!
//! `ProjectBaseModel.save()` (`db/models/project.py:309-311`) sets
//! `workspace` from `project.workspace` on every save: Rust
//! inserts/updates must resolve `workspace_id` from the `project_id`
//! row explicitly.
//!
//! # Ported bugs (translate, don't redesign; also listed in the PR)
//!
//! * `Module.save()` reads `MIN(sort_order)` and then inserts outside
//!   any transaction (`module.py:115-124`): two concurrent creates for
//!   one project can compute the same minimum and collide. Ported as-is
//!   ([`module::new_sort_order`] is the pure half; the queries layer
//!   owns the lookup + insert).
//! * `Meta.unique_together` still lists `deleted_at`
//!   (`module.py:102,135`) alongside the partial unique constraints
//!   that supersede it. Ported verbatim as [`module::UNIQUE_TOGETHER`]
//!   / [`module_member::UNIQUE_TOGETHER`].
//! * Membership writes bypass the `members` M2M manager: the serializer
//!   soft-deletes existing rows then
//!   `bulk_create(ignore_conflicts=True, batch_size=10)` sender-side
//!   (`module.py:87-93` declaration vs
//!   `app/serializers/module.py:75-90,102-118` writes; fixture
//!   `write_notes`). [`module::MEMBERS_THROUGH_TABLE`] and friends pin
//!   the join contract only.

use serde::{Deserialize, Serialize};

/// Django-level FK delete behavior (ORM-emulated; same shape as the D-27
/// `db::app_cycles::models::OnDelete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// `ModuleStatus` choices (`module.py:58-64`).
///
/// Django `choices=` is form-validation only — it creates no DB
/// constraint — so the six wire values plus the `"planned"` default is
/// the complete port. `CharField(max_length=20)` bound pinned by
/// [`module::STATUS_MAX_LENGTH`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModuleStatus {
    /// `"backlog"`.
    #[serde(rename = "backlog")]
    Backlog,
    /// `"planned"` — the field default (`module.py:83`).
    #[serde(rename = "planned")]
    Planned,
    /// `"in-progress"`.
    #[serde(rename = "in-progress")]
    InProgress,
    /// `"paused"`.
    #[serde(rename = "paused")]
    Paused,
    /// `"completed"`.
    #[serde(rename = "completed")]
    Completed,
    /// `"cancelled"`.
    #[serde(rename = "cancelled")]
    Cancelled,
}

impl Default for ModuleStatus {
    /// `default="planned"` (`module.py:83`).
    fn default() -> Self {
        ModuleStatus::Planned
    }
}

impl ModuleStatus {
    /// Wire value of each choice (`module.py:59-64`).
    pub fn as_str(self) -> &'static str {
        match self {
            ModuleStatus::Backlog => "backlog",
            ModuleStatus::Planned => "planned",
            ModuleStatus::InProgress => "in-progress",
            ModuleStatus::Paused => "paused",
            ModuleStatus::Completed => "completed",
            ModuleStatus::Cancelled => "cancelled",
        }
    }
}

impl std::str::FromStr for ModuleStatus {
    type Err = ();

    /// Parse a wire value (`module.py:59-64`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "backlog" => Ok(ModuleStatus::Backlog),
            "planned" => Ok(ModuleStatus::Planned),
            "in-progress" => Ok(ModuleStatus::InProgress),
            "paused" => Ok(ModuleStatus::Paused),
            "completed" => Ok(ModuleStatus::Completed),
            "cancelled" => Ok(ModuleStatus::Cancelled),
            _ => Err(()),
        }
    }
}

/// `modules` table (`module.py:67-127`).
pub mod module {
    use super::{ModuleStatus, OnDelete};
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `module.py:112`).
    pub const TABLE: &str = "modules";
    /// Default ordering (`Meta.ordering`, `module.py:113`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`module.py:110`).
    pub const VERBOSE_NAME: &str = "Module";
    /// `verbose_name_plural` (`module.py:111`).
    pub const VERBOSE_NAME_PLURAL: &str = "Modules";

    /// Physical columns in fixture order: 8 inherited audit/project
    /// columns, then `module.py:68-99` in declaration order. The
    /// `members` M2M (`module.py:87-93`) has no column and is pinned by
    /// the `MEMBERS_*` consts below instead.
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
        "description_text",
        "description_html",
        "start_date",
        "target_date",
        "status",
        "lead_id",
        "view_props",
        "sort_order",
        "external_source",
        "external_id",
        "archived_at",
        "logo_props",
    ];

    /// `name` bound (`module.py:68`, `max_length=255`).
    pub const NAME_MAX_LENGTH: usize = 255;
    /// `status` bound (`module.py:74-85`, `max_length=20`).
    pub const STATUS_MAX_LENGTH: usize = 20;
    /// `external_source` bound (`module.py:96`, `max_length=255`).
    pub const EXTERNAL_SOURCE_MAX_LENGTH: usize = 255;
    /// `external_id` bound (`module.py:97`, `max_length=255`).
    pub const EXTERNAL_ID_MAX_LENGTH: usize = 255;

    /// `status` choices in declaration order (`module.py:59-64`,
    /// mirrored by `ModuleStatus::as_str`).
    pub const STATUS_CHOICES: &[&str] = &[
        "backlog",
        "planned",
        "in-progress",
        "paused",
        "completed",
        "cancelled",
    ];
    /// `status` Django-side default (`module.py:83`,
    /// `default="planned"`).
    pub const DEFAULT_STATUS: &str = "planned";
    /// `sort_order` Django-side default (`module.py:95`,
    /// `default=65535`). Kept for the first module of a project, when
    /// the `MIN` aggregate is `None` (`module.py:121-122`).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;
    /// Step applied below the project minimum on create
    /// (`module.py:122`).
    pub const SORT_ORDER_STEP: f64 = 10000.0;
    /// `view_props` / `logo_props` Django-side defaults
    /// (`module.py:94,99`, `default=dict`): empty JSON object,
    /// supplied explicitly on every Rust insert.
    pub const EMPTY_PROPS: &str = "{}";
    /// `description` has `blank=True` with no `null` and no Django
    /// default (`module.py:69`): the ORM stores the empty string.
    pub const DEFAULT_DESCRIPTION: &str = "";
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `mixins.py:51-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `Meta.unique_together` (`module.py:102`), Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["name", "project", "deleted_at"];
    /// Partial unique constraint name (`module.py:107`).
    pub const UNIQUE_MODULE_NAME: &str = "module_unique_name_project_when_deleted_at_null";
    /// Columns of [`UNIQUE_MODULE_NAME`] (`module.py:105`), physical.
    pub const UNIQUE_MODULE_COLUMNS: &[&str] = &["name", "project_id"];
    /// `WHERE` of [`UNIQUE_MODULE_NAME`] (`module.py:106`,
    /// `deleted_at__isnull=True`): the name is unique among live rows
    /// of a project only, so a soft-deleted module frees its name.
    pub const UNIQUE_MODULE_WHERE: &str = "deleted_at IS NULL";

    /// `lead` FK: `SET_NULL`, nullable (`module.py:86`).
    pub const LEAD_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `lead` reverse accessor (`module.py:86`).
    pub const LEAD_RELATED_NAME: &str = "module_leads";
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:31-37`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:38-44`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `members` M2M join table (`module.py:87-93`,
    /// `through="ModuleMember"`): no column on `modules`.
    pub const MEMBERS_THROUGH_TABLE: &str = "module_members";
    /// `through_fields=("module", "member")` (`module.py:92`).
    pub const MEMBERS_THROUGH_FIELDS: &[&str] = &["module", "member"];
    /// `related_name="module_members"` (`module.py:90`).
    pub const MEMBERS_RELATED_NAME: &str = "module_members";

    /// Render one Python `date` / `None` exactly as `Module.__str__`
    /// interpolates it (`module.py:127`): `str(None)` is `"None"`,
    /// a date renders `YYYY-MM-DD`.
    pub fn py_date(value: Option<chrono::NaiveDate>) -> String {
        value
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "None".to_owned())
    }

    /// `__str__` (`module.py:126-127`):
    /// `f"{self.name} {self.start_date} {self.target_date}"`.
    pub fn label(
        name: &str,
        start_date: Option<chrono::NaiveDate>,
        target_date: Option<chrono::NaiveDate>,
    ) -> String {
        format!("{name} {} {}", py_date(start_date), py_date(target_date))
    }

    /// Pure half of `Module.save` (`module.py:115-124`): on create
    /// (`_state.adding`) only, `sort_order = smallest - 10000`; when
    /// no siblings exist (`smallest` is `None`) the Django-side
    /// default is kept. Returns `None` to mean "keep
    /// `DEFAULT_SORT_ORDER`". Runs BEFORE `super().save()`
    /// (`module.py:124`).
    pub fn new_sort_order(smallest: Option<f64>) -> Option<f64> {
        smallest.map(|min| min - SORT_ORDER_STEP)
    }

    /// One module row. `start_date`/`target_date` are calendar dates
    /// (`DateField`, `module.py:72-73`); `sort_order` is a float
    /// (`FloatField`, `module.py:95`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Module {
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
        pub description_text: Option<serde_json::Value>,
        pub description_html: Option<serde_json::Value>,
        pub start_date: Option<chrono::NaiveDate>,
        pub target_date: Option<chrono::NaiveDate>,
        pub status: ModuleStatus,
        pub lead_id: Option<uuid::Uuid>,
        pub view_props: serde_json::Value,
        pub sort_order: f64,
        pub external_source: Option<String>,
        pub external_id: Option<String>,
        pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
        pub logo_props: serde_json::Value,
    }

    impl std::fmt::Display for Module {
        /// `__str__` (`module.py:126-127`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{}",
                label(&self.name, self.start_date, self.target_date)
            )
        }
    }
}

/// `module_members` join table (`module.py:130-149`).
pub mod module_member {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `module.py:145`).
    pub const TABLE: &str = "module_members";
    /// Default ordering (`Meta.ordering`, `module.py:146`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`module.py:143`).
    pub const VERBOSE_NAME: &str = "Module Member";
    /// `verbose_name_plural` (`module.py:144`).
    pub const VERBOSE_NAME_PLURAL: &str = "Module Members";

    /// Physical columns in fixture order: 8 inherited audit/project
    /// columns, then `module_id` (`module.py:131`) and `member_id`
    /// (`module.py:132`). FK columns use the Django attnames.
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
        "member_id",
    ];

    /// `Meta.unique_together` (`module.py:135`), Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["module", "member", "deleted_at"];
    /// Partial unique constraint name (`module.py:140`).
    pub const UNIQUE_MODULE_MEMBER_NAME: &str =
        "module_member_unique_module_member_when_deleted_at_null";
    /// Columns of [`UNIQUE_MODULE_MEMBER_NAME`] (`module.py:138`),
    /// physical.
    pub const UNIQUE_MODULE_MEMBER_COLUMNS: &[&str] = &["module_id", "member_id"];
    /// `WHERE` of [`UNIQUE_MODULE_MEMBER_NAME`] (`module.py:139`,
    /// `deleted_at__isnull=True`): one live membership per
    /// (module, member); a soft-deleted row can be re-created.
    pub const UNIQUE_MODULE_MEMBER_WHERE: &str = "deleted_at IS NULL";
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `mixins.py:51-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `module` FK: `CASCADE` (`module.py:131`).
    pub const MODULE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `member` FK: `CASCADE` (`module.py:132`).
    pub const MEMBER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:31-37`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:38-44`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One module-membership row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ModuleMember {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub module_id: uuid::Uuid,
        pub member_id: uuid::Uuid,
    }

    impl ModuleMember {
        /// `__str__` (`module.py:148-149`):
        /// `f"{self.module.name} {self.member}"`. Rust holds only the
        /// FKs, so the label takes the joined module name and the
        /// member's own `__str__`; the joins are owned by the queries
        /// layer.
        pub fn label(module_name: &str, member_label: &str) -> String {
            format!("{module_name} {member_label}")
        }
    }
}

/// `module_issues` link table (`module.py:152-171`).
///
/// Same 8-column inherited audit/project prefix as [`module`] and
/// [`module_member`], then the two FKs in declaration order. Writes
/// couple sender-side: link endpoints
/// `bulk_create(ignore_conflicts=True, batch_size=10)` and destroy
/// paths hard-filter then `.delete()` (soft stamp), per the fixture
/// `write_notes`; the SQL itself lives with the queries layer
/// (PIDASHCONV-374).
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

    /// Physical columns in fixture order: 8 inherited audit/project
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
    /// Ported verbatim: `deleted_at` is listed alongside the partial
    /// unique constraint that supersedes it (same legacy shape as
    /// [`module::UNIQUE_TOGETHER`](super::module::UNIQUE_TOGETHER)).
    pub const UNIQUE_TOGETHER: &[&str] = &["issue", "module", "deleted_at"];
    /// Partial unique constraint name (`module.py:162`).
    pub const UNIQUE_MODULE_ISSUE_NAME: &str =
        "module_issue_unique_issue_module_when_deleted_at_null";
    /// Columns of [`UNIQUE_MODULE_ISSUE_NAME`] (`module.py:160`),
    /// physical, in `fields` order.
    pub const UNIQUE_MODULE_ISSUE_COLUMNS: &[&str] = &["issue_id", "module_id"];
    /// `WHERE` of [`UNIQUE_MODULE_ISSUE_NAME`] (`module.py:161`,
    /// `deleted_at__isnull=True`): one live link per (issue, module);
    /// a soft-deleted row can be re-created.
    pub const UNIQUE_MODULE_ISSUE_WHERE: &str = "deleted_at IS NULL";
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `mixins.py:51-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `module` FK: `CASCADE` (`module.py:153`).
    pub const MODULE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `module` reverse accessor (`module.py:153`).
    pub const MODULE_RELATED_NAME: &str = "issue_module";
    /// `issue` FK: `CASCADE` (`module.py:154`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `issue` reverse accessor (`module.py:154`).
    pub const ISSUE_RELATED_NAME: &str = "issue_module";
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:31-37`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:38-44`).
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
        /// the FKs, so the label takes the joined module and issue
        /// names; the joins are owned by the queries layer.
        pub fn label(module_name: &str, issue_name: &str) -> String {
            format!("{module_name} {issue_name}")
        }
    }
}

/// `module_links` table (`module.py:174-187`).
///
/// Duplicate URLs are rejected in serializer code only
/// (`app/serializers/module.py:190-201`); there is no DB-level unique
/// constraint on this table. Ported as-is: no `UNIQUE_*` consts here.
pub mod module_link {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `module.py:183`).
    pub const TABLE: &str = "module_links";
    /// Default ordering (`Meta.ordering`, `module.py:184`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`module.py:181`).
    pub const VERBOSE_NAME: &str = "Module Link";
    /// `verbose_name_plural` (`module.py:182`).
    pub const VERBOSE_NAME_PLURAL: &str = "Module Links";

    /// Physical columns in fixture order: 8 inherited audit/project
    /// columns, then `title` (`module.py:175`), `url` (`module.py:176`),
    /// `module_id` (`module.py:177`) and `metadata` (`module.py:178`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "title",
        "url",
        "module_id",
        "metadata",
    ];

    /// `title` bound (`module.py:175`, `max_length=255`); nullable via
    /// `blank=True, null=True`.
    pub const TITLE_MAX_LENGTH: usize = 255;
    /// `url` bound (`module.py:176`): `URLField` with no explicit
    /// `max_length` keeps the Django default of 200.
    pub const URL_MAX_LENGTH: usize = 200;
    /// `metadata` Django-side default (`module.py:178`,
    /// `default=dict`): empty JSON object, supplied explicitly on
    /// every Rust insert.
    pub const DEFAULT_METADATA: &str = "{}";
    /// Duplicate-URL rejection lives in serializer code, not in the
    /// schema (`app/serializers/module.py:190-201`): there is no
    /// `UNIQUE_*` const for this table.
    pub const DUPLICATE_URL_ENFORCED_IN: &str = "app/serializers/module.py:190-201";
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `mixins.py:51-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `module` FK: `CASCADE` (`module.py:177`).
    pub const MODULE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `module` reverse accessor (`module.py:177`).
    pub const MODULE_RELATED_NAME: &str = "link_module";
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:31-37`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:38-44`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One module-link row. `url` is a plain string: Django's
    /// `URLField` validates shape in Python (`validate_url`,
    /// `app/serializers/module.py:178-186`) and stores `varchar`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ModuleLink {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub title: Option<String>,
        pub url: String,
        pub module_id: uuid::Uuid,
        pub metadata: serde_json::Value,
    }

    impl ModuleLink {
        /// `__str__` (`module.py:186-187`):
        /// `f"{self.module.name} {self.url}"`. Rust holds only the
        /// FK, so the label takes the joined module name; the join is
        /// owned by the queries layer.
        pub fn label(module_name: &str, url: &str) -> String {
            format!("{module_name} {url}")
        }
    }
}

/// `module_user_properties` table (`module.py:190-217`).
///
/// The three `get_default_*` producers (`module.py:14-55`) are plain
/// module-level functions returning a fresh dict per call; the Rust
/// halves below are functions returning a fresh
/// [`serde_json::Value`] per call for the same reason — never a
/// shared mutable static.
pub mod module_user_properties {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `module.py:213`).
    pub const TABLE: &str = "module_user_properties";
    /// Default ordering (`Meta.ordering`, `module.py:214`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`module.py:211`).
    pub const VERBOSE_NAME: &str = "Module User Property";
    /// `verbose_name_plural` (`module.py:212`): kept singular verbatim
    /// — the Django source repeats the singular, ported as-is.
    pub const VERBOSE_NAME_PLURAL: &str = "Module User Property";

    /// Physical columns in fixture order: 8 inherited audit/project
    /// columns, then `module_id` (`module.py:191`), `user_id`
    /// (`module.py:192-196`), `filters` (`module.py:197`),
    /// `display_filters` (`module.py:198`), `display_properties`
    /// (`module.py:199`) and `rich_filters` (`module.py:200`).
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
        "user_id",
        "filters",
        "display_filters",
        "display_properties",
        "rich_filters",
    ];

    /// `Meta.unique_together` (`module.py:203`), Django field names.
    /// Ported verbatim: `deleted_at` is listed alongside the partial
    /// unique constraint that supersedes it (same legacy shape as
    /// [`module::UNIQUE_TOGETHER`](super::module::UNIQUE_TOGETHER)).
    pub const UNIQUE_TOGETHER: &[&str] = &["module", "user", "deleted_at"];
    /// Partial unique constraint name (`module.py:208`).
    pub const UNIQUE_MODULE_USER_NAME: &str =
        "module_user_properties_unique_module_user_when_deleted_at_null";
    /// Columns of [`UNIQUE_MODULE_USER_NAME`] (`module.py:206`),
    /// physical, in `fields` order.
    pub const UNIQUE_MODULE_USER_COLUMNS: &[&str] = &["module_id", "user_id"];
    /// `WHERE` of [`UNIQUE_MODULE_USER_NAME`] (`module.py:207`,
    /// `deleted_at__isnull=True`): one live property row per
    /// (module, user); a soft-deleted row can be re-created.
    pub const UNIQUE_MODULE_USER_WHERE: &str = "deleted_at IS NULL";
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `mixins.py:51-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `module` FK: `CASCADE` (`module.py:191`).
    pub const MODULE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `module` reverse accessor (`module.py:191`).
    pub const MODULE_RELATED_NAME: &str = "module_user_properties";
    /// `user` FK: `CASCADE` (`module.py:192-196`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `user` reverse accessor (`module.py:195`).
    pub const USER_RELATED_NAME: &str = "module_user_properties";
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:31-37`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:38-44`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `rich_filters` Django-side default (`module.py:200`,
    /// `default=dict`): empty JSON object, supplied explicitly on
    /// every Rust insert.
    pub const DEFAULT_RICH_FILTERS: &str = "{}";

    /// `get_default_filters` (`module.py:14-25`): all nine keys null.
    /// Returns a fresh value per call, like the Python function.
    pub fn default_filters() -> serde_json::Value {
        serde_json::json!({
            "priority": null,
            "state": null,
            "state_group": null,
            "assignees": null,
            "created_by": null,
            "labels": null,
            "start_date": null,
            "target_date": null,
            "subscriber": null,
        })
    }

    /// `get_default_display_filters` (`module.py:28-37`). Returns a
    /// fresh value per call, like the Python function.
    pub fn default_display_filters() -> serde_json::Value {
        serde_json::json!({
            "group_by": null,
            "order_by": "-created_at",
            "type": null,
            "sub_issue": true,
            "show_empty_groups": true,
            "layout": "list",
            "calendar_date_range": "",
        })
    }

    /// `get_default_display_properties` (`module.py:40-55`): all
    /// thirteen keys `True`. Returns a fresh value per call, like the
    /// Python function.
    pub fn default_display_properties() -> serde_json::Value {
        serde_json::json!({
            "assignee": true,
            "attachment_count": true,
            "created_on": true,
            "due_date": true,
            "estimate": true,
            "key": true,
            "labels": true,
            "link": true,
            "priority": true,
            "start_date": true,
            "state": true,
            "sub_issue_count": true,
            "updated_on": true,
        })
    }

    /// One module-user-properties row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ModuleUserProperties {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub module_id: uuid::Uuid,
        pub user_id: uuid::Uuid,
        pub filters: serde_json::Value,
        pub display_filters: serde_json::Value,
        pub display_properties: serde_json::Value,
        pub rich_filters: serde_json::Value,
    }

    impl ModuleUserProperties {
        /// `__str__` (`module.py:216-217`):
        /// `f"{self.module.name} {self.user.email}"`. Rust holds only
        /// the FKs, so the label takes the joined module name and the
        /// user's email; the joins are owned by the queries layer.
        pub fn label(module_name: &str, user_email: &str) -> String {
            format!("{module_name} {user_email}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::module::{self, new_sort_order};
    use super::module_member;
    use super::{ModuleStatus, OnDelete};

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/app_modules/models")
    }

    fn fixture(name: &str) -> serde_json::Value {
        let path = fixtures_dir().join(name);
        let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));
        serde_json::from_str(&body).expect("fixture is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Fixture `columns` entries are descriptive strings whose first
    /// whitespace-separated token is the column (or pseudo-column)
    /// name, e.g. `"lead_id uuid NULL FK users SET_NULL ..."`.
    fn column_tokens(value: &serde_json::Value) -> Vec<String> {
        value["columns"]
            .as_array()
            .expect("fixture has columns array")
            .iter()
            .map(|c| {
                c.as_str()
                    .expect("column entry is str")
                    .split_whitespace()
                    .next()
                    .expect("column entry has name")
                    .to_string()
            })
            .collect()
    }

    fn column_entry<'a>(value: &'a serde_json::Value, name: &str) -> &'a str {
        value["columns"]
            .as_array()
            .expect("fixture has columns array")
            .iter()
            .map(|c| c.as_str().expect("column entry is str"))
            .find(|c| c.split_whitespace().next() == Some(name))
            .unwrap_or_else(|| panic!("fixture has column {name}"))
    }

    #[test]
    fn module_columns_match_fixture() {
        let v = fixture("module.columns.json");
        let tokens = column_tokens(&v);
        // 22 physical columns, then the column-less `members` M2M entry.
        assert_eq!(owned(module::COLUMNS), tokens[..22].to_vec());
        assert_eq!(module::COLUMNS.len(), 22);
        assert_eq!(tokens[22], "members");
        assert!(column_entry(&v, "members").contains("no column"));
        let table: &str = module::TABLE;
        assert_eq!(table, v["db_table"].as_str().unwrap());
        assert_eq!(table, "modules");
        let ordering: &str = module::ORDERING;
        assert_eq!(ordering, v["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        let verbose = format!("{} / {}", module::VERBOSE_NAME, module::VERBOSE_NAME_PLURAL);
        assert_eq!(verbose, v["verbose"].as_str().unwrap());
        assert_eq!(verbose, "Module / Modules");
        assert!(v["trace"]
            .as_str()
            .unwrap()
            .contains("db/models/module.py:58-100"));
    }

    #[test]
    fn module_field_details_match_fixture() {
        let v = fixture("module.columns.json");
        // Status: choices + default recorded in the type string.
        let status = column_entry(&v, "status");
        assert!(status.contains("default 'planned'"), "{status}");
        for choice in module::STATUS_CHOICES {
            assert!(status.contains(choice), "status lists {choice}");
        }
        assert_eq!(module::DEFAULT_STATUS, "planned");
        assert_eq!(module::STATUS_CHOICES.len(), 6);
        assert_eq!(module::STATUS_MAX_LENGTH, 20);
        // Django-side defaults recorded in the type strings.
        assert!(column_entry(&v, "sort_order").contains("default 65535.0"));
        assert_eq!(module::DEFAULT_SORT_ORDER, 65535.0);
        assert_eq!(module::SORT_ORDER_STEP, 10000.0);
        assert!(column_entry(&v, "view_props").contains("default dict"));
        assert!(column_entry(&v, "logo_props").contains("default dict"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(module::EMPTY_PROPS).unwrap(),
            serde_json::json!({})
        );
        // `description` (blank, non-null) stores the empty string.
        assert!(column_entry(&v, "description").contains("ORM empty-string default"));
        assert_eq!(module::DEFAULT_DESCRIPTION, "");
        // Nullability: audit FKs, JSON descriptions, dates, lead,
        // external ids and archive stamp nullable.
        for name in [
            "created_by_id",
            "updated_by_id",
            "deleted_at",
            "description_text",
            "description_html",
            "start_date",
            "target_date",
            "lead_id",
            "external_source",
            "external_id",
            "archived_at",
        ] {
            assert!(
                column_entry(&v, name).contains("NULL"),
                "{name} nullable per fixture"
            );
        }
        // Length bounds.
        assert_eq!(module::NAME_MAX_LENGTH, 255);
        assert_eq!(module::EXTERNAL_SOURCE_MAX_LENGTH, 255);
        assert_eq!(module::EXTERNAL_ID_MAX_LENGTH, 255);
        // FK delete behavior + M2M join contract.
        assert!(column_entry(&v, "lead_id").contains("SET_NULL"));
        assert!(column_entry(&v, "lead_id").contains("module_leads"));
        assert_eq!(module::LEAD_ON_DELETE, OnDelete::SetNull);
        assert_eq!(module::LEAD_RELATED_NAME, "module_leads");
        assert!(column_entry(&v, "project_id").contains("CASCADE"));
        assert!(column_entry(&v, "project_id").contains("workspace auto-set"));
        assert_eq!(module::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(module::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
        let members = column_entry(&v, "members");
        assert!(members.contains("THROUGH module_members"), "{members}");
        assert!(
            members.contains("through_fields(module,member)"),
            "{members}"
        );
        assert!(members.contains("related module_members"), "{members}");
        assert_eq!(module::MEMBERS_THROUGH_TABLE, "module_members");
        assert_eq!(
            owned(module::MEMBERS_THROUGH_FIELDS),
            vec!["module", "member"]
        );
        assert_eq!(module::MEMBERS_RELATED_NAME, "module_members");
        // Managers: default scope excludes tombstones.
        assert!(v["managers"]["objects"]
            .as_str()
            .unwrap()
            .contains("SoftDeletionManager"));
        assert_eq!(module::LIVE_SCOPE_WHERE, "deleted_at IS NULL");
    }

    #[test]
    fn module_constraints_match_fixture() {
        let v = fixture("module.columns.json");
        let constraints: Vec<&str> = v["constraints"]
            .as_array()
            .expect("constraints array")
            .iter()
            .map(|c| c.as_str().expect("constraint is str"))
            .collect();
        assert_eq!(constraints.len(), 2);
        let partial = constraints
            .iter()
            .find(|c| c.contains(module::UNIQUE_MODULE_NAME))
            .expect("partial unique constraint recorded");
        assert!(partial.contains("(name, project_id)"), "{partial}");
        assert!(partial.contains("WHERE deleted_at IS NULL"), "{partial}");
        assert_eq!(
            owned(module::UNIQUE_MODULE_COLUMNS),
            vec!["name", "project_id"]
        );
        assert_eq!(module::UNIQUE_MODULE_WHERE, "deleted_at IS NULL");
        assert_eq!(module::UNIQUE_MODULE_WHERE, module::LIVE_SCOPE_WHERE);
        let legacy = constraints
            .iter()
            .find(|c| c.contains("unique_together"))
            .expect("legacy unique_together recorded");
        assert!(legacy.contains("(name, project, deleted_at)"), "{legacy}");
        assert_eq!(
            owned(module::UNIQUE_TOGETHER),
            vec!["name", "project", "deleted_at"]
        );
    }

    #[test]
    fn module_save_sort_order_matches_fixture() {
        let v = fixture("module.columns.json");
        let rule = v["save_side_effect"].as_str().unwrap();
        assert!(
            rule.contains("MIN(sibling sort_order in project) - 10000"),
            "{rule}"
        );
        assert!(rule.contains("keeps field default 65535.0"), "{rule}");
        assert!(rule.contains("BEFORE super().save()"), "{rule}");
        // None (no siblings) keeps the Django-side default ...
        assert_eq!(new_sort_order(None), None);
        // ... otherwise smallest minus the step, as f64 arithmetic.
        assert_eq!(new_sort_order(Some(65535.0)), Some(55535.0));
        assert_eq!(new_sort_order(Some(0.0)), Some(-10000.0));
        assert_eq!(new_sort_order(Some(-500.25)), Some(-10500.25));
    }

    #[test]
    fn module_status_choices_match_fixture() {
        let v = fixture("module.columns.json");
        let status = column_entry(&v, "status");
        assert!(
            status.contains("backlog/planned/in-progress/paused/completed/cancelled"),
            "{status}"
        );
        for (choice, parsed) in [
            ("backlog", ModuleStatus::Backlog),
            ("planned", ModuleStatus::Planned),
            ("in-progress", ModuleStatus::InProgress),
            ("paused", ModuleStatus::Paused),
            ("completed", ModuleStatus::Completed),
            ("cancelled", ModuleStatus::Cancelled),
        ] {
            use std::str::FromStr as _;
            assert_eq!(ModuleStatus::from_str(choice), Ok(parsed));
            assert_eq!(parsed.as_str(), choice);
            // Serde wire form is the same string Django stores.
            assert_eq!(
                serde_json::to_value(parsed).unwrap(),
                serde_json::Value::String(choice.to_owned())
            );
            assert_eq!(
                serde_json::from_value::<ModuleStatus>(serde_json::Value::String(
                    choice.to_owned()
                ))
                .unwrap(),
                parsed
            );
        }
        use std::str::FromStr as _;
        assert_eq!(ModuleStatus::from_str("unknown"), Err(()));
        assert_eq!(ModuleStatus::default(), ModuleStatus::Planned);
        assert_eq!(ModuleStatus::default().as_str(), module::DEFAULT_STATUS);
    }

    #[test]
    fn module_member_columns_match_fixture() {
        let v = fixture("module_member.columns.json");
        assert_eq!(owned(module_member::COLUMNS), column_tokens(&v));
        assert_eq!(module_member::COLUMNS.len(), 10);
        // Inherited prefix is the same 8 audit/project columns.
        assert_eq!(
            owned(&module_member::COLUMNS[..8]),
            owned(&module::COLUMNS[..8])
        );
        assert_eq!(
            owned(&module_member::COLUMNS[8..]),
            vec!["module_id", "member_id"]
        );
        let table: &str = module_member::TABLE;
        assert_eq!(table, v["db_table"].as_str().unwrap());
        assert_eq!(table, "module_members");
        let ordering: &str = module_member::ORDERING;
        assert_eq!(ordering, v["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        let verbose = format!(
            "{} / {}",
            module_member::VERBOSE_NAME,
            module_member::VERBOSE_NAME_PLURAL
        );
        assert_eq!(verbose, v["verbose"].as_str().unwrap());
        assert_eq!(module_member::MODULE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module_member::MEMBER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module_member::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module_member::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module_member::LIVE_SCOPE_WHERE, "deleted_at IS NULL");
        assert!(v["trace"]
            .as_str()
            .unwrap()
            .contains("db/models/module.py:130-149"));
    }

    #[test]
    fn module_member_constraints_match_fixture() {
        let v = fixture("module_member.columns.json");
        let constraints: Vec<&str> = v["constraints"]
            .as_array()
            .expect("constraints array")
            .iter()
            .map(|c| c.as_str().expect("constraint is str"))
            .collect();
        assert_eq!(constraints.len(), 2);
        let partial = constraints
            .iter()
            .find(|c| c.contains(module_member::UNIQUE_MODULE_MEMBER_NAME))
            .expect("partial unique constraint recorded");
        assert!(partial.contains("(module_id, member_id)"), "{partial}");
        assert!(partial.contains("WHERE deleted_at IS NULL"), "{partial}");
        assert_eq!(
            owned(module_member::UNIQUE_MODULE_MEMBER_COLUMNS),
            vec!["module_id", "member_id"]
        );
        assert_eq!(
            module_member::UNIQUE_MODULE_MEMBER_WHERE,
            "deleted_at IS NULL"
        );
        let legacy = constraints
            .iter()
            .find(|c| c.contains("unique_together"))
            .expect("legacy unique_together recorded");
        assert!(legacy.contains("(module, member, deleted_at)"), "{legacy}");
        assert_eq!(
            owned(module_member::UNIQUE_TOGETHER),
            vec!["module", "member", "deleted_at"]
        );
    }

    #[test]
    fn module_row_round_trips_with_identical_columns() {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap();
        let row = module::Module {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            name: "Auth".to_owned(),
            description: module::DEFAULT_DESCRIPTION.to_owned(),
            description_text: None,
            description_html: None,
            start_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1),
            target_date: chrono::NaiveDate::from_ymd_opt(2026, 2, 1),
            status: ModuleStatus::default(),
            lead_id: None,
            view_props: serde_json::from_str(module::EMPTY_PROPS).unwrap(),
            sort_order: module::DEFAULT_SORT_ORDER,
            external_source: None,
            external_id: None,
            archived_at: None,
            logo_props: serde_json::from_str(module::EMPTY_PROPS).unwrap(),
        };
        let value = serde_json::to_value(&row).expect("row serializes");
        // Every physical column is present exactly once under its
        // Django attname; the column-less M2M is absent.
        let mut keys: Vec<String> = value
            .as_object()
            .expect("row is object")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        let mut expected = owned(module::COLUMNS);
        expected.sort();
        assert_eq!(keys, expected);
        assert!(value.get("members").is_none());
        // Django-side defaults land on the wire verbatim.
        assert_eq!(value["status"], serde_json::json!("planned"));
        assert_eq!(value["sort_order"], serde_json::json!(65535.0));
        assert_eq!(value["view_props"], serde_json::json!({}));
        assert_eq!(value["description"], serde_json::json!(""));
        assert_eq!(
            serde_json::from_value::<module::Module>(value).unwrap(),
            row
        );
        let link = module_member::ModuleMember {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            module_id: uuid::Uuid::nil(),
            member_id: uuid::Uuid::nil(),
        };
        let link_value = serde_json::to_value(&link).expect("link serializes");
        let mut link_keys: Vec<String> = link_value
            .as_object()
            .expect("link is object")
            .keys()
            .cloned()
            .collect();
        link_keys.sort();
        let mut link_expected = owned(module_member::COLUMNS);
        link_expected.sort();
        assert_eq!(link_keys, link_expected);
        assert_eq!(
            serde_json::from_value::<module_member::ModuleMember>(link_value).unwrap(),
            link
        );
    }

    #[test]
    fn display_matches_python_str() {
        let v = fixture("module.columns.json");
        assert!(v["str"]
            .as_str()
            .unwrap()
            .contains("'{name} {start_date} {target_date}'"));
        let dated = module::label(
            "Auth",
            chrono::NaiveDate::from_ymd_opt(2026, 1, 1),
            chrono::NaiveDate::from_ymd_opt(2026, 2, 1),
        );
        assert_eq!(dated, "Auth 2026-01-01 2026-02-01");
        // `str(None)` is `"None"`, not the empty string.
        assert_eq!(module::label("Auth", None, None), "Auth None None");
        let m = module::Module {
            id: uuid::Uuid::nil(),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            updated_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            name: "Auth".to_owned(),
            description: String::new(),
            description_text: None,
            description_html: None,
            start_date: None,
            target_date: None,
            status: ModuleStatus::Planned,
            lead_id: None,
            view_props: serde_json::json!({}),
            sort_order: module::DEFAULT_SORT_ORDER,
            external_source: None,
            external_id: None,
            archived_at: None,
            logo_props: serde_json::json!({}),
        };
        assert_eq!(m.to_string(), "Auth None None");
        let mv = fixture("module_member.columns.json");
        assert!(mv["str"]
            .as_str()
            .unwrap()
            .contains("'{module.name} {member}'"));
        assert_eq!(
            module_member::ModuleMember::label("Auth", "ada"),
            "Auth ada"
        );
    }

    #[test]
    fn module_issue_columns_match_fixture() {
        use super::module_issue;
        let v = fixture("module_issue.columns.json");
        assert_eq!(owned(module_issue::COLUMNS), column_tokens(&v));
        assert_eq!(module_issue::COLUMNS.len(), 10);
        // Inherited prefix is the same 8 audit/project columns.
        assert_eq!(
            owned(&module_issue::COLUMNS[..8]),
            owned(&module::COLUMNS[..8])
        );
        assert_eq!(
            owned(&module_issue::COLUMNS[8..]),
            vec!["module_id", "issue_id"]
        );
        let table: &str = module_issue::TABLE;
        assert_eq!(table, v["db_table"].as_str().unwrap());
        assert_eq!(table, "module_issues");
        let ordering: &str = module_issue::ORDERING;
        assert_eq!(ordering, v["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        let verbose = format!(
            "{} / {}",
            module_issue::VERBOSE_NAME,
            module_issue::VERBOSE_NAME_PLURAL
        );
        assert_eq!(verbose, v["verbose"].as_str().unwrap());
        assert_eq!(verbose, "Module Issue / Module Issues");
        // FKs: CASCADE with the shared `issue_module` reverse name.
        assert!(column_entry(&v, "module_id").contains("CASCADE"));
        assert!(column_entry(&v, "module_id").contains("issue_module"));
        assert!(column_entry(&v, "issue_id").contains("CASCADE"));
        assert!(column_entry(&v, "issue_id").contains("issue_module"));
        assert_eq!(module_issue::MODULE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module_issue::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module_issue::MODULE_RELATED_NAME, "issue_module");
        assert_eq!(module_issue::ISSUE_RELATED_NAME, "issue_module");
        assert_eq!(module_issue::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module_issue::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module_issue::LIVE_SCOPE_WHERE, "deleted_at IS NULL");
        assert!(v["managers"]["objects"]
            .as_str()
            .unwrap()
            .contains("SoftDeletionManager"));
        assert!(v["trace"]
            .as_str()
            .unwrap()
            .contains("db/models/module.py:152-171"));
        // Sender-side write coupling recorded in the fixture.
        assert!(v["write_notes"]
            .as_str()
            .unwrap()
            .contains("ignore_conflicts"));
    }

    #[test]
    fn module_issue_constraints_match_fixture() {
        use super::module_issue;
        let v = fixture("module_issue.columns.json");
        let constraints: Vec<&str> = v["constraints"]
            .as_array()
            .expect("constraints array")
            .iter()
            .map(|c| c.as_str().expect("constraint is str"))
            .collect();
        assert_eq!(constraints.len(), 2);
        let partial = constraints
            .iter()
            .find(|c| c.contains(module_issue::UNIQUE_MODULE_ISSUE_NAME))
            .expect("partial unique constraint recorded");
        assert!(partial.contains("(issue_id, module_id)"), "{partial}");
        assert!(partial.contains("WHERE deleted_at IS NULL"), "{partial}");
        assert_eq!(
            owned(module_issue::UNIQUE_MODULE_ISSUE_COLUMNS),
            vec!["issue_id", "module_id"]
        );
        assert_eq!(
            module_issue::UNIQUE_MODULE_ISSUE_WHERE,
            "deleted_at IS NULL"
        );
        let legacy = constraints
            .iter()
            .find(|c| c.contains("unique_together"))
            .expect("legacy unique_together recorded");
        assert!(legacy.contains("(issue, module, deleted_at)"), "{legacy}");
        assert_eq!(
            owned(module_issue::UNIQUE_TOGETHER),
            vec!["issue", "module", "deleted_at"]
        );
    }

    #[test]
    fn module_link_columns_match_fixture() {
        use super::module_link;
        let v = fixture("module_link.columns.json");
        assert_eq!(owned(module_link::COLUMNS), column_tokens(&v));
        assert_eq!(module_link::COLUMNS.len(), 12);
        // Inherited prefix is the same 8 audit/project columns.
        assert_eq!(
            owned(&module_link::COLUMNS[..8]),
            owned(&module::COLUMNS[..8])
        );
        assert_eq!(
            owned(&module_link::COLUMNS[8..]),
            vec!["title", "url", "module_id", "metadata"]
        );
        let table: &str = module_link::TABLE;
        assert_eq!(table, v["db_table"].as_str().unwrap());
        assert_eq!(table, "module_links");
        let ordering: &str = module_link::ORDERING;
        assert_eq!(ordering, v["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        let verbose = format!(
            "{} / {}",
            module_link::VERBOSE_NAME,
            module_link::VERBOSE_NAME_PLURAL
        );
        assert_eq!(verbose, v["verbose"].as_str().unwrap());
        // Bounds: title 255 nullable, url 200 (URLField default).
        assert_eq!(module_link::TITLE_MAX_LENGTH, 255);
        assert!(column_entry(&v, "title").contains("NULL"));
        assert_eq!(module_link::URL_MAX_LENGTH, 200);
        assert!(column_entry(&v, "url").contains("200"));
        // `metadata` dict default.
        assert!(column_entry(&v, "metadata").contains("default dict"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(module_link::DEFAULT_METADATA).unwrap(),
            serde_json::json!({})
        );
        // No DB-level uniqueness: duplicates rejected in serializer code.
        assert_eq!(v["constraints"].as_array().unwrap().len(), 1);
        assert!(v["constraints"][0]
            .as_str()
            .unwrap()
            .contains("serializer code only"));
        assert_eq!(
            module_link::DUPLICATE_URL_ENFORCED_IN,
            "app/serializers/module.py:190-201"
        );
        assert!(column_entry(&v, "module_id").contains("CASCADE"));
        assert!(column_entry(&v, "module_id").contains("link_module"));
        assert_eq!(module_link::MODULE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module_link::MODULE_RELATED_NAME, "link_module");
        assert_eq!(module_link::LIVE_SCOPE_WHERE, "deleted_at IS NULL");
        assert!(v["trace"]
            .as_str()
            .unwrap()
            .contains("db/models/module.py:174-187"));
    }

    #[test]
    fn module_user_properties_columns_match_fixture() {
        use super::module_user_properties as props;
        let v = fixture("module_user_properties.columns.json");
        assert_eq!(owned(props::COLUMNS), column_tokens(&v));
        assert_eq!(props::COLUMNS.len(), 14);
        // Inherited prefix is the same 8 audit/project columns.
        assert_eq!(owned(&props::COLUMNS[..8]), owned(&module::COLUMNS[..8]));
        assert_eq!(
            owned(&props::COLUMNS[8..]),
            vec![
                "module_id",
                "user_id",
                "filters",
                "display_filters",
                "display_properties",
                "rich_filters"
            ]
        );
        let table: &str = props::TABLE;
        assert_eq!(table, v["db_table"].as_str().unwrap());
        assert_eq!(table, "module_user_properties");
        let ordering: &str = props::ORDERING;
        assert_eq!(ordering, v["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        // Plural kept singular verbatim (`module.py:211-212`); the
        // fixture records the verbatim note alongside the value.
        let verbose = format!("{} / {}", props::VERBOSE_NAME, props::VERBOSE_NAME_PLURAL);
        assert_eq!(verbose, "Module User Property / Module User Property");
        assert!(v["verbose"].as_str().unwrap().contains(&verbose));
        assert!(v["verbose"]
            .as_str()
            .unwrap()
            .contains("singular kept verbatim"));
        // JSON defaults recorded per column.
        assert!(column_entry(&v, "filters").contains("get_default_filters"));
        assert!(column_entry(&v, "display_filters").contains("get_default_display_filters"));
        assert!(column_entry(&v, "display_properties").contains("get_default_display_properties"));
        assert!(column_entry(&v, "rich_filters").contains("default dict"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(props::DEFAULT_RICH_FILTERS).unwrap(),
            serde_json::json!({})
        );
        assert!(column_entry(&v, "module_id").contains("CASCADE"));
        assert!(column_entry(&v, "module_id").contains("module_user_properties"));
        assert!(column_entry(&v, "user_id").contains("CASCADE"));
        assert!(column_entry(&v, "user_id").contains("module_user_properties"));
        assert_eq!(props::MODULE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(props::USER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(props::MODULE_RELATED_NAME, "module_user_properties");
        assert_eq!(props::USER_RELATED_NAME, "module_user_properties");
        assert_eq!(props::LIVE_SCOPE_WHERE, "deleted_at IS NULL");
        let trace = v["trace"].as_str().unwrap();
        assert!(trace.contains(":190-217"), "{trace}");
        assert!(trace.contains("14-55"), "{trace}");
    }

    #[test]
    fn module_user_properties_constraints_match_fixture() {
        use super::module_user_properties as props;
        let v = fixture("module_user_properties.columns.json");
        let constraints: Vec<&str> = v["constraints"]
            .as_array()
            .expect("constraints array")
            .iter()
            .map(|c| c.as_str().expect("constraint is str"))
            .collect();
        assert_eq!(constraints.len(), 2);
        let partial = constraints
            .iter()
            .find(|c| c.contains(props::UNIQUE_MODULE_USER_NAME))
            .expect("partial unique constraint recorded");
        assert!(partial.contains("(module_id, user_id)"), "{partial}");
        assert!(partial.contains("WHERE deleted_at IS NULL"), "{partial}");
        assert_eq!(
            owned(props::UNIQUE_MODULE_USER_COLUMNS),
            vec!["module_id", "user_id"]
        );
        assert_eq!(props::UNIQUE_MODULE_USER_WHERE, "deleted_at IS NULL");
        let legacy = constraints
            .iter()
            .find(|c| c.contains("unique_together"))
            .expect("legacy unique_together recorded");
        assert!(legacy.contains("(module, user, deleted_at)"), "{legacy}");
        assert_eq!(
            owned(props::UNIQUE_TOGETHER),
            vec!["module", "user", "deleted_at"]
        );
    }

    #[test]
    fn module_user_properties_defaults_match_python() {
        use super::module_user_properties as props;
        let v = fixture("module_user_properties.columns.json");
        // `filters`: nine keys, all null (`module.py:14-25`).
        let filters = props::default_filters();
        let filter_keys: Vec<&str> = filters
            .as_object()
            .expect("filters is object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            filter_keys,
            vec![
                "priority",
                "state",
                "state_group",
                "assignees",
                "created_by",
                "labels",
                "start_date",
                "target_date",
                "subscriber",
            ]
        );
        for key in &filter_keys {
            assert_eq!(
                filters[*key],
                serde_json::Value::Null,
                "{key} defaults null"
            );
        }
        assert!(column_entry(&v, "filters").contains("all null"));
        // `display_filters` (`module.py:28-37`).
        let display = props::default_display_filters();
        assert_eq!(
            display,
            serde_json::json!({
                "group_by": null,
                "order_by": "-created_at",
                "type": null,
                "sub_issue": true,
                "show_empty_groups": true,
                "layout": "list",
                "calendar_date_range": "",
            })
        );
        // `display_properties`: thirteen keys, all true (`module.py:40-55`).
        let properties = props::default_display_properties();
        let prop_obj = properties.as_object().expect("properties is object");
        assert_eq!(prop_obj.len(), 13);
        for (key, value) in prop_obj {
            assert_eq!(*value, serde_json::Value::Bool(true), "{key} defaults true");
        }
        // Fresh value per call, like the Python functions: mutating one
        // result leaves the next call untouched.
        let mut first = props::default_filters();
        first["priority"] = serde_json::json!("high");
        assert_eq!(
            props::default_filters()["priority"],
            serde_json::Value::Null
        );
        let mut first_display = props::default_display_filters();
        first_display["layout"] = serde_json::json!("kanban");
        assert_eq!(
            props::default_display_filters()["layout"],
            serde_json::json!("list")
        );
    }

    #[test]
    fn models_b_rows_round_trip_with_identical_columns() {
        use super::{module_issue, module_link, module_user_properties as props};
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap();
        let issue_row = module_issue::ModuleIssue {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            module_id: uuid::Uuid::nil(),
            issue_id: uuid::Uuid::nil(),
        };
        let issue_value = serde_json::to_value(&issue_row).expect("row serializes");
        let mut issue_keys: Vec<String> = issue_value
            .as_object()
            .expect("row is object")
            .keys()
            .cloned()
            .collect();
        issue_keys.sort();
        let mut issue_expected = owned(module_issue::COLUMNS);
        issue_expected.sort();
        assert_eq!(issue_keys, issue_expected);
        assert_eq!(
            serde_json::from_value::<module_issue::ModuleIssue>(issue_value).unwrap(),
            issue_row
        );
        let link_row = module_link::ModuleLink {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            title: Some("Spec".to_owned()),
            url: "https://example.com/spec".to_owned(),
            module_id: uuid::Uuid::nil(),
            metadata: serde_json::from_str(module_link::DEFAULT_METADATA).unwrap(),
        };
        let link_value = serde_json::to_value(&link_row).expect("row serializes");
        let mut link_keys: Vec<String> = link_value
            .as_object()
            .expect("row is object")
            .keys()
            .cloned()
            .collect();
        link_keys.sort();
        let mut link_expected = owned(module_link::COLUMNS);
        link_expected.sort();
        assert_eq!(link_keys, link_expected);
        assert_eq!(link_value["metadata"], serde_json::json!({}));
        assert_eq!(
            serde_json::from_value::<module_link::ModuleLink>(link_value).unwrap(),
            link_row.clone()
        );
        // A null title round-trips too (`blank=True, null=True`).
        let untitled = module_link::ModuleLink {
            title: None,
            ..link_row
        };
        let untitled_value = serde_json::to_value(&untitled).expect("row serializes");
        assert_eq!(untitled_value["title"], serde_json::Value::Null);
        let props_row = props::ModuleUserProperties {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            module_id: uuid::Uuid::nil(),
            user_id: uuid::Uuid::nil(),
            filters: props::default_filters(),
            display_filters: props::default_display_filters(),
            display_properties: props::default_display_properties(),
            rich_filters: serde_json::from_str(props::DEFAULT_RICH_FILTERS).unwrap(),
        };
        let props_value = serde_json::to_value(&props_row).expect("row serializes");
        let mut props_keys: Vec<String> = props_value
            .as_object()
            .expect("row is object")
            .keys()
            .cloned()
            .collect();
        props_keys.sort();
        let mut props_expected = owned(props::COLUMNS);
        props_expected.sort();
        assert_eq!(props_keys, props_expected);
        assert_eq!(
            props_value["display_filters"]["order_by"],
            serde_json::json!("-created_at")
        );
        assert_eq!(
            serde_json::from_value::<props::ModuleUserProperties>(props_value).unwrap(),
            props_row
        );
    }

    #[test]
    fn models_b_display_matches_python_str() {
        use super::{module_issue, module_link, module_user_properties as props};
        let iv = fixture("module_issue.columns.json");
        assert!(iv["str"]
            .as_str()
            .unwrap()
            .contains("'{module.name} {issue.name}'"));
        assert_eq!(
            module_issue::ModuleIssue::label("Auth", "Login bug"),
            "Auth Login bug"
        );
        let lv = fixture("module_link.columns.json");
        assert!(lv["str"]
            .as_str()
            .unwrap()
            .contains("'{module.name} {url}'"));
        assert_eq!(
            module_link::ModuleLink::label("Auth", "https://example.com/spec"),
            "Auth https://example.com/spec"
        );
        let pv = fixture("module_user_properties.columns.json");
        assert!(pv["str"]
            .as_str()
            .unwrap()
            .contains("'{module.name} {user.email}'"));
        assert_eq!(
            props::ModuleUserProperties::label("Auth", "ada@example.com"),
            "Auth ada@example.com"
        );
    }
}
