#![forbid(unsafe_code)]

//! Per-user workspace prefs/links + home/sidebar prefs + recent visits (D-24, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/workspace.py:62-111` (JSON defaults),
//! `:185-195` (`WorkspaceBaseModel`), `:372-517` (`WorkspaceUserProperties`,
//! `WorkspaceUserLink`, `WorkspaceHomePreference`, `WorkspaceUserPreference`)
//! and `apps/api/pi_dash/db/models/recent_visit.py:13-39` (`EntityNameEnum`,
//! `UserRecentVisit`), adopting the Django-owned schema column-for-column;
//! migrations are not ported — Django stays schema owner. Shape follows the
//! merged `crate::app_modules::models` precedent: one `pub mod` per table
//! (`TABLE`, `ORDERING`, `VERBOSE_NAME(_PLURAL)`, `COLUMNS`, field consts,
//! `UNIQUE_*`, FK `ON_DELETE` + `RELATED_NAME`, `LIVE_SCOPE_WHERE`, a typed
//! row struct, and the pure halves of `save()` / `__str__`).
//!
//! Column order in each `COLUMNS` const follows the F-W24-07 fixture
//! (`rust-api/fixtures/app_workspace/models/workspace_prefs.columns.json`,
//! recorded by PIDASHCONV-599): the 6 inherited audit columns first (`id`,
//! `created_at`, `updated_at`, `created_by_id`, `updated_by_id`,
//! `deleted_at`, from `BaseModel` at `db/models/base.py:18` and
//! `TimeAuditModel`/`UserAuditModel`/`SoftDeleteModel` at
//! `db/mixins.py:19-64`, shared with F-W24-06), then `workspace_id` /
//! `project_id` for the two `WorkspaceBaseModel` children, then the model
//! fields in declaration order. FK entries use the Django attnames
//! (`workspace_id`, `user_id`, `owner_id`, …). Every application-level
//! default below is Django-side (the live tables carry no
//! `column_default` in `information_schema`, as established for D-01);
//! Rust inserts must supply these values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! All five tables inherit the soft-delete marker (`deleted_at`, from
//! `SoftDeleteModel` in `pi_dash/db/mixins.py:61-67`) and the default
//! manager filters `deleted_at IS NULL` (`objects =
//! SoftDeletionManager`, `mixins.py:51-58`; `all_objects` is the plain
//! unscoped manager). Every read built from these tables must add
//! `deleted_at IS NULL` (each module's `LIVE_SCOPE_WHERE`); the SQL
//! itself lives with the queries layer (PIDASHCONV-611). The partial
//! unique constraints stay as they are (tombstones are excluded by the
//! scope, so a deleted row frees its key).
//!
//! # Writes backfill the workspace
//!
//! `WorkspaceBaseModel.save()` (`db/models/workspace.py:192-195`) sets
//! `workspace` from `project.workspace` on every save whenever a project
//! is set: Rust inserts/updates on [`workspace_user_link`] and
//! [`user_recent_visit`] must resolve `workspace_id` from the
//! `project_id` row explicitly ([`backfill_workspace_id`]).
//!
//! # Ported bugs (translate, don't redesign; also listed in the PR)
//!
//! * `key` / `entity_name` carry a `TextChoices` class that is never
//!   wired as `choices=` (`workspace.py:456,501`,
//!   `recent_visit.py:24`): the columns are free text. The enums are
//!   ported for the known values, but the struct fields stay `String`.
//! * `WorkspaceBaseModel.save` overwrites `workspace` from the project
//!   even when explicitly set (`workspace.py:192-195`).
//!   [`backfill_workspace_id`] keeps that overwrite.
//! * `get_default_display_filters` / `get_default_display_properties`
//!   wrap their payload under a same-named key (`workspace.py:76-87,
//!   90-107`), unlike the flat `get_default_filters` (`:62-73`).
//!   Ported verbatim ([`workspace_user_properties::default_filters`] and
//!   friends).
//! * `WorkspaceUserProperties.Meta.verbose_name_plural` is the singular
//!   `"Workspace User Property"` (`workspace.py:408`). Ported verbatim.
//! * `entity_identifier` (`recent_visit.py:23`) and `project`
//!   (`workspace.py:187`) pass `null=True` with no `blank=True`: the
//!   columns are nullable while Django forms still require them.
//! * `visited_at` is `auto_now` (`recent_visit.py:30`): Django stamps it
//!   on every save; it is never supplied by the caller.
//!
//! # Out of scope (documented, not ported)
//!
//! * `UserFavorite` (`db/models/favorite.py:14-69`) shares the F-W24-07
//!   fixture file but is owned by PIDASHCONV-607 (`models_user.rs`).
//! * `Sticky` is not fixtured here: reuse `db::v1_assets::Sticky`
//!   (D-21, merged).
//! * `get_default_props` / `get_issue_props` (`workspace.py:22-60,
//!   110-111`) default `WorkspaceMember` fields (`:207-209`), owned by
//!   PIDASHCONV-605 (`models_workspace.rs`).
//! * `BaseModel.save` audit backfill (`base.py:23-44`) is recorded under
//!   F-W24-06 and ported with PIDASHCONV-605; the write path lives with
//!   the queries layer (PIDASHCONV-611).
//! * The home/sidebar autocreate loops — including the home view's
//!   exclusion of `quick_tutorial` / `new_at_pi_dash`
//!   (`app/views/workspace/home.py:30-34`) and the sidebar's
//!   `drafts` / `your_work` / `stickies` pinning
//!   (`app/views/workspace/user_preference.py:37-52`) — are view
//!   behavior, owned by PIDASHCONV-623.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Pages read: Porting guide `4496e321-dd24-40f7-bfdf-f771e45fac0c`
//! (updated_at 2026-09-28T03:51:35.921141Z); PIDASHCONV-1 rulebook.

use serde::{Deserialize, Serialize};

/// Django-level FK delete behavior (ORM-emulated; same shape as the
/// `crate::app_modules::models::OnDelete` precedent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// `NavigationControlPreference` choices (`workspace.py:373-375`).
///
/// The only choices class in this file that is actually wired as
/// `choices=` (`workspace.py:392-396`), with `default=ACCORDION`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NavigationControlPreference {
    /// `"ACCORDION"` — the field default (`workspace.py:395`).
    #[serde(rename = "ACCORDION")]
    Accordion,
    /// `"TABBED"`.
    #[serde(rename = "TABBED")]
    Tabbed,
}

impl Default for NavigationControlPreference {
    /// `default=NavigationControlPreference.ACCORDION`
    /// (`workspace.py:395`).
    fn default() -> Self {
        NavigationControlPreference::Accordion
    }
}

impl NavigationControlPreference {
    /// Wire value of each choice (`workspace.py:374-375`).
    pub fn as_str(self) -> &'static str {
        match self {
            NavigationControlPreference::Accordion => "ACCORDION",
            NavigationControlPreference::Tabbed => "TABBED",
        }
    }
}

impl std::str::FromStr for NavigationControlPreference {
    type Err = ();

    /// Parse a wire value (`workspace.py:374-375`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ACCORDION" => Ok(NavigationControlPreference::Accordion),
            "TABBED" => Ok(NavigationControlPreference::Tabbed),
            _ => Err(()),
        }
    }
}

