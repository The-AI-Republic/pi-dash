//! Cycle app table models (D-27, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/cycle.py:1-157` (`Cycle`,
//! `CycleIssue`, `CycleUserProperties` plus `get_default_filters`,
//! `get_default_display_filters`, `get_default_display_properties`),
//! adopting the Django-owned schema column-for-column; migrations are not
//! ported — Django stays schema owner until switchover.
//!
//! Column order in each `*_COLUMNS` const follows the fixture F-C27-02
//! (`rust-api/fixtures/app_cycles/models/columns.json`): the 8 inherited
//! audit/project columns first (`id`, `created_at`, `updated_at`,
//! `created_by_id`, `updated_by_id`, `deleted_at`, `project_id`,
//! `workspace_id`, from `BaseModel` at `db/models/base.py:17-18`,
//! `TimeAuditModel`/`UserAuditModel` at `db/mixins.py:21-44`,
//! `SoftDeleteModel` at `db/mixins.py:67`, and `ProjectBaseModel` at
//! `db/models/project.py:302-311`), then the model fields in declaration
//! order. FK entries use the Django attnames (`project_id`,
//! `workspace_id`, `owned_by_id`, `issue_id`, `cycle_id`, `user_id`).
//! Every application-level default below is Django-side (the live tables
//! carry no `column_default` in `information_schema`, as established for
//! D-01); Rust inserts must supply these values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! All three tables inherit the soft-delete marker (`deleted_at`, from
//! `SoftDeleteModel` in `pi_dash/db/mixins.py:57-69`) and the default
//! manager filters `deleted_at IS NULL` (`objects = SoftDeletionManager`,
//! `mixins.py:56-58`; `all_objects` is the plain unscoped manager,
//! `mixins.py:67`). Every read built from these tables must apply
//! [`crate::soft_delete::active_condition`]; the tests pin this by
//! rendering a scoped `SELECT` per table. The partial unique constraints
//! stay as they are (tombstones are excluded by the `deleted_at IS NULL`
//! condition, so a deleted link can be re-created).
//!
//! # Writes backfill the workspace
//!
//! `ProjectBaseModel.save()` (`db/models/project.py:309-311`) sets
//! `workspace` from `project.workspace` on every save: Rust inserts/updates
//! must resolve `workspace_id` from the `project_id` row explicitly.
//!
//! # No models-scope ported bugs
//!
//! `cycle.py:1-157` was read line by line for this layer and carries no
//! behavior that mistranslates: the `save()` adding-only guard (`:89`)
//! and the `None`-keeps-default branch (`:94-95`) are ported exactly;
//! `unique_together` including `deleted_at` (`:113`, `:143`) is Django's
//! standard soft-delete pattern, ported as-is alongside the partial
//! unique constraints; `choices=TIMEZONE_CHOICES` (`:79`) is
//! form-validation only (no DB constraint), so the default `"UTC"` plus
//! the documented source of the allowed values is a complete port.
//! The recorded cycle bugs all live outside this layer (views date
//! ordering, transfer refusal shape, progress/analytics vectors) and are
//! owned by the sibling D-27 issues.

use serde::{Deserialize, Serialize};

