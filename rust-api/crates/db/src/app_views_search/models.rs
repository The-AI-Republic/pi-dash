//! Views + favorites table models (D-29, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/view.py:58-99` (`IssueView` with the
//! `:14-55` display/filter defaults and the `:79-95` save) and
//! `apps/api/pi_dash/db/models/favorite.py:14-69` (`UserFavorite` with the
//! `:52-64` save), plus the `Issue` (`db/models/issue.py:95-199`,
//! `:255-266`) and `IssueComment` (`:550-666`) columns this domain reads
//! with their FTS index expression text. Recorded in
//! `rust-api/fixtures/app_views_search/FX-MODEL.json`; Django stays schema
//! owner until switchover, so no DDL is emitted here.
//!
//! Column order in each `*_COLUMNS` const follows the Django `_meta` field
//! order (base `id`, audit columns, workspace/project FKs, then the
//! model's own fields in declaration order). FK entries use the Django
//! attnames (`workspace_id`, `project_id`, `owned_by_id`, …). Every
//! application-level default below is Django-side (the live tables carry
//! no `column_default` in `information_schema`, as established for D-01);
//! Rust inserts must supply these values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! All four tables inherit the soft-delete marker (`deleted_at`, from
//! `SoftDeleteModel` in `pi_dash/db/mixins.py:57-69`) and the default
//! manager filters `deleted_at IS NULL` (`objects = SoftDeletionManager`,
//! `mixins.py:56-58`; `all_objects` is the plain unscoped manager,
//! `mixins.py:67`). Every read built from these tables must apply
//! [`crate::soft_delete::active_condition`]; the tests pin this by
//! rendering a scoped `SELECT` per owned table. The partial unique
//! constraint on favorites stays as it is (tombstones are excluded by the
//! `deleted_at IS NULL` condition, so a deleted favorite can be recreated).
//!
//! # Writes backfill the workspace
//!
//! `WorkspaceBaseModel.save()` (`db/models/workspace.py:191-194`) sets
//! `workspace` from `project.workspace` on every save: Rust inserts/updates
//! must resolve `workspace_id` from the `project_id` row explicitly.
//!
//! # Ported bugs
//!
//! * `IssueView.__str__` (`view.py:97-99`) dereferences `self.project.name`
//!   unconditionally, so stringifying a global (project-less) view raises
//!   `AttributeError` in Python. The [`issue_view::IssueView`] display
//!   renders `None` for a missing project instead of raising; every other
//!   row renders identically.
//! * `UserFavorite.__str__` (`favorite.py:67-69`) renders `user.email`; the
//!   row carries only `user_id`, so the port renders the id (same
//!   substitution as the D-32 intake display ports).

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Django-level FK delete behavior (ORM-emulated; same shape as the D-32
/// `app_intake::models::OnDelete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// View visibility (`view.py:66`, `choices=((0, "Private"), (1, "Public"))`).
///
/// Stored as a `PositiveSmallIntegerField`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ViewAccess {
    /// Private view (`0`).
    Private,
    /// Public view (`1`, the field default).
    Public,
}

impl ViewAccess {
    /// The stored integer (`view.py:66`).
    pub fn as_i16(self) -> i16 {
        match self {
            ViewAccess::Private => 0,
            ViewAccess::Public => 1,
        }
    }

    /// The human label (`choices=` label, `view.py:66`).
    pub fn label(self) -> &'static str {
        match self {
            ViewAccess::Private => "Private",
            ViewAccess::Public => "Public",
        }
    }

    /// Parse a stored integer; `None` for values Django never writes.
    pub fn from_i16(value: i16) -> Option<ViewAccess> {
        match value {
            0 => Some(ViewAccess::Private),
            1 => Some(ViewAccess::Public),
            _ => None,
        }
    }

    /// All values in declaration order.
    pub const ALL: &[ViewAccess] = &[ViewAccess::Private, ViewAccess::Public];
}

/// Comment visibility (`issue.py:575-579`,
/// `choices=(("INTERNAL", …), ("EXTERNAL", …))`, default `"INTERNAL"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommentAccess {
    /// Internal comment (`INTERNAL`, the field default).
    Internal,
    /// External comment (`EXTERNAL`).
    External,
}

impl CommentAccess {
    /// The stored string (`issue.py:576`).
    pub fn as_str(self) -> &'static str {
        match self {
            CommentAccess::Internal => "INTERNAL",
            CommentAccess::External => "EXTERNAL",
        }
    }

    /// All values in declaration order.
    pub const ALL: &[CommentAccess] = &[CommentAccess::Internal, CommentAccess::External];
}

/// Error for unknown comment-access strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownCommentAccess(pub String);

impl std::fmt::Display for UnknownCommentAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown comment access: {}", self.0)
    }
}

impl std::error::Error for UnknownCommentAccess {}

