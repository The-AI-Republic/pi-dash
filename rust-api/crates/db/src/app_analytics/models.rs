//! Analytics + exporter/importer table models (D-35, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/analytic.py:11-26`
//! (`AnalyticView`), `db/models/exporter.py:20-67`
//! (`ExporterHistory` plus `generate_token`) and
//! `db/models/importer.py:13-40` (`Importer`), adopting the
//! Django-owned schema column-for-column; migrations are not ported —
//! Django stays schema owner until switchover.
//!
//! Column order in each `*_COLUMNS` const follows the Django `_meta`
//! field order: the 6 inherited audit columns first (`id` from
//! `BaseModel` at `db/models/base.py:17-18`, `created_at`/`updated_at`
//! from `TimeAuditModel` at `db/mixins.py:16-23`,
//! `created_by_id`/`updated_by_id` from `UserAuditModel` at
//! `db/mixins.py:26-45`, `deleted_at` from `SoftDeleteModel` at
//! `db/mixins.py:61-67`), then `project_id`/`workspace_id` for the
//! `ProjectBaseModel` child (`db/models/project.py:302-311`), then the
//! model fields in declaration order. FK entries use the Django
//! attnames (`workspace_id`, `initiated_by_id`, `project_id`,
//! `token_id`). Every application-level default below is Django-side
//! (the live tables carry no `column_default` in `information_schema`,
//! as established for D-01); Rust inserts must supply these values
//! explicitly.
//!
//! Fixture source of truth: `rust-api/fixtures/app_analytics/models/`
//! recorded by PIDASHCONV-316 (`FX-A-MOD-01` analytic, `FX-A-MOD-02`
//! exporter, `FX-A-MOD-03` importer, traced in
//! `rust-api/fixtures/app_analytics/TRACE.md`). The `#[cfg(test)]`
//! suite asserts these consts equal the fixture column lists, defaults
//! and choice sets.
//!
//! # Reads are soft-delete scoped
//!
//! All three tables inherit the soft-delete marker (`deleted_at`) and
//! the default manager filters `deleted_at IS NULL`
//! (`objects = SoftDeletionManager`; `all_objects` is the plain
//! unscoped manager). Every read built from these tables must apply
//! [`crate::soft_delete::active_condition`]; the tests pin this by
//! rendering a scoped `SELECT` per table.
//!
//! # Writes backfill the workspace
//!
//! `ProjectBaseModel.save()` (`db/models/project.py:309-311`) sets
//! `workspace` from `project.workspace` on every save: Rust
//! inserts/updates of `Importer` must resolve `workspace_id` from the
//! `project_id` row explicitly.
//!
//! # Ported bugs (translated, not fixed)
//!
//! - BUG (`exporter.py:34`): the `workspace` FK string is
//!   `"db.WorkSpace"` with a capital S, while `AnalyticView` uses
//!   `"db.Workspace"` (`analytic.py:12`). Both resolve to the same
//!   table at runtime; the const below records the literal source
//!   string so the difference stays visible.
//! - No other models-scope ported bugs: the recorded D-35 bugs all
//!   live outside this layer (serializer `query_data` read + dead
//!   `if/else` in `app/serializers/analytic.py`, `Sum("point")` in
//!   `DefaultAnalyticsEndpoint`, intake-branch and project-chart
//!   queryset differences) and are owned by the sibling D-35 issues.

use serde::{Deserialize, Serialize};

