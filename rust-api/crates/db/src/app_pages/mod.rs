//! Page app table models (D-30, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/page.py:1-182`: `Page` (`:23-77`),
//! `PageLog` (`:80-117`), `PageLabel` (`:120-132`), `ProjectPage`
//! (`:135-155`) and `PageVersion` (`:158-182`), adopting the Django-owned
//! schema column-for-column; migrations are not ported — Django stays
//! schema owner until switchover.
//!
//! Column order in each `*_COLUMNS` const follows the Django `_meta` field
//! order recorded in `rust-api/fixtures/app_pages/models/page_columns.json`
//! (F30-04) and `through_columns.json` (F30-05): `BaseModel.id`
//! (`db/models/base.py:18`), audit columns (`TimeAuditModel`,
//! `UserAuditModel`, `SoftDeleteModel` in `db/mixins.py:19-73`, i.e.
//! `created_at`, `updated_at`, `created_by_id`, `updated_by_id`,
//! `deleted_at`), then the model's own fields in declaration order. FK
//! entries use the Django attnames (`workspace_id`, `owned_by_id`,
//! `parent_id`, …). The `labels` / `projects` `ManyToManyField`s
//! (`page.py:39,52`) create no column on `pages` (their rows live in
//! `page_labels` / `project_pages`); the issue's "20 columns" are the 22
//! recorded fields minus those two. Every application-level default below
//! is Django-side (the live tables carry no `column_default` in
//! `information_schema`, as established for D-01); Rust inserts must supply
//! these values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! All five tables inherit the soft-delete marker (`deleted_at`, from
//! `SoftDeleteModel` in `pi_dash/db/mixins.py:57-73`) and the default
//! manager filters `deleted_at IS NULL` (`objects = SoftDeletionManager`,
//! `mixins.py:56-58`; `all_objects` is the plain unscoped manager,
//! `mixins.py:67`). Every read built from these tables must apply
//! [`crate::soft_delete::active_condition`]; the tests pin this by
//! rendering a scoped `SELECT` per table. The partial unique constraint on
//! project pages stays as it is (tombstones are excluded by the
//! `deleted_at IS NULL` condition, so a deleted project link can be
//! recreated).
//!
//! # Writes recompute the stripped text on every save
//!
//! `Page.save()` (`page.py:70-77`) and `PageVersion.save()` (`:175-182`)
//! unconditionally recompute `description_stripped` — including saves that
//! touch nothing textual (lock/unlock/access/parent-detach). The port keeps
//! that: [`strip::sync_description_stripped`] runs on every write. The
//! rule itself is `None` when `description_html` is `""` or `None`, else
//! `strip_tags` of the HTML (`utils/html_processor.py`, MLStripper).
//!
//! # Ported bugs and display substitutions
//!
//! * `save()` recomputes `description_stripped` on every save even when
//!   only flags change (recorded in F30-04 `bugs`); ported as-is via
//!   [`strip::sync_description_stripped`].
//! * `Page.__str__` (`page.py:66-68`, `"{owned_by.email} <{name}>"`),
//!   `PageLog.__str__` (`:116-117`, `"{page.name} {entity_name}"`),
//!   `PageLabel.__str__` (`:131-132`, `"{page.name} {label.name}"`) and
//!   `ProjectPage.__str__` (`:154-155`, `"{project.name} {page.name}"`)
//!   dereference related rows; a row carries only the FK ids, so each
//!   `Display` renders the id in place of the joined name (same
//!   substitution as the D-29 display ports).

pub mod strip;

mod entities;

use serde::{Deserialize, Serialize};

/// Django-level FK delete behavior (ORM-emulated; same shape as the D-32
/// `app_intake::models::OnDelete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// Page visibility (`page.py:24-28,37`).
///
/// Stored as a `PositiveSmallIntegerField` with `default=0`; the class
/// consts declare private first (`PRIVATE_ACCESS = 1`, `PUBLIC_ACCESS =
/// 0`, `:24-25`) while the field `choices` list public first
/// (`((0, "Public"), (1, "Private"))`, `:37`). Both orders are preserved
/// below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PageAccess {
    /// Public page (`0`, the field default).
    Public,
    /// Private page (`1`).
    Private,
}

impl PageAccess {
    /// The stored integer (`page.py:37`).
    pub fn as_i16(self) -> i16 {
        match self {
            PageAccess::Public => 0,
            PageAccess::Private => 1,
        }
    }

    /// The human label (`choices=` label, `page.py:37`).
    pub fn label(self) -> &'static str {
        match self {
            PageAccess::Public => "Public",
            PageAccess::Private => "Private",
        }
    }

    /// Parse a stored integer; `None` for values Django never writes.
    pub fn from_i16(value: i16) -> Option<PageAccess> {
        match value {
            0 => Some(PageAccess::Public),
            1 => Some(PageAccess::Private),
            _ => None,
        }
    }

    /// Both values in field-`choices` order (public first, `page.py:37`).
    pub const ALL: &[PageAccess] = &[PageAccess::Public, PageAccess::Private];
}