impl FromStr for CommentAccess {
    type Err = UnknownCommentAccess;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "INTERNAL" => Ok(CommentAccess::Internal),
            "EXTERNAL" => Ok(CommentAccess::External),
            other => Err(UnknownCommentAccess(other.to_string())),
        }
    }
}

impl std::fmt::Display for CommentAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `issue_views` table (`view.py:58-77`).
pub mod issue_view {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `view.py:76`).
    pub const TABLE: &str = "issue_views";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`, `view.py:77`).
    pub const ORDERING: &str = "-created_at";

    /// `Meta.verbose_name` (`view.py:74`).
    pub const VERBOSE_NAME: &str = "Issue View";
    /// `Meta.verbose_name_plural` (`view.py:75`).
    pub const VERBOSE_NAME_PLURAL: &str = "Issue Views";

    /// Columns in Django `_meta` field order: `BaseModel.id`
    /// (`db/models/base.py:18`), audit columns (`db/mixins.py:19-42` plus
    /// `deleted_at`, `:62`), workspace/project FKs (`WorkspaceBaseModel`,
    /// `db/models/workspace.py:185-187`), then `view.py:59-71` in
    /// declaration order. FK entries use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "project_id",
        "name",
        "description",
        "query",
        "filters",
        "display_filters",
        "display_properties",
        "rich_filters",
        "access",
        "sort_order",
        "logo_props",
        "owned_by_id",
        "is_locked",
        "archived_at",
    ];

    /// `name` bound (`view.py:59`, `max_length=255`).
    pub const NAME_MAX_LENGTH: usize = 255;

    /// `access` default (`view.py:66`): public.
    pub const DEFAULT_ACCESS: i16 = 1;
    /// [`DEFAULT_ACCESS`] as the enum value.
    pub const DEFAULT_ACCESS_ENUM: super::ViewAccess = super::ViewAccess::Public;
    /// `sort_order` default (`view.py:67`).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;
    /// Sort-order step for new siblings (`view.py:93`).
    pub const SORT_ORDER_STEP: f64 = 10000.0;
    /// `is_locked` default (`view.py:70`).
    pub const DEFAULT_IS_LOCKED: bool = false;

    /// `workspace` FK: `CASCADE`, non-nullable (`workspace.py:186`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE`, nullable (`workspace.py:187`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `owned_by` FK: `CASCADE`, non-nullable (`view.py:69`).
    pub const OWNED_BY_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` / `updated_by` audit FKs: `SET_NULL`, nullable
    /// (from `UserAuditModel`, `db/mixins.py:27-40`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// See [`CREATED_BY_ON_DELETE`].
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `filters` application default (`get_default_filters`,
    /// `view.py:14-25`).
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

    /// `display_filters` application default
    /// (`get_default_display_filters`, `view.py:28-37`).
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

    /// `display_properties` application default
    /// (`get_default_display_properties`, `view.py:40-55`).
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

    /// Whether `save()` takes the empty-filters branch (`view.py:79-81`):
    /// `query = issue_filters(filters, "POST") if filters else {}`. An
    /// empty (or null) filter map compiles to `{}`; a non-empty map is
    /// compiled by the queries layer (PIDASHCONV-271), which ports
    /// `issue_filters`.
    pub fn filters_is_empty(filters: &serde_json::Value) -> bool {
        filters.is_null() || *filters == serde_json::json!({})
    }

    /// Sibling sort order for a create (`view.py:83-93`): one step above
    /// the largest sibling sort order (siblings share the same project, or
    /// the same workspace with a null project for global views). `None`
    /// keeps the field default: Python only assigns when the aggregate is
    /// not `None`.
    pub fn next_sort_order(largest: Option<f64>) -> Option<f64> {
        largest.map(|m| m + SORT_ORDER_STEP)
    }

    /// One issue-view row. `description` stores `""`, never `NULL`
    /// (`TextField(blank=True)` without `null=True`, `view.py:60`);
    /// `query`, `filters`, `display_filters`, `display_properties`,
    /// `rich_filters` and `logo_props` store JSON objects, never `NULL`
    /// (`JSONField`, `view.py:61-68`); `archived_at` (`DateTimeField`,
    /// `:71`) declares no `blank=True` (form-level only, ported as-is).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IssueView {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub project_id: Option<uuid::Uuid>,
        pub name: String,
        pub description: String,
        pub query: serde_json::Value,
        pub filters: serde_json::Value,
        pub display_filters: serde_json::Value,
        pub display_properties: serde_json::Value,
        pub rich_filters: serde_json::Value,
        pub access: i16,
        pub sort_order: f64,
        pub logo_props: serde_json::Value,
        pub owned_by_id: uuid::Uuid,
        pub is_locked: bool,
        pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    }

    impl IssueView {
        /// The visibility as the enum; `None` for integers Django never
        /// writes (see [`super::ViewAccess::from_i16`]).
        pub fn access_enum(&self) -> Option<super::ViewAccess> {
            super::ViewAccess::from_i16(self.access)
        }
    }

    impl std::fmt::Display for IssueView {
        /// `__str__` (`view.py:97-99`, `"{name} <{project.name}>"`). The
        /// row carries only `project_id`; the name is a queries-layer
        /// join, so this renders the id in its place. Ported bug: Python
        /// raises `AttributeError` for a global (project-less) view;
        /// this renders `None` instead (see the module docs).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self.project_id {
                Some(id) => write!(f, "{} <{}>", self.name, id),
                None => write!(f, "{} <None>", self.name),
            }
        }
    }
}

/// `user_favorites` table (`favorite.py:14-50`).
pub mod user_favorite {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `favorite.py:44`).
    pub const TABLE: &str = "user_favorites";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`,
    /// `favorite.py:45`).
    pub const ORDERING: &str = "-created_at";

    /// `Meta.verbose_name` (`favorite.py:42`).
    pub const VERBOSE_NAME: &str = "User Favorite";
    /// `Meta.verbose_name_plural` (`favorite.py:43`).
    pub const VERBOSE_NAME_PLURAL: &str = "User Favorites";

    /// `Meta.unique_together` (`favorite.py:34`): field names as Django
    /// spells them.
    pub const UNIQUE_TOGETHER: &[&str] =
        &["entity_type", "user", "entity_identifier", "deleted_at"];
    /// Partial unique constraint backing the live-favorite scope
    /// (`favorite.py:35-41`).
    pub const UNIQUE_ENTITY_USER_NAME: &str =
        "user_favorite_unique_entity_type_entity_identifier_user_when_deleted_at_null";
    /// Columns of [`UNIQUE_ENTITY_USER_NAME`] (`favorite.py:37`).
    pub const UNIQUE_ENTITY_USER_COLUMNS: &[&str] =
        &["entity_type", "entity_identifier", "user_id"];
    /// `WHERE` of the partial unique index (`favorite.py:38`,
    /// `deleted_at__isnull=True`): the entity is unique per user among
    /// live rows only.
    pub const UNIQUE_ENTITY_USER_WHERE: &str = "deleted_at IS NULL";

    /// `Meta.indexes` (`favorite.py:46-50`).
    pub const INDEXES: &[(&str, &[&str])] = &[
        ("fav_entity_type_idx", &["entity_type"]),
        ("fav_entity_identifier_idx", &["entity_identifier"]),
        ("fav_entity_idx", &["entity_type", "entity_identifier"]),
    ];

    /// Columns in Django `_meta` field order: `BaseModel.id`, audit
    /// columns, workspace/project FKs, then `favorite.py:19-31` in
    /// declaration order. FK entries use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "project_id",
        "user_id",
        "entity_type",
        "entity_identifier",
        "name",
        "is_folder",
        "sequence",
        "parent_id",
    ];

    /// `entity_type` bound (`favorite.py:20`, `max_length=100`).
    pub const ENTITY_TYPE_MAX_LENGTH: usize = 100;
    /// `name` bound (`favorite.py:22`, `max_length=255`).
    pub const NAME_MAX_LENGTH: usize = 255;

    /// `is_folder` default (`favorite.py:23`).
    pub const DEFAULT_IS_FOLDER: bool = false;
    /// `sequence` default (`favorite.py:24`).
    pub const DEFAULT_SEQUENCE: f64 = 65535.0;
    /// Sequence step for new siblings (`favorite.py:63`).
    pub const SEQUENCE_STEP: f64 = 10000.0;

    /// `user` FK: `CASCADE`, non-nullable (`favorite.py:19`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `parent` self FK: `CASCADE`, nullable (`favorite.py:25-31`).
    pub const PARENT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE`, non-nullable (`workspace.py:186`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE`, nullable (`workspace.py:187`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` / `updated_by` audit FKs: `SET_NULL`, nullable
    /// (from `UserAuditModel`, `db/mixins.py:27-40`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// See [`CREATED_BY_ON_DELETE`].
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// Sibling sequence for a create (`favorite.py:52-63`): one step above
    /// the largest sibling sequence (siblings share the project workspace
    /// when a project is set, else the workspace). `None` keeps the field
    /// default: Python only assigns when the aggregate is not `None`.
    pub fn next_sequence(largest: Option<f64>) -> Option<f64> {
        largest.map(|m| m + SEQUENCE_STEP)
    }

    /// One user-favorite row. `name` (`CharField(blank=True, null=True)`,
    /// `favorite.py:22`) and `entity_identifier` (`UUIDField(null=True,
    /// blank=True)`, `:21`) are nullable; `is_folder` and `sequence`
    /// store plain booleans/floats, never `NULL`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct UserFavorite {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub project_id: Option<uuid::Uuid>,
        pub user_id: uuid::Uuid,
        pub entity_type: String,
        pub entity_identifier: Option<uuid::Uuid>,
        pub name: Option<String>,
        pub is_folder: bool,
        pub sequence: f64,
        pub parent_id: Option<uuid::Uuid>,
    }

    impl std::fmt::Display for UserFavorite {
        /// `__str__` (`favorite.py:67-69`,
        /// `"{user.email} <{entity_type}>"`). The row carries only
        /// `user_id`; the email is a queries-layer join, so this renders
        /// the id in its place.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} <{}>", self.user_id, self.entity_type)
        }
    }
}