/// `HomeWidgetKeys` choices (`workspace.py:439-444`).
///
/// Never wired as `choices=` on `key` (`workspace.py:456`) — the known
/// values only; the column itself is free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HomeWidgetKeys {
    /// `"quick_links"`.
    #[serde(rename = "quick_links")]
    QuickLinks,
    /// `"recents"`.
    #[serde(rename = "recents")]
    Recents,
    /// `"my_stickies"`.
    #[serde(rename = "my_stickies")]
    MyStickies,
    /// `"new_at_pi_dash"` — excluded from view autocreate
    /// (`app/views/workspace/home.py:30-34`).
    #[serde(rename = "new_at_pi_dash")]
    NewAtPiDash,
    /// `"quick_tutorial"` — excluded from view autocreate
    /// (`app/views/workspace/home.py:30-34`).
    #[serde(rename = "quick_tutorial")]
    QuickTutorial,
}

impl HomeWidgetKeys {
    /// Wire value of each choice (`workspace.py:440-444`).
    pub fn as_str(self) -> &'static str {
        match self {
            HomeWidgetKeys::QuickLinks => "quick_links",
            HomeWidgetKeys::Recents => "recents",
            HomeWidgetKeys::MyStickies => "my_stickies",
            HomeWidgetKeys::NewAtPiDash => "new_at_pi_dash",
            HomeWidgetKeys::QuickTutorial => "quick_tutorial",
        }
    }
}

impl std::str::FromStr for HomeWidgetKeys {
    type Err = ();

    /// Parse a wire value (`workspace.py:440-444`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "quick_links" => Ok(HomeWidgetKeys::QuickLinks),
            "recents" => Ok(HomeWidgetKeys::Recents),
            "my_stickies" => Ok(HomeWidgetKeys::MyStickies),
            "new_at_pi_dash" => Ok(HomeWidgetKeys::NewAtPiDash),
            "quick_tutorial" => Ok(HomeWidgetKeys::QuickTutorial),
            _ => Err(()),
        }
    }
}

/// `UserPreferenceKeys` choices (`workspace.py:482-489`).
///
/// Never wired as `choices=` on `key` (`workspace.py:501`) — the known
/// values only; the column itself is free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UserPreferenceKeys {
    /// `"views"`.
    #[serde(rename = "views")]
    Views,
    /// `"active_cycles"`.
    #[serde(rename = "active_cycles")]
    ActiveCycles,
    /// `"analytics"`.
    #[serde(rename = "analytics")]
    Analytics,
    /// `"drafts"` — pinned on view autocreate
    /// (`app/views/workspace/user_preference.py:37-52`).
    #[serde(rename = "drafts")]
    Drafts,
    /// `"your_work"` — pinned on view autocreate.
    #[serde(rename = "your_work")]
    YourWork,
    /// `"archives"`.
    #[serde(rename = "archives")]
    Archives,
    /// `"stickies"` — pinned on view autocreate.
    #[serde(rename = "stickies")]
    Stickies,
}

impl UserPreferenceKeys {
    /// Wire value of each choice (`workspace.py:483-489`).
    pub fn as_str(self) -> &'static str {
        match self {
            UserPreferenceKeys::Views => "views",
            UserPreferenceKeys::ActiveCycles => "active_cycles",
            UserPreferenceKeys::Analytics => "analytics",
            UserPreferenceKeys::Drafts => "drafts",
            UserPreferenceKeys::YourWork => "your_work",
            UserPreferenceKeys::Archives => "archives",
            UserPreferenceKeys::Stickies => "stickies",
        }
    }
}

impl std::str::FromStr for UserPreferenceKeys {
    type Err = ();

    /// Parse a wire value (`workspace.py:483-489`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "views" => Ok(UserPreferenceKeys::Views),
            "active_cycles" => Ok(UserPreferenceKeys::ActiveCycles),
            "analytics" => Ok(UserPreferenceKeys::Analytics),
            "drafts" => Ok(UserPreferenceKeys::Drafts),
            "your_work" => Ok(UserPreferenceKeys::YourWork),
            "archives" => Ok(UserPreferenceKeys::Archives),
            "stickies" => Ok(UserPreferenceKeys::Stickies),
            _ => Err(()),
        }
    }
}

/// `EntityNameEnum` choices (`recent_visit.py:13-19`).
///
/// Never wired as `choices=` on `entity_name`
/// (`recent_visit.py:24`) — the known values only; the column itself is
/// free text (`max_length=30`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityNameEnum {
    /// `"VIEW"`.
    #[serde(rename = "VIEW")]
    View,
    /// `"PAGE"`.
    #[serde(rename = "PAGE")]
    Page,
    /// `"ISSUE"`.
    #[serde(rename = "ISSUE")]
    Issue,
    /// `"CYCLE"`.
    #[serde(rename = "CYCLE")]
    Cycle,
    /// `"MODULE"`.
    #[serde(rename = "MODULE")]
    Module,
    /// `"PROJECT"`.
    #[serde(rename = "PROJECT")]
    Project,
}

impl EntityNameEnum {
    /// Wire value of each choice (`recent_visit.py:14-19`).
    pub fn as_str(self) -> &'static str {
        match self {
            EntityNameEnum::View => "VIEW",
            EntityNameEnum::Page => "PAGE",
            EntityNameEnum::Issue => "ISSUE",
            EntityNameEnum::Cycle => "CYCLE",
            EntityNameEnum::Module => "MODULE",
            EntityNameEnum::Project => "PROJECT",
        }
    }
}

impl std::str::FromStr for EntityNameEnum {
    type Err = ();

    /// Parse a wire value (`recent_visit.py:14-19`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "VIEW" => Ok(EntityNameEnum::View),
            "PAGE" => Ok(EntityNameEnum::Page),
            "ISSUE" => Ok(EntityNameEnum::Issue),
            "CYCLE" => Ok(EntityNameEnum::Cycle),
            "MODULE" => Ok(EntityNameEnum::Module),
            "PROJECT" => Ok(EntityNameEnum::Project),
            _ => Err(()),
        }
    }
}

/// Pure half of `WorkspaceBaseModel.save` (`db/models/workspace.py:192-195`):
/// `if self.project: self.workspace = self.project.workspace`.
///
/// `project_workspace_id` is the `workspace_id` of the `project_id` row
/// (`None` when no project is set — `project` is `null=True`,
/// `workspace.py:187`). Returns the `workspace_id` to store: the
/// project-derived one whenever a project is set — overwriting an
/// explicitly set value (ported bug) — else the current value
/// unchanged. The project-row lookup is owned by the queries layer.
pub fn backfill_workspace_id(
    current_workspace_id: uuid::Uuid,
    project_workspace_id: Option<uuid::Uuid>,
) -> uuid::Uuid {
    project_workspace_id.unwrap_or(current_workspace_id)
}