/// Django-level FK delete behavior (ORM-emulated; same shape as the
/// D-03 `loop::models::OnDelete`, D-05 `integrations::OnDelete`, D-10
/// `tasks_ticker::models::OnDelete` and D-32 `app_intake::models::OnDelete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// `cycles` table (`cycle.py:60-101`).
pub mod cycle {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `cycle.py:85`).
    pub const TABLE: &str = "cycles";
    /// Default ordering (`Meta.ordering`, `cycle.py:86`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` / `verbose_name_plural` (`cycle.py:83-84`).
    pub const VERBOSE_NAME: &str = "Cycle";
    /// `verbose_name_plural` (`cycle.py:84`).
    pub const VERBOSE_NAME_PLURAL: &str = "Cycles";

    /// Columns in fixture F-C27-02 order: 8 inherited audit/project
    /// columns, then `cycle.py:61-80` in declaration order. FK columns
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
        "name",
        "description",
        "start_date",
        "end_date",
        "owned_by_id",
        "view_props",
        "sort_order",
        "external_source",
        "external_id",
        "progress_snapshot",
        "archived_at",
        "logo_props",
        "timezone",
        "version",
    ];

    /// `name` bound (`cycle.py:61`, `max_length=255`).
    pub const NAME_MAX_LENGTH: usize = 255;
    /// `external_source` bound (`cycle.py:72`, `max_length=255`).
    pub const EXTERNAL_SOURCE_MAX_LENGTH: usize = 255;
    /// `external_id` bound (`cycle.py:73`, `max_length=255`).
    pub const EXTERNAL_ID_MAX_LENGTH: usize = 255;
    /// `timezone` bound (`cycle.py:79`, `max_length=255`).
    pub const TIMEZONE_MAX_LENGTH: usize = 255;

    /// `sort_order` Django-side default (`cycle.py:71`,
    /// `default=65535`). Kept for the first cycle of a project, when the
    /// `MIN` aggregate is `None` (`cycle.py:94-95`).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;
    /// Step applied below the project minimum on create (`cycle.py:95`).
    pub const SORT_ORDER_STEP: f64 = 10000.0;
    /// `timezone` default (`cycle.py:79`, `default="UTC"`).
    pub const DEFAULT_TIMEZONE: &str = "UTC";
    /// Source of the allowed `timezone` values (`cycle.py:78-79`):
    /// `pytz.common_timezones`. Django `choices=` is form-validation
    /// only — it creates no DB constraint — so the default plus this
    /// documented source is the complete port; Rust inserts must supply
    /// an IANA name from that set explicitly.
    pub const TIMEZONE_CHOICES_SOURCE: &str = "pytz.common_timezones";
    /// `version` default (`cycle.py:80`, `default=1`).
    pub const DEFAULT_VERSION: i32 = 1;
    /// `view_props` / `progress_snapshot` / `logo_props` Django-side
    /// defaults (`cycle.py:70,74,76`, `default=dict`): empty JSON object,
    /// supplied explicitly on every Rust insert.
    pub const EMPTY_PROPS: &str = "{}";

    /// `owned_by` FK: `CASCADE` (`cycle.py:65-69`).
    pub const OWNED_BY_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:31-37`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:38-44`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One cycle row. `start_date`/`end_date` are stored UTC
    /// (`DateTimeField`, `cycle.py:63-64`); `sort_order` is a float
    /// (`FloatField`, `cycle.py:71`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Cycle {
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
        pub start_date: Option<chrono::DateTime<chrono::Utc>>,
        pub end_date: Option<chrono::DateTime<chrono::Utc>>,
        pub owned_by_id: uuid::Uuid,
        pub view_props: serde_json::Value,
        pub sort_order: f64,
        pub external_source: Option<String>,
        pub external_id: Option<String>,
        pub progress_snapshot: serde_json::Value,
        pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
        pub logo_props: serde_json::Value,
        pub timezone: String,
        pub version: i32,
    }

    impl std::fmt::Display for Cycle {
        /// `__str__` (`cycle.py:99-101`): `"{name} <{project.name}>"`.
        /// Rust holds only the project FK, so the display renders the
        /// project id; the name join is owned by the queries layer.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} <{}>", self.name, self.project_id)
        }
    }

    /// Render the `Cycle.save` minimum lookup (`cycle.py:88-97`) as SQL:
    /// `SELECT MIN("sort_order") FROM "cycles" WHERE "project_id" = ...`
    /// `AND "deleted_at" IS NULL`.
    ///
    /// The `deleted_at IS NULL` conjunct is the default-manager scope
    /// (`SoftDeletionManager`, `mixins.py:56-58`): `Cycle` declares no
    /// custom manager, so `Cycle.objects.filter(project=...)` in `:90`
    /// already excludes tombstones. The caller binds no params — the
    /// project id is rendered literally, matching the probe style used
    /// for the read scopes below.
    pub fn smallest_sort_order_sql(project_id: uuid::Uuid) -> String {
        use sea_query::{Alias, Expr, Func, PostgresQueryBuilder, Query};
        let mut select = Query::select();
        select
            .expr(Func::min(Expr::col(Alias::new("sort_order"))))
            .from(Alias::new(TABLE))
            .cond_where(Expr::col(Alias::new("project_id")).eq(project_id))
            .cond_where(crate::soft_delete::active_condition());
        select.to_string(PostgresQueryBuilder)
    }

    /// Pure half of `Cycle.save` (`cycle.py:89-95`): on create
    /// (`_state.adding`) only, `sort_order = smallest - 10000`; when no
    /// rows exist (`smallest` is `None`) the Django-side default is kept.
    /// Returns `None` to mean "keep `DEFAULT_SORT_ORDER`".
    pub fn new_sort_order(smallest: Option<f64>) -> Option<f64> {
        smallest.map(|min| min - SORT_ORDER_STEP)
    }
}