/// `Issue` columns read by this domain (read-only reference).
///
/// The full `Issue` struct belongs to its owning domain; this module pins
/// only what views/search/favorites read: the favorite-entity columns, the
/// `IssueManager` scoping, and the FTS index expression text (byte-exact,
/// for `EXPLAIN` parity on `issues_fts_idx`).
pub mod issue_ref {
    /// Physical table (`Meta.db_table`, `issue.py:253`).
    pub const TABLE: &str = "issues";

    /// Columns this domain reads (`FX-MODEL.json`
    /// `issue_favorite_entity_columns.own`): the serializer entity columns
    /// (`name`, `description_stripped`, `sequence_id`) plus the
    /// `project__identifier` and `state__group` join targets. FK entries
    /// use the Django attnames.
    pub const READ_COLUMNS: &[&str] = &[
        "name",
        "description_stripped",
        "sequence_id",
        "project_id",
        "state_id",
    ];

    /// `IssueManager.get_queryset` (`issue.py:95-104`): the soft-delete
    /// scope plus four exclusions. Each exclusion is pinned as its own
    /// const so queries-layer builders (PIDASHCONV-271/272) compile the
    /// same scope.
    /// Exclude `state__group = 'triage'` (`issue.py:100`).
    pub const MANAGER_EXCLUDE_STATE_GROUP: &str = "triage";
    /// Exclude `archived_at IS NOT NULL` (`issue.py:101`).
    pub const MANAGER_EXCLUDE_ARCHIVED: bool = true;
    /// Exclude `project.archived_at IS NOT NULL` (`issue.py:102`).
    pub const MANAGER_EXCLUDE_PROJECT_ARCHIVED: bool = true;
    /// Exclude `is_draft = TRUE` (`issue.py:103`).
    pub const MANAGER_EXCLUDE_DRAFTS: bool = true;
    /// The default manager name (`issue.py:229`).
    pub const MANAGER_NAME: &str = "issue_objects";

