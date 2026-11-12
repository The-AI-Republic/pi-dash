#![forbid(unsafe_code)]

//! D-20 module model reads (PIDASHCONV-292).
//!
//! Ports the column lists, `ModuleStatus` values, manager exclusion filters,
//! `archived_at` guards, and per-module scoping for the five tables in
//! `apps/api/pi_dash/db/models/module.py:1-217` (`Module` `:67-127`,
//! `ModuleMember` `:130-149`, `ModuleIssue` `:152-171`, `ModuleLink`
//! `:174-187`, `ModuleUserProperties` `:190-217`, `ModuleStatus` `:58-64`),
//! adopting the Django-owned schema column-for-column. Migrations are not
//! ported; Django stays schema owner until switchover.
//!
//! Fixture (FX-CYCMOD-01):
//! `rust-api/fixtures/v1_cycles_modules/models/module.columns.json`.
//! Column order in each `COLUMNS` const: the 8 inherited audit/project
//! columns first (`id`, `created_at`, `updated_at`, `created_by_id`,
//! `updated_by_id`, `deleted_at`, `project_id`, `workspace_id`, from
//! `BaseModel` at `db/models/base.py:17-21`, `TimeAuditModel`/`UserAuditModel`
//! at `db/mixins.py:19-44`, `SoftDeleteModel` at `db/mixins.py:64`, and
//! `ProjectBaseModel` at `db/models/project.py:302-311`), then the model
//! fields in declaration order as recorded in the fixture. The inherited
//! prefix is `id`-first by family convention (same prefix as the D-20
//! `cycle` port and the D-27 `app_cycles` port): live `_meta` reports the
//! same 8 columns with `id` sixth, so membership — not ordinal position —
//! is the contract against Django. FK entries use the Django attnames
//! (`project_id`, `workspace_id`, `lead_id`, `module_id`, `member_id`,
//! `issue_id`, `user_id`). Every application-level default below is
//! Django-side (the live tables carry no `column_default`); Rust inserts
//! must supply these values explicitly.
//!
//! # `members` declares no column
//!
//! `Module.members` (`module.py:87-93`) is a `ManyToManyField` through
//! `ModuleMember`: Django stores the membership rows in `module_members`
//! only, so `modules` has no `members` column. [`COLUMNS`] therefore skips
//! the fixture's `members` entry and the tests pin that exclusion; the
//! membership helpers live on [`module_member`].
//!
//! # Reads are soft-delete scoped
//!
//! All five tables inherit the soft-delete marker (`deleted_at`, from
//! `SoftDeleteModel` in `pi_dash/db/mixins.py:64-89`) and declare no
//! custom manager, so the default manager on every table is
//! `SoftDeletionManager` (`mixins.py:56-58`), which filters
//! `deleted_at IS NULL`; `all_objects` is the plain unscoped manager
//! (`mixins.py:67`). Every read built from these tables must apply
//! [`objects_condition`]; the tests pin this by rendering a scoped
//! `SELECT` per table. Reads serve from the per-table soft-delete views
//! (`<table>_active`, [`crate::soft_delete::active_view_ddl`]); writes hit
//! the tables so the partial unique indexes keep working. The partial
//! unique constraints stay as they are (tombstones are excluded by the
//! `deleted_at IS NULL` condition, so a deleted link can be re-created).
//!
//! # `archived_at` guards (views, not the model)
//!
//! `Module.archived_at` itself is a plain nullable `DateTimeField`
//! (`module.py:98`) with no manager behind it; the live/archived split is
//! enforced in `api/views/module.py`: list (`:272`) and detail (`:473`)
//! filter `archived_at__isnull=True`, the archived list (`:899`) filters
//! `archived_at__isnull=False`, patch refuses an archived row with
//! `{"error": "Archived module cannot be edited"}` (`:412-416`), and
//! archive requires `status` in `["completed", "cancelled"]` with
//! `{"error": "Only completed or cancelled modules can be archived"}`
//! (`:1039-1043`). This module provides [`live_condition`],
//! [`archived_condition`], [`is_editable`] and [`can_archive`] so the
//! queries/handlers layers compile those exact guards; the response bodies
//! themselves are owned there.
//!
//! # Writes carry explicit request context
//!
//! `ProjectBaseModel.save()` (`db/models/project.py:309-311`) sets
//! `workspace` from `project.workspace` on every save, and `BaseModel.save`
//! (`db/models/base.py:23-46`) stamps `created_by`/`updated_by` from the
//! requesting user. Rust inserts/updates must therefore resolve
//! `workspace_id` from the `project_id` row explicitly and stamp the audit
//! columns from the [`crate::RequestContext`] actor — no write path takes
//! an unscoped handle. The pure halves of the save rules live here
//! ([`new_sort_order`], [`smallest_sort_order_sql`]); the
//! DB reads that feed them are owned by the queries layer (PIDASHCONV-308).
//!
//! # No models-scope ported bugs
//!
//! `module.py:1-217` was read line by line for this layer and carries no
//! behavior that mistranslates: the `save()` adding-only guard (`:116`)
//! and the `None`-keeps-default branch (`:121-122`) are ported exactly;
//! `unique_together` including `deleted_at` (`:102`, `:135`, `:157`,
//! `:203`) is Django's standard soft-delete pattern, ported as-is
//! alongside the partial unique constraints; `status choices=` (`:75-82`)
//! is form-validation only (no DB constraint), so the default `"planned"`
//! plus the value consts below is a complete port. The recorded module
//! bugs all live outside this layer (unrouted issue-detail GET, duplicate
//! match-loop shapes, link-create message, archive estimate asymmetry) and
//! are owned by the sibling D-20 issues. One verbatim quirk is preserved:
//! `ModuleUserProperties.Meta.verbose_name_plural` is `"Module User
//! Property"` (singular, `module.py:212`), unlike every other plural —
//! ported as-is under `module_user_properties::VERBOSE_NAME_PLURAL`.