/// `cycle_issues` link table (`cycle.py:104-127`).
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

    /// Columns in fixture F-C27-02 order: 8 inherited audit/project
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

    /// `issue` FK: `CASCADE` (`cycle.py:109`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `cycle` FK: `CASCADE` (`cycle.py:110`).
    pub const CYCLE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:31-37`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:38-44`).
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
        /// `__str__` (`cycle.py:126-127`): `"{self.cycle}"`, i.e. the
        /// cycle's own `__str__`. Rust holds only the cycle FK, so the
        /// display renders the cycle id; the join is owned by the
        /// queries layer.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.cycle_id)
        }
    }
}

/// `cycle_user_properties` table (`cycle.py:130-157`).
pub mod cycle_user_properties {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `cycle.py:153`).
    pub const TABLE: &str = "cycle_user_properties";
    /// Default ordering (`Meta.ordering`, `cycle.py:154`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`cycle.py:151`).
    pub const VERBOSE_NAME: &str = "Cycle User Property";
    /// `verbose_name_plural` (`cycle.py:152`).
    pub const VERBOSE_NAME_PLURAL: &str = "Cycle User Properties";

    /// Columns in fixture F-C27-02 order: 8 inherited audit/project
    /// columns, then `cycle.py:131-140` in declaration order. FK columns
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
        "cycle_id",
        "user_id",
        "filters",
        "display_filters",
        "display_properties",
        "rich_filters",
    ];

    /// `Meta.unique_together` (`cycle.py:143`), Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["cycle", "user", "deleted_at"];
    /// Partial unique constraint name (`cycle.py:148`).
    pub const UNIQUE_CYCLE_USER_NAME: &str =
        "cycle_user_properties_unique_cycle_user_when_deleted_at_null";
    /// Columns of [`UNIQUE_CYCLE_USER_NAME`] (`cycle.py:146`), physical.
    pub const UNIQUE_CYCLE_USER_COLUMNS: &[&str] = &["cycle_id", "user_id"];
    /// `WHERE` of [`UNIQUE_CYCLE_USER_NAME`] (`cycle.py:147`,
    /// `deleted_at__isnull=True`): one live properties row per
    /// (cycle, user); a soft-deleted row can be replaced.
    pub const UNIQUE_CYCLE_USER_WHERE: &str = "deleted_at IS NULL";

    /// `cycle` FK: `CASCADE` (`cycle.py:131`).
    pub const CYCLE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `user` FK: `CASCADE` (`cycle.py:132-136`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:31-37`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:38-44`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `filters` Django-side default (`cycle.py:137`,
    /// `default=get_default_filters`, `:16-28`). Fresh value per call,
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

    /// `display_filters` Django-side default (`cycle.py:138`,
    /// `default=get_default_display_filters`, `:30-40`). Fresh value per
    /// call.
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

    /// `display_properties` Django-side default (`cycle.py:139`,
    /// `default=get_default_display_properties`, `:42-57`). Fresh value
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

    /// One cycle-user-properties row. The JSON columns carry the
    /// Django-side defaults above on insert (same as the Python
    /// callables); `rich_filters` defaults to `{}` (`cycle.py:140`,
    /// `default=dict`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct CycleUserProperties {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub cycle_id: uuid::Uuid,
        pub user_id: uuid::Uuid,
        pub filters: serde_json::Value,
        pub display_filters: serde_json::Value,
        pub display_properties: serde_json::Value,
        pub rich_filters: serde_json::Value,
    }

    impl CycleUserProperties {
        /// `__str__` (`cycle.py:156-157`): `"{cycle.name} {user.email}"`.
        /// Rust holds only the FKs, so the label takes both joined
        /// values; the join itself is owned by the queries layer.
        pub fn label(cycle_name: &str, user_email: &str) -> String {
            format!("{cycle_name} {user_email}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cycle::{self, new_sort_order, smallest_sort_order_sql};
    use super::cycle_issue;
    use super::cycle_user_properties::{
        self, default_display_filters, default_display_properties, default_filters,
    };
    use super::OnDelete;
    use crate::soft_delete::active_condition;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/app_cycles/models")
    }

    fn fixture() -> serde_json::Value {
        let path = fixtures_dir().join("columns.json");
        let body =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read columns.json: {e}"));
        serde_json::from_str(&body).expect("columns.json is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// `cycle.columns[].name` in order (the object-shaped entries).
    fn cycle_column_names(value: &serde_json::Value) -> Vec<String> {
        value["cycle"]["columns"]
            .as_array()
            .expect("cycle has columns array")
            .iter()
            .map(|c| c["name"].as_str().expect("column entry has name").to_string())
            .collect()
    }

    /// Find a `cycle.columns` entry by column name.
    fn cycle_entry<'a>(value: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        value["cycle"]["columns"]
            .as_array()
            .expect("cycle has columns array")
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("cycle fixture has column {name}"))
    }

    fn unique_together(value: &serde_json::Value, model: &str) -> Vec<String> {
        value[model]["meta"]["unique_together"]
            .as_array()
            .expect("meta has unique_together")
            .iter()
            .map(|f| f.as_str().expect("together entry is str").to_string())
            .collect()
    }

    fn constraint<'a>(value: &'a serde_json::Value, model: &str, name: &str) -> &'a serde_json::Value {
        value[model]["meta"]["constraints"]
            .as_array()
            .expect("meta has constraints")
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("{model} fixture has constraint {name}"))
    }

    #[test]
    fn cycle_columns_match_fixture() {
        let v = fixture();
        assert_eq!(owned(cycle::COLUMNS), cycle_column_names(&v));
        assert_eq!(cycle::COLUMNS.len(), 22);
        let table: &str = cycle::TABLE;
        assert_eq!(table, v["cycle"]["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "cycles");
        let ordering: &str = cycle::ORDERING;
        assert_eq!(ordering, v["cycle"]["meta"]["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        let verbose: &str = cycle::VERBOSE_NAME;
        let verbose_plural: &str = cycle::VERBOSE_NAME_PLURAL;
        assert_eq!(
            format!("{verbose} / {verbose_plural}"),
            v["cycle"]["meta"]["verbose_name"].as_str().unwrap()
        );
        assert_eq!(v["cycle"]["source"].as_str().unwrap(), "db/models/cycle.py:60-101");
    }

    #[test]
    fn cycle_field_details_match_fixture() {
        let v = fixture();
        // Django-side defaults recorded in the fixture type strings.
        for (name, fragment) in [
            ("sort_order", "default=65535"),
            ("timezone", "default='UTC'"),
            ("version", "default=1"),
        ] {
            let ty = cycle_entry(&v, name)["type"].as_str().unwrap();
            assert!(ty.contains(fragment), "{name} fixture type says {ty}");
        }
        assert_eq!(cycle::DEFAULT_SORT_ORDER, 65535.0);
        assert_eq!(cycle::SORT_ORDER_STEP, 10000.0);
        assert_eq!(cycle::DEFAULT_TIMEZONE, "UTC");
        assert_eq!(cycle::DEFAULT_VERSION, 1);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(cycle::EMPTY_PROPS).unwrap(),
            serde_json::json!({})
        );
        // Nullability: audit FKs, datetimes, external ids nullable.
        for name in [
            "created_by_id",
            "updated_by_id",
            "deleted_at",
            "start_date",
            "end_date",
            "external_source",
            "external_id",
            "archived_at",
        ] {
            let ty = cycle_entry(&v, name)["type"].as_str().unwrap();
            assert!(ty.contains("null"), "{name} nullable per fixture ({ty})");
        }
        // Length bounds.
        assert_eq!(cycle::NAME_MAX_LENGTH, 255);
        assert_eq!(cycle::EXTERNAL_SOURCE_MAX_LENGTH, 255);
        assert_eq!(cycle::EXTERNAL_ID_MAX_LENGTH, 255);
        assert_eq!(cycle::TIMEZONE_MAX_LENGTH, 255);
        // FK delete behavior.
        assert_eq!(cycle::OWNED_BY_ON_DELETE, OnDelete::Cascade);
        assert_eq!(cycle::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(cycle::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(cycle::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(cycle::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn cycle_save_sort_order_matches_fixture() {
        let v = fixture();
        let rule = v["cycle"]["save_sort_order"]["rule"].as_str().unwrap();
        assert!(rule.contains("MIN(sort_order over same project) - 10000"), "{rule}");
        assert!(v["cycle"]["save_sort_order"]["first_cycle_keeps_default"]
            .as_bool()
            .unwrap());
        // None (no rows) keeps the Django-side default ...
        assert_eq!(new_sort_order(None), None);
        // ... otherwise smallest minus the step, as f64 arithmetic.
        assert_eq!(new_sort_order(Some(65535.0)), Some(55535.0));
        assert_eq!(new_sort_order(Some(0.0)), Some(-10000.0));
        assert_eq!(new_sort_order(Some(-500.25)), Some(-10500.25));
    }

    #[test]
    fn smallest_sort_order_sql_matches_django_semantics() {
        let project = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let sql = smallest_sort_order_sql(project);
        // MIN aggregate over the cycles table, scoped to the project and
        // to live rows (the default-manager scope in cycle.py:90).
        assert_eq!(
            sql,
            r#"SELECT MIN("sort_order") FROM "cycles" WHERE "project_id" = '11111111-1111-1111-1111-111111111111' AND "deleted_at" IS NULL"#
        );
    }

    #[test]
    fn cycle_issue_columns_match_fixture() {
        let v = fixture();
        // The fixture records the 8 inherited columns once ("same
        // semantics as Cycle"); the probe expands them.
        let inherited: &[&str] = &cycle_issue::COLUMNS[..8];
        assert_eq!(owned(inherited), owned(&cycle::COLUMNS[..8]));
        assert_eq!(owned(&cycle_issue::COLUMNS[8..]), vec!["issue_id", "cycle_id"]);
        assert_eq!(cycle_issue::COLUMNS.len(), 10);
        let table: &str = cycle_issue::TABLE;
        assert_eq!(table, v["cycle_issue"]["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "cycle_issues");
        let ordering: &str = cycle_issue::ORDERING;
        assert_eq!(ordering, v["cycle_issue"]["meta"]["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        let together: Vec<String> = owned(cycle_issue::UNIQUE_TOGETHER);
        assert_eq!(together, unique_together(&v, "cycle_issue"));
        assert_eq!(together, vec!["issue", "cycle", "deleted_at"]);
        let name: &str = cycle_issue::UNIQUE_CYCLE_ISSUE_NAME;
        assert_eq!(name, "cycle_issue_when_deleted_at_null");
        let c = constraint(&v, "cycle_issue", name);
        let cfields: Vec<String> = c["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| match f.as_str().unwrap() {
                "cycle" => "cycle_id".to_string(),
                "issue" => "issue_id".to_string(),
                other => panic!("unexpected constraint field {other}"),
            })
            .collect();
        assert_eq!(owned(cycle_issue::UNIQUE_CYCLE_ISSUE_COLUMNS), cfields);
        assert_eq!(
            c["condition"].as_str().unwrap(),
            cycle_issue::UNIQUE_CYCLE_ISSUE_WHERE
        );
        assert_eq!(cycle_issue::UNIQUE_CYCLE_ISSUE_WHERE, "deleted_at IS NULL");
        assert_eq!(cycle_issue::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(cycle_issue::CYCLE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(cycle_issue::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(cycle_issue::WORKSPACE_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn cycle_user_properties_columns_match_fixture() {
        let v = fixture();
        let inherited: &[&str] = &cycle_user_properties::COLUMNS[..8];
        assert_eq!(owned(inherited), owned(&cycle::COLUMNS[..8]));
        assert_eq!(
            owned(&cycle_user_properties::COLUMNS[8..]),
            vec![
                "cycle_id",
                "user_id",
                "filters",
                "display_filters",
                "display_properties",
                "rich_filters",
            ]
        );
        assert_eq!(cycle_user_properties::COLUMNS.len(), 14);
        let table: &str = cycle_user_properties::TABLE;
        assert_eq!(
            table,
            v["cycle_user_properties"]["meta"]["db_table"].as_str().unwrap()
        );
        assert_eq!(table, "cycle_user_properties");
        let ordering: &str = cycle_user_properties::ORDERING;
        assert_eq!(
            ordering,
            v["cycle_user_properties"]["meta"]["ordering"][0].as_str().unwrap()
        );
        assert_eq!(ordering, "-created_at");
        let together: Vec<String> = owned(cycle_user_properties::UNIQUE_TOGETHER);
        assert_eq!(together, unique_together(&v, "cycle_user_properties"));
        assert_eq!(together, vec!["cycle", "user", "deleted_at"]);
        let name: &str = cycle_user_properties::UNIQUE_CYCLE_USER_NAME;
        assert_eq!(name, "cycle_user_properties_unique_cycle_user_when_deleted_at_null");
        let c = constraint(&v, "cycle_user_properties", name);
        let cfields: Vec<String> = c["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| match f.as_str().unwrap() {
                "cycle" => "cycle_id".to_string(),
                "user" => "user_id".to_string(),
                other => panic!("unexpected constraint field {other}"),
            })
            .collect();
        assert_eq!(owned(cycle_user_properties::UNIQUE_CYCLE_USER_COLUMNS), cfields);
        assert_eq!(
            c["condition"].as_str().unwrap(),
            cycle_user_properties::UNIQUE_CYCLE_USER_WHERE
        );
        assert_eq!(cycle_user_properties::UNIQUE_CYCLE_USER_WHERE, "deleted_at IS NULL");
        assert_eq!(cycle_user_properties::CYCLE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(cycle_user_properties::USER_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn user_property_defaults_match_fixture() {
        let v = fixture();
        assert_eq!(default_filters(), v["defaults"]["get_default_filters"]);
        assert_eq!(
            default_display_filters(),
            v["defaults"]["get_default_display_filters"]
        );
        assert_eq!(
            default_display_properties(),
            v["defaults"]["get_default_display_properties"]
        );
        // Key counts pin silent additions/drops (9 / 7 / 13).
        assert_eq!(default_filters().as_object().unwrap().len(), 9);
        assert_eq!(default_display_filters().as_object().unwrap().len(), 7);
        assert_eq!(default_display_properties().as_object().unwrap().len(), 13);
        // Order-sensitive spot checks (None vs absent key matters).
        assert_eq!(
            default_display_filters()["order_by"],
            serde_json::json!("-created_at")
        );
        assert_eq!(
            default_display_filters()["calendar_date_range"],
            serde_json::json!("")
        );
    }

    #[test]
    fn user_property_defaults_are_fresh_per_call() {
        // Mirrors the Python callables returning a new dict each time:
        // mutating one caller's value must not leak into the next.
        let mut first = default_filters();
        first["priority"] = serde_json::json!(["high"]);
        assert_eq!(default_filters()["priority"], serde_json::Value::Null);
        let mut disp = default_display_filters();
        disp["layout"] = serde_json::json!("board");
        assert_eq!(default_display_filters()["layout"], serde_json::json!("list"));
    }

    #[test]
    fn reads_are_soft_delete_scoped() {
        for table in [
            cycle::TABLE,
            cycle_issue::TABLE,
            cycle_user_properties::TABLE,
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
        let row = cycle::Cycle {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            name: "Sprint 1".to_string(),
            description: String::new(),
            start_date: None,
            end_date: None,
            owned_by_id: uuid::Uuid::nil(),
            view_props: serde_json::json!({}),
            sort_order: cycle::DEFAULT_SORT_ORDER,
            external_source: None,
            external_id: None,
            progress_snapshot: serde_json::json!({}),
            archived_at: None,
            logo_props: serde_json::json!({}),
            timezone: cycle::DEFAULT_TIMEZONE.to_string(),
            version: cycle::DEFAULT_VERSION,
        };
        assert_eq!(row.to_string(), format!("Sprint 1 <{}>", uuid::Uuid::nil()));
        let link = cycle_issue::CycleIssue {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            issue_id: uuid::Uuid::nil(),
            cycle_id: uuid::Uuid::nil(),
        };
        assert_eq!(link.to_string(), uuid::Uuid::nil().to_string());
        assert_eq!(
            cycle_user_properties::CycleUserProperties::label("Sprint 1", "a@x.com"),
            "Sprint 1 a@x.com"
        );
    }
}