/// Django-level FK delete behavior (ORM-emulated; same shape as the
/// D-03 `loop::models::OnDelete`, D-05 `integrations::OnDelete`, D-10
/// `tasks_ticker::models::OnDelete`, D-27 `app_cycles::models::OnDelete`
/// and D-32 `app_intake::models::OnDelete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// Port of `generate_token` (`exporter.py:20-21`): `uuid4().hex`, i.e.
/// 32 lowercase hex chars with no dashes.
pub fn generate_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// `analytic_views` table (`analytic.py:11-22`).
pub mod analytic_view {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `analytic.py:21`).
    pub const TABLE: &str = "analytic_views";
    /// Default ordering (`Meta.ordering`, `analytic.py:22`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`analytic.py:19`).
    pub const VERBOSE_NAME: &str = "Analytic";
    /// `verbose_name_plural` (`analytic.py:20`).
    pub const VERBOSE_NAME_PLURAL: &str = "Analytics";

    /// Columns in Django `_meta` field order: 6 inherited audit
    /// columns, then `analytic.py:12-16` in declaration order. FK
    /// columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "name",
        "description",
        "query",
        "query_dict",
    ];

    /// `name` bound (`analytic.py:13`, `max_length=255`).
    pub const NAME_MAX_LENGTH: usize = 255;

    /// `query_dict` Django-side default (`analytic.py:16`,
    /// `default=dict`): empty JSON object, supplied explicitly on
    /// every Rust insert. Fresh value per call, matching the Python
    /// callable returning a new dict each time.
    pub fn default_query_dict() -> serde_json::Value {
        serde_json::json!({})
    }

    /// `workspace` FK: `CASCADE` (`analytic.py:12`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One analytic-view row. `query` is required (no Django default,
    /// `analytic.py:15`); `description` is `blank=True` without
    /// `null=True`, so Django stores `""`, never `NULL`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct AnalyticView {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub name: String,
        pub description: String,
        pub query: serde_json::Value,
        pub query_dict: serde_json::Value,
    }

    impl std::fmt::Display for AnalyticView {
        /// `__str__` (`analytic.py:24-26`):
        /// `"{name} <{workspace.name}>"`. Rust holds only the
        /// workspace FK, so the display renders the workspace id; the
        /// name join is owned by the queries layer.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} <{}>", self.name, self.workspace_id)
        }
    }
}

/// `exporters` table (`exporter.py:24-57`).
pub mod exporter_history {
    use super::{generate_token, OnDelete};
    use serde::{Deserialize, Serialize};
    use std::str::FromStr;

    /// Physical table (`Meta.db_table`, `exporter.py:62`).
    pub const TABLE: &str = "exporters";
    /// Default ordering (`Meta.ordering`, `exporter.py:63`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`exporter.py:60`).
    pub const VERBOSE_NAME: &str = "Exporter";
    /// `verbose_name_plural` (`exporter.py:61`).
    pub const VERBOSE_NAME_PLURAL: &str = "Exporters";

    /// The FK model string exactly as declared (`exporter.py:34`):
    /// `"db.WorkSpace"` with a capital S (ported bug — see module
    /// docs). `AnalyticView` uses `"db.Workspace"`.
    pub const WORKSPACE_FK_MODEL: &str = "db.WorkSpace";