use crate::license::models::OnDelete;
use sea_query::{Alias, Condition, Expr};
use serde::{Deserialize, Serialize};

/// `modules` table (`module.py:67-127`).
///
/// Physical table (`Meta.db_table`, `module.py:112`).
pub const TABLE: &str = "modules";
/// Soft-delete read view (`modules_active`).
pub const VIEW: &str = "modules_active";
/// Default ordering (`Meta.ordering`, `module.py:113`).
pub const ORDERING: &str = "-created_at";
/// `verbose_name` (`module.py:110`).
pub const VERBOSE_NAME: &str = "Module";
/// `verbose_name_plural` (`module.py:111`).
pub const VERBOSE_NAME_PLURAL: &str = "Modules";
/// `Meta.unique_together` (`module.py:102`), Django field names.
pub const UNIQUE_TOGETHER: &[&str] = &["name", "project", "deleted_at"];
/// Partial unique constraint name (`module.py:107`).
pub const UNIQUE_NAME: &str = "module_unique_name_project_when_deleted_at_null";
/// Columns of [`UNIQUE_NAME`] (`module.py:105`), physical.
pub const UNIQUE_COLUMNS: &[&str] = &["name", "project_id"];
/// `WHERE` of [`UNIQUE_NAME`] (`module.py:106`,
/// `deleted_at__isnull=True`): the name is unique among live rows only,
/// so a soft-deleted module name can be re-created.
pub const UNIQUE_WHERE: &str = "deleted_at IS NULL";

/// Columns in fixture FX-CYCMOD-01 order: 8 inherited audit/project
/// columns, then `module.py:68-99` in declaration order, skipping the
/// `members` M2M (`module.py:87-93`, through-table only, no column).
/// FK columns use the Django attnames.
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
/// `external_source` bound (`module.py:96`, `max_length=255`).
pub const EXTERNAL_SOURCE_MAX_LENGTH: usize = 255;
/// `external_id` bound (`module.py:97`, `max_length=255`).
pub const EXTERNAL_ID_MAX_LENGTH: usize = 255;
/// `status` bound (`module.py:74`, `max_length=20`).
pub const STATUS_MAX_LENGTH: usize = 20;

/// `ModuleStatus.BACKLOG` value (`module.py:59`).
pub const STATUS_BACKLOG: &str = "backlog";
/// `ModuleStatus.PLANNED` value (`module.py:60`).
pub const STATUS_PLANNED: &str = "planned";
/// `ModuleStatus.IN_PROGRESS` value (`module.py:61`).
pub const STATUS_IN_PROGRESS: &str = "in-progress";
/// `ModuleStatus.PAUSED` value (`module.py:62`).
pub const STATUS_PAUSED: &str = "paused";
/// `ModuleStatus.COMPLETED` value (`module.py:63`).
pub const STATUS_COMPLETED: &str = "completed";
/// `ModuleStatus.CANCELLED` value (`module.py:64`).
pub const STATUS_CANCELLED: &str = "cancelled";
/// All `ModuleStatus` values in declaration order (`module.py:58-64`).
pub const STATUS_VALUES: &[&str] = &[
    STATUS_BACKLOG,
    STATUS_PLANNED,
    STATUS_IN_PROGRESS,
    STATUS_PAUSED,
    STATUS_COMPLETED,
    STATUS_CANCELLED,
];
/// `status` Django-side default (`module.py:74,83`, `default="planned"`).
pub const DEFAULT_STATUS: &str = STATUS_PLANNED;
/// Statuses the archive endpoint accepts (`views/module.py:1039`).
pub const ARCHIVE_ALLOWED_STATUSES: &[&str] = &[STATUS_COMPLETED, STATUS_CANCELLED];

/// `sort_order` Django-side default (`module.py:95`,
/// `default=65535`). Kept for the first module of a project, when the
/// `MIN` aggregate is `None` (`module.py:121-122`).
pub const DEFAULT_SORT_ORDER: f64 = 65535.0;
/// Step applied below the project minimum on create (`module.py:122`).
pub const SORT_ORDER_STEP: f64 = 10000.0;
/// `view_props` / `logo_props` Django-side defaults (`module.py:94,99`,
/// `default=dict`): empty JSON object, supplied explicitly on every
/// Rust insert.
pub const EMPTY_PROPS: &str = "{}";

/// `lead` FK: `SET_NULL`, nullable (`module.py:86`).
pub const LEAD_ON_DELETE: OnDelete = OnDelete::SetNull;
/// `project` FK: `CASCADE` (`project.py:303`).
pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
/// `workspace` FK: `CASCADE` (`project.py:304`).
pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
/// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
/// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

/// Default-manager scope for `modules` (`SoftDeletionManager`,
/// `mixins.py:56-58`): `Module` declares no custom manager, so
/// `Module.objects` already excludes tombstones.
pub fn objects_condition() -> Condition {
    crate::soft_delete::active_condition()
}