/// `pages` table (`page.py:23-77`).
pub mod page {
    use super::{OnDelete, PageAccess};
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `page.py:63`).
    pub const TABLE: &str = "pages";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`, `page.py:64`).
    pub const ORDERING: &str = "-created_at";

    /// `Meta.verbose_name` (`page.py:61`).
    pub const VERBOSE_NAME: &str = "Page";
    /// `Meta.verbose_name_plural` (`page.py:62`).
    pub const VERBOSE_NAME_PLURAL: &str = "Pages";

    /// Class const (`page.py:24`).
    pub const PRIVATE_ACCESS: i16 = 1;
    /// Class const (`page.py:25`).
    pub const PUBLIC_ACCESS: i16 = 0;
    /// `DEFAULT_SORT_ORDER` (`page.py:26`).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;

    /// Columns in Django `_meta` field order (matches F30-04
    /// `models/page_columns.json`): `BaseModel.id`, audit columns, then
    /// `page.py:30-58` in declaration order. FK entries use the Django
    /// attnames. The `labels` (`:39`) and `projects` (`:52`)
    /// `ManyToManyField`s hold no column here.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "name",
        "description_json",
        "description_binary",
        "description_html",
        "description_stripped",
        "owned_by_id",
        "access",
        "color",
        "parent_id",
        "archived_at",
        "is_locked",
        "view_props",
        "logo_props",
        "is_global",
        "moved_to_page",
        "moved_to_project",
        "sort_order",
        "external_id",
        "external_source",
    ];

    /// `access` default (`page.py:37`): public.
    pub const DEFAULT_ACCESS: i16 = 0;
    /// [`DEFAULT_ACCESS`] as the enum value.
    pub const DEFAULT_ACCESS_ENUM: PageAccess = PageAccess::Public;
    /// `sort_order` default (`page.py:55`, [`DEFAULT_SORT_ORDER`]).
    pub const DEFAULT_SORT_ORDER_VALUE: f64 = DEFAULT_SORT_ORDER;
    /// `description_html` default (`page.py:34`).
    pub const DEFAULT_DESCRIPTION_HTML: &str = "<p></p>";
    /// `is_locked` default (`page.py:48`).
    pub const DEFAULT_IS_LOCKED: bool = false;
    /// `is_global` default (`page.py:51`).
    pub const DEFAULT_IS_GLOBAL: bool = false;

    /// `name` has no bound (`TextField`, `page.py:31`).
    /// `color` bound (`page.py:38`, `max_length=255`).
    pub const COLOR_MAX_LENGTH: usize = 255;
    /// `external_id` bound (`page.py:57`, `max_length=255`).
    pub const EXTERNAL_ID_MAX_LENGTH: usize = 255;
    /// `external_source` bound (`page.py:58`, `max_length=255`).
    pub const EXTERNAL_SOURCE_MAX_LENGTH: usize = 255;

    /// `workspace` FK: `CASCADE`, non-nullable (`page.py:30`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `owned_by` FK: `CASCADE`, non-nullable (`page.py:36`).
    pub const OWNED_BY_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `parent` self FK: `CASCADE`, nullable (`page.py:40-46`).
    pub const PARENT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` / `updated_by` audit FKs: `SET_NULL`, nullable
    /// (from `UserAuditModel`, `db/mixins.py:27-40`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// See [`CREATED_BY_ON_DELETE`].
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `description_json` application default (`default=dict`, `page.py:32`).
    pub fn default_description_json() -> serde_json::Value {
        serde_json::json!({})
    }

    /// `view_props` application default (`get_view_props`, `page.py:19-20`).
    pub fn default_view_props() -> serde_json::Value {
        serde_json::json!({"full_width": false})
    }

    /// `logo_props` application default (`default=dict`, `page.py:50`).
    pub fn default_logo_props() -> serde_json::Value {
        serde_json::json!({})
    }

    /// One page row. `name` (`TextField(blank=True)`, `page.py:31`) and
    /// `color` (`CharField(blank=True)`, `:38`) store `""`, never `NULL`;
    /// `description_binary` (`BinaryField(null=True)`, `:33`),
    /// `description_stripped` (`TextField(blank=True, null=True)`, `:35`,
    /// maintained by `save()`), `parent` (`:40-46`), `archived_at`
    /// (`DateField(null=True)`, `:47`), `moved_to_page`/`moved_to_project`
    /// (`UUIDField(null=True, blank=True)`, `:53-54`) and
    /// `external_id`/`external_source` (`:57-58`) are nullable;
    /// `description_json`, `view_props` and `logo_props` store JSON
    /// objects, never `NULL` (`JSONField` with dict defaults).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Page {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub name: String,
        pub description_json: serde_json::Value,
        pub description_binary: Option<Vec<u8>>,
        pub description_html: String,
        pub description_stripped: Option<String>,
        pub owned_by_id: uuid::Uuid,
        pub access: i16,
        pub color: String,
        pub parent_id: Option<uuid::Uuid>,
        pub archived_at: Option<chrono::NaiveDate>,
        pub is_locked: bool,
        pub view_props: serde_json::Value,
        pub logo_props: serde_json::Value,
        pub is_global: bool,
        pub moved_to_page: Option<uuid::Uuid>,
        pub moved_to_project: Option<uuid::Uuid>,
        pub sort_order: f64,
        pub external_id: Option<String>,
        pub external_source: Option<String>,
    }

    impl Page {
        /// The visibility as the enum; `None` for integers Django never
        /// writes (see [`PageAccess::from_i16`]).
        pub fn access_enum(&self) -> Option<PageAccess> {
            PageAccess::from_i16(self.access)
        }
    }

    impl std::fmt::Display for Page {
        /// `__str__` (`page.py:66-68`,
        /// `"{owned_by.email} <{name}>"`). The row carries only
        /// `owned_by_id`; the email is a queries-layer join, so this
        /// renders the id in its place.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} <{}>", self.owned_by_id, self.name)
        }
    }
}