    /// Columns in Django `_meta` field order: 6 inherited audit
    /// columns, then `exporter.py:25-57` in declaration order. FK
    /// columns use the Django attnames (`workspace_id`,
    /// `initiated_by_id`); the array column keeps its field name
    /// (`project`, `exporter.py:35`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "name",
        "type",
        "workspace_id",
        "project",
        "provider",
        "status",
        "reason",
        "key",
        "url",
        "token",
        "initiated_by_id",
        "filters",
        "rich_filters",
    ];

    /// `name` bound (`exporter.py:25`, `max_length=255`).
    pub const NAME_MAX_LENGTH: usize = 255;
    /// `type` bound (`exporter.py:26`, `max_length=50`).
    pub const TYPE_MAX_LENGTH: usize = 50;
    /// `provider` bound (`exporter.py:36`, `max_length=50`).
    pub const PROVIDER_MAX_LENGTH: usize = 50;
    /// `status` bound (`exporter.py:37`, `max_length=50`).
    pub const STATUS_MAX_LENGTH: usize = 50;
    /// `token` bound (`exporter.py:50`, `max_length=255`).
    pub const TOKEN_MAX_LENGTH: usize = 255;
    /// `url` bound (`exporter.py:49`, `max_length=800`).
    pub const URL_MAX_LENGTH: usize = 800;

    /// `type` Django-side default (`exporter.py:28`).
    pub const DEFAULT_TYPE: &str = "issue_exports";
    /// `status` Django-side default (`exporter.py:45`).
    pub const DEFAULT_STATUS: &str = "queued";

    /// Export payload (`type` choices, `exporter.py:29-32`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum ExporterType {
        /// Issue exports (`issue_exports`, the field default).
        IssueExports,
        /// Issue worklogs (`issue_worklogs`).
        IssueWorklogs,
    }

    impl ExporterType {
        /// The stored string (`choices=` value).
        pub fn as_str(self) -> &'static str {
            match self {
                ExporterType::IssueExports => "issue_exports",
                ExporterType::IssueWorklogs => "issue_worklogs",
            }
        }

        /// All values in declaration order.
        pub const ALL: &[ExporterType] = &[ExporterType::IssueExports, ExporterType::IssueWorklogs];
    }

    /// Error for unknown exporter-type strings.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct UnknownExporterType(pub String);

    impl std::fmt::Display for UnknownExporterType {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "unknown exporter type: {}", self.0)
        }
    }

    impl std::error::Error for UnknownExporterType {}

    impl FromStr for ExporterType {
        type Err = UnknownExporterType;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "issue_exports" => Ok(ExporterType::IssueExports),
                "issue_worklogs" => Ok(ExporterType::IssueWorklogs),
                other => Err(UnknownExporterType(other.to_string())),
            }
        }
    }

    impl std::fmt::Display for ExporterType {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.as_str())
        }
    }

    impl Default for ExporterType {
        /// The Django-side default (`exporter.py:28`).
        fn default() -> Self {
            ExporterType::IssueExports
        }
    }

    /// Export file format (`provider` choices, `exporter.py:36`).
    /// No Django default — required on every insert.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum Provider {
        /// JSON (`json`).
        Json,
        /// CSV (`csv`).
        Csv,
        /// Excel (`xlsx`).
        Xlsx,
    }

    impl Provider {
        /// The stored string (`choices=` value).
        pub fn as_str(self) -> &'static str {
            match self {
                Provider::Json => "json",
                Provider::Csv => "csv",
                Provider::Xlsx => "xlsx",
            }
        }

        /// All values in declaration order.
        pub const ALL: &[Provider] = &[Provider::Json, Provider::Csv, Provider::Xlsx];
    }

    /// Error for unknown provider strings.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct UnknownProvider(pub String);

    impl std::fmt::Display for UnknownProvider {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "unknown exporter provider: {}", self.0)
        }
    }

    impl std::error::Error for UnknownProvider {}

    impl FromStr for Provider {
        type Err = UnknownProvider;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "json" => Ok(Provider::Json),
                "csv" => Ok(Provider::Csv),
                "xlsx" => Ok(Provider::Xlsx),
                other => Err(UnknownProvider(other.to_string())),
            }
        }
    }

    impl std::fmt::Display for Provider {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.as_str())
        }
    }

    /// Export run state (`status` choices, `exporter.py:39-44`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum Status {
        /// Queued (`queued`, the field default).
        Queued,
        /// Processing (`processing`).
        Processing,
        /// Completed (`completed`).
        Completed,
        /// Failed (`failed`).
        Failed,
    }

    impl Status {
        /// The stored string (`choices=` value).
        pub fn as_str(self) -> &'static str {
            match self {
                Status::Queued => "queued",
                Status::Processing => "processing",
                Status::Completed => "completed",
                Status::Failed => "failed",
            }
        }

        /// All values in declaration order.
        pub const ALL: &[Status] = &[
            Status::Queued,
            Status::Processing,
            Status::Completed,
            Status::Failed,
        ];
    }

    /// Error for unknown status strings.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct UnknownStatus(pub String);

    impl std::fmt::Display for UnknownStatus {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "unknown exporter status: {}", self.0)
        }
    }

    impl std::error::Error for UnknownStatus {}

    impl FromStr for Status {
        type Err = UnknownStatus;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "queued" => Ok(Status::Queued),
                "processing" => Ok(Status::Processing),
                "completed" => Ok(Status::Completed),
                "failed" => Ok(Status::Failed),
                other => Err(UnknownStatus(other.to_string())),
            }
        }
    }

    impl std::fmt::Display for Status {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.as_str())
        }
    }

    impl Default for Status {
        /// The Django-side default (`exporter.py:45`).
        fn default() -> Self {
            Status::Queued
        }
    }

    /// `rich_filters` Django-side default (`exporter.py:57`,
    /// `default=dict` with `blank=True, null=True`): empty JSON
    /// object when the caller supplies no value (Django applies the
    /// default, not `NULL`), supplied explicitly on every Rust
    /// insert. Fresh value per call.
    pub fn default_rich_filters() -> serde_json::Value {
        serde_json::json!({})
    }

    /// A fresh `token` default (`exporter.py:50`,
    /// `default=generate_token`): `uuid4().hex`, unique per row.
    pub fn default_token() -> String {
        generate_token()
    }

    /// `workspace` FK: `CASCADE` (`exporter.py:34`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `initiated_by` FK: `CASCADE` (`exporter.py:51-54`).
    pub const INITIATED_BY_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One exporter-history row. `name` is `null=True, blank=True`
    /// (`exporter.py:25`); `project` is the `ArrayField` of project
    /// ids (`exporter.py:35`, `blank=True, null=True`); `url` is
    /// `blank=True, null=True` (`exporter.py:49`); `filters` is
    /// `blank=True, null=True` with no Django default
    /// (`exporter.py:56`); `rich_filters` is `Option` at the DB level
    /// but Django defaults it to `{}` (see [`default_rich_filters`]).
    /// `token` is `unique=True` (`exporter.py:50`) — uniqueness is a
    /// DB constraint owned by Django until switchover, not modeled
    /// here beyond the column. The `type` field keeps its Django name
    /// via a raw identifier so serialized rows carry `"type"`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ExporterHistory {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub name: Option<String>,
        pub r#type: String,
        pub workspace_id: uuid::Uuid,
        pub project: Option<Vec<uuid::Uuid>>,
        pub provider: String,
        pub status: String,
        pub reason: String,
        pub key: String,
        pub url: Option<String>,
        pub token: String,
        pub initiated_by_id: uuid::Uuid,
        pub filters: Option<serde_json::Value>,
        pub rich_filters: Option<serde_json::Value>,
    }

    impl std::fmt::Display for ExporterHistory {
        /// `__str__` (`exporter.py:65-67`):
        /// `"{provider} <{workspace.name}>"`. Rust holds only the
        /// workspace FK, so the display renders the workspace id; the
        /// name join is owned by the queries layer.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} <{}>", self.provider, self.workspace_id)
        }
    }
}