    /// FTS index name (`issue.py:263`).
    pub const FTS_INDEX_NAME: &str = "issues_fts_idx";
    /// FTS indexed table.
    pub const FTS_TABLE: &str = "issues";
    /// Full-text config (`SearchVector(..., config="english")`,
    /// `issue.py:262`).
    pub const FTS_CONFIG: &str = "english";
    /// Source columns of the search vector (`issue.py:262`).
    pub const FTS_SOURCE_COLUMNS: &[&str] = &["name", "description_stripped"];
    /// Index expression text, byte-exact (`issue.py:255-264`): the
    /// expression must stay identical to the runtime expression for the
    /// planner to use the index (see the `Meta.indexes` comment).
    pub const FTS_EXPRESSION: &str =
        "to_tsvector('english'::regconfig, COALESCE(name,'') || ' ' || COALESCE(description_stripped,''))";
}

/// `IssueComment` columns read by this domain (read-only reference).
///
/// The full `IssueComment` struct belongs to its owning domain; this
/// module pins only what search reads: the comment-subquery columns and
/// the FTS index expression text (byte-exact, for `EXPLAIN` parity on
/// `issue_comments_fts_idx`).
pub mod issue_comment_ref {
    /// Physical table (`Meta.db_table`, `issue.py:652`).
    pub const TABLE: &str = "issue_comments";

    /// Columns this domain reads (`FX-MODEL.json`
    /// `issuecomment_favorite_entity_columns.own`): the comment subquery
    /// reads `comment_stripped` + `issue_id`; `comment_json`,
    /// `comment_html`, `actor` and `access` travel with the shape. FK
    /// entries use the Django attnames.
    pub const READ_COLUMNS: &[&str] = &[
        "comment_stripped",
        "comment_json",
        "comment_html",
        "issue_id",
        "actor_id",
        "access",
    ];

    /// `access` default (`issue.py:577`): internal.
    pub const DEFAULT_ACCESS: &str = "INTERNAL";
    /// [`DEFAULT_ACCESS`] as the enum value.
    pub const DEFAULT_ACCESS_ENUM: super::CommentAccess = super::CommentAccess::Internal;

