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
}