/// `workspace_user_properties` table (`workspace.py:372-413`).
pub mod workspace_user_properties {
    use super::{NavigationControlPreference, OnDelete};
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `workspace.py:409`).
    pub const TABLE: &str = "workspace_user_properties";
    /// Default ordering (`Meta.ordering`, `workspace.py:410`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`workspace.py:407`).
    pub const VERBOSE_NAME: &str = "Workspace User Property";
    /// `verbose_name_plural` (`workspace.py:408`) — the singular,
    /// verbatim (ported quirk).
    pub const VERBOSE_NAME_PLURAL: &str = "Workspace User Property";

    /// Physical columns in fixture order: 6 inherited audit columns,
    /// then `workspace.py:377-396` in declaration order. FK columns use
    /// the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "user_id",
        "filters",
        "display_filters",
        "display_properties",
        "rich_filters",
        "navigation_project_limit",
        "navigation_control_preference",
    ];

    /// `navigation_control_preference` bound (`workspace.py:392`,
    /// `max_length=25`).
    pub const NAVIGATION_CONTROL_PREFERENCE_MAX_LENGTH: usize = 25;
    /// `navigation_control_preference` choices in declaration order
    /// (`workspace.py:374-375`, mirrored by
    /// `NavigationControlPreference::as_str`).
    pub const NAVIGATION_CONTROL_PREFERENCE_CHOICES: &[&str] = &["ACCORDION", "TABBED"];
    /// `navigation_control_preference` Django-side default
    /// (`workspace.py:395`, `default=ACCORDION`).
    pub const DEFAULT_NAVIGATION_CONTROL_PREFERENCE: &str = "ACCORDION";
    /// `navigation_project_limit` Django-side default
    /// (`workspace.py:391`, `default=10`).
    pub const DEFAULT_NAVIGATION_PROJECT_LIMIT: i32 = 10;
    /// `rich_filters` Django-side default (`workspace.py:390`,
    /// `default=dict`): empty JSON object, supplied explicitly on every
    /// Rust insert.
    pub const DEFAULT_RICH_FILTERS: &str = "{}";
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `mixins.py:51-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `Meta.unique_together` (`workspace.py:399`), Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["workspace", "user", "deleted_at"];
    /// Partial unique constraint name (`workspace.py:404`).
    pub const UNIQUE_WORKSPACE_USER_NAME: &str =
        "workspace_user_properties_unique_workspace_user_when_deleted_at_null";
    /// Columns of [`UNIQUE_WORKSPACE_USER_NAME`] (`workspace.py:402`),
    /// physical.
    pub const UNIQUE_WORKSPACE_USER_COLUMNS: &[&str] = &["workspace_id", "user_id"];
    /// `WHERE` of [`UNIQUE_WORKSPACE_USER_NAME`] (`workspace.py:403`,
    /// `deleted_at__isnull=True`): one live row per (workspace, user);
    /// a soft-deleted row can be re-created.
    pub const UNIQUE_WORKSPACE_USER_WHERE: &str = "deleted_at IS NULL";

    /// `workspace` FK: `CASCADE` (`workspace.py:377-381`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` reverse accessor (`workspace.py:380`).
    pub const WORKSPACE_RELATED_NAME: &str = "workspace_user_properties";
    /// `user` FK: `CASCADE` (`workspace.py:382-386`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `user` reverse accessor (`workspace.py:385`).
    pub const USER_RELATED_NAME: &str = "workspace_user_properties";
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `filters` Django-side default: `get_default_filters()`
    /// (`workspace.py:62-73`) — flat, all nine values `None`.
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

    /// `display_filters` Django-side default:
    /// `get_default_display_filters()` (`workspace.py:76-87`) — nested
    /// under a same-named key (ported quirk: asymmetric with the flat
    /// [`default_filters`]).
    pub fn default_display_filters() -> serde_json::Value {
        serde_json::json!({
            "display_filters": {
                "group_by": null,
                "order_by": "-created_at",
                "type": null,
                "sub_issue": true,
                "show_empty_groups": true,
                "layout": "list",
                "calendar_date_range": "",
            }
        })
    }

    /// `display_properties` Django-side default:
    /// `get_default_display_properties()` (`workspace.py:90-107`) —
    /// nested under a same-named key (ported quirk, same as
    /// [`default_display_filters`]).
    pub fn default_display_properties() -> serde_json::Value {
        serde_json::json!({
            "display_properties": {
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
            }
        })
    }

    /// One workspace-user-properties row. JSON blobs are
    /// `serde_json::Value` (`JSONField`, `workspace.py:387-390`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct WorkspaceUserProperties {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub user_id: uuid::Uuid,
        pub filters: serde_json::Value,
        pub display_filters: serde_json::Value,
        pub display_properties: serde_json::Value,
        pub rich_filters: serde_json::Value,
        pub navigation_project_limit: i32,
        pub navigation_control_preference: NavigationControlPreference,
    }

    impl WorkspaceUserProperties {
        /// `__str__` (`workspace.py:412-413`):
        /// `f"{self.workspace.name} {self.user.email}"`. Rust holds only
        /// the FKs, so the label takes the joined workspace name and the
        /// user's email; the joins are owned by the queries layer.
        pub fn label(workspace_name: &str, user_email: &str) -> String {
            format!("{workspace_name} {user_email}")
        }
    }
}