/// Live-module guard for the list (`views/module.py:272`) and detail
/// (`views/module.py:473`) reads: `archived_at__isnull=True`.
pub fn live_condition() -> Condition {
    Condition::all().add(Expr::col(Alias::new("archived_at")).is_null())
}

/// Archived-list guard (`views/module.py:899`):
/// `archived_at__isnull=False`.
pub fn archived_condition() -> Condition {
    Condition::all().add(Expr::col(Alias::new("archived_at")).is_not_null())
}

/// Patch guard (`views/module.py:412-416`): an archived module cannot
/// be edited. Returns `false` exactly when `archived_at` is set; the
/// `{"error": "Archived module cannot be edited"}` 400 body is owned
/// by the handlers layer.
pub fn is_editable(archived_at: Option<chrono::DateTime<chrono::Utc>>) -> bool {
    archived_at.is_none()
}

/// Archive gate (`views/module.py:1039-1043`): only completed or
/// cancelled modules can be archived. The
/// `{"error": "Only completed or cancelled modules can be archived"}`
/// 400 body is owned by the handlers layer.
pub fn can_archive(status: &str) -> bool {
    ARCHIVE_ALLOWED_STATUSES.contains(&status)
}

/// Render one `Module.save` minimum lookup (`module.py:117-119`) as SQL:
/// `SELECT MIN("sort_order") FROM "modules" WHERE "project_id" = ...`
/// `AND "deleted_at" IS NULL`.
///
/// The `deleted_at IS NULL` conjunct is the default-manager scope
/// (`SoftDeletionManager`, `mixins.py:56-58`): `Module` declares no
/// custom manager, so `Module.objects.filter(project=...)` in `:117`
/// already excludes tombstones.
pub fn smallest_sort_order_sql(project_id: uuid::Uuid) -> String {
    use sea_query::{Func, PostgresQueryBuilder, Query};
    let mut select = Query::select();
    select
        .expr(Func::min(Expr::col(Alias::new("sort_order"))))
        .from(Alias::new(TABLE))
        .cond_where(Expr::col(Alias::new("project_id")).eq(project_id))
        .cond_where(crate::soft_delete::active_condition());
    select.to_string(PostgresQueryBuilder)
}

/// Pure half of `Module.save` (`module.py:116-122`): on create
/// (`_state.adding`) only, `sort_order = smallest - 10000`; when no
/// rows exist (`smallest` is `None`) the Django-side default is kept.
/// Returns `None` to mean "keep `DEFAULT_SORT_ORDER`".
pub fn new_sort_order(smallest: Option<f64>) -> Option<f64> {
    smallest.map(|min| min - SORT_ORDER_STEP)
}

/// One module row. `start_date`/`target_date` are stored dates
/// (`DateField`, `module.py:72-73`); `sort_order` is a float
/// (`FloatField`, `module.py:95`); `status` renders the `ModuleStatus`
/// value verbatim (`module.py:74-84`).
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
    pub status: String,
    pub lead_id: Option<uuid::Uuid>,
    pub view_props: serde_json::Value,
    pub sort_order: f64,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    pub logo_props: serde_json::Value,
}

/// Render an optional date the way Python's `__str__` does:
/// `str(date)` is ISO (`2026-03-01`), `None` renders as `"None"`.
fn date_label(date: Option<chrono::NaiveDate>) -> String {
    date.map(|d| d.to_string())
        .unwrap_or_else(|| "None".to_string())
}

impl std::fmt::Display for Module {
    /// `__str__` (`module.py:126-127`):
    /// `f"{self.name} {self.start_date} {self.target_date}"`.
    /// All three fields live on the row itself, so this is exact —
    /// no join is needed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {}",
            self.name,
            date_label(self.start_date),
            date_label(self.target_date)
        )
    }
}

/// `module_members` membership table (`module.py:130-149`).
pub mod module_member {
    use super::OnDelete;
    use sea_query::{Alias, Condition, Expr};
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `module.py:145`).
    pub const TABLE: &str = "module_members";
    /// Soft-delete read view (`module_members_active`).
    pub const VIEW: &str = "module_members_active";
    /// Default ordering (`Meta.ordering`, `module.py:146`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`module.py:143`).
    pub const VERBOSE_NAME: &str = "Module Member";
    /// `verbose_name_plural` (`module.py:144`).
    pub const VERBOSE_NAME_PLURAL: &str = "Module Members";

    /// Columns in fixture FX-CYCMOD-01 order: 8 inherited audit/project
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
    pub const UNIQUE_MEMBER_NAME: &str = "module_member_unique_module_member_when_deleted_at_null";
    /// Columns of [`UNIQUE_MEMBER_NAME`] (`module.py:138`), physical.
    pub const UNIQUE_MEMBER_COLUMNS: &[&str] = &["module_id", "member_id"];
    /// `WHERE` of [`UNIQUE_MEMBER_NAME`] (`module.py:139`,
    /// `deleted_at__isnull=True`): the membership is unique among live
    /// rows only, so a soft-deleted membership can be re-created.
    pub const UNIQUE_MEMBER_WHERE: &str = "deleted_at IS NULL";

    /// `module` FK: `CASCADE` (`module.py:131`).
    pub const MODULE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `member` FK: `CASCADE` (`module.py:132`).
    pub const MEMBER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// Default-manager scope for `module_members`
    /// (`SoftDeletionManager`, `mixins.py:56-58`): no custom manager is
    /// declared, so `ModuleMember.objects` already excludes tombstones.
    pub fn objects_condition() -> Condition {
        crate::soft_delete::active_condition()
    }

    /// Scope one membership lookup to its module (`module.py:131`):
    /// live rows of a single module.
    pub fn for_module_condition(module_id: uuid::Uuid) -> Condition {
        Condition::all()
            .add(Expr::col(Alias::new("deleted_at")).is_null())
            .add(Expr::col(Alias::new("module_id")).eq(module_id))
    }

    /// One module-membership row. This is also the physical backing of
    /// the `Module.members` M2M (`module.py:87-93`,
    /// `through="ModuleMember"`, `through_fields=("module", "member")`).
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
        /// FKs, so the label takes both joined values; the join itself
        /// is owned by the queries layer.
        pub fn label(module_name: &str, member: &str) -> String {
            format!("{module_name} {member}")
        }
    }
}

