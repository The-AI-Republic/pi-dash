#![forbid(unsafe_code)]

//! D-20 cycle model reads (PIDASHCONV-291).
//!
//! Ports the column lists, manager exclusion filters, `archived_at`
//! guards, and `CycleUserProperties` scoping for the three tables in
//! `apps/api/pi_dash/db/models/cycle.py:1-157` (`Cycle` `:60-101`,
//! `CycleIssue` `:104-127`, `CycleUserProperties` `:130-157`), adopting
//! the Django-owned schema column-for-column. Migrations are not ported;
//! Django stays schema owner until switchover.
//!
//! Fixture (FX-CYCMOD-01):
//! `rust-api/fixtures/v1_cycles_modules/models/cycle.columns.json`.
//! Column order in each `COLUMNS` const: the 8 inherited audit/project
//! columns first (`id`, `created_at`, `updated_at`, `created_by_id`,
//! `updated_by_id`, `deleted_at`, `project_id`, `workspace_id`, from
//! `BaseModel` at `db/models/base.py:17-21`, `TimeAuditModel`/`UserAuditModel`
//! at `db/mixins.py:19-44`, `SoftDeleteModel` at `db/mixins.py:64`, and
//! `ProjectBaseModel` at `db/models/project.py:302-311`), then the model
//! fields in declaration order as recorded in the fixture. The inherited
//! prefix is `id`-first by family convention (same prefix as the D-27
//! `app_cycles` port of these tables and the D-02 `space` port): live
//! `_meta` reports the same 8 columns with `id` sixth, so membership —
//! not ordinal position — is the contract against Django. FK entries use
//! the Django attnames
//! (`project_id`, `workspace_id`, `owned_by_id`, `issue_id`, `cycle_id`,
//! `user_id`). Every application-level default below is Django-side (the
//! live tables carry no `column_default`); Rust inserts must supply these
//! values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! All three tables inherit the soft-delete marker (`deleted_at`, from
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
//! `Cycle.archived_at` itself is a plain nullable `DateTimeField`
//! (`cycle.py:75`) with no manager behind it; the live/archived split is
//! enforced in `api/views/cycle.py`: list (`:197`) and detail (`:469`)
//! filter `archived_at__isnull=True`, the archived list (`:630`) filters
//! `archived_at__isnull=False`, and patch refuses an archived row with
//! `{"error": "Archived cycle cannot be edited"}` (`:504-508`). This
//! module provides [`live_condition`], [`archived_condition`]
//! and [`is_editable`] so the queries/handlers layers compile those
//! exact guards; the response bodies themselves are owned there.
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
//! DB reads that feed them are owned by the queries layer (PIDASHCONV-307).
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
//! filtering, transfer shapes, serializer edge cases) and are owned by
//! the sibling D-20 issues.

use crate::license::models::OnDelete;

/// `cycles` table (`cycle.py:60-101`).
use sea_query::{Alias, Condition, Expr};
use serde::{Deserialize, Serialize};

/// Physical table (`Meta.db_table`, `cycle.py:85`).
pub const TABLE: &str = "cycles";
/// Soft-delete read view (`cycles_active`).
pub const VIEW: &str = "cycles_active";
/// Default ordering (`Meta.ordering`, `cycle.py:86`).
pub const ORDERING: &str = "-created_at";
/// `verbose_name` (`cycle.py:83`).
pub const VERBOSE_NAME: &str = "Cycle";
/// `verbose_name_plural` (`cycle.py:84`).
pub const VERBOSE_NAME_PLURAL: &str = "Cycles";

/// Columns in fixture FX-CYCMOD-01 order: 8 inherited audit/project
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
/// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
/// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

/// Default-manager scope for `cycles` (`SoftDeletionManager`,
/// `mixins.py:56-58`): `Cycle` declares no custom manager, so
/// `Cycle.objects` already excludes tombstones.
pub fn objects_condition() -> Condition {
    crate::soft_delete::active_condition()
}

/// Live-cycle guard for the list (`views/cycle.py:197`) and detail
/// (`views/cycle.py:469`) reads: `archived_at__isnull=True`.
pub fn live_condition() -> Condition {
    Condition::all().add(Expr::col(Alias::new("archived_at")).is_null())
}

/// Archived-list guard (`views/cycle.py:630`):
/// `archived_at__isnull=False`.
pub fn archived_condition() -> Condition {
    Condition::all().add(Expr::col(Alias::new("archived_at")).is_not_null())
}