/// `page_logs` table (`page.py:80-117`).
pub mod page_log {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `page.py:106`).
    pub const TABLE: &str = "page_logs";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`, `page.py:107`).
    pub const ORDERING: &str = "-created_at";

    /// `Meta.verbose_name` (`page.py:104`).
    pub const VERBOSE_NAME: &str = "Page Log";
    /// `Meta.verbose_name_plural` (`page.py:105`).
    pub const VERBOSE_NAME_PLURAL: &str = "Page Logs";

    /// `Meta.unique_together` (`page.py:103`): field names as Django
    /// spells them.
    pub const UNIQUE_TOGETHER: &[&str] = &["page", "transaction"];

    /// `Meta.indexes` (`page.py:108-114`): `(name, columns)` in source order.
    pub const INDEXES: &[(&str, &[&str])] = &[
        ("pagelog_entity_type_idx", &["entity_type"]),
        ("pagelog_entity_id_idx", &["entity_identifier"]),
        ("pagelog_entity_name_idx", &["entity_name"]),
        ("pagelog_type_id_idx", &["entity_type", "entity_identifier"]),
        ("pagelog_name_id_idx", &["entity_name", "entity_identifier"]),
    ];

    /// Columns in Django `_meta` field order (matches F30-05
    /// `models/through_columns.json`): `BaseModel.id`, audit columns, then
    /// `page.py:95-100` in declaration order. FK entries use the Django
    /// attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "transaction",
        "page_id",
        "entity_identifier",
        "entity_name",
        "entity_type",
        "workspace_id",
    ];

    /// `TYPE_CHOICES` values (`page.py:81-94`): a plain tuple describing
    /// the transaction vocabulary, not a field `choices=` (neither
    /// `entity_name` nor `entity_type` declares one).
    pub const TYPE_CHOICES: &[&str] = &[
        "to_do",
        "issue",
        "image",
        "video",
        "file",
        "link",
        "cycle",
        "module",
        "back_link",
        "forward_link",
        "page_mention",
        "user_mention",
    ];

    /// `entity_name` bound (`page.py:98`, `max_length=30`).
    pub const ENTITY_NAME_MAX_LENGTH: usize = 30;
    /// `entity_type` bound (`page.py:99`, `max_length=30`).
    pub const ENTITY_TYPE_MAX_LENGTH: usize = 30;

    /// `page` FK: `CASCADE`, non-nullable (`page.py:96`).
    pub const PAGE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE`, non-nullable (`page.py:100`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` / `updated_by` audit FKs: `SET_NULL`, nullable
    /// (from `UserAuditModel`, `db/mixins.py:27-40`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// See [`CREATED_BY_ON_DELETE`].
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One page-log row. `transaction` carries a `uuid4` application
    /// default (`page.py:95`); `entity_identifier`
    /// (`UUIDField(null=True, blank=True)`, `:97`) and `entity_type`
    /// (`CharField(max_length=30, null=True, blank=True)`, `:99`) are
    /// nullable; `entity_name` (`CharField(max_length=30)`, `:98`) stores
    /// `""`, never `NULL`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct PageLog {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub transaction: uuid::Uuid,
        pub page_id: uuid::Uuid,
        pub entity_identifier: Option<uuid::Uuid>,
        pub entity_name: String,
        pub entity_type: Option<String>,
        pub workspace_id: uuid::Uuid,
    }

    impl std::fmt::Display for PageLog {
        /// `__str__` (`page.py:116-117`, `"{page.name} {entity_name}"`).
        /// The row carries only `page_id`; the name is a queries-layer
        /// join, so this renders the id in its place.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} {}", self.page_id, self.entity_name)
        }
    }
}

/// `page_labels` table (`page.py:120-132`).
pub mod page_label {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `page.py:128`).
    pub const TABLE: &str = "page_labels";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`, `page.py:129`).
    pub const ORDERING: &str = "-created_at";

    /// `Meta.verbose_name` (`page.py:126`).
    pub const VERBOSE_NAME: &str = "Page Label";
    /// `Meta.verbose_name_plural` (`page.py:127`).
    pub const VERBOSE_NAME_PLURAL: &str = "Page Labels";

    /// `Meta.unique_together`: none (`page.py:125-129` declares no
    /// `unique_together` and no `constraints`) — duplicate `(page, label)`
    /// rows are allowed at DB level (recorded in F30-05).
    pub const UNIQUE_TOGETHER: &[&str] = &[];

    /// Columns in Django `_meta` field order (matches F30-05): base
    /// columns, then `page.py:121-123` in declaration order. FK entries use
    /// the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "label_id",
        "page_id",
        "workspace_id",
    ];

    /// `label` FK: `CASCADE`, non-nullable (`page.py:121`).
    pub const LABEL_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `page` FK: `CASCADE`, non-nullable (`page.py:122`).
    pub const PAGE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE`, non-nullable (`page.py:123`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` / `updated_by` audit FKs: `SET_NULL`, nullable
    /// (from `UserAuditModel`, `db/mixins.py:27-40`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// See [`CREATED_BY_ON_DELETE`].
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One page-label link row: three non-nullable FKs.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct PageLabel {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub label_id: uuid::Uuid,
        pub page_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
    }

    impl std::fmt::Display for PageLabel {
        /// `__str__` (`page.py:131-132`,
        /// `"{page.name} {label.name}"`). The row carries only the FK ids;
        /// the names are queries-layer joins, so this renders the ids in
        /// their place.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} {}", self.page_id, self.label_id)
        }
    }
}