/// `module_issues` link table (`module.py:152-171`).
pub mod module_issue {
    use super::OnDelete;
    use sea_query::{Alias, Condition, Expr};
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `module.py:167`).
    pub const TABLE: &str = "module_issues";
    /// Soft-delete read view (`module_issues_active`).
    pub const VIEW: &str = "module_issues_active";
    /// Default ordering (`Meta.ordering`, `module.py:168`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`module.py:165`).
    pub const VERBOSE_NAME: &str = "Module Issue";
    /// `verbose_name_plural` (`module.py:166`).
    pub const VERBOSE_NAME_PLURAL: &str = "Module Issues";

    /// Columns in fixture FX-CYCMOD-01 order: 8 inherited audit/project
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
    pub const UNIQUE_ISSUE_NAME: &str = "module_issue_unique_issue_module_when_deleted_at_null";
    /// Columns of [`UNIQUE_ISSUE_NAME`] (`module.py:160`), physical.
    pub const UNIQUE_ISSUE_COLUMNS: &[&str] = &["issue_id", "module_id"];
    /// `WHERE` of [`UNIQUE_ISSUE_NAME`] (`module.py:161`,
    /// `deleted_at__isnull=True`): the link is unique among live rows
    /// only, so a soft-deleted link can be re-created.
    pub const UNIQUE_ISSUE_WHERE: &str = "deleted_at IS NULL";

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

    /// Default-manager scope for `module_issues` (`SoftDeletionManager`,
    /// `mixins.py:56-58`): no custom manager is declared, so
    /// `ModuleIssue.objects` already excludes tombstones.
    pub fn objects_condition() -> Condition {
        crate::soft_delete::active_condition()
    }

    /// Scope one link lookup to its module (`module.py:153`): live rows
    /// of a single module.
    pub fn for_module_condition(module_id: uuid::Uuid) -> Condition {
        Condition::all()
            .add(Expr::col(Alias::new("deleted_at")).is_null())
            .add(Expr::col(Alias::new("module_id")).eq(module_id))
    }

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
        /// the FKs, so the label takes both joined values; the join
        /// itself is owned by the queries layer.
        pub fn label(module_name: &str, issue_name: &str) -> String {
            format!("{module_name} {issue_name}")
        }
    }
}

/// `module_links` table (`module.py:174-187`).
pub mod module_link {
    use super::OnDelete;
    use sea_query::{Alias, Condition, Expr};
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `module.py:183`).
    pub const TABLE: &str = "module_links";
    /// Soft-delete read view (`module_links_active`).
    pub const VIEW: &str = "module_links_active";
    /// Default ordering (`Meta.ordering`, `module.py:184`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`module.py:181`).
    pub const VERBOSE_NAME: &str = "Module Link";
    /// `verbose_name_plural` (`module.py:182`).
    pub const VERBOSE_NAME_PLURAL: &str = "Module Links";

    /// Columns in fixture FX-CYCMOD-01 order: 8 inherited audit/project
    /// columns, then `module.py:175-178` in declaration order. FK
    /// columns use the Django attnames.
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

    /// `title` bound (`module.py:175`, `max_length=255`).
    pub const TITLE_MAX_LENGTH: usize = 255;

    /// `module` FK: `CASCADE` (`module.py:177`,
    /// `related_name="link_module"`).
    pub const MODULE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// Default-manager scope for `module_links` (`SoftDeletionManager`,
    /// `mixins.py:56-58`): no custom manager is declared, so
    /// `ModuleLink.objects` already excludes tombstones.
    pub fn objects_condition() -> Condition {
        crate::soft_delete::active_condition()
    }

    /// Scope one link lookup to its module (`module.py:177`): live rows
    /// of a single module.
    pub fn for_module_condition(module_id: uuid::Uuid) -> Condition {
        Condition::all()
            .add(Expr::col(Alias::new("deleted_at")).is_null())
            .add(Expr::col(Alias::new("module_id")).eq(module_id))
    }

    /// One module-link row. `metadata` defaults to `{}` Django-side
    /// (`module.py:178`, `default=dict`), supplied explicitly on every
    /// Rust insert.
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
        /// module FK, so the label takes the joined module name; the
        /// join itself is owned by the queries layer.
        pub fn label(module_name: &str, url: &str) -> String {
            format!("{module_name} {url}")
        }
    }
}