/// `workspace_user_links` table (`workspace.py:416-433`).
///
/// A `WorkspaceBaseModel` child: writes backfill `workspace_id` from
/// the project row ([`backfill_workspace_id`]).
pub mod workspace_user_link {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `workspace.py:429`).
    pub const TABLE: &str = "workspace_user_links";
    /// Default ordering (`Meta.ordering`, `workspace.py:430`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`workspace.py:427`).
    pub const VERBOSE_NAME: &str = "Workspace User Link";
    /// `verbose_name_plural` (`workspace.py:428`).
    pub const VERBOSE_NAME_PLURAL: &str = "Workspace User Links";

    /// Physical columns in fixture order: 6 inherited audit columns,
    /// then `workspace_id` / `project_id` (`WorkspaceBaseModel`,
    /// `workspace.py:186-187`), then `workspace.py:417-424` in
    /// declaration order. FK columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "project_id",
        "title",
        "url",
        "metadata",
        "owner_id",
    ];

    /// `title` bound (`workspace.py:417`, `max_length=255`,
    /// `null=True, blank=True`).
    pub const TITLE_MAX_LENGTH: usize = 255;
    /// `metadata` Django-side default (`workspace.py:419`,
    /// `default=dict`): empty JSON object, supplied explicitly on every
    /// Rust insert.
    pub const DEFAULT_METADATA: &str = "{}";
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `mixins.py:51-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `workspace` FK: `CASCADE` (`workspace.py:186`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` reverse accessor: `workspace_%(class)s`
    /// (`workspace.py:186`) with `%(class)s` resolved to the lowercased
    /// model name.
    pub const WORKSPACE_RELATED_NAME: &str = "workspace_workspaceuserlink";
    /// `project` FK: `CASCADE`, nullable (`workspace.py:187`,
    /// `null=True` with no `blank=True`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` reverse accessor: `project_%(class)s`
    /// (`workspace.py:187`) with `%(class)s` resolved.
    pub const PROJECT_RELATED_NAME: &str = "project_workspaceuserlink";
    /// `owner` FK: `CASCADE` (`workspace.py:420-424`).
    pub const OWNER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `owner` reverse accessor (`workspace.py:423`).
    pub const OWNER_RELATED_NAME: &str = "owner_workspace_user_link";
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:38-44`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One workspace-user-link row. `title` is nullable
    /// (`workspace.py:417`); `url` is a non-null `TextField` (`:418`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct WorkspaceUserLink {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub project_id: Option<uuid::Uuid>,
        pub title: Option<String>,
        pub url: String,
        pub metadata: serde_json::Value,
        pub owner_id: uuid::Uuid,
    }

    impl std::fmt::Display for WorkspaceUserLink {
        /// `__str__` (`workspace.py:432-433`):
        /// `f"{self.workspace.id} {self.url}"` — the FK value renders as
        /// the hyphenated UUID string, exactly like Python `str(UUID)`.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} {}", self.workspace_id, self.url)
        }
    }
}

/// `workspace_home_preferences` table (`workspace.py:436-476`).
pub mod workspace_home_preference {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `workspace.py:472`).
    pub const TABLE: &str = "workspace_home_preferences";
    /// Default ordering (`Meta.ordering`, `workspace.py:473`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`workspace.py:470`).
    pub const VERBOSE_NAME: &str = "Workspace Home Preference";
    /// `verbose_name_plural` (`workspace.py:471`).
    pub const VERBOSE_NAME_PLURAL: &str = "Workspace Home Preferences";

    /// Physical columns in fixture order: 6 inherited audit columns,
    /// then `workspace.py:446-459` in declaration order. FK columns use
    /// the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "user_id",
        "key",
        "is_enabled",
        "config",
        "sort_order",
    ];

    /// `key` bound (`workspace.py:456`, `max_length=255`, no `choices=`:
    /// free text — ported bug).
    pub const KEY_MAX_LENGTH: usize = 255;
    /// Known `key` values in declaration order
    /// (`workspace.py:440-444`, mirrored by `HomeWidgetKeys::as_str`).
    /// The home view autocreates all but `new_at_pi_dash` and
    /// `quick_tutorial` (`app/views/workspace/home.py:30-34`,
    /// handler-owned).
    pub const KEY_CHOICES: &[&str] = &[
        "quick_links",
        "recents",
        "my_stickies",
        "new_at_pi_dash",
        "quick_tutorial",
    ];
    /// `is_enabled` Django-side default (`workspace.py:457`,
    /// `default=True`).
    pub const DEFAULT_IS_ENABLED: bool = true;
    /// `config` Django-side default (`workspace.py:458`,
    /// `default=dict`): empty JSON object, supplied explicitly on every
    /// Rust insert.
    pub const DEFAULT_CONFIG: &str = "{}";
    /// `sort_order` Django-side default (`workspace.py:459`,
    /// `default=65535`).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `mixins.py:51-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `Meta.unique_together` (`workspace.py:462`), Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["workspace", "user", "key", "deleted_at"];
    /// Partial unique constraint name (`workspace.py:467`).
    pub const UNIQUE_WORKSPACE_USER_KEY_NAME: &str =
        "workspace_user_home_preferences_unique_workspace_user_key_when_deleted_at_null";
    /// Columns of [`UNIQUE_WORKSPACE_USER_KEY_NAME`]
    /// (`workspace.py:465`), physical.
    pub const UNIQUE_WORKSPACE_USER_KEY_COLUMNS: &[&str] = &["workspace_id", "user_id", "key"];
    /// `WHERE` of [`UNIQUE_WORKSPACE_USER_KEY_NAME`]
    /// (`workspace.py:466`, `deleted_at__isnull=True`): one live row
    /// per (workspace, user, key); a soft-deleted row can be
    /// re-created.
    pub const UNIQUE_WORKSPACE_USER_KEY_WHERE: &str = "deleted_at IS NULL";

    /// `workspace` FK: `CASCADE` (`workspace.py:446-450`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` reverse accessor (`workspace.py:449`).
    pub const WORKSPACE_RELATED_NAME: &str = "workspace_user_home_preferences";
    /// `user` FK: `CASCADE` (`workspace.py:451-455`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `user` reverse accessor (`workspace.py:454`).
    pub const USER_RELATED_NAME: &str = "workspace_user_home_preferences";
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:38-44`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One workspace-home-preference row. `key` is free-text `String`
    /// (`workspace.py:456`, no `choices=` — ported bug);
    /// [`super::HomeWidgetKeys`] pins the known values.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct WorkspaceHomePreference {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub user_id: uuid::Uuid,
        pub key: String,
        pub is_enabled: bool,
        pub config: serde_json::Value,
        pub sort_order: f64,
    }

    impl WorkspaceHomePreference {
        /// `__str__` (`workspace.py:475-476`):
        /// `f"{self.workspace.name} {self.user.email} {self.key}"`. Rust
        /// holds only the FKs, so the label takes the joined workspace
        /// name and the user's email; the joins are owned by the queries
        /// layer.
        pub fn label(workspace_name: &str, user_email: &str, key: &str) -> String {
            format!("{workspace_name} {user_email} {key}")
        }
    }
}

/// `workspace_user_preferences` table (`workspace.py:479-517`).
pub mod workspace_user_preference {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `workspace.py:516`).
    pub const TABLE: &str = "workspace_user_preferences";
    /// Default ordering (`Meta.ordering`, `workspace.py:517`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`workspace.py:514`).
    pub const VERBOSE_NAME: &str = "Workspace User Preference";
    /// `verbose_name_plural` (`workspace.py:515`).
    pub const VERBOSE_NAME_PLURAL: &str = "Workspace User Preferences";

    /// Physical columns in fixture order: 6 inherited audit columns,
    /// then `workspace.py:491-503` in declaration order. FK columns use
    /// the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "user_id",
        "key",
        "is_pinned",
        "sort_order",
    ];

    /// `key` bound (`workspace.py:501`, `max_length=255`, no `choices=`:
    /// free text — ported bug).
    pub const KEY_MAX_LENGTH: usize = 255;
    /// Known `key` values in declaration order
    /// (`workspace.py:483-489`, mirrored by
    /// `UserPreferenceKeys::as_str`). The sidebar view autocreates all
    /// seven, pinning `drafts` / `your_work` / `stickies`
    /// (`app/views/workspace/user_preference.py:37-52`, handler-owned).
    pub const KEY_CHOICES: &[&str] = &[
        "views",
        "active_cycles",
        "analytics",
        "drafts",
        "your_work",
        "archives",
        "stickies",
    ];
    /// `is_pinned` Django-side default (`workspace.py:502`,
    /// `default=False`).
    pub const DEFAULT_IS_PINNED: bool = false;
    /// `sort_order` Django-side default (`workspace.py:503`,
    /// `default=65535`).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `mixins.py:51-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `Meta.unique_together` (`workspace.py:506`), Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["workspace", "user", "key", "deleted_at"];
    /// Partial unique constraint name (`workspace.py:511`).
    pub const UNIQUE_WORKSPACE_USER_KEY_NAME: &str =
        "workspace_user_preferences_unique_workspace_user_key_when_deleted_at_null";
    /// Columns of [`UNIQUE_WORKSPACE_USER_KEY_NAME`]
    /// (`workspace.py:509`), physical.
    pub const UNIQUE_WORKSPACE_USER_KEY_COLUMNS: &[&str] = &["workspace_id", "user_id", "key"];
    /// `WHERE` of [`UNIQUE_WORKSPACE_USER_KEY_NAME`]
    /// (`workspace.py:510`, `deleted_at__isnull=True`): one live row
    /// per (workspace, user, key); a soft-deleted row can be
    /// re-created.
    pub const UNIQUE_WORKSPACE_USER_KEY_WHERE: &str = "deleted_at IS NULL";

    /// `workspace` FK: `CASCADE` (`workspace.py:491-495`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` reverse accessor (`workspace.py:494`).
    pub const WORKSPACE_RELATED_NAME: &str = "workspace_user_preferences";
    /// `user` FK: `CASCADE` (`workspace.py:496-500`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `user` reverse accessor (`workspace.py:499`).
    pub const USER_RELATED_NAME: &str = "workspace_user_preferences";
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:38-44`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One workspace-user-preference row. `key` is free-text `String`
    /// (`workspace.py:501`, no `choices=` — ported bug);
    /// [`super::UserPreferenceKeys`] pins the known values.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct WorkspaceUserPreference {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub user_id: uuid::Uuid,
        pub key: String,
        pub is_pinned: bool,
        pub sort_order: f64,
    }

    impl std::fmt::Display for WorkspaceUserPreference {
        /// Inherited `BaseModel.__str__` (`base.py:46-47`):
        /// `str(self.id)` — this model declares no `__str__` of its
        /// own (`workspace.py:479-517` ends at the `Meta`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.id)
        }
    }
}

/// `user_recent_visits` table (`recent_visit.py:22-39`).
///
/// A `WorkspaceBaseModel` child: writes backfill `workspace_id` from
/// the project row ([`backfill_workspace_id`]).
pub mod user_recent_visit {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `recent_visit.py:35`).
    pub const TABLE: &str = "user_recent_visits";
    /// Default ordering (`Meta.ordering`, `recent_visit.py:36`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`recent_visit.py:33`).
    pub const VERBOSE_NAME: &str = "User Recent Visit";
    /// `verbose_name_plural` (`recent_visit.py:34`).
    pub const VERBOSE_NAME_PLURAL: &str = "User Recent Visits";

    /// Physical columns in fixture order: 6 inherited audit columns,
    /// then `workspace_id` / `project_id` (`WorkspaceBaseModel`,
    /// `workspace.py:186-187`), then `recent_visit.py:23-30` in
    /// declaration order. FK columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "project_id",
        "entity_identifier",
        "entity_name",
        "user_id",
        "visited_at",
    ];

    /// `entity_name` bound (`recent_visit.py:24`, `max_length=30`, no
    /// `choices=` despite `EntityNameEnum`: free text — ported bug).
    pub const ENTITY_NAME_MAX_LENGTH: usize = 30;
    /// Known `entity_name` values in declaration order
    /// (`recent_visit.py:14-19`, mirrored by
    /// `EntityNameEnum::as_str`).
    pub const ENTITY_NAME_CHOICES: &[&str] =
        &["VIEW", "PAGE", "ISSUE", "CYCLE", "MODULE", "PROJECT"];
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `mixins.py:51-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `workspace` FK: `CASCADE` (`workspace.py:186`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` reverse accessor: `workspace_%(class)s`
    /// (`workspace.py:186`) with `%(class)s` resolved to the lowercased
    /// model name.
    pub const WORKSPACE_RELATED_NAME: &str = "workspace_userrecentvisit";
    /// `project` FK: `CASCADE`, nullable (`workspace.py:187`,
    /// `null=True` with no `blank=True`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` reverse accessor: `project_%(class)s`
    /// (`workspace.py:187`) with `%(class)s` resolved.
    pub const PROJECT_RELATED_NAME: &str = "project_userrecentvisit";
    /// `user` FK: `CASCADE` (`recent_visit.py:25-29`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `user` reverse accessor (`recent_visit.py:28`).
    pub const USER_RELATED_NAME: &str = "user_recent_visit";
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:38-44`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One user-recent-visit row. `entity_identifier` is nullable
    /// (`recent_visit.py:23`, `null=True` with no `blank=True`);
    /// `entity_name` is free-text `String` (`:24`, no `choices=` —
    /// ported bug) with [`super::EntityNameEnum`] pinning the known values;
    /// `visited_at` (`:30`, `auto_now`) is stamped by Django on every
    /// save and never supplied by the caller.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct UserRecentVisit {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub project_id: Option<uuid::Uuid>,
        pub entity_identifier: Option<uuid::Uuid>,
        pub entity_name: String,
        pub user_id: uuid::Uuid,
        pub visited_at: chrono::DateTime<chrono::Utc>,
    }

    impl UserRecentVisit {
        /// `__str__` (`recent_visit.py:38-39`):
        /// `f"{self.entity_name} {self.user.email}"`. Rust holds only
        /// the FK, so the label takes the user's email; the join is
        /// owned by the queries layer.
        pub fn label(entity_name: &str, user_email: &str) -> String {
            format!("{entity_name} {user_email}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::user_recent_visit;
    use super::workspace_home_preference;
    use super::workspace_user_link;
    use super::workspace_user_preference;
    use super::workspace_user_properties;
    use super::{backfill_workspace_id, EntityNameEnum, HomeWidgetKeys};
    use super::{NavigationControlPreference, OnDelete, UserPreferenceKeys};
    use std::str::FromStr as _;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/app_workspace/models")
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

    /// Map a Django field name from the fixture to the physical column
    /// (FKs use the attname).
    fn physical(name: &str) -> String {
        match name {
            "workspace" | "project" | "user" | "owner" | "created_by" | "updated_by" => {
                format!("{name}_id")
            }
            _ => name.to_string(),
        }
    }

    /// Expected `COLUMNS`: the 6 shared audit columns (F-W24-06
    /// inherited block) + `workspace_id` / `project_id` for
    /// `WorkspaceBaseModel` children (F-W24-07 inherited block) + the
    /// model's declared fields in fixture order.
    fn expected_columns(
        core: &serde_json::Value,
        prefs: &serde_json::Value,
        model: &str,
        base_child: bool,
    ) -> Vec<String> {
        let mut cols: Vec<String> = core["inherited"]["columns"]
            .as_array()
            .expect("core has inherited columns")
            .iter()
            .map(|c| physical(c["name"].as_str().expect("inherited name is str")))
            .collect();
        if base_child {
            for c in prefs["inherited"]["columns"]
                .as_array()
                .expect("prefs has inherited columns")
            {
                cols.push(physical(c["name"].as_str().expect("inherited name is str")));
            }
        }
        for c in prefs["models"][model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has columns"))
        {
            cols.push(physical(c["name"].as_str().expect("column name is str")));
        }
        cols
    }

    fn ddl<'a>(prefs: &'a serde_json::Value, model: &str, name: &str) -> &'a str {
        prefs["models"][model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has columns"))
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("{model} has column {name}"))["ddl"]
            .as_str()
            .expect("ddl is str")
    }

    fn meta<'a>(prefs: &'a serde_json::Value, model: &str) -> &'a serde_json::Value {
        &prefs["models"][model]["meta"]
    }

    fn str_list(value: &serde_json::Value) -> Vec<String> {
        value
            .as_array()
            .expect("meta list is array")
            .iter()
            .map(|v| v.as_str().expect("meta entry is str").to_string())
            .collect()
    }

    #[test]
    fn props_columns_match_fixtures() {
        let core = fixture("workspace_core.columns.json");
        let prefs = fixture("workspace_prefs.columns.json");
        let model = "WorkspaceUserProperties";
        assert_eq!(
            owned(workspace_user_properties::COLUMNS),
            expected_columns(&core, &prefs, model, false)
        );
        assert_eq!(workspace_user_properties::COLUMNS.len(), 14);
        let table: &str = workspace_user_properties::TABLE;
        assert_eq!(table, meta(&prefs, model)["db_table"].as_str().unwrap());
        assert_eq!(table, "workspace_user_properties");
        let ordering: &str = workspace_user_properties::ORDERING;
        assert_eq!(
            ordering,
            meta(&prefs, model)["ordering"][0].as_str().unwrap()
        );
        assert_eq!(ordering, "-created_at");
        let verbose = format!(
            "{} / {}",
            workspace_user_properties::VERBOSE_NAME,
            workspace_user_properties::VERBOSE_NAME_PLURAL
        );
        assert_eq!(
            verbose,
            format!(
                "{} / {}",
                meta(&prefs, model)["verbose_name"].as_str().unwrap(),
                meta(&prefs, model)["verbose_name_plural"].as_str().unwrap()
            )
        );
        // The plural is the singular, verbatim (ported quirk).
        assert_eq!(verbose, "Workspace User Property / Workspace User Property");
        assert_eq!(
            owned(workspace_user_properties::UNIQUE_TOGETHER),
            str_list(&meta(&prefs, model)["unique_together"])
        );
        let constraint = meta(&prefs, model)["constraints"][0].as_str().unwrap();
        let name: &str = workspace_user_properties::UNIQUE_WORKSPACE_USER_NAME;
        assert!(constraint.contains(name), "{constraint}");
        assert!(
            constraint.contains("fields=[workspace,user]"),
            "{constraint}"
        );
        assert!(constraint.contains("deleted_at__isnull"), "{constraint}");
        assert_eq!(
            owned(workspace_user_properties::UNIQUE_WORKSPACE_USER_COLUMNS),
            vec!["workspace_id".to_string(), "user_id".to_string()]
        );
        let where_clause: &str = workspace_user_properties::UNIQUE_WORKSPACE_USER_WHERE;
        assert_eq!(where_clause, "deleted_at IS NULL");
        let scope: &str = workspace_user_properties::LIVE_SCOPE_WHERE;
        assert_eq!(scope, "deleted_at IS NULL");
        // Field details recorded in the ddl strings.
        assert!(ddl(&prefs, model, "workspace").contains("related_name=workspace_user_properties"));
        assert!(ddl(&prefs, model, "workspace").contains("CASCADE"));
        assert!(ddl(&prefs, model, "user").contains("related_name=workspace_user_properties"));
        assert!(ddl(&prefs, model, "filters").contains("default=get_default_filters"));
        assert!(
            ddl(&prefs, model, "display_filters").contains("default=get_default_display_filters")
        );
        assert!(ddl(&prefs, model, "display_properties")
            .contains("default=get_default_display_properties"));
        assert!(ddl(&prefs, model, "rich_filters").contains("default=dict"));
        assert!(ddl(&prefs, model, "navigation_project_limit").contains("default=10"));
        let nav = ddl(&prefs, model, "navigation_control_preference");
        assert!(nav.contains("max_length=25"), "{nav}");
        assert!(
            nav.contains("choices=NavigationControlPreference.choices"),
            "{nav}"
        );
        assert!(nav.contains("default=ACCORDION"), "{nav}");
        assert_eq!(
            workspace_user_properties::NAVIGATION_CONTROL_PREFERENCE_MAX_LENGTH,
            25
        );
        let default_nav: &str = workspace_user_properties::DEFAULT_NAVIGATION_CONTROL_PREFERENCE;
        assert_eq!(default_nav, "ACCORDION");
        assert_eq!(
            workspace_user_properties::DEFAULT_NAVIGATION_PROJECT_LIMIT,
            10
        );
        let rich: &str = workspace_user_properties::DEFAULT_RICH_FILTERS;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(rich).unwrap(),
            serde_json::json!({})
        );
        assert_eq!(
            workspace_user_properties::WORKSPACE_ON_DELETE,
            OnDelete::Cascade
        );
        assert_eq!(
            workspace_user_properties::CREATED_BY_ON_DELETE,
            OnDelete::SetNull
        );
        assert_eq!(
            workspace_user_properties::UPDATED_BY_ON_DELETE,
            OnDelete::SetNull
        );
        let related: &str = workspace_user_properties::WORKSPACE_RELATED_NAME;
        assert_eq!(related, "workspace_user_properties");
        let user_related: &str = workspace_user_properties::USER_RELATED_NAME;
        assert_eq!(user_related, "workspace_user_properties");
        let label = workspace_user_properties::WorkspaceUserProperties::label("Acme", "a@x.io");
        assert_eq!(label, "Acme a@x.io");
    }

    #[test]
    fn props_defaults_match_python() {
        assert_eq!(
            workspace_user_properties::default_filters(),
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
        );
        // Nested under a same-named key (ported quirk, :76-87).
        assert_eq!(
            workspace_user_properties::default_display_filters(),
            serde_json::json!({
                "display_filters": {
                    "group_by": null,
                    "order_by": "-created_at",
                    "type": null,
                    "sub_issue": true,
                    "show_empty_groups": true,
                    "layout": "list",
                    "calendar_date_range": "",
                }
            })
        );
        // Nested under a same-named key (ported quirk, :90-107).
        assert_eq!(
            workspace_user_properties::default_display_properties(),
            serde_json::json!({
                "display_properties": {
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
                }
            })
        );
        assert_eq!(
            NavigationControlPreference::default(),
            NavigationControlPreference::Accordion
        );
        assert_eq!(
            owned(workspace_user_properties::NAVIGATION_CONTROL_PREFERENCE_CHOICES),
            vec!["ACCORDION".to_string(), "TABBED".to_string()]
        );
    }

    #[test]
    fn link_columns_match_fixtures() {
        let core = fixture("workspace_core.columns.json");
        let prefs = fixture("workspace_prefs.columns.json");
        let model = "WorkspaceUserLink";
        assert_eq!(
            owned(workspace_user_link::COLUMNS),
            expected_columns(&core, &prefs, model, true)
        );
        assert_eq!(workspace_user_link::COLUMNS.len(), 12);
        let table: &str = workspace_user_link::TABLE;
        assert_eq!(table, meta(&prefs, model)["db_table"].as_str().unwrap());
        assert_eq!(table, "workspace_user_links");
        let ordering: &str = workspace_user_link::ORDERING;
        assert_eq!(ordering, "-created_at");
        let verbose = format!(
            "{} / {}",
            workspace_user_link::VERBOSE_NAME,
            workspace_user_link::VERBOSE_NAME_PLURAL
        );
        assert_eq!(verbose, "Workspace User Link / Workspace User Links");
        // No unique_together, no constraints on this model.
        assert!(meta(&prefs, model).get("unique_together").is_none());
        assert!(meta(&prefs, model).get("constraints").is_none());
        let title = ddl(&prefs, model, "title");
        assert!(title.contains("max_length=255"), "{title}");
        assert!(title.contains("null=True"), "{title}");
        assert!(title.contains("blank=True"), "{title}");
        assert_eq!(workspace_user_link::TITLE_MAX_LENGTH, 255);
        assert!(ddl(&prefs, model, "url").contains("TextField"));
        assert!(ddl(&prefs, model, "metadata").contains("default=dict"));
        let metadata: &str = workspace_user_link::DEFAULT_METADATA;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(metadata).unwrap(),
            serde_json::json!({})
        );
        assert!(ddl(&prefs, model, "owner").contains("related_name=owner_workspace_user_link"));
        assert_eq!(workspace_user_link::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(workspace_user_link::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(workspace_user_link::OWNER_ON_DELETE, OnDelete::Cascade);
        let owner_related: &str = workspace_user_link::OWNER_RELATED_NAME;
        assert_eq!(owner_related, "owner_workspace_user_link");
        // %(class)s interpolation resolved per model.
        let workspace_related: &str = workspace_user_link::WORKSPACE_RELATED_NAME;
        assert_eq!(workspace_related, "workspace_workspaceuserlink");
        let project_related: &str = workspace_user_link::PROJECT_RELATED_NAME;
        assert_eq!(project_related, "project_workspaceuserlink");
        let scope: &str = workspace_user_link::LIVE_SCOPE_WHERE;
        assert_eq!(scope, "deleted_at IS NULL");
        // `__str__`: "{workspace.id} {url}" with a hyphenated UUID.
        let row: workspace_user_link::WorkspaceUserLink =
            serde_json::from_value(serde_json::json!({
                "id": "11111111-1111-1111-1111-111111111111",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-02T00:00:00Z",
                "created_by_id": null,
                "updated_by_id": null,
                "deleted_at": null,
                "workspace_id": "22222222-2222-2222-2222-222222222222",
                "project_id": null,
                "title": null,
                "url": "https://example.test/x",
                "metadata": {},
                "owner_id": "33333333-3333-3333-3333-333333333333",
            }))
            .unwrap();
        let rendered = format!("{row}");
        assert_eq!(
            rendered,
            "22222222-2222-2222-2222-222222222222 https://example.test/x"
        );
    }

    #[test]
    fn home_columns_match_fixtures() {
        let core = fixture("workspace_core.columns.json");
        let prefs = fixture("workspace_prefs.columns.json");
        let model = "WorkspaceHomePreference";
        assert_eq!(
            owned(workspace_home_preference::COLUMNS),
            expected_columns(&core, &prefs, model, false)
        );
        assert_eq!(workspace_home_preference::COLUMNS.len(), 12);
        let table: &str = workspace_home_preference::TABLE;
        assert_eq!(table, "workspace_home_preferences");
        let ordering: &str = workspace_home_preference::ORDERING;
        assert_eq!(ordering, "-created_at");
        let verbose = format!(
            "{} / {}",
            workspace_home_preference::VERBOSE_NAME,
            workspace_home_preference::VERBOSE_NAME_PLURAL
        );
        assert_eq!(
            verbose,
            "Workspace Home Preference / Workspace Home Preferences"
        );
        assert_eq!(
            owned(workspace_home_preference::UNIQUE_TOGETHER),
            str_list(&meta(&prefs, model)["unique_together"])
        );
        let constraint = meta(&prefs, model)["constraints"][0].as_str().unwrap();
        let name: &str = workspace_home_preference::UNIQUE_WORKSPACE_USER_KEY_NAME;
        assert!(constraint.contains(name), "{constraint}");
        assert!(
            constraint.contains("fields=[workspace,user,key]"),
            "{constraint}"
        );
        assert_eq!(
            owned(workspace_home_preference::UNIQUE_WORKSPACE_USER_KEY_COLUMNS),
            vec![
                "workspace_id".to_string(),
                "user_id".to_string(),
                "key".to_string()
            ]
        );
        let key_ddl = ddl(&prefs, model, "key");
        assert!(key_ddl.contains("max_length=255"), "{key_ddl}");
        assert!(key_ddl.contains("no choices"), "{key_ddl}");
        assert_eq!(workspace_home_preference::KEY_MAX_LENGTH, 255);
        assert_eq!(
            owned(workspace_home_preference::KEY_CHOICES),
            vec![
                "quick_links".to_string(),
                "recents".to_string(),
                "my_stickies".to_string(),
                "new_at_pi_dash".to_string(),
                "quick_tutorial".to_string(),
            ]
        );
        assert!(ddl(&prefs, model, "is_enabled").contains("default=True"));
        let enabled: bool = workspace_home_preference::DEFAULT_IS_ENABLED;
        assert!(enabled);
        assert!(ddl(&prefs, model, "config").contains("default=dict"));
        let config: &str = workspace_home_preference::DEFAULT_CONFIG;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(config).unwrap(),
            serde_json::json!({})
        );
        assert!(ddl(&prefs, model, "sort_order").contains("default=65535"));
        let sort_order: f64 = workspace_home_preference::DEFAULT_SORT_ORDER;
        assert_eq!(sort_order, 65535.0);
        let label =
            workspace_home_preference::WorkspaceHomePreference::label("Acme", "a@x.io", "recents");
        assert_eq!(label, "Acme a@x.io recents");
    }

    #[test]
    fn sidebar_columns_match_fixtures() {
        let core = fixture("workspace_core.columns.json");
        let prefs = fixture("workspace_prefs.columns.json");
        let model = "WorkspaceUserPreference";
        assert_eq!(
            owned(workspace_user_preference::COLUMNS),
            expected_columns(&core, &prefs, model, false)
        );
        assert_eq!(workspace_user_preference::COLUMNS.len(), 11);
        let table: &str = workspace_user_preference::TABLE;
        assert_eq!(table, "workspace_user_preferences");
        let ordering: &str = workspace_user_preference::ORDERING;
        assert_eq!(ordering, "-created_at");
        let verbose = format!(
            "{} / {}",
            workspace_user_preference::VERBOSE_NAME,
            workspace_user_preference::VERBOSE_NAME_PLURAL
        );
        assert_eq!(
            verbose,
            "Workspace User Preference / Workspace User Preferences"
        );
        assert_eq!(
            owned(workspace_user_preference::UNIQUE_TOGETHER),
            str_list(&meta(&prefs, model)["unique_together"])
        );
        let constraint = meta(&prefs, model)["constraints"][0].as_str().unwrap();
        let name: &str = workspace_user_preference::UNIQUE_WORKSPACE_USER_KEY_NAME;
        assert!(constraint.contains(name), "{constraint}");
        assert!(
            constraint.contains("fields=[workspace,user,key]"),
            "{constraint}"
        );
        let key_ddl = ddl(&prefs, model, "key");
        assert!(key_ddl.contains("max_length=255"), "{key_ddl}");
        assert!(key_ddl.contains("no choices"), "{key_ddl}");
        assert_eq!(workspace_user_preference::KEY_MAX_LENGTH, 255);
        assert_eq!(
            owned(workspace_user_preference::KEY_CHOICES),
            vec![
                "views".to_string(),
                "active_cycles".to_string(),
                "analytics".to_string(),
                "drafts".to_string(),
                "your_work".to_string(),
                "archives".to_string(),
                "stickies".to_string(),
            ]
        );
        assert!(ddl(&prefs, model, "is_pinned").contains("default=False"));
        let pinned: bool = workspace_user_preference::DEFAULT_IS_PINNED;
        assert!(!pinned);
        let sort_order: f64 = workspace_user_preference::DEFAULT_SORT_ORDER;
        assert_eq!(sort_order, 65535.0);
        // Inherited `BaseModel.__str__`: `str(id)`.
        let row: workspace_user_preference::WorkspaceUserPreference =
            serde_json::from_value(serde_json::json!({
                "id": "11111111-1111-1111-1111-111111111111",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-02T00:00:00Z",
                "created_by_id": null,
                "updated_by_id": null,
                "deleted_at": null,
                "workspace_id": "22222222-2222-2222-2222-222222222222",
                "user_id": "33333333-3333-3333-3333-333333333333",
                "key": "views",
                "is_pinned": false,
                "sort_order": 65535.0,
            }))
            .unwrap();
        let rendered = format!("{row}");
        assert_eq!(rendered, "11111111-1111-1111-1111-111111111111");
    }

    #[test]
    fn visit_columns_match_fixtures() {
        let core = fixture("workspace_core.columns.json");
        let prefs = fixture("workspace_prefs.columns.json");
        let model = "UserRecentVisit";
        assert_eq!(
            owned(user_recent_visit::COLUMNS),
            expected_columns(&core, &prefs, model, true)
        );
        assert_eq!(user_recent_visit::COLUMNS.len(), 12);
        let table: &str = user_recent_visit::TABLE;
        assert_eq!(table, "user_recent_visits");
        let ordering: &str = user_recent_visit::ORDERING;
        assert_eq!(ordering, "-created_at");
        let verbose = format!(
            "{} / {}",
            user_recent_visit::VERBOSE_NAME,
            user_recent_visit::VERBOSE_NAME_PLURAL
        );
        assert_eq!(verbose, "User Recent Visit / User Recent Visits");
        assert!(meta(&prefs, model).get("unique_together").is_none());
        assert!(meta(&prefs, model).get("constraints").is_none());
        let identifier = ddl(&prefs, model, "entity_identifier");
        assert!(identifier.contains("UUIDField"), "{identifier}");
        assert!(identifier.contains("null=True"), "{identifier}");
        assert!(identifier.contains("NO blank=True"), "{identifier}");
        let entity = ddl(&prefs, model, "entity_name");
        assert!(entity.contains("max_length=30"), "{entity}");
        assert!(entity.contains("no choices"), "{entity}");
        assert_eq!(user_recent_visit::ENTITY_NAME_MAX_LENGTH, 30);
        assert_eq!(
            owned(user_recent_visit::ENTITY_NAME_CHOICES),
            vec![
                "VIEW".to_string(),
                "PAGE".to_string(),
                "ISSUE".to_string(),
                "CYCLE".to_string(),
                "MODULE".to_string(),
                "PROJECT".to_string(),
            ]
        );
        assert!(ddl(&prefs, model, "user").contains("related_name=user_recent_visit"));
        assert!(ddl(&prefs, model, "visited_at").contains("auto_now"));
        let user_related: &str = user_recent_visit::USER_RELATED_NAME;
        assert_eq!(user_related, "user_recent_visit");
        let workspace_related: &str = user_recent_visit::WORKSPACE_RELATED_NAME;
        assert_eq!(workspace_related, "workspace_userrecentvisit");
        let project_related: &str = user_recent_visit::PROJECT_RELATED_NAME;
        assert_eq!(project_related, "project_userrecentvisit");
        let label = user_recent_visit::UserRecentVisit::label("ISSUE", "a@x.io");
        assert_eq!(label, "ISSUE a@x.io");
    }

    #[test]
    fn choices_enums_round_trip() {
        assert_eq!(NavigationControlPreference::Accordion.as_str(), "ACCORDION");
        assert_eq!(NavigationControlPreference::Tabbed.as_str(), "TABBED");
        assert_eq!(
            NavigationControlPreference::from_str("ACCORDION").unwrap(),
            NavigationControlPreference::Accordion
        );
        assert!(NavigationControlPreference::from_str("accordion").is_err());
        assert_eq!(HomeWidgetKeys::QuickLinks.as_str(), "quick_links");
        assert_eq!(HomeWidgetKeys::QuickTutorial.as_str(), "quick_tutorial");
        assert_eq!(
            HomeWidgetKeys::from_str("my_stickies").unwrap(),
            HomeWidgetKeys::MyStickies
        );
        assert!(HomeWidgetKeys::from_str("MY_STICKIES").is_err());
        assert_eq!(UserPreferenceKeys::YourWork.as_str(), "your_work");
        assert_eq!(
            UserPreferenceKeys::from_str("stickies").unwrap(),
            UserPreferenceKeys::Stickies
        );
        assert!(UserPreferenceKeys::from_str("Stickies").is_err());
        assert_eq!(EntityNameEnum::Issue.as_str(), "ISSUE");
        assert_eq!(
            EntityNameEnum::from_str("PAGE").unwrap(),
            EntityNameEnum::Page
        );
        assert!(EntityNameEnum::from_str("page").is_err());
        // Serde renders the wire values.
        assert_eq!(
            serde_json::to_value(HomeWidgetKeys::NewAtPiDash).unwrap(),
            serde_json::json!("new_at_pi_dash")
        );
        assert_eq!(
            serde_json::to_value(EntityNameEnum::Module).unwrap(),
            serde_json::json!("MODULE")
        );
        assert_eq!(
            serde_json::from_value::<NavigationControlPreference>(serde_json::json!("TABBED"))
                .unwrap(),
            NavigationControlPreference::Tabbed
        );
    }

    #[test]
    fn backfill_workspace_id_semantics() {
        let current = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let from_project = uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        // Project set: overwrite, even when explicitly set (ported bug).
        assert_eq!(
            backfill_workspace_id(current, Some(from_project)),
            from_project
        );
        // No project: keep the current value.
        assert_eq!(backfill_workspace_id(current, None), current);
    }

    #[test]
    fn unenforced_text_fields_accept_free_text() {
        // `key` / `entity_name` carry no `choices=`: rows with unknown
        // values must still deserialize (ported bug).
        let home: workspace_home_preference::WorkspaceHomePreference =
            serde_json::from_value(serde_json::json!({
                "id": "11111111-1111-1111-1111-111111111111",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-02T00:00:00Z",
                "created_by_id": null,
                "updated_by_id": null,
                "deleted_at": null,
                "workspace_id": "22222222-2222-2222-2222-222222222222",
                "user_id": "33333333-3333-3333-3333-333333333333",
                "key": "not-a-known-widget",
                "is_enabled": true,
                "config": {},
                "sort_order": 999.0,
            }))
            .unwrap();
        assert_eq!(home.key, "not-a-known-widget");
        let visit: user_recent_visit::UserRecentVisit = serde_json::from_value(serde_json::json!({
            "id": "11111111-1111-1111-1111-111111111111",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-02T00:00:00Z",
            "created_by_id": null,
            "updated_by_id": null,
            "deleted_at": null,
            "workspace_id": "22222222-2222-2222-2222-222222222222",
            "project_id": null,
            "entity_identifier": null,
            "entity_name": "SOMETHING_ELSE",
            "user_id": "33333333-3333-3333-3333-333333333333",
            "visited_at": "2026-01-03T00:00:00Z",
        }))
        .unwrap();
        assert_eq!(visit.entity_name, "SOMETHING_ELSE");
    }

    #[test]
    fn fixture_trace_names_sources() {
        let prefs = fixture("workspace_prefs.columns.json");
        let source = prefs["source"].as_str().expect("source is str");
        assert!(
            source.contains("db/models/workspace.py:62-111,185-196,372-517"),
            "{source}"
        );
        assert!(
            source.contains("db/models/recent_visit.py:13-39"),
            "{source}"
        );
        let from = prefs["inherited"]["from"]
            .as_str()
            .expect("inherited from is str");
        assert!(from.contains("WorkspaceBaseModel"), "{from}");
        // This module ports 5 of the 6 fixtured models; UserFavorite is
        // owned by PIDASHCONV-607.
        assert!(prefs["models"].get("UserFavorite").is_some());
        assert!(prefs["models"].get("WorkspaceUserProperties").is_some());
        assert!(prefs["models"].get("WorkspaceUserLink").is_some());
        assert!(prefs["models"].get("WorkspaceHomePreference").is_some());
        assert!(prefs["models"].get("WorkspaceUserPreference").is_some());
        assert!(prefs["models"].get("UserRecentVisit").is_some());
    }
}