/// Patch guard (`views/cycle.py:504-508`): an archived cycle cannot
/// be edited. Returns `false` exactly when `archived_at` is set; the
/// `{"error": "Archived cycle cannot be edited"}` 400 body is owned
/// by the handlers layer.
pub fn is_editable(archived_at: Option<chrono::DateTime<chrono::Utc>>) -> bool {
    archived_at.is_none()
}

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

/// Pure half of `Cycle.save` (`cycle.py:89-95`): on create
/// (`_state.adding`) only, `sort_order = smallest - 10000`; when no
/// rows exist (`smallest` is `None`) the Django-side default is kept.
/// Returns `None` to mean "keep `DEFAULT_SORT_ORDER`".
pub fn new_sort_order(smallest: Option<f64>) -> Option<f64> {
    smallest.map(|min| min - SORT_ORDER_STEP)
}

/// `cycle_issues` link table (`cycle.py:104-127`).
pub mod cycle_issue {
    use super::OnDelete;
    use sea_query::{Alias, Condition, Expr};
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `cycle.py:123`).
    pub const TABLE: &str = "cycle_issues";
    /// Soft-delete read view (`cycle_issues_active`).
    pub const VIEW: &str = "cycle_issues_active";
    /// Default ordering (`Meta.ordering`, `cycle.py:124`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`cycle.py:121`).
    pub const VERBOSE_NAME: &str = "Cycle Issue";
    /// `verbose_name_plural` (`cycle.py:122`).
    pub const VERBOSE_NAME_PLURAL: &str = "Cycle Issues";

    /// Columns in fixture FX-CYCMOD-01 order: 8 inherited audit/project
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
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// Default-manager scope for `cycle_issues` (`SoftDeletionManager`,
    /// `mixins.py:56-58`): no custom manager is declared, so
    /// `CycleIssue.objects` already excludes tombstones.
    pub fn objects_condition() -> Condition {
        crate::soft_delete::active_condition()
    }

    /// Scope one link lookup to its cycle (`cycle.py:110`): live rows of
    /// a single cycle.
    pub fn for_cycle_condition(cycle_id: uuid::Uuid) -> Condition {
        Condition::all()
            .add(Expr::col(Alias::new("deleted_at")).is_null())
            .add(Expr::col(Alias::new("cycle_id")).eq(cycle_id))
    }

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
        /// display renders the cycle id; the name join is owned by the
        /// queries layer.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.cycle_id)
        }
    }
}