/// `module_user_properties` table (`module.py:190-217`).
pub mod module_user_properties {
    use super::OnDelete;
    use sea_query::{Alias, Condition, Expr};
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `module.py:213`).
    pub const TABLE: &str = "module_user_properties";
    /// Soft-delete read view (`module_user_properties_active`).
    pub const VIEW: &str = "module_user_properties_active";
    /// Default ordering (`Meta.ordering`, `module.py:214`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`module.py:211`).
    pub const VERBOSE_NAME: &str = "Module User Property";
    /// `verbose_name_plural` (`module.py:212`): singular in Python,
    /// ported verbatim (see the module docs).
    pub const VERBOSE_NAME_PLURAL: &str = "Module User Property";

    /// Columns in fixture FX-CYCMOD-01 order: 8 inherited audit/project
    /// columns, then `module.py:191-200` in declaration order. FK
    /// columns use the Django attnames.
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
    pub const UNIQUE_TOGETHER: &[&str] = &["module", "user", "deleted_at"];
    /// Partial unique constraint name (`module.py:208`).
    pub const UNIQUE_MODULE_USER_NAME: &str =
        "module_user_properties_unique_module_user_when_deleted_at_null";
    /// Columns of [`UNIQUE_MODULE_USER_NAME`] (`module.py:206`),
    /// physical.
    pub const UNIQUE_MODULE_USER_COLUMNS: &[&str] = &["module_id", "user_id"];
    /// `WHERE` of [`UNIQUE_MODULE_USER_NAME`] (`module.py:207`,
    /// `deleted_at__isnull=True`): one live properties row per
    /// (module, user); a soft-deleted row can be replaced.
    pub const UNIQUE_MODULE_USER_WHERE: &str = "deleted_at IS NULL";

    /// `module` FK: `CASCADE` (`module.py:191`).
    pub const MODULE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `user` FK: `CASCADE` (`module.py:192-196`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// Default-manager scope for `module_user_properties`
    /// (`SoftDeletionManager`, `mixins.py:56-58`): no custom manager is
    /// declared, so the default queryset already excludes tombstones.
    pub fn objects_condition() -> Condition {
        crate::soft_delete::active_condition()
    }

    /// Per-user scoping: the live properties row for one (module, user)
    /// pair. Every read and write of this table is scoped this way (the
    /// partial unique constraint enforces at most one live row per
    /// pair); callers additionally bind `project_id`/`workspace_id` from
    /// the module row and stamp audit columns from the request context.
    pub fn for_module_user_condition(module_id: uuid::Uuid, user_id: uuid::Uuid) -> Condition {
        Condition::all()
            .add(Expr::col(Alias::new("deleted_at")).is_null())
            .add(Expr::col(Alias::new("module_id")).eq(module_id))
            .add(Expr::col(Alias::new("user_id")).eq(user_id))
    }

    /// `filters` Django-side default (`module.py:197`,
    /// `default=get_default_filters`, `:14-25`). Fresh value per call,
    /// matching the Python function returning a new dict each time.
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

    /// `display_filters` Django-side default (`module.py:198`,
    /// `default=get_default_display_filters`, `:28-37`). Fresh value
    /// per call.
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

    /// `display_properties` Django-side default (`module.py:199`,
    /// `default=get_default_display_properties`, `:40-55`). Fresh value
    /// per call.
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

    /// One module-user-properties row. The JSON columns carry the
    /// Django-side defaults above on insert (same as the Python
    /// callables); `rich_filters` defaults to `{}` (`module.py:200`,
    /// `default=dict`).
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
        /// the FKs, so the label takes both joined values; the join
        /// itself is owned by the queries layer.
        pub fn label(module_name: &str, user_email: &str) -> String {
            format!("{module_name} {user_email}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super as module;
    use super::module_issue;
    use super::module_link;
    use super::module_member;
    use super::module_user_properties::{
        self, default_display_filters, default_display_properties, default_filters,
    };
    use super::OnDelete;
    use super::{can_archive, is_editable, new_sort_order, smallest_sort_order_sql};
    use sea_query::{Alias, Condition, PostgresQueryBuilder, Query};

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/v1_cycles_modules/models")
    }

    fn fixture() -> serde_json::Value {
        let path = fixtures_dir().join("module.columns.json");
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read module.columns.json: {e}"));
        serde_json::from_str(&body).expect("module.columns.json is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Fixture field names mapped to physical columns: the fixture records
    /// Django field names (`module`, `member`, `issue`, `user`, `lead`),
    /// while `COLUMNS` uses the attnames Django actually stores
    /// (`module_id`, `member_id`, `issue_id`, `user_id`, `lead_id`). A
    /// `ForeignKey` declaration maps `name` to `{name}_id`; every other
    /// field maps to itself. The `members` M2M is skipped: it declares
    /// no column on `modules` (through-table only).
    fn field_names(v: &serde_json::Value, model: &str) -> Vec<String> {
        v["models"][model]["fields"]
            .as_array()
            .expect("fields is an array")
            .iter()
            .filter_map(|f| {
                let name = f["name"].as_str().expect("field name");
                let func = f["decl"]["func"].as_str().unwrap_or("");
                if func.contains("ManyToManyField") {
                    None
                } else if func.contains("ForeignKey") {
                    Some(format!("{name}_id"))
                } else {
                    Some(name.to_string())
                }
            })
            .collect()
    }

    fn unique_together(v: &serde_json::Value, model: &str) -> Vec<String> {
        v["models"][model]["meta"]["unique_together"]
            .as_array()
            .expect("unique_together is an array")
            .iter()
            .map(|c| c.as_str().expect("constraint column").to_string())
            .collect()
    }

    /// The fixture records `Meta.constraints` as a truncated AST dump: it
    /// carries the `UniqueConstraint` node plus the constrained fields but
    /// is cut off mid-`condition`. Pin that recorded shape here; the
    /// `deleted_at__isnull=True` condition and the constraint name are
    /// pinned against the Python source lines cited on each
    /// `UNIQUE_*_WHERE` / `UNIQUE_*_NAME` const (`module.py:104-108`,
    /// `:136-141`, `:158-163`, `:204-209`).
    fn constraint_shape(v: &serde_json::Value, model: &str, fields: &[&str]) {
        let raw = v["models"][model]["meta"]["constraints"]
            .as_str()
            .expect("constraints recorded as text");
        assert!(
            raw.contains("UniqueConstraint"),
            "{model} has a UniqueConstraint"
        );
        for field in fields {
            assert!(raw.contains(field), "{model} constraint covers {field}");
        }
    }

    fn select_where(table: &str, cond: Condition) -> String {
        let mut q = Query::select();
        q.column(Alias::new("id"))
            .from(Alias::new(table.to_owned()))
            .cond_where(cond);
        q.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn module_columns_match_fixture() {
        let v = fixture();
        let mut expected: Vec<String> = [
            "id",
            "created_at",
            "updated_at",
            "created_by_id",
            "updated_by_id",
            "deleted_at",
            "project_id",
            "workspace_id",
        ]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
        expected.extend(field_names(&v, "Module"));
        assert_eq!(owned(module::COLUMNS), expected);
        assert_eq!(module::COLUMNS.len(), 22);
        assert!(
            !module::COLUMNS.contains(&"members"),
            "members M2M declares no column on modules"
        );
        let table: &str = module::TABLE;
        assert_eq!(table, v["models"]["Module"]["meta"]["db_table"]);
        assert_eq!(table, "modules");
        assert_eq!(module::VIEW, "modules_active");
        let ordering: &str = module::ORDERING;
        assert_eq!(ordering, v["models"]["Module"]["meta"]["ordering"][0]);
        assert_eq!(ordering, "-created_at");
        assert_eq!(module::VERBOSE_NAME, "Module");
        assert_eq!(module::VERBOSE_NAME_PLURAL, "Modules");
        let together: Vec<String> = owned(module::UNIQUE_TOGETHER);
        assert_eq!(together, unique_together(&v, "Module"));
        assert_eq!(together, vec!["name", "project", "deleted_at"]);
        let name: &str = module::UNIQUE_NAME;
        assert_eq!(name, "module_unique_name_project_when_deleted_at_null");
        constraint_shape(&v, "Module", &["name", "project"]);
        let cols: Vec<String> = owned(module::UNIQUE_COLUMNS);
        assert_eq!(cols, vec!["name", "project_id"]);
        let where_clause: &str = module::UNIQUE_WHERE;
        assert_eq!(where_clause, "deleted_at IS NULL");
        assert_eq!(module::DEFAULT_SORT_ORDER, 65535.0);
        assert_eq!(module::SORT_ORDER_STEP, 10000.0);
        assert_eq!(module::EMPTY_PROPS, "{}");
        assert_eq!(module::NAME_MAX_LENGTH, 255);
        assert_eq!(module::EXTERNAL_SOURCE_MAX_LENGTH, 255);
        assert_eq!(module::EXTERNAL_ID_MAX_LENGTH, 255);
        assert_eq!(module::STATUS_MAX_LENGTH, 20);
        assert_eq!(module::LEAD_ON_DELETE, OnDelete::SetNull);
        assert_eq!(module::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(module::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn module_status_values_match_fixture() {
        let v = fixture();
        let recorded: Vec<String> = v["module_status_values"]
            .as_array()
            .expect("module_status_values is an array")
            .iter()
            .map(|e| {
                e.as_array().expect("status entry")[1]
                    .as_str()
                    .expect("status value")
                    .to_string()
            })
            .collect();
        let values: Vec<String> = owned(module::STATUS_VALUES);
        assert_eq!(values, recorded);
        assert_eq!(
            values,
            vec![
                "backlog",
                "planned",
                "in-progress",
                "paused",
                "completed",
                "cancelled"
            ]
        );
        let default: &str = module::DEFAULT_STATUS;
        assert_eq!(default, "planned");
        assert_eq!(default, module::STATUS_PLANNED);
        assert_eq!(module::STATUS_BACKLOG, "backlog");
        assert_eq!(module::STATUS_IN_PROGRESS, "in-progress");
        assert_eq!(module::STATUS_PAUSED, "paused");
        assert_eq!(module::STATUS_COMPLETED, "completed");
        assert_eq!(module::STATUS_CANCELLED, "cancelled");
    }

    #[test]
    fn member_columns_match_fixture() {
        let v = fixture();
        let mut expected: Vec<String> = [
            "id",
            "created_at",
            "updated_at",
            "created_by_id",
            "updated_by_id",
            "deleted_at",
            "project_id",
            "workspace_id",
        ]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
        expected.extend(field_names(&v, "ModuleMember"));
        assert_eq!(owned(module_member::COLUMNS), expected);
        assert_eq!(module_member::COLUMNS.len(), 10);
        let table: &str = module_member::TABLE;
        assert_eq!(table, v["models"]["ModuleMember"]["meta"]["db_table"]);
        assert_eq!(table, "module_members");
        assert_eq!(module_member::VIEW, "module_members_active");
        let ordering: &str = module_member::ORDERING;
        assert_eq!(ordering, v["models"]["ModuleMember"]["meta"]["ordering"][0]);
        let together: Vec<String> = owned(module_member::UNIQUE_TOGETHER);
        assert_eq!(together, unique_together(&v, "ModuleMember"));
        assert_eq!(together, vec!["module", "member", "deleted_at"]);
        let name: &str = module_member::UNIQUE_MEMBER_NAME;
        assert_eq!(
            name,
            "module_member_unique_module_member_when_deleted_at_null"
        );
        constraint_shape(&v, "ModuleMember", &["module", "member"]);
        let cols: Vec<String> = owned(module_member::UNIQUE_MEMBER_COLUMNS);
        assert_eq!(cols, vec!["module_id", "member_id"]);
        let where_clause: &str = module_member::UNIQUE_MEMBER_WHERE;
        assert_eq!(where_clause, "deleted_at IS NULL");
        assert_eq!(module_member::MODULE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module_member::MEMBER_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn issue_columns_match_fixture() {
        let v = fixture();
        let mut expected: Vec<String> = [
            "id",
            "created_at",
            "updated_at",
            "created_by_id",
            "updated_by_id",
            "deleted_at",
            "project_id",
            "workspace_id",
        ]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
        expected.extend(field_names(&v, "ModuleIssue"));
        assert_eq!(owned(module_issue::COLUMNS), expected);
        assert_eq!(module_issue::COLUMNS.len(), 10);
        let table: &str = module_issue::TABLE;
        assert_eq!(table, v["models"]["ModuleIssue"]["meta"]["db_table"]);
        assert_eq!(table, "module_issues");
        assert_eq!(module_issue::VIEW, "module_issues_active");
        let ordering: &str = module_issue::ORDERING;
        assert_eq!(ordering, v["models"]["ModuleIssue"]["meta"]["ordering"][0]);
        let together: Vec<String> = owned(module_issue::UNIQUE_TOGETHER);
        assert_eq!(together, unique_together(&v, "ModuleIssue"));
        assert_eq!(together, vec!["issue", "module", "deleted_at"]);
        let name: &str = module_issue::UNIQUE_ISSUE_NAME;
        assert_eq!(
            name,
            "module_issue_unique_issue_module_when_deleted_at_null"
        );
        constraint_shape(&v, "ModuleIssue", &["issue", "module"]);
        let cols: Vec<String> = owned(module_issue::UNIQUE_ISSUE_COLUMNS);
        assert_eq!(cols, vec!["issue_id", "module_id"]);
        let where_clause: &str = module_issue::UNIQUE_ISSUE_WHERE;
        assert_eq!(where_clause, "deleted_at IS NULL");
        assert_eq!(module_issue::MODULE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module_issue::ISSUE_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn link_columns_match_fixture() {
        let v = fixture();
        let mut expected: Vec<String> = [
            "id",
            "created_at",
            "updated_at",
            "created_by_id",
            "updated_by_id",
            "deleted_at",
            "project_id",
            "workspace_id",
        ]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
        expected.extend(field_names(&v, "ModuleLink"));
        assert_eq!(owned(module_link::COLUMNS), expected);
        assert_eq!(module_link::COLUMNS.len(), 12);
        let table: &str = module_link::TABLE;
        assert_eq!(table, v["models"]["ModuleLink"]["meta"]["db_table"]);
        assert_eq!(table, "module_links");
        assert_eq!(module_link::VIEW, "module_links_active");
        let ordering: &str = module_link::ORDERING;
        assert_eq!(ordering, v["models"]["ModuleLink"]["meta"]["ordering"][0]);
        assert_eq!(module_link::VERBOSE_NAME, "Module Link");
        assert_eq!(module_link::VERBOSE_NAME_PLURAL, "Module Links");
        assert_eq!(module_link::TITLE_MAX_LENGTH, 255);
        assert_eq!(module_link::MODULE_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn user_properties_columns_match_fixture() {
        let v = fixture();
        let mut expected: Vec<String> = [
            "id",
            "created_at",
            "updated_at",
            "created_by_id",
            "updated_by_id",
            "deleted_at",
            "project_id",
            "workspace_id",
        ]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
        expected.extend(field_names(&v, "ModuleUserProperties"));
        assert_eq!(owned(module_user_properties::COLUMNS), expected);
        assert_eq!(module_user_properties::COLUMNS.len(), 14);
        let table: &str = module_user_properties::TABLE;
        assert_eq!(
            table,
            v["models"]["ModuleUserProperties"]["meta"]["db_table"]
        );
        assert_eq!(table, "module_user_properties");
        assert_eq!(
            module_user_properties::VIEW,
            "module_user_properties_active"
        );
        let ordering: &str = module_user_properties::ORDERING;
        assert_eq!(
            ordering,
            v["models"]["ModuleUserProperties"]["meta"]["ordering"][0]
        );
        // Verbatim quirk: the Python plural is singular
        // (`module.py:212`); the fixture pins the same value.
        assert_eq!(
            module_user_properties::VERBOSE_NAME_PLURAL,
            v["models"]["ModuleUserProperties"]["meta"]["verbose_name_plural"]
        );
        let together: Vec<String> = owned(module_user_properties::UNIQUE_TOGETHER);
        assert_eq!(together, unique_together(&v, "ModuleUserProperties"));
        assert_eq!(together, vec!["module", "user", "deleted_at"]);
        let name: &str = module_user_properties::UNIQUE_MODULE_USER_NAME;
        assert_eq!(
            name,
            "module_user_properties_unique_module_user_when_deleted_at_null"
        );
        constraint_shape(&v, "ModuleUserProperties", &["module", "user"]);
        let cols: Vec<String> = owned(module_user_properties::UNIQUE_MODULE_USER_COLUMNS);
        assert_eq!(cols, vec!["module_id", "user_id"]);
        let where_clause: &str = module_user_properties::UNIQUE_MODULE_USER_WHERE;
        assert_eq!(where_clause, "deleted_at IS NULL");
        assert_eq!(module_user_properties::MODULE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(module_user_properties::USER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            default_filters(),
            serde_json::json!({
                "priority": null, "state": null, "state_group": null,
                "assignees": null, "created_by": null, "labels": null,
                "start_date": null, "target_date": null, "subscriber": null,
            })
        );
        assert_eq!(
            default_display_filters()["order_by"],
            serde_json::json!("-created_at")
        );
        assert_eq!(
            default_display_filters()["sub_issue"],
            serde_json::json!(true)
        );
        assert_eq!(default_display_properties()["key"], serde_json::json!(true));
    }

    #[test]
    fn reads_are_soft_delete_scoped() {
        for (table, cond) in [
            (module::TABLE, module::objects_condition()),
            (module_member::TABLE, module_member::objects_condition()),
            (module_issue::TABLE, module_issue::objects_condition()),
            (module_link::TABLE, module_link::objects_condition()),
            (
                module_user_properties::TABLE,
                module_user_properties::objects_condition(),
            ),
        ] {
            assert_eq!(
                select_where(table, cond),
                format!("SELECT \"id\" FROM \"{table}\" WHERE \"deleted_at\" IS NULL")
            );
            assert_eq!(
                crate::soft_delete::active_view_ddl(table),
                format!(
                    "CREATE OR REPLACE VIEW {table}_active AS SELECT * FROM \"{table}\" WHERE \"deleted_at\" IS NULL;"
                )
            );
        }
    }

    #[test]
    fn archived_guards_match_views() {
        assert_eq!(
            select_where(module::TABLE, module::live_condition()),
            "SELECT \"id\" FROM \"modules\" WHERE \"archived_at\" IS NULL"
        );
        assert_eq!(
            select_where(module::TABLE, module::archived_condition()),
            "SELECT \"id\" FROM \"modules\" WHERE \"archived_at\" IS NOT NULL"
        );
        assert!(is_editable(None));
        assert!(!is_editable(Some(chrono::Utc::now())));
        assert!(can_archive("completed"));
        assert!(can_archive("cancelled"));
        assert!(!can_archive("planned"));
        assert!(!can_archive("backlog"));
        assert!(!can_archive("in-progress"));
        assert!(!can_archive("paused"));
        assert!(!can_archive(""));
    }

    #[test]
    fn scoping_helpers_match_django_semantics() {
        let mid = uuid::Uuid::new_v4();
        let user = uuid::Uuid::new_v4();
        for (table, cond) in [
            (
                module_member::TABLE,
                module_member::for_module_condition(mid),
            ),
            (module_issue::TABLE, module_issue::for_module_condition(mid)),
            (module_link::TABLE, module_link::for_module_condition(mid)),
        ] {
            let sql = select_where(table, cond);
            assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
            assert!(sql.contains("\"module_id\""), "{sql}");
        }
        let sql = select_where(
            module_user_properties::TABLE,
            module_user_properties::for_module_user_condition(mid, user),
        );
        assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
        assert!(sql.contains("\"module_id\""), "{sql}");
        assert!(sql.contains("\"user_id\""), "{sql}");
    }

    #[test]
    fn smallest_sort_order_sql_matches_django_semantics() {
        let project = uuid::Uuid::nil();
        let sql = smallest_sort_order_sql(project);
        assert!(sql.contains("MIN(\"sort_order\")"), "{sql}");
        assert!(sql.contains("\"modules\""), "{sql}");
        assert!(sql.contains("\"project_id\""), "{sql}");
        assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
        assert_eq!(new_sort_order(None), None);
        assert_eq!(new_sort_order(Some(55000.0)), Some(45000.0));
        assert_eq!(module::DEFAULT_SORT_ORDER, 65535.0);
    }

    #[test]
    fn display_impls_match_str() {
        let now = chrono::Utc::now();
        let project = uuid::Uuid::nil();
        let row = module::Module {
            id: uuid::Uuid::nil(),
            created_at: now,
            updated_at: now,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: project,
            workspace_id: uuid::Uuid::nil(),
            name: "Auth".to_string(),
            description: String::new(),
            description_text: None,
            description_html: None,
            start_date: chrono::NaiveDate::from_ymd_opt(2026, 3, 1),
            target_date: None,
            status: module::DEFAULT_STATUS.to_string(),
            lead_id: None,
            view_props: serde_json::json!({}),
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            archived_at: None,
            logo_props: serde_json::json!({}),
        };
        assert_eq!(format!("{row}"), "Auth 2026-03-01 None");
        let undated = module::Module {
            start_date: None,
            target_date: None,
            ..row.clone()
        };
        assert_eq!(format!("{undated}"), "Auth None None");
        assert_eq!(
            module_member::ModuleMember::label("Auth", "a@x.com"),
            "Auth a@x.com"
        );
        assert_eq!(
            module_issue::ModuleIssue::label("Auth", "Login bug"),
            "Auth Login bug"
        );
        assert_eq!(
            module_link::ModuleLink::label("Auth", "https://x.example/s"),
            "Auth https://x.example/s"
        );
        assert_eq!(
            module_user_properties::ModuleUserProperties::label("Auth", "a@x.com"),
            "Auth a@x.com"
        );
    }
}