/// `importers` table (`importer.py:13-31`).
pub mod importer {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};
    use std::str::FromStr;

    /// Physical table (`Meta.db_table`, `importer.py:35`).
    pub const TABLE: &str = "importers";
    /// Default ordering (`Meta.ordering`, `importer.py:36`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`importer.py:33`).
    pub const VERBOSE_NAME: &str = "Importer";
    /// `verbose_name_plural` (`importer.py:34`).
    pub const VERBOSE_NAME_PLURAL: &str = "Importers";

    /// Columns in Django `_meta` field order: 6 inherited audit
    /// columns, `project_id`/`workspace_id` from `ProjectBaseModel`
    /// (`project.py:302-311`), then `importer.py:14-30` in
    /// declaration order. FK columns use the Django attnames
    /// (`project_id`, `workspace_id`, `initiated_by_id`, `token_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "service",
        "status",
        "initiated_by_id",
        "metadata",
        "config",
        "data",
        "token_id",
        "imported_data",
    ];

    /// `service` bound (`importer.py:14`, `max_length=50`).
    pub const SERVICE_MAX_LENGTH: usize = 50;
    /// `status` bound (`importer.py:15`, `max_length=50`).
    pub const STATUS_MAX_LENGTH: usize = 50;

    /// `status` Django-side default (`importer.py:23`).
    pub const DEFAULT_STATUS: &str = "queued";

    /// Import source (`service` choices, `importer.py:14`).
    /// No Django default — required on every insert.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum Service {
        /// GitHub (`github`).
        Github,
        /// Jira (`jira`).
        Jira,
    }

    impl Service {
        /// The stored string (`choices=` value).
        pub fn as_str(self) -> &'static str {
            match self {
                Service::Github => "github",
                Service::Jira => "jira",
            }
        }

        /// All values in declaration order.
        pub const ALL: &[Service] = &[Service::Github, Service::Jira];
    }

    /// Error for unknown service strings.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct UnknownService(pub String);

    impl std::fmt::Display for UnknownService {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "unknown importer service: {}", self.0)
        }
    }

    impl std::error::Error for UnknownService {}

    impl FromStr for Service {
        type Err = UnknownService;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "github" => Ok(Service::Github),
                "jira" => Ok(Service::Jira),
                other => Err(UnknownService(other.to_string())),
            }
        }
    }

    impl std::fmt::Display for Service {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.as_str())
        }
    }

    /// Import run state (`status` choices, `importer.py:16-22`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum Status {
        /// Queued (`queued`, the field default).
        Queued,
        /// Processing (`processing`).
        Processing,
        /// Completed (`completed`).
        Completed,
        /// Failed (`failed`).
        Failed,
    }

    impl Status {
        /// The stored string (`choices=` value).
        pub fn as_str(self) -> &'static str {
            match self {
                Status::Queued => "queued",
                Status::Processing => "processing",
                Status::Completed => "completed",
                Status::Failed => "failed",
            }
        }

        /// All values in declaration order.
        pub const ALL: &[Status] = &[
            Status::Queued,
            Status::Processing,
            Status::Completed,
            Status::Failed,
        ];
    }

    /// Error for unknown status strings.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct UnknownStatus(pub String);

    impl std::fmt::Display for UnknownStatus {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "unknown importer status: {}", self.0)
        }
    }

    impl std::error::Error for UnknownStatus {}

    impl FromStr for Status {
        type Err = UnknownStatus;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "queued" => Ok(Status::Queued),
                "processing" => Ok(Status::Processing),
                "completed" => Ok(Status::Completed),
                "failed" => Ok(Status::Failed),
                other => Err(UnknownStatus(other.to_string())),
            }
        }
    }

    impl std::fmt::Display for Status {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.as_str())
        }
    }

    impl Default for Status {
        /// The Django-side default (`importer.py:23`).
        fn default() -> Self {
            Status::Queued
        }
    }

    /// `metadata` Django-side default (`importer.py:26`,
    /// `default=dict`). Fresh value per call.
    pub fn default_metadata() -> serde_json::Value {
        serde_json::json!({})
    }

    /// `config` Django-side default (`importer.py:27`,
    /// `default=dict`). Fresh value per call.
    pub fn default_config() -> serde_json::Value {
        serde_json::json!({})
    }

    /// `data` Django-side default (`importer.py:28`,
    /// `default=dict`). Fresh value per call.
    pub fn default_data() -> serde_json::Value {
        serde_json::json!({})
    }

    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`); backfilled from
    /// the project on every save (`project.py:309-311`, see module
    /// docs).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `initiated_by` FK: `CASCADE` (`importer.py:25`).
    pub const INITIATED_BY_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `token` FK to `db.APIToken`: `CASCADE`, required
    /// (`importer.py:29`).
    pub const TOKEN_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One importer row. `metadata`/`config`/`data` carry their
    /// Django-side `{}` defaults on insert; `imported_data` is
    /// `null=True` with no default (`importer.py:30`); `token_id`
    /// references the API token used for the import and is required.
    /// `metadata` keeps its Django field name: it shadows nothing in
    /// Rust, so no rename is needed.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Importer {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub service: String,
        pub status: String,
        pub initiated_by_id: uuid::Uuid,
        pub metadata: serde_json::Value,
        pub config: serde_json::Value,
        pub data: serde_json::Value,
        pub token_id: uuid::Uuid,
        pub imported_data: Option<serde_json::Value>,
    }

    impl std::fmt::Display for Importer {
        /// `__str__` (`importer.py:38-40`):
        /// `"{service} <{project.name}>"`. Rust holds only the
        /// project FK, so the display renders the project id; the
        /// name join is owned by the queries layer.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} <{}>", self.service, self.project_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::analytic_view;
    use super::exporter_history;
    use super::generate_token;
    use super::importer;
    use super::OnDelete;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/app_analytics/models")
    }

    fn fixture(name: &str) -> serde_json::Value {
        let path = fixtures_dir().join(name);
        let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));
        serde_json::from_str(&body).expect("fixture is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two
    /// runtime values.
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Fixture `columns[].name` in order, expanding the grouped audit
    /// entry (`"created_at/updated_at/created_by/updated_by"`) to the
    /// Django attnames used in `*_COLUMNS`.
    fn fixture_column_names(value: &serde_json::Value) -> Vec<String> {
        value["columns"]
            .as_array()
            .expect("fixture has columns array")
            .iter()
            .flat_map(|c| {
                let name = c["name"].as_str().expect("column entry has name");
                if name.contains('/') {
                    name.split('/')
                        .map(|part| match part {
                            "created_by" => "created_by_id".to_string(),
                            "updated_by" => "updated_by_id".to_string(),
                            other => other.to_string(),
                        })
                        .collect::<Vec<_>>()
                } else {
                    vec![name.to_string()]
                }
            })
            .collect()
    }

    /// Find a fixture `columns` entry by column name.
    fn entry<'a>(value: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        value["columns"]
            .as_array()
            .expect("fixture has columns array")
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("fixture has column {name}"))
    }

    /// Render `SELECT <cols> FROM <table> WHERE deleted_at IS NULL`
    /// the way every read built from these tables must.
    fn scoped_select_sql(table: &str, columns: &[&str]) -> String {
        use sea_query::{Alias, PostgresQueryBuilder, Query};
        let mut select = Query::select();
        for col in columns {
            select.column(Alias::new(*col));
        }
        select
            .from(Alias::new(table))
            .cond_where(crate::soft_delete::active_condition());
        select.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn analytic_columns_match_fixture() {
        let fx = fixture("analytic_models.golden.json");
        assert_eq!(fx["fixture"], "FX-A-MOD-01");
        assert_eq!(fx["table"], analytic_view::TABLE);
        assert_eq!(fx["ordering"].as_array().unwrap().len(), 1);
        assert_eq!(fx["ordering"][0], analytic_view::ORDERING);
        assert_eq!(fx["meta"]["verbose_name"], analytic_view::VERBOSE_NAME);
        assert_eq!(
            fx["meta"]["verbose_name_plural"],
            analytic_view::VERBOSE_NAME_PLURAL
        );
        // Every fixture column is present; exact order is the Django
        // `_meta` order (audit cols first, then declaration order).
        // The fixture groups only the 4 `AuditModel` cols and does not
        // record the soft-delete marker — `deleted_at`
        // (`SoftDeleteModel`, `db/mixins.py:61-67`) is asserted
        // separately below.
        for name in fixture_column_names(&fx) {
            assert!(analytic_view::COLUMNS.contains(&name.as_str()), "{name}");
        }
        assert!(analytic_view::COLUMNS.contains(&"deleted_at"));
        assert_eq!(
            owned(analytic_view::COLUMNS),
            vec![
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "workspace_id",
                "name",
                "description",
                "query",
                "query_dict",
            ]
        );
        // `query` is required (no default); `query_dict` defaults to
        // `dict`.
        assert!(
            entry(&fx, "query")["ddl"]
                .as_str()
                .unwrap()
                .contains("no default"),
            "{}",
            entry(&fx, "query")["ddl"]
        );
        assert!(
            entry(&fx, "query_dict")["ddl"]
                .as_str()
                .unwrap()
                .contains("default=dict"),
            "{}",
            entry(&fx, "query_dict")["ddl"]
        );
        assert_eq!(analytic_view::default_query_dict(), serde_json::json!({}));
        assert_eq!(analytic_view::NAME_MAX_LENGTH, 255);
        assert_eq!(analytic_view::WORKSPACE_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn exporter_columns_match_fixture() {
        let fx = fixture("exporter_models.golden.json");
        assert_eq!(fx["fixture"], "FX-A-MOD-02");
        assert_eq!(fx["table"], exporter_history::TABLE);
        assert_eq!(fx["ordering"][0], exporter_history::ORDERING);
        assert_eq!(fx["meta"]["verbose_name"], exporter_history::VERBOSE_NAME);
        assert_eq!(
            fx["meta"]["verbose_name_plural"],
            exporter_history::VERBOSE_NAME_PLURAL
        );
        for name in fixture_column_names(&fx) {
            assert!(exporter_history::COLUMNS.contains(&name.as_str()), "{name}");
        }
        assert!(exporter_history::COLUMNS.contains(&"deleted_at"));
        assert_eq!(
            owned(exporter_history::COLUMNS),
            vec![
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "name",
                "type",
                "workspace_id",
                "project",
                "provider",
                "status",
                "reason",
                "key",
                "url",
                "token",
                "initiated_by_id",
                "filters",
                "rich_filters",
            ]
        );
        // Ported bug: the FK string keeps its capital S.
        assert_eq!(exporter_history::WORKSPACE_FK_MODEL, "db.WorkSpace");
        assert!(
            entry(&fx, "workspace_id")["ddl"]
                .as_str()
                .unwrap()
                .contains("db.WorkSpace"),
            "{}",
            entry(&fx, "workspace_id")["ddl"]
        );
        // `type` default + choices.
        assert_eq!(exporter_history::DEFAULT_TYPE, "issue_exports");
        assert_eq!(
            exporter_history::ExporterType::default().as_str(),
            "issue_exports"
        );
        assert!(
            entry(&fx, "type")["ddl"]
                .as_str()
                .unwrap()
                .contains("default=issue_exports"),
            "{}",
            entry(&fx, "type")["ddl"]
        );
        // `project` is the ArrayField of UUIDs, blank + null.
        assert!(
            entry(&fx, "project")["ddl"]
                .as_str()
                .unwrap()
                .contains("ArrayField"),
            "{}",
            entry(&fx, "project")["ddl"]
        );
        // `provider` has choices and no default (required).
        assert!(
            entry(&fx, "provider")["ddl"]
                .as_str()
                .unwrap()
                .contains("no default"),
            "{}",
            entry(&fx, "provider")["ddl"]
        );
        // `status` default + choices.
        assert_eq!(exporter_history::DEFAULT_STATUS, "queued");
        assert_eq!(exporter_history::Status::default().as_str(), "queued");
        // `token` default is `generate_token`, unique.
        assert!(
            entry(&fx, "token")["ddl"]
                .as_str()
                .unwrap()
                .contains("generate_token"),
            "{}",
            entry(&fx, "token")["ddl"]
        );
        assert_eq!(exporter_history::TOKEN_MAX_LENGTH, 255);
        assert_eq!(exporter_history::URL_MAX_LENGTH, 800);
        assert_eq!(
            exporter_history::default_rich_filters(),
            serde_json::json!({})
        );
        assert_eq!(exporter_history::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(exporter_history::INITIATED_BY_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn importer_columns_match_fixture() {
        let fx = fixture("importer_models.golden.json");
        assert_eq!(fx["fixture"], "FX-A-MOD-03");
        assert_eq!(fx["table"], importer::TABLE);
        assert_eq!(fx["ordering"][0], importer::ORDERING);
        assert_eq!(fx["meta"]["verbose_name"], importer::VERBOSE_NAME);
        assert_eq!(
            fx["meta"]["verbose_name_plural"],
            importer::VERBOSE_NAME_PLURAL
        );
        for name in fixture_column_names(&fx) {
            assert!(importer::COLUMNS.contains(&name.as_str()), "{name}");
        }
        assert!(importer::COLUMNS.contains(&"deleted_at"));
        assert_eq!(
            owned(importer::COLUMNS),
            vec![
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "project_id",
                "workspace_id",
                "service",
                "status",
                "initiated_by_id",
                "metadata",
                "config",
                "data",
                "token_id",
                "imported_data",
            ]
        );
        // `service` has choices and no default (required).
        assert!(
            entry(&fx, "service")["ddl"]
                .as_str()
                .unwrap()
                .contains("no default"),
            "{}",
            entry(&fx, "service")["ddl"]
        );
        // `status` default + choices.
        assert_eq!(importer::DEFAULT_STATUS, "queued");
        assert_eq!(importer::Status::default().as_str(), "queued");
        // `token` FK to APIToken is required.
        assert!(
            entry(&fx, "token_id")["ddl"]
                .as_str()
                .unwrap()
                .contains("APIToken"),
            "{}",
            entry(&fx, "token_id")["ddl"]
        );
        // `metadata`/`config`/`data` default to `dict`;
        // `imported_data` is nullable with no default.
        for col in ["metadata", "config", "data"] {
            assert!(
                entry(&fx, col)["ddl"]
                    .as_str()
                    .unwrap()
                    .contains("default=dict"),
                "{}",
                entry(&fx, col)["ddl"]
            );
        }
        assert!(
            entry(&fx, "imported_data")["ddl"]
                .as_str()
                .unwrap()
                .contains("null=True"),
            "{}",
            entry(&fx, "imported_data")["ddl"]
        );
        assert_eq!(importer::default_metadata(), serde_json::json!({}));
        assert_eq!(importer::default_config(), serde_json::json!({}));
        assert_eq!(importer::default_data(), serde_json::json!({}));
        assert_eq!(importer::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(importer::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(importer::TOKEN_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn exporter_choices_round_trip() {
        use std::str::FromStr;
        assert_eq!(
            exporter_history::ExporterType::ALL
                .iter()
                .map(|v| v.as_str())
                .collect::<Vec<_>>(),
            vec!["issue_exports", "issue_worklogs"]
        );
        assert_eq!(
            exporter_history::Provider::ALL
                .iter()
                .map(|v| v.as_str())
                .collect::<Vec<_>>(),
            vec!["json", "csv", "xlsx"]
        );
        assert_eq!(
            exporter_history::Status::ALL
                .iter()
                .map(|v| v.as_str())
                .collect::<Vec<_>>(),
            vec!["queued", "processing", "completed", "failed"]
        );
        assert_eq!(
            exporter_history::ExporterType::from_str("issue_worklogs").unwrap(),
            exporter_history::ExporterType::IssueWorklogs
        );
        assert_eq!(
            exporter_history::Provider::from_str("xlsx").unwrap(),
            exporter_history::Provider::Xlsx
        );
        assert_eq!(
            exporter_history::Status::from_str("failed").unwrap(),
            exporter_history::Status::Failed
        );
        assert!(exporter_history::ExporterType::from_str("pdf").is_err());
        assert!(exporter_history::Provider::from_str("yaml").is_err());
        assert!(exporter_history::Status::from_str("done").is_err());
        assert_eq!(
            importer::Service::ALL
                .iter()
                .map(|v| v.as_str())
                .collect::<Vec<_>>(),
            vec!["github", "jira"]
        );
        assert_eq!(
            importer::Status::ALL
                .iter()
                .map(|v| v.as_str())
                .collect::<Vec<_>>(),
            vec!["queued", "processing", "completed", "failed"]
        );
        assert_eq!(
            importer::Service::from_str("jira").unwrap(),
            importer::Service::Jira
        );
        assert_eq!(
            importer::Status::from_str("queued").unwrap(),
            importer::Status::Queued
        );
        assert!(importer::Service::from_str("gitlab").is_err());
        assert!(importer::Status::from_str("done").is_err());
    }

    #[test]
    fn generate_token_has_uuid_hex_shape() {
        // Port of `generate_token` (`exporter.py:20-21`):
        // `uuid4().hex` — 32 lowercase hex chars, unique per call.
        let first = generate_token();
        let second = generate_token();
        for token in [&first, &second] {
            assert_eq!(token.len(), 32, "{token}");
            assert!(token.chars().all(|c| c.is_ascii_hexdigit()), "{token}");
            assert_eq!(&token.to_lowercase(), token, "{token}");
        }
        assert_ne!(first, second);
        assert_eq!(exporter_history::default_token().len(), 32);
    }

    #[test]
    fn row_structs_round_trip_json() {
        let now = chrono::Utc::now();
        let analytic = analytic_view::AnalyticView {
            id: uuid::Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: uuid::Uuid::new_v4(),
            name: "Active by priority".to_string(),
            description: String::new(),
            query: serde_json::json!({"type": "bar"}),
            query_dict: analytic_view::default_query_dict(),
        };
        let back: analytic_view::AnalyticView =
            serde_json::from_value(serde_json::to_value(&analytic).unwrap()).unwrap();
        assert_eq!(analytic, back);
        assert_eq!(
            format!("{analytic}"),
            format!("{} <{}>", analytic.name, analytic.workspace_id)
        );

        let exporter = exporter_history::ExporterHistory {
            id: uuid::Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            name: None,
            r#type: exporter_history::ExporterType::default().to_string(),
            workspace_id: uuid::Uuid::new_v4(),
            project: None,
            provider: exporter_history::Provider::Csv.to_string(),
            status: exporter_history::Status::default().to_string(),
            reason: String::new(),
            key: String::new(),
            url: None,
            token: exporter_history::default_token(),
            initiated_by_id: uuid::Uuid::new_v4(),
            filters: None,
            rich_filters: Some(exporter_history::default_rich_filters()),
        };
        let exported = serde_json::to_value(&exporter).unwrap();
        assert_eq!(exported["type"], "issue_exports");
        let back: exporter_history::ExporterHistory = serde_json::from_value(exported).unwrap();
        assert_eq!(exporter, back);
        assert_eq!(
            format!("{exporter}"),
            format!("{} <{}>", exporter.provider, exporter.workspace_id)
        );

        let importer_row = importer::Importer {
            id: uuid::Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::new_v4(),
            workspace_id: uuid::Uuid::new_v4(),
            service: importer::Service::Github.to_string(),
            status: importer::Status::default().to_string(),
            initiated_by_id: uuid::Uuid::new_v4(),
            metadata: importer::default_metadata(),
            config: importer::default_config(),
            data: importer::default_data(),
            token_id: uuid::Uuid::new_v4(),
            imported_data: None,
        };
        let back: importer::Importer =
            serde_json::from_value(serde_json::to_value(&importer_row).unwrap()).unwrap();
        assert_eq!(importer_row, back);
        assert_eq!(
            format!("{importer_row}"),
            format!("{} <{}>", importer_row.service, importer_row.project_id)
        );
    }

    #[test]
    fn reads_are_soft_delete_scoped() {
        for (table, columns) in [
            (analytic_view::TABLE, analytic_view::COLUMNS),
            (exporter_history::TABLE, exporter_history::COLUMNS),
            (importer::TABLE, importer::COLUMNS),
        ] {
            let sql = scoped_select_sql(table, columns);
            assert!(sql.contains(table), "{sql}");
            assert!(
                sql.contains("deleted_at") && sql.contains("IS NULL"),
                "{sql}"
            );
        }
    }
}