/// `project_pages` table (`page.py:135-155`).
pub mod project_page {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `page.py:151`).
    pub const TABLE: &str = "project_pages";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`, `page.py:152`).
    pub const ORDERING: &str = "-created_at";

    /// `Meta.verbose_name` (`page.py:149`).
    pub const VERBOSE_NAME: &str = "Project Page";
    /// `Meta.verbose_name_plural` (`page.py:150`).
    pub const VERBOSE_NAME_PLURAL: &str = "Project Pages";

    /// `Meta.unique_together` (`page.py:141`): field names as Django
    /// spells them.
    pub const UNIQUE_TOGETHER: &[&str] = &["project", "page", "deleted_at"];
    /// Partial unique constraint backing the live-link scope
    /// (`page.py:142-148`).
    pub const UNIQUE_PROJECT_PAGE_NAME: &str =
        "project_page_unique_project_page_when_deleted_at_null";
    /// Columns of [`UNIQUE_PROJECT_PAGE_NAME`] (`page.py:144`).
    pub const UNIQUE_PROJECT_PAGE_COLUMNS: &[&str] = &["project_id", "page_id"];
    /// `WHERE` of the partial unique index (`page.py:145`,
    /// `deleted_at__isnull=True`): the link is unique per project among
    /// live rows only.
    pub const UNIQUE_PROJECT_PAGE_WHERE: &str = "deleted_at IS NULL";

    /// Columns in Django `_meta` field order (matches F30-05): base
    /// columns, then `page.py:136-138` in declaration order. FK entries use
    /// the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "page_id",
        "workspace_id",
    ];

    /// `project` FK: `CASCADE`, non-nullable (`page.py:136`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `page` FK: `CASCADE`, non-nullable (`page.py:137`).
    pub const PAGE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE`, non-nullable (`page.py:138`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` / `updated_by` audit FKs: `SET_NULL`, nullable
    /// (from `UserAuditModel`, `db/mixins.py:27-40`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// See [`CREATED_BY_ON_DELETE`].
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One project-page link row: three non-nullable FKs.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ProjectPage {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub page_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
    }

    impl std::fmt::Display for ProjectPage {
        /// `__str__` (`page.py:154-155`,
        /// `"{project.name} {page.name}"`). The row carries only the FK
        /// ids; the names are queries-layer joins, so this renders the ids
        /// in their place.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} {}", self.project_id, self.page_id)
        }
    }
}

/// `page_versions` table (`page.py:158-182`).
pub mod page_version {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `page.py:172`).
    pub const TABLE: &str = "page_versions";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`, `page.py:173`).
    pub const ORDERING: &str = "-created_at";

    /// `Meta.verbose_name` (`page.py:170`).
    pub const VERBOSE_NAME: &str = "Page Version";
    /// `Meta.verbose_name_plural` (`page.py:171`).
    pub const VERBOSE_NAME_PLURAL: &str = "Page Versions";

    /// `Meta.unique_together`: none (`page.py:169-173` declares no
    /// `unique_together` and no `constraints`).
    pub const UNIQUE_TOGETHER: &[&str] = &[];

    /// Columns in Django `_meta` field order (matches F30-05): base
    /// columns, then `page.py:159-167` in declaration order. FK entries use
    /// the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "page_id",
        "last_saved_at",
        "owned_by_id",
        "description_binary",
        "description_html",
        "description_stripped",
        "description_json",
        "sub_pages_data",
    ];

    /// `description_html` default (`page.py:164`).
    pub const DEFAULT_DESCRIPTION_HTML: &str = "<p></p>";

    /// `workspace` FK: `CASCADE`, non-nullable (`page.py:159`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `page` FK: `CASCADE`, non-nullable (`page.py:160`).
    pub const PAGE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `owned_by` FK: `CASCADE`, non-nullable (`page.py:162`).
    pub const OWNED_BY_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` / `updated_by` audit FKs: `SET_NULL`, nullable
    /// (from `UserAuditModel`, `db/mixins.py:27-40`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// See [`CREATED_BY_ON_DELETE`].
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `description_json` application default (`default=dict`, `page.py:166`).
    pub fn default_description_json() -> serde_json::Value {
        serde_json::json!({})
    }

    /// `sub_pages_data` application default (`default=dict`, `page.py:167`).
    pub fn default_sub_pages_data() -> serde_json::Value {
        serde_json::json!({})
    }

    /// One page-version row. `last_saved_at` carries a `timezone.now`
    /// application default (`page.py:161`); `description_binary`
    /// (`BinaryField(null=True)`, `:163`) and `description_stripped`
    /// (`TextField(blank=True, null=True)`, `:165`, maintained by
    /// `save()`) are nullable; `description_html` (`:164`) stores `""`,
    /// never `NULL`; `description_json` and `sub_pages_data` store JSON
    /// objects, never `NULL`. (`PageVersion` declares no `__str__`.)
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct PageVersion {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub page_id: uuid::Uuid,
        pub last_saved_at: chrono::DateTime<chrono::Utc>,
        pub owned_by_id: uuid::Uuid,
        pub description_binary: Option<Vec<u8>>,
        pub description_html: String,
        pub description_stripped: Option<String>,
        pub description_json: serde_json::Value,
        pub sub_pages_data: serde_json::Value,
    }
}

#[cfg(test)]
mod tests {
    use super::strip::sync_description_stripped;
    use super::*;
    use crate::soft_delete::active_condition;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/app_pages/models")
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

    /// Sorted struct field names from a serialized row: serde field order
    /// follows declaration order, but the comparison is order-free so the
    /// check stays a field-for-field set match either way.
    fn struct_fields<T: serde::Serialize>(row: &T) -> Vec<String> {
        let mut keys: Vec<String> = serde_json::to_value(row)
            .expect("row serializes")
            .as_object()
            .expect("row is a JSON object")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    fn sorted_columns(cols: &[&str]) -> Vec<String> {
        let mut out = owned(cols);
        out.sort();
        out
    }

    /// Map a fixture `columns[].name` (Django field name) to the physical
    /// column: FK `workspace` -> `workspace_id` etc.; `None` for M2M
    /// entries (`labels`, `projects`), which hold no column.
    fn physical_column(name: &str) -> Option<String> {
        match name {
            "labels" | "projects" => None,
            "workspace" => Some("workspace_id".to_string()),
            "owned_by" => Some("owned_by_id".to_string()),
            "parent" => Some("parent_id".to_string()),
            "page" => Some("page_id".to_string()),
            "label" => Some("label_id".to_string()),
            "project" => Some("project_id".to_string()),
            other => Some(other.to_string()),
        }
    }

    /// Owned (post-base) columns of a fixture `columns` array, in order.
    fn fixture_owned_columns(value: &serde_json::Value) -> Vec<String> {
        value["columns"]
            .as_array()
            .expect("fixture has columns array")
            .iter()
            .filter_map(|c| physical_column(c["name"].as_str().expect("column has name")))
            .collect()
    }

    fn epoch() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(0, 0).unwrap()
    }

    fn sample_page() -> page::Page {
        page::Page {
            id: uuid::Uuid::nil(),
            created_at: epoch(),
            updated_at: epoch(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: uuid::Uuid::nil(),
            name: String::new(),
            description_json: serde_json::json!({}),
            description_binary: None,
            description_html: page::DEFAULT_DESCRIPTION_HTML.to_string(),
            description_stripped: None,
            owned_by_id: uuid::Uuid::nil(),
            access: page::DEFAULT_ACCESS,
            color: String::new(),
            parent_id: None,
            archived_at: None,
            is_locked: false,
            view_props: page::default_view_props(),
            logo_props: page::default_logo_props(),
            is_global: false,
            moved_to_page: None,
            moved_to_project: None,
            sort_order: page::DEFAULT_SORT_ORDER,
            external_id: None,
            external_source: None,
        }
    }

    #[test]
    fn page_columns_match_fixture() {
        let v = fixture("page_columns.json");
        // 22 recorded fields minus the 2 M2M entries = the 20 physical
        // columns after the 6 base columns.
        assert_eq!(v["columns"].as_array().unwrap().len(), 22);
        assert_eq!(
            &page::COLUMNS[..6],
            &[
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at"
            ]
        );
        assert_eq!(owned(&page::COLUMNS[6..]), fixture_owned_columns(&v));
        assert_eq!(page::COLUMNS.len(), 26);
        let table: &str = page::TABLE;
        assert_eq!(table, v["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "pages");
        let ordering: &str = page::ORDERING;
        assert_eq!(ordering, v["meta"]["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        assert_eq!(v["meta"]["verbose_name"].as_str().unwrap(), "Page");
        assert_eq!(v["meta"]["verbose_name_plural"].as_str().unwrap(), "Pages");
        let verbose: &str = page::VERBOSE_NAME;
        assert_eq!(verbose, "Page");
        let verbose_plural: &str = page::VERBOSE_NAME_PLURAL;
        assert_eq!(verbose_plural, "Pages");
        // Class consts (page.py:24-26) with the private-first declaration
        // order preserved.
        let (private, public): (i16, i16) = (page::PRIVATE_ACCESS, page::PUBLIC_ACCESS);
        assert_eq!((private, public), (1, 0));
        let access: i16 = page::DEFAULT_ACCESS;
        assert_eq!(access, 0);
        assert_eq!(access, page::DEFAULT_ACCESS_ENUM.as_i16());
        let sort: f64 = page::DEFAULT_SORT_ORDER;
        assert_eq!(sort, 65535.0);
        assert_eq!(sort, page::DEFAULT_SORT_ORDER_VALUE);
        let html: &str = page::DEFAULT_DESCRIPTION_HTML;
        assert_eq!(html, "<p></p>");
        assert_eq!(
            page::default_view_props(),
            serde_json::json!({"full_width": false})
        );
        assert_eq!(page::default_description_json(), serde_json::json!({}));
        assert_eq!(page::default_logo_props(), serde_json::json!({}));
        let (color, eid, esrc): (usize, usize, usize) = (
            page::COLOR_MAX_LENGTH,
            page::EXTERNAL_ID_MAX_LENGTH,
            page::EXTERNAL_SOURCE_MAX_LENGTH,
        );
        assert_eq!((color, eid, esrc), (255, 255, 255));
        assert_eq!(page::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(page::OWNED_BY_ON_DELETE, OnDelete::Cascade);
        assert_eq!(page::PARENT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(page::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(page::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
        // Struct covers the column list field for field.
        assert_eq!(struct_fields(&sample_page()), sorted_columns(page::COLUMNS));
        // Display renders the owner id (the email is a join).
        assert_eq!(
            sample_page().to_string(),
            format!("{} <>", uuid::Uuid::nil())
        );
    }

    #[test]
    fn page_save_vectors_replay() {
        let v = fixture("page_columns.json");
        let vectors = v["save_vectors"].as_array().expect("save_vectors array");
        assert_eq!(vectors.len(), 4);
        for vec in vectors {
            let html: Option<&str> = vec["description_html"].as_str();
            let want: Option<&str> = vec["description_stripped_after"].as_str();
            let got = sync_description_stripped(html).expect("strip succeeds");
            assert_eq!(got.as_deref(), want, "vector {}", vec["source"]);
        }
        // The None-vs-"" split the issue calls out: both yield None, while
        // the default "<p></p>" is non-empty so the stripper runs ("" out).
        assert_eq!(sync_description_stripped(None).unwrap(), None);
        assert_eq!(sync_description_stripped(Some("")).unwrap(), None);
        assert_eq!(
            sync_description_stripped(Some("<p></p>")).unwrap(),
            Some(String::new())
        );
    }

    fn through_model(value: &serde_json::Value, name: &str) -> serde_json::Value {
        value["models"]
            .as_array()
            .expect("models array")
            .iter()
            .find(|m| m["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("fixture has model {name}"))
            .clone()
    }

    #[test]
    fn page_log_columns_match_fixture() {
        let v = fixture("through_columns.json");
        let m = through_model(&v, "PageLog");
        assert_eq!(owned(&page_log::COLUMNS[6..]), fixture_owned_columns(&m));
        assert_eq!(page_log::COLUMNS.len(), 12);
        let table: &str = page_log::TABLE;
        assert_eq!(table, m["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "page_logs");
        let ordering: &str = page_log::ORDERING;
        assert_eq!(ordering, m["meta"]["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        let together: Vec<String> = owned(page_log::UNIQUE_TOGETHER);
        assert_eq!(
            together,
            m["meta"]["unique_together"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f.as_str().unwrap().to_string())
                .collect::<Vec<String>>()
        );
        assert_eq!(
            together,
            vec!["page".to_string(), "transaction".to_string()]
        );
        // All 5 indexes in source order: "name (col, ...)" strings.
        let indexes: Vec<(String, Vec<String>)> = m["indexes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                let s = s.as_str().unwrap();
                let open = s.find('(').unwrap();
                let name = s[..open].trim_end().to_string();
                let cols = s[open + 1..s.len() - 1]
                    .split(", ")
                    .map(|c| c.to_string())
                    .collect();
                (name, cols)
            })
            .collect();
        assert_eq!(indexes.len(), 5);
        for (pos, (name, cols)) in page_log::INDEXES.iter().enumerate() {
            assert_eq!(*name, indexes[pos].0);
            assert_eq!(owned(cols), indexes[pos].1);
        }
        let choices: Vec<String> = m["type_choices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap().to_string())
            .collect();
        assert_eq!(owned(page_log::TYPE_CHOICES), choices);
        assert_eq!(page_log::TYPE_CHOICES.len(), 12);
        let (name_len, type_len): (usize, usize) = (
            page_log::ENTITY_NAME_MAX_LENGTH,
            page_log::ENTITY_TYPE_MAX_LENGTH,
        );
        assert_eq!((name_len, type_len), (30, 30));
        assert_eq!(page_log::PAGE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(page_log::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(page_log::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(page_log::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn page_label_columns_match_fixture() {
        let v = fixture("through_columns.json");
        let m = through_model(&v, "PageLabel");
        assert_eq!(owned(&page_label::COLUMNS[6..]), fixture_owned_columns(&m));
        assert_eq!(page_label::COLUMNS.len(), 9);
        let table: &str = page_label::TABLE;
        assert_eq!(table, m["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "page_labels");
        // No unique_together: the fixture carries no such key.
        assert!(m["meta"].get("unique_together").is_none());
        let empty: &[&str] = page_label::UNIQUE_TOGETHER;
        assert!(empty.is_empty());
        assert_eq!(page_label::LABEL_ON_DELETE, OnDelete::Cascade);
        assert_eq!(page_label::PAGE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(page_label::WORKSPACE_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn project_page_columns_match_fixture() {
        let v = fixture("through_columns.json");
        let m = through_model(&v, "ProjectPage");
        assert_eq!(
            owned(&project_page::COLUMNS[6..]),
            fixture_owned_columns(&m)
        );
        assert_eq!(project_page::COLUMNS.len(), 9);
        let table: &str = project_page::TABLE;
        assert_eq!(table, m["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "project_pages");
        let together: Vec<String> = owned(project_page::UNIQUE_TOGETHER);
        assert_eq!(
            together,
            m["meta"]["unique_together"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f.as_str().unwrap().to_string())
                .collect::<Vec<String>>()
        );
        assert_eq!(
            together,
            vec![
                "project".to_string(),
                "page".to_string(),
                "deleted_at".to_string()
            ]
        );
        // Partial unique constraint: name, physical columns, live-rows scope.
        let name: &str = project_page::UNIQUE_PROJECT_PAGE_NAME;
        assert_eq!(
            name,
            "project_page_unique_project_page_when_deleted_at_null"
        );
        let recorded = m["constraints"][0].as_str().unwrap();
        assert!(recorded.contains(name), "{recorded}");
        assert!(recorded.contains("deleted_at IS NULL"), "{recorded}");
        assert_eq!(
            owned(project_page::UNIQUE_PROJECT_PAGE_COLUMNS),
            vec!["project_id", "page_id"]
        );
        assert_eq!(
            project_page::UNIQUE_PROJECT_PAGE_WHERE,
            "deleted_at IS NULL"
        );
        assert!(recorded.contains(project_page::UNIQUE_PROJECT_PAGE_WHERE));
        assert_eq!(project_page::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(project_page::PAGE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(project_page::WORKSPACE_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn page_version_columns_match_fixture() {
        let v = fixture("through_columns.json");
        let m = through_model(&v, "PageVersion");
        assert_eq!(
            owned(&page_version::COLUMNS[6..]),
            fixture_owned_columns(&m)
        );
        assert_eq!(page_version::COLUMNS.len(), 15);
        let table: &str = page_version::TABLE;
        assert_eq!(table, m["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "page_versions");
        assert!(m["meta"].get("unique_together").is_none());
        let empty: &[&str] = page_version::UNIQUE_TOGETHER;
        assert!(empty.is_empty());
        let html: &str = page_version::DEFAULT_DESCRIPTION_HTML;
        assert_eq!(html, "<p></p>");
        assert_eq!(
            page_version::default_description_json(),
            serde_json::json!({})
        );
        assert_eq!(
            page_version::default_sub_pages_data(),
            serde_json::json!({})
        );
        assert_eq!(page_version::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(page_version::PAGE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(page_version::OWNED_BY_ON_DELETE, OnDelete::Cascade);
        // Same stripped rule as Page (page.py:175-182): None for ""/None,
        // strip otherwise.
        assert_eq!(sync_description_stripped(None).unwrap(), None);
        assert_eq!(sync_description_stripped(Some("")).unwrap(), None);
        assert_eq!(
            sync_description_stripped(Some("<p>Hello <b>World</b></p>")).unwrap(),
            Some("Hello World".to_string())
        );
    }

    #[test]
    fn access_enum_round_trips() {
        for a in PageAccess::ALL {
            assert_eq!(PageAccess::from_i16(a.as_i16()), Some(*a));
        }
        assert_eq!(PageAccess::from_i16(2), None);
        assert_eq!(PageAccess::from_i16(-1), None);
        assert_eq!(PageAccess::Public.label(), "Public");
        assert_eq!(PageAccess::Private.label(), "Private");
        // Field-choices order is public first (page.py:37), even though the
        // class consts declare private first (page.py:24-25).
        assert_eq!(PageAccess::ALL, &[PageAccess::Public, PageAccess::Private]);
    }

    #[test]
    fn strip_edge_cases_match_python() {
        // Verified byte-identical against `html_processor.strip_tags`
        // before committing (see the differential fuzz in the workpad);
        // a selection is pinned here so regressions fail loudly.
        let cases: &[(&str, &str)] = &[
            ("", ""),
            ("<p></p>", ""),
            ("<p>Hello <b>World</b></p>", "Hello World"),
            ("<p>R&amp;D</p>", "R&D"),
            ("<p>&nbsp;hi&nbsp;</p>", "\u{a0}hi\u{a0}"),
            ("&lt;script&gt;", "<script>"),
            ("&#65;&#x42;", "AB"),
            ("&amp;amp;", "&amp;"),
            ("&copy;", "©"),
            ("fish & chips", "fish & chips"),
            ("a < b", "a < b"),
            ("<3", "<3"),
            ("<>", "<>"),
            ("</>", ""),
            ("</p >", ""),
            ("</ div>", ""),
            ("<!--c-->", ""),
            ("<!DOCTYPE html>", ""),
            ("<!foo bar>", ""),
            ("<?php ?>", ""),
            ("<div class=\"a>b\">x</div>", "x"),
            ("<a href='x>y'>t</a>", "t"),
            ("</div class=\"a>b\">", "b\">"),
            ("<div<span>", ""),
            ("<div<span>x", "x"),
            ("<a!", ""),
            ("<div/>", ""),
            ("<div/ >", ""),
            ("<p>a</p><p>b</p>", "ab"),
            ("<p>a</p trailing", "a"),
            ("text<!--c1-->mid<!--c2-->end", "textmidend"),
            ("<script>var x = 1;</script>", "var x = 1;"),
            ("<style>a{color:red}</style>", "a{color:red}"),
            ("<script>a</b>c</script>d", "a</b>cd"),
            ("<script>a<b>c</script>", "a<b>c"),
            // `re.I` fold quirks in the CDATA-end scan: the folded end is
            // found but `endtagfind` still rejects it, so it is kept as
            // text and the mode stays on.
            ("<script>a</ſcript>b", "a</ſcript>"),
            ("<script>a</scrıpt>b", "a</scrıpt>"),
            ("<style>a</ſtyle>b", "a</ſtyle>"),
            ("<script>a</ſtyle>b", ""),
            ("x<script>y</script>z", "xyz"),
            ("&<p>;", "&;"),
            ("&;", "&;"),
            ("&#;", "&#;"),
            ("&#x;", "&#x;"),
            ("&ampersand;", "&ersand;"),
            ("&unknown;", "&unknown;"),
            ("&#0;", "�"),
            ("&#11;", ""),
            ("&#128;", "€"),
            ("&#xD800;", "�"),
        ];
        for (html, want) in cases {
            let got = super::strip::ml_strip_tags(html).expect("strip succeeds");
            assert_eq!(&got, want, "input {html:?}");
        }
        // Dropped tails: `feed` without `close` never emits them (text
        // before the incomplete construct is still kept: "x</" -> "x").
        for html in [
            "R&D",
            "fish &amp",
            "&amp",
            "a&ampb",
            "a <",
            "x</",
            "<!-- unclosed",
            "<? unclosed",
            "<script>X",
            "<script>a</script",
            "<![CDATA[x",
        ] {
            let got = super::strip::ml_strip_tags(html).expect("strip succeeds");
            let want = match html {
                "a <" => "a ",
                "x</" => "x",
                _ => "",
            };
            assert_eq!(&got, want, "input {html:?}");
        }
        // Unknown `<![` keywords raise in Python (`ParserBase.error`).
        for html in ["<![foo]>", "<![>", "x<![foo]>y"] {
            assert!(super::strip::ml_strip_tags(html).is_err(), "input {html:?}");
        }
    }

    #[test]
    fn reads_are_soft_delete_scoped() {
        for table in [
            page::TABLE,
            page_log::TABLE,
            page_label::TABLE,
            project_page::TABLE,
            page_version::TABLE,
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
}