    /// FTS index name (`issue.py:660`).
    pub const FTS_INDEX_NAME: &str = "issue_comments_fts_idx";
    /// FTS indexed table.
    pub const FTS_TABLE: &str = "issue_comments";
    /// Full-text config (`SearchVector(..., config="english")`,
    /// `issue.py:659`).
    pub const FTS_CONFIG: &str = "english";
    /// Source columns of the search vector (`issue.py:659`).
    pub const FTS_SOURCE_COLUMNS: &[&str] = &["comment_stripped"];
    /// Index expression text, byte-exact (`issue.py:655-661`).
    pub const FTS_EXPRESSION: &str =
        "to_tsvector('english'::regconfig, COALESCE(comment_stripped,''))";

    /// Sync `comment_stripped` from `comment_html` on save
    /// (`issue.py:606`): `strip_tags(comment_html)` unless the html is
    /// empty, in which case `""`. Tag spans (`<...>`) are dropped; text
    /// outside spans is kept verbatim.
    pub fn sync_comment_stripped(comment_html: &str) -> String {
        if comment_html.is_empty() {
            return String::new();
        }
        let mut out = String::with_capacity(comment_html.len());
        let mut in_tag = false;
        for ch in comment_html.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => out.push(ch),
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soft_delete::active_condition;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn fixture() -> serde_json::Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/app_views_search/FX-MODEL.json");
        let body =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read FX-MODEL.json: {e}"));
        serde_json::from_str(&body).expect("FX-MODEL.json is valid JSON")
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

    fn sample_view() -> issue_view::IssueView {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap();
        issue_view::IssueView {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: uuid::Uuid::nil(),
            project_id: Some(uuid::Uuid::nil()),
            name: "My view".to_string(),
            description: String::new(),
            query: serde_json::json!({}),
            filters: serde_json::json!({}),
            display_filters: issue_view::default_display_filters(),
            display_properties: issue_view::default_display_properties(),
            rich_filters: serde_json::json!({}),
            access: issue_view::DEFAULT_ACCESS,
            sort_order: issue_view::DEFAULT_SORT_ORDER,
            logo_props: serde_json::json!({}),
            owned_by_id: uuid::Uuid::nil(),
            is_locked: false,
            archived_at: None,
        }
    }

    fn sample_favorite() -> user_favorite::UserFavorite {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap();
        user_favorite::UserFavorite {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: uuid::Uuid::nil(),
            project_id: None,
            user_id: uuid::Uuid::nil(),
            entity_type: "issue".to_string(),
            entity_identifier: None,
            name: None,
            is_folder: false,
            sequence: user_favorite::DEFAULT_SEQUENCE,
            parent_id: None,
        }
    }

    #[test]
    fn issue_view_columns_match_fixture() {
        let v = fixture();
        let fx = &v["issue_view"];
        let columns = fx["columns"].as_array().expect("issue_view has columns");
        let rust: &[&str] = issue_view::COLUMNS;
        // The fixture folds the five audit columns into one descriptive
        // entry, so 17 entries cover 21 physical columns.
        assert_eq!(columns.len(), 17);
        assert_eq!(rust.len(), 21);
        // Spot-check the descriptive entries stay pinned to the sources.
        assert_eq!(columns[0], "id (BaseModel UUID pk)");
        assert_eq!(
            columns[1],
            "created_at/updated_at/created_by/updated_by/deleted_at (AuditModel)"
        );
        assert_eq!(columns[4], "name Char(255)");
        assert_eq!(columns[6], "query JSON");
        assert_eq!(
            columns[11],
            "access PositiveSmallInt default 1 choices (0 Private / 1 Public)"
        );
        assert_eq!(columns[12], "sort_order Float default 65535");
        assert_eq!(columns[16], "archived_at DateTime null");
        // The audit entry names all five audit columns.
        let audit = columns[1].as_str().unwrap().to_lowercase();
        for col in [
            "created_at",
            "updated_at",
            "created_by",
            "updated_by",
            "deleted_at",
        ] {
            assert!(audit.contains(col), "audit entry names {col}");
        }
        // Every other physical column is named by its descriptive entry,
        // in order (fixture entries 2.. map to physical columns 6..).
        for (entry, col) in columns.iter().skip(2).zip(rust.iter().skip(6)) {
            let text = entry.as_str().unwrap().to_lowercase();
            let base = col.strip_suffix("_id").unwrap_or(col);
            assert!(
                text.contains(base),
                "fixture entry {entry} names column {col}"
            );
        }
        let table: &str = issue_view::TABLE;
        assert_eq!(table, fx["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "issue_views");
        let ordering: &str = issue_view::ORDERING;
        assert_eq!(ordering, fx["meta"]["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        assert_eq!(issue_view::VERBOSE_NAME, "Issue View");
        assert_eq!(issue_view::VERBOSE_NAME_PLURAL, "Issue Views");
        assert_eq!(
            fx["columns_source"].as_str().unwrap(),
            "db/models/view.py:58-77"
        );
    }

    #[test]
    fn issue_view_defaults_match_fixture() {
        let v = fixture();
        let defaults = &v["issue_view"]["defaults"];
        assert_eq!(issue_view::default_filters(), defaults["filters"]);
        assert_eq!(
            issue_view::default_display_filters(),
            defaults["display_filters"]
        );
        assert_eq!(
            issue_view::default_display_properties(),
            defaults["display_properties"]
        );
        assert_eq!(
            defaults["source"].as_str().unwrap(),
            "db/models/view.py:14-55"
        );
        let access: i16 = issue_view::DEFAULT_ACCESS;
        assert_eq!(access, 1);
        assert_eq!(access, issue_view::DEFAULT_ACCESS_ENUM.as_i16());
        let sort: f64 = issue_view::DEFAULT_SORT_ORDER;
        assert_eq!(sort, 65535.0);
        let locked: bool = issue_view::DEFAULT_IS_LOCKED;
        assert!(!locked);
        let name_len: usize = issue_view::NAME_MAX_LENGTH;
        assert_eq!(name_len, 255);
        assert_eq!(issue_view::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_view::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_view::OWNED_BY_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_view::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue_view::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn issue_view_save_semantics() {
        // Empty (or null) filters take the `{}` branch (view.py:79-81).
        assert!(issue_view::filters_is_empty(&serde_json::json!({})));
        assert!(issue_view::filters_is_empty(&serde_json::Value::Null));
        assert!(!issue_view::filters_is_empty(
            &serde_json::json!({"state": null})
        ));
        assert!(!issue_view::filters_is_empty(&issue_view::default_filters()));
        // Sibling sort order: largest + 10000; None keeps the default.
        assert_eq!(issue_view::next_sort_order(None), None);
        let stepped: f64 = issue_view::next_sort_order(Some(65535.0)).unwrap();
        assert_eq!(stepped, 75535.0);
        assert_eq!(issue_view::SORT_ORDER_STEP, 10000.0);
        let fx = fixture();
        let save: &str = fx["issue_view"]["save"].as_str().unwrap();
        assert!(save.contains("issue_filters(filters,'POST') if filters else {}"));
        assert!(save.contains("max sibling sort_order + 10000"));
    }

    #[test]
    fn user_favorite_columns_match_fixture() {
        let v = fixture();
        let fx = &v["user_favorite"];
        let columns = fx["columns"].as_array().expect("user_favorite has columns");
        let rust: &[&str] = user_favorite::COLUMNS;
        // The fixture folds the five audit columns into one descriptive
        // entry, so 11 entries cover 15 physical columns.
        assert_eq!(columns.len(), 11);
        assert_eq!(rust.len(), 15);
        assert_eq!(columns[0], "id (BaseModel UUID pk)");
        assert_eq!(
            columns[1],
            "created_at/updated_at/created_by/updated_by/deleted_at (AuditModel)"
        );
        assert_eq!(columns[4], "user FK users CASCADE related favorites");
        assert_eq!(columns[5], "entity_type Char(100)");
        assert_eq!(columns[9], "sequence Float default 65535");
        let audit = columns[1].as_str().unwrap().to_lowercase();
        for col in [
            "created_at",
            "updated_at",
            "created_by",
            "updated_by",
            "deleted_at",
        ] {
            assert!(audit.contains(col), "audit entry names {col}");
        }
        for (entry, col) in columns.iter().skip(2).zip(rust.iter().skip(6)) {
            let text = entry.as_str().unwrap().to_lowercase();
            let base = col.strip_suffix("_id").unwrap_or(col);
            assert!(
                text.contains(base),
                "fixture entry {entry} names column {col}"
            );
        }
        let table: &str = user_favorite::TABLE;
        assert_eq!(table, fx["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "user_favorites");
        let ordering: &str = user_favorite::ORDERING;
        assert_eq!(ordering, fx["meta"]["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        assert_eq!(user_favorite::VERBOSE_NAME, "User Favorite");
        assert_eq!(user_favorite::VERBOSE_NAME_PLURAL, "User Favorites");
        // Constraints verbatim.
        let together: Vec<String> = owned(user_favorite::UNIQUE_TOGETHER);
        assert_eq!(
            together,
            vec!["entity_type", "user", "entity_identifier", "deleted_at"]
        );
        assert!(fx["constraints"][0]
            .as_str()
            .unwrap()
            .contains("unique_together(entity_type, user, entity_identifier, deleted_at)"));
        let name: &str = user_favorite::UNIQUE_ENTITY_USER_NAME;
        assert_eq!(
            name,
            "user_favorite_unique_entity_type_entity_identifier_user_when_deleted_at_null"
        );
        assert!(fx["constraints"][1].as_str().unwrap().contains(name));
        assert_eq!(
            owned(user_favorite::UNIQUE_ENTITY_USER_COLUMNS),
            vec!["entity_type", "entity_identifier", "user_id"]
        );
        assert_eq!(
            user_favorite::UNIQUE_ENTITY_USER_WHERE,
            "deleted_at IS NULL"
        );
        // Indexes verbatim.
        let names: Vec<&str> = user_favorite::INDEXES.iter().map(|(n, _)| *n).collect();
        let cols: Vec<Vec<String>> = user_favorite::INDEXES
            .iter()
            .map(|(_, c)| owned(c))
            .collect();
        assert_eq!(
            names,
            vec![
                "fav_entity_type_idx",
                "fav_entity_identifier_idx",
                "fav_entity_idx"
            ]
        );
        assert_eq!(
            cols,
            vec![
                vec!["entity_type".to_string()],
                vec!["entity_identifier".to_string()],
                vec!["entity_type".to_string(), "entity_identifier".to_string()],
            ]
        );
        for (n, c) in user_favorite::INDEXES {
            let joined = c.join(", ");
            assert!(
                fx["indexes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e.as_str().unwrap().contains(n)
                        && e.as_str().unwrap().contains(&joined)),
                "fixture pins index {n} ({joined})"
            );
        }
    }

    #[test]
    fn user_favorite_save_semantics() {
        assert_eq!(user_favorite::next_sequence(None), None);
        let stepped: f64 = user_favorite::next_sequence(Some(65535.0)).unwrap();
        assert_eq!(stepped, 75535.0);
        assert_eq!(user_favorite::SEQUENCE_STEP, 10000.0);
        let folder: bool = user_favorite::DEFAULT_IS_FOLDER;
        assert!(!folder);
        let seq: f64 = user_favorite::DEFAULT_SEQUENCE;
        assert_eq!(seq, 65535.0);
        assert_eq!(user_favorite::ENTITY_TYPE_MAX_LENGTH, 100);
        assert_eq!(user_favorite::NAME_MAX_LENGTH, 255);
        assert_eq!(user_favorite::USER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(user_favorite::PARENT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(user_favorite::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(user_favorite::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(user_favorite::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(user_favorite::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
        let fx = fixture();
        let save: &str = fx["user_favorite"]["save"].as_str().unwrap();
        assert!(save.contains("max sibling sequence + 10000"));
    }

    #[test]
    fn issue_read_reference_matches_fixture() {
        let v = fixture();
        let table: &str = issue_ref::TABLE;
        assert_eq!(
            table,
            v["issue_favorite_entity_columns"]["table"]
                .as_str()
                .unwrap()
        );
        assert_eq!(table, "issues");
        // Fixture `own` entries use Django field names; FK attnames map
        // `project` -> `project_id`, `state` -> `state_id`.
        let own: Vec<String> = v["issue_favorite_entity_columns"]["own"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                e.as_str()
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .to_string()
            })
            .collect();
        let mapped: Vec<String> = own
            .iter()
            .map(|f| match f.as_str() {
                "project" => "project_id".to_string(),
                "state" => "state_id".to_string(),
                other => other.to_string(),
            })
            .collect();
        assert_eq!(owned(issue_ref::READ_COLUMNS), mapped);
        // Manager scoping tokens.
        let manager: &str = v["issue_manager"].as_str().unwrap();
        assert!(manager.contains("exclude(state__group='triage')"));
        assert!(manager.contains("exclude(archived_at not null)"));
        assert!(manager.contains("exclude(project.archived_at not null)"));
        assert!(manager.contains("exclude(is_draft=True)"));
        assert_eq!(issue_ref::MANAGER_EXCLUDE_STATE_GROUP, "triage");
        let (archived, project_archived, drafts): (bool, bool, bool) = (
            issue_ref::MANAGER_EXCLUDE_ARCHIVED,
            issue_ref::MANAGER_EXCLUDE_PROJECT_ARCHIVED,
            issue_ref::MANAGER_EXCLUDE_DRAFTS,
        );
        assert!(archived);
        assert!(project_archived);
        assert!(drafts);
        assert_eq!(issue_ref::MANAGER_NAME, "issue_objects");
        // FTS expression byte-exact.
        let fts = &v["issue_fts_index"];
        assert_eq!(issue_ref::FTS_INDEX_NAME, fts["name"].as_str().unwrap());
        assert_eq!(issue_ref::FTS_INDEX_NAME, "issues_fts_idx");
        assert_eq!(issue_ref::FTS_TABLE, fts["table"].as_str().unwrap());
        assert_eq!(issue_ref::FTS_CONFIG, "english");
        assert_eq!(
            owned(issue_ref::FTS_SOURCE_COLUMNS),
            vec!["name".to_string(), "description_stripped".to_string()]
        );
        let expr: &str = issue_ref::FTS_EXPRESSION;
        assert_eq!(expr, fts["expression"].as_str().unwrap());
        assert_eq!(
            expr,
            "to_tsvector('english'::regconfig, COALESCE(name,'') || ' ' || COALESCE(description_stripped,''))"
        );
    }

    #[test]
    fn issue_comment_read_reference_matches_fixture() {
        let v = fixture();
        let table: &str = issue_comment_ref::TABLE;
        assert_eq!(
            table,
            v["issuecomment_favorite_entity_columns"]["table"]
                .as_str()
                .unwrap()
        );
        assert_eq!(table, "issue_comments");
        let own: Vec<String> = v["issuecomment_favorite_entity_columns"]["own"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                e.as_str()
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .to_string()
            })
            .collect();
        let mapped: Vec<String> = own
            .iter()
            .map(|f| match f.as_str() {
                "issue" => "issue_id".to_string(),
                "actor" => "actor_id".to_string(),
                other => other.to_string(),
            })
            .collect();
        assert_eq!(owned(issue_comment_ref::READ_COLUMNS), mapped);
        let access: &str = issue_comment_ref::DEFAULT_ACCESS;
        assert_eq!(access, "INTERNAL");
        assert_eq!(access, issue_comment_ref::DEFAULT_ACCESS_ENUM.as_str());
        let fts = &v["issuecomment_fts_index"];
        assert_eq!(
            issue_comment_ref::FTS_INDEX_NAME,
            fts["name"].as_str().unwrap()
        );
        assert_eq!(issue_comment_ref::FTS_INDEX_NAME, "issue_comments_fts_idx");
        assert_eq!(issue_comment_ref::FTS_TABLE, fts["table"].as_str().unwrap());
        assert_eq!(issue_comment_ref::FTS_CONFIG, "english");
        assert_eq!(
            owned(issue_comment_ref::FTS_SOURCE_COLUMNS),
            vec!["comment_stripped".to_string()]
        );
        let expr: &str = issue_comment_ref::FTS_EXPRESSION;
        assert_eq!(expr, fts["expression"].as_str().unwrap());
        assert_eq!(
            expr,
            "to_tsvector('english'::regconfig, COALESCE(comment_stripped,''))"
        );
        // strip_tags sync on save (issue.py:599-606).
        assert_eq!(issue_comment_ref::sync_comment_stripped(""), "");
        assert_eq!(issue_comment_ref::sync_comment_stripped("<p></p>"), "");
        assert_eq!(
            issue_comment_ref::sync_comment_stripped("<p>hello</p>"),
            "hello"
        );
    }

    #[test]
    fn structs_match_columns_field_for_field() {
        assert_eq!(
            struct_fields(&sample_view()),
            sorted_columns(issue_view::COLUMNS)
        );
        assert_eq!(
            struct_fields(&sample_favorite()),
            sorted_columns(user_favorite::COLUMNS)
        );
    }

    #[test]
    fn reads_are_soft_delete_scoped() {
        for table in [issue_view::TABLE, user_favorite::TABLE] {
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
    fn enums_match_fixture() {
        let access: Vec<(i16, String)> = ViewAccess::ALL
            .iter()
            .map(|a| (a.as_i16(), a.label().to_string()))
            .collect();
        assert_eq!(
            access,
            vec![(0, "Private".to_string()), (1, "Public".to_string())]
        );
        assert_eq!(ViewAccess::from_i16(0), Some(ViewAccess::Private));
        assert_eq!(ViewAccess::from_i16(1), Some(ViewAccess::Public));
        assert_eq!(ViewAccess::from_i16(2), None);
        let comment: Vec<String> = CommentAccess::ALL
            .iter()
            .map(|a| a.as_str().to_string())
            .collect();
        assert_eq!(
            comment,
            vec!["INTERNAL".to_string(), "EXTERNAL".to_string()]
        );
        assert_eq!(
            "INTERNAL".parse::<CommentAccess>(),
            Ok(CommentAccess::Internal)
        );
        assert_eq!(
            "EXTERNAL".parse::<CommentAccess>(),
            Ok(CommentAccess::External)
        );
        assert!("ARCHIVED".parse::<CommentAccess>().is_err());
    }

    #[test]
    fn display_matches_python_str() {
        let row = sample_view();
        assert_eq!(row.access_enum(), Some(ViewAccess::Public));
        assert_eq!(row.to_string(), format!("My view <{}>", uuid::Uuid::nil()));
        let mut global = sample_view();
        global.project_id = None;
        assert_eq!(global.to_string(), "My view <None>");
        let fav = sample_favorite();
        assert_eq!(fav.to_string(), format!("{} <issue>", uuid::Uuid::nil()));
    }
}