/// `cycle_user_properties` table (`cycle.py:130-157`).
pub mod cycle_user_properties {
    use super::OnDelete;
    use sea_query::{Alias, Condition, Expr};
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `cycle.py:153`).
    pub const TABLE: &str = "cycle_user_properties";
    /// Soft-delete read view (`cycle_user_properties_active`).
    pub const VIEW: &str = "cycle_user_properties_active";
    /// Default ordering (`Meta.ordering`, `cycle.py:154`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`cycle.py:151`).
    pub const VERBOSE_NAME: &str = "Cycle User Property";
    /// `verbose_name_plural` (`cycle.py:152`).
    pub const VERBOSE_NAME_PLURAL: &str = "Cycle User Properties";

    /// Columns in fixture FX-CYCMOD-01 order: 8 inherited audit/project
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
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// Default-manager scope for `cycle_user_properties`
    /// (`SoftDeletionManager`, `mixins.py:56-58`): no custom manager is
    /// declared, so the default queryset already excludes tombstones.
    pub fn objects_condition() -> Condition {
        crate::soft_delete::active_condition()
    }

    /// Per-user scoping: the live properties row for one (cycle, user)
    /// pair. Every read and write of this table is scoped this way (the
    /// partial unique constraint enforces at most one live row per
    /// pair); callers additionally bind `project_id`/`workspace_id` from
    /// the cycle row and stamp audit columns from the request context.
    pub fn for_cycle_user_condition(cycle_id: uuid::Uuid, user_id: uuid::Uuid) -> Condition {
        Condition::all()
            .add(Expr::col(Alias::new("deleted_at")).is_null())
            .add(Expr::col(Alias::new("cycle_id")).eq(cycle_id))
            .add(Expr::col(Alias::new("user_id")).eq(user_id))
    }

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
    use super as cycle;
    use super::cycle_issue;
    use super::cycle_user_properties::{
        self, default_display_filters, default_display_properties, default_filters,
    };
    use super::OnDelete;
    use super::{is_editable, new_sort_order, smallest_sort_order_sql};
    use sea_query::{Alias, Condition, PostgresQueryBuilder, Query};

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/v1_cycles_modules/models")
    }

    fn fixture() -> serde_json::Value {
        let path = fixtures_dir().join("cycle.columns.json");
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read cycle.columns.json: {e}"));
        serde_json::from_str(&body).expect("cycle.columns.json is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Fixture field names mapped to physical columns: the fixture records
    /// Django field names (`cycle`, `user`), while `COLUMNS` uses the
    /// attnames Django actually stores (`cycle_id`, `user_id`). A
    /// `ForeignKey` declaration maps `name` to `{name}_id`; every other
    /// field maps to itself.
    fn field_names(v: &serde_json::Value, model: &str) -> Vec<String> {
        v["models"][model]["fields"]
            .as_array()
            .expect("fields is an array")
            .iter()
            .map(|f| {
                let name = f["name"].as_str().expect("field name");
                let func = f["decl"]["func"].as_str().unwrap_or("");
                if func.contains("ForeignKey") {
                    format!("{name}_id")
                } else {
                    name.to_string()
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
    /// `UNIQUE_*_WHERE` / `UNIQUE_*_NAME` const (`cycle.py:116-118`,
    /// `:146-148`).
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
    fn cycle_columns_match_fixture() {
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
        expected.extend(field_names(&v, "Cycle"));
        assert_eq!(owned(cycle::COLUMNS), expected);
        assert_eq!(cycle::COLUMNS.len(), 22);
        let table: &str = cycle::TABLE;
        assert_eq!(table, v["models"]["Cycle"]["meta"]["db_table"]);
        assert_eq!(table, "cycles");
        assert_eq!(cycle::VIEW, "cycles_active");
        let ordering: &str = cycle::ORDERING;
        assert_eq!(ordering, v["models"]["Cycle"]["meta"]["ordering"][0]);
        assert_eq!(ordering, "-created_at");
        assert_eq!(cycle::VERBOSE_NAME, "Cycle");
        assert_eq!(cycle::VERBOSE_NAME_PLURAL, "Cycles");
        assert_eq!(cycle::DEFAULT_SORT_ORDER, 65535.0);
        assert_eq!(cycle::SORT_ORDER_STEP, 10000.0);
        assert_eq!(cycle::DEFAULT_TIMEZONE, "UTC");
        assert_eq!(cycle::TIMEZONE_CHOICES_SOURCE, "pytz.common_timezones");
        assert_eq!(cycle::DEFAULT_VERSION, 1);
        assert_eq!(cycle::EMPTY_PROPS, "{}");
        assert_eq!(cycle::NAME_MAX_LENGTH, 255);
        assert_eq!(cycle::EXTERNAL_SOURCE_MAX_LENGTH, 255);
        assert_eq!(cycle::EXTERNAL_ID_MAX_LENGTH, 255);
        assert_eq!(cycle::TIMEZONE_MAX_LENGTH, 255);
        assert_eq!(cycle::OWNED_BY_ON_DELETE, OnDelete::Cascade);
        assert_eq!(cycle::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(cycle::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(cycle::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(cycle::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn cycle_issue_columns_match_fixture() {
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
        expected.extend(field_names(&v, "CycleIssue"));
        assert_eq!(owned(cycle_issue::COLUMNS), expected);
        assert_eq!(cycle_issue::COLUMNS.len(), 10);
        let table: &str = cycle_issue::TABLE;
        assert_eq!(table, v["models"]["CycleIssue"]["meta"]["db_table"]);
        assert_eq!(table, "cycle_issues");
        assert_eq!(cycle_issue::VIEW, "cycle_issues_active");
        let ordering: &str = cycle_issue::ORDERING;
        assert_eq!(ordering, v["models"]["CycleIssue"]["meta"]["ordering"][0]);
        let together: Vec<String> = owned(cycle_issue::UNIQUE_TOGETHER);
        assert_eq!(together, unique_together(&v, "CycleIssue"));
        assert_eq!(together, vec!["issue", "cycle", "deleted_at"]);
        let name: &str = cycle_issue::UNIQUE_CYCLE_ISSUE_NAME;
        assert_eq!(name, "cycle_issue_when_deleted_at_null");
        constraint_shape(&v, "CycleIssue", &["cycle", "issue"]);
        let cols: Vec<String> = owned(cycle_issue::UNIQUE_CYCLE_ISSUE_COLUMNS);
        assert_eq!(cols, vec!["cycle_id", "issue_id"]);
        let where_clause: &str = cycle_issue::UNIQUE_CYCLE_ISSUE_WHERE;
        assert_eq!(where_clause, "deleted_at IS NULL");
        assert_eq!(cycle_issue::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(cycle_issue::CYCLE_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn cycle_user_properties_columns_match_fixture() {
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
        expected.extend(field_names(&v, "CycleUserProperties"));
        assert_eq!(owned(cycle_user_properties::COLUMNS), expected);
        assert_eq!(cycle_user_properties::COLUMNS.len(), 14);
        let table: &str = cycle_user_properties::TABLE;
        assert_eq!(
            table,
            v["models"]["CycleUserProperties"]["meta"]["db_table"]
        );
        assert_eq!(table, "cycle_user_properties");
        assert_eq!(cycle_user_properties::VIEW, "cycle_user_properties_active");
        let ordering: &str = cycle_user_properties::ORDERING;
        assert_eq!(
            ordering,
            v["models"]["CycleUserProperties"]["meta"]["ordering"][0]
        );
        let together: Vec<String> = owned(cycle_user_properties::UNIQUE_TOGETHER);
        assert_eq!(together, unique_together(&v, "CycleUserProperties"));
        assert_eq!(together, vec!["cycle", "user", "deleted_at"]);
        let name: &str = cycle_user_properties::UNIQUE_CYCLE_USER_NAME;
        assert_eq!(
            name,
            "cycle_user_properties_unique_cycle_user_when_deleted_at_null"
        );
        constraint_shape(&v, "CycleUserProperties", &["cycle", "user"]);
        let cols: Vec<String> = owned(cycle_user_properties::UNIQUE_CYCLE_USER_COLUMNS);
        assert_eq!(cols, vec!["cycle_id", "user_id"]);
        let where_clause: &str = cycle_user_properties::UNIQUE_CYCLE_USER_WHERE;
        assert_eq!(where_clause, "deleted_at IS NULL");
        assert_eq!(cycle_user_properties::CYCLE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(cycle_user_properties::USER_ON_DELETE, OnDelete::Cascade);
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
            (cycle::TABLE, cycle::objects_condition()),
            (cycle_issue::TABLE, cycle_issue::objects_condition()),
            (
                cycle_user_properties::TABLE,
                cycle_user_properties::objects_condition(),
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
            select_where(cycle::TABLE, cycle::live_condition()),
            "SELECT \"id\" FROM \"cycles\" WHERE \"archived_at\" IS NULL"
        );
        assert_eq!(
            select_where(cycle::TABLE, cycle::archived_condition()),
            "SELECT \"id\" FROM \"cycles\" WHERE \"archived_at\" IS NOT NULL"
        );
        assert!(is_editable(None));
        assert!(!is_editable(Some(chrono::Utc::now())));
    }

    #[test]
    fn scoping_helpers_match_django_semantics() {
        let cycle_id = uuid::Uuid::new_v4();
        let user = uuid::Uuid::new_v4();
        let sql = select_where(
            cycle_issue::TABLE,
            cycle_issue::for_cycle_condition(cycle_id),
        );
        assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
        assert!(sql.contains("\"cycle_id\""), "{sql}");
        let sql = select_where(
            cycle_user_properties::TABLE,
            cycle_user_properties::for_cycle_user_condition(cycle_id, user),
        );
        assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
        assert!(sql.contains("\"cycle_id\""), "{sql}");
        assert!(sql.contains("\"user_id\""), "{sql}");
    }

    #[test]
    fn smallest_sort_order_sql_matches_django_semantics() {
        let project = uuid::Uuid::nil();
        let sql = smallest_sort_order_sql(project);
        assert!(sql.contains("MIN(\"sort_order\")"), "{sql}");
        assert!(sql.contains("\"cycles\""), "{sql}");
        assert!(sql.contains("\"project_id\""), "{sql}");
        assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
        assert_eq!(new_sort_order(None), None);
        assert_eq!(new_sort_order(Some(55000.0)), Some(45000.0));
        assert_eq!(cycle::DEFAULT_SORT_ORDER, 65535.0);
    }

    #[test]
    fn display_impls_match_str() {
        let now = chrono::Utc::now();
        let project = uuid::Uuid::nil();
        let row = cycle::Cycle {
            id: uuid::Uuid::nil(),
            created_at: now,
            updated_at: now,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: project,
            workspace_id: uuid::Uuid::nil(),
            name: "Sprint 1".to_string(),
            description: String::new(),
            start_date: None,
            end_date: None,
            owned_by_id: uuid::Uuid::nil(),
            view_props: serde_json::json!({}),
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            progress_snapshot: serde_json::json!({}),
            archived_at: None,
            logo_props: serde_json::json!({}),
            timezone: "UTC".to_string(),
            version: 1,
        };
        assert_eq!(format!("{row}"), format!("Sprint 1 <{project}>"));
        assert_eq!(
            cycle_user_properties::CycleUserProperties::label("Sprint 1", "a@x.com"),
            "Sprint 1 a@x.com"
        );
    }
}
