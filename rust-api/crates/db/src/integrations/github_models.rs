//! Github table models (D-05, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/integration/github.py:15-262` as used
//! by `github_sync_task` (`GithubRepository`, `GithubRepositorySync`,
//! `GithubIssueSync`, `GithubCommentSync`, `GithubAppInstallation`,
//! `GithubWebhookDelivery`, `GithubAppInstallSession`,
//! `GithubPullRequestLink`, incl. `State.OPEN`/`CLOSED` and
//! `pr_updated_at`), adopting the Django-owned schema column-for-column.
//! Read models / row mappings only; no migrations.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * Every column default below is application-level (Django); like the
//!   D-01 tables, Rust inserts must supply these values explicitly — there
//!   is no DB fallback.
//! * FK `on_delete` (`CASCADE`, `SET_NULL`) is ORM-emulated, so Rust write
//!   paths (later layer issues) must replicate the cascading / nulling
//!   explicitly.
//! * `GithubIssueSync` / `GithubCommentSync` use `unique_together`, a plain
//!   unique index enforced soft-delete-unaware: soft-deleted rows still
//!   collide (same class of quirk as D-01's `InstanceAdmin` note).
//! * `GithubRepositorySync.repository` is deliberately a `ForeignKey`, not
//!   a `OneToOneField`: the implicit unique on `repository_id` (like the
//!   old `unique_together = [project, repository]` dropped in migration
//!   0128) would block rebinding a project to a previously-soft-deleted
//!   repo. The active-binding invariant is the partial unique on `project`
//!   below (`github.py:33-41` comment, preserved).
//! * `GithubPullRequestLink.State` has only `OPEN` / `CLOSED` — no `MERGED`
//!   (unlike `GitCodeReviewLink`); `merged` is a separate bool
//!   (`github.py:217-262`).
//! * `GithubAppInstallation.repository_count` is a `PositiveIntegerField`
//!   (`github.py:139`); `pr_number` likewise (`github.py:233`).
//! * `GithubRepository.url` is `null=True` with no default and
//!   `GithubRepositorySync.label` is `null=True` with no default: both stay
//!   nullable with no Rust-side default.
//! * `GithubWebhookDelivery.delivery_id` is a `UUIDField` (`github.py:170`),
//!   unlike the git delivery table's `CharField` id.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

use super::OnDelete;

/// Account-type choices for
/// [`github_app_installation::GithubAppInstallation`]
/// (`github.py:123-126`, ported as-is; note the capitalized stored values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum GithubAccountType {
    User,
    Organization,
    #[default]
    Unknown,
}

impl GithubAccountType {
    pub const USER: &'static str = "User";
    pub const ORGANIZATION: &'static str = "Organization";
    pub const UNKNOWN: &'static str = "Unknown";

    pub fn as_str(self) -> &'static str {
        match self {
            GithubAccountType::User => Self::USER,
            GithubAccountType::Organization => Self::ORGANIZATION,
            GithubAccountType::Unknown => Self::UNKNOWN,
        }
    }
}

impl std::fmt::Display for GithubAccountType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownGithubAccountType(pub String);

impl std::fmt::Display for UnknownGithubAccountType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown github account type: {}", self.0)
    }
}

impl std::error::Error for UnknownGithubAccountType {}

impl FromStr for GithubAccountType {
    type Err = UnknownGithubAccountType;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "User" => Ok(GithubAccountType::User),
            "Organization" => Ok(GithubAccountType::Organization),
            "Unknown" => Ok(GithubAccountType::Unknown),
            other => Err(UnknownGithubAccountType(other.to_string())),
        }
    }
}

/// Repository-selection choices (`github.py:128-130`, ported as-is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum GithubRepositorySelection {
    All,
    #[default]
    Selected,
}

impl GithubRepositorySelection {
    pub const ALL: &'static str = "all";
    pub const SELECTED: &'static str = "selected";

    pub fn as_str(self) -> &'static str {
        match self {
            GithubRepositorySelection::All => Self::ALL,
            GithubRepositorySelection::Selected => Self::SELECTED,
        }
    }
}

impl std::fmt::Display for GithubRepositorySelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownGithubRepositorySelection(pub String);

impl std::fmt::Display for UnknownGithubRepositorySelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown github repository selection: {}", self.0)
    }
}

impl std::error::Error for UnknownGithubRepositorySelection {}

impl FromStr for GithubRepositorySelection {
    type Err = UnknownGithubRepositorySelection;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "all" => Ok(GithubRepositorySelection::All),
            "selected" => Ok(GithubRepositorySelection::Selected),
            other => Err(UnknownGithubRepositorySelection(other.to_string())),
        }
    }
}

/// Webhook delivery status (`github.py:164-168`, ported as-is; same stored
/// strings as the git delivery status).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum GithubWebhookStatus {
    #[default]
    Received,
    Processed,
    Failed,
    Skipped,
}

impl GithubWebhookStatus {
    pub const RECEIVED: &'static str = "received";
    pub const PROCESSED: &'static str = "processed";
    pub const FAILED: &'static str = "failed";
    pub const SKIPPED: &'static str = "skipped";

    pub fn as_str(self) -> &'static str {
        match self {
            GithubWebhookStatus::Received => Self::RECEIVED,
            GithubWebhookStatus::Processed => Self::PROCESSED,
            GithubWebhookStatus::Failed => Self::FAILED,
            GithubWebhookStatus::Skipped => Self::SKIPPED,
        }
    }
}

impl std::fmt::Display for GithubWebhookStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownGithubWebhookStatus(pub String);

impl std::fmt::Display for UnknownGithubWebhookStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown github webhook status: {}", self.0)
    }
}

impl std::error::Error for UnknownGithubWebhookStatus {}

impl FromStr for GithubWebhookStatus {
    type Err = UnknownGithubWebhookStatus;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "received" => Ok(GithubWebhookStatus::Received),
            "processed" => Ok(GithubWebhookStatus::Processed),
            "failed" => Ok(GithubWebhookStatus::Failed),
            "skipped" => Ok(GithubWebhookStatus::Skipped),
            other => Err(UnknownGithubWebhookStatus(other.to_string())),
        }
    }
}

/// App-install-session status (`github.py:192-196`, ported as-is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum GithubSessionStatus {
    #[default]
    Started,
    Completed,
    Expired,
    Failed,
}

impl GithubSessionStatus {
    pub const STARTED: &'static str = "started";
    pub const COMPLETED: &'static str = "completed";
    pub const EXPIRED: &'static str = "expired";
    pub const FAILED: &'static str = "failed";

    pub fn as_str(self) -> &'static str {
        match self {
            GithubSessionStatus::Started => Self::STARTED,
            GithubSessionStatus::Completed => Self::COMPLETED,
            GithubSessionStatus::Expired => Self::EXPIRED,
            GithubSessionStatus::Failed => Self::FAILED,
        }
    }
}

impl std::fmt::Display for GithubSessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownGithubSessionStatus(pub String);

impl std::fmt::Display for UnknownGithubSessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown github session status: {}", self.0)
    }
}

impl std::error::Error for UnknownGithubSessionStatus {}

impl FromStr for GithubSessionStatus {
    type Err = UnknownGithubSessionStatus;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "started" => Ok(GithubSessionStatus::Started),
            "completed" => Ok(GithubSessionStatus::Completed),
            "expired" => Ok(GithubSessionStatus::Expired),
            "failed" => Ok(GithubSessionStatus::Failed),
            other => Err(UnknownGithubSessionStatus(other.to_string())),
        }
    }
}

/// Pull-request link state (`github.py:229-231`, ported as-is): only
/// `OPEN` / `CLOSED` — no `MERGED`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PullRequestState {
    #[default]
    Open,
    Closed,
}

impl PullRequestState {
    pub const OPEN: &'static str = "open";
    pub const CLOSED: &'static str = "closed";

    pub fn as_str(self) -> &'static str {
        match self {
            PullRequestState::Open => Self::OPEN,
            PullRequestState::Closed => Self::CLOSED,
        }
    }
}

impl std::fmt::Display for PullRequestState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownPullRequestState(pub String);

impl std::fmt::Display for UnknownPullRequestState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown pull request state: {}", self.0)
    }
}

impl std::error::Error for UnknownPullRequestState {}

impl FromStr for PullRequestState {
    type Err = UnknownPullRequestState;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(PullRequestState::Open),
            "closed" => Ok(PullRequestState::Closed),
            other => Err(UnknownPullRequestState(other.to_string())),
        }
    }
}

/// `github_repositories` table (`github.py:15-32`).
pub mod github_repository {
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "github_repositories";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order.
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
        "url",
        "config",
        "repository_id",
        "owner",
    ];

    /// One `GithubRepository` row. `url` is `null=True` with no default
    /// (`github.py:17`); `repository_id` is a GitHub-side `BigIntegerField`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GithubRepository {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub name: String,
        pub url: Option<String>,
        pub config: serde_json::Value,
        pub repository_id: i64,
        pub owner: String,
    }
}

/// `github_repository_syncs` table (`github.py:33-75`).
pub mod github_repository_sync {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Default `is_sync_enabled` (`github.py:55`).
    pub const DEFAULT_IS_SYNC_ENABLED: bool = false;

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "github_repository_syncs";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "repository_id",
        "credentials",
        "actor_id",
        "workspace_integration_id",
        "label_id",
        "is_sync_enabled",
        "last_synced_at",
        "last_sync_error",
    ];

    /// `repository` FK (`github.py:41`): `CASCADE`, `related_name="syncs"`.
    /// Deliberately not a `OneToOneField` (see module docs).
    pub const REPOSITORY_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const REPOSITORY_RELATED_NAME: &str = "syncs";

    /// `actor` FK (`github.py:44`, the bot user): `CASCADE`,
    /// `related_name="user_syncs"`.
    pub const ACTOR_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const ACTOR_RELATED_NAME: &str = "user_syncs";

    /// `workspace_integration` FK (`github.py:45-47`): `CASCADE`,
    /// `related_name="github_syncs"`.
    pub const WORKSPACE_INTEGRATION_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_INTEGRATION_RELATED_NAME: &str = "github_syncs";

    /// `label` FK (`github.py:48`): `SET_NULL`, nullable,
    /// `related_name="repo_syncs"`.
    pub const LABEL_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const LABEL_NULLABLE: bool = true;
    pub const LABEL_RELATED_NAME: &str = "repo_syncs";

    /// Partial unique on `project` (`github.py:64-70`): one active binding
    /// per project (and no `unique_together = [project, repository]` — see
    /// module docs).
    pub const UNIQUE_CONSTRAINTS: &[(&str, &[&str])] = &[(
        "github_repository_sync_unique_per_project_when_active",
        &["project_id"],
    )];

    /// One `GithubRepositorySync` row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GithubRepositorySync {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub repository_id: uuid::Uuid,
        pub credentials: serde_json::Value,
        pub actor_id: uuid::Uuid,
        pub workspace_integration_id: uuid::Uuid,
        pub label_id: Option<uuid::Uuid>,
        pub is_sync_enabled: bool,
        pub last_synced_at: Option<chrono::DateTime<chrono::Utc>>,
        pub last_sync_error: String,
    }
}

/// `github_issue_syncs` table (`github.py:76-103`).
pub mod github_issue_sync {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "github_issue_syncs";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "repo_issue_id",
        "github_issue_id",
        "issue_url",
        "issue_id",
        "repository_sync_id",
        "metadata",
        "gh_issue_created_at",
        "gh_issue_updated_at",
    ];

    /// `issue` FK (`github.py:79`): `CASCADE`, `related_name="github_syncs"`.
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const ISSUE_RELATED_NAME: &str = "github_syncs";

    /// `repository_sync` FK (`github.py:80`): `CASCADE`,
    /// `related_name="issue_syncs"`.
    pub const REPOSITORY_SYNC_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const REPOSITORY_SYNC_RELATED_NAME: &str = "issue_syncs";

    /// `unique_together = ["repository_sync", "issue"]` (`github.py:97`):
    /// plain unique index, enforced soft-delete-unaware.
    pub const UNIQUE_TOGETHER: &[&[&str]] = &[&["repository_sync_id", "issue_id"]];

    /// One `GithubIssueSync` row. `metadata` stores `github_user_login`,
    /// `upstream_gone_at`, `completion_comment_id`, and
    /// `completion_comment_error` (design §5, `github.py:81-85`);
    /// `gh_issue_created_at` / `gh_issue_updated_at` are the GitHub-side
    /// timestamps kept off `Issue` (`github.py:88-90`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GithubIssueSync {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub repo_issue_id: i64,
        pub github_issue_id: i64,
        pub issue_url: String,
        pub issue_id: uuid::Uuid,
        pub repository_sync_id: uuid::Uuid,
        pub metadata: serde_json::Value,
        pub gh_issue_created_at: Option<chrono::DateTime<chrono::Utc>>,
        pub gh_issue_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    }
}

/// `github_comment_syncs` table (`github.py:104-120`).
pub mod github_comment_sync {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "github_comment_syncs";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "repo_comment_id",
        "comment_id",
        "issue_sync_id",
    ];

    /// `comment` FK (`github.py:106`): `CASCADE`,
    /// `related_name="comment_syncs"`.
    pub const COMMENT_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const COMMENT_RELATED_NAME: &str = "comment_syncs";

    /// `issue_sync` FK (`github.py:107`): `CASCADE`,
    /// `related_name="comment_syncs"`.
    pub const ISSUE_SYNC_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const ISSUE_SYNC_RELATED_NAME: &str = "comment_syncs";

    /// `unique_together = ["issue_sync", "comment"]` (`github.py:116`):
    /// plain unique index, enforced soft-delete-unaware.
    pub const UNIQUE_TOGETHER: &[&[&str]] = &[&["issue_sync_id", "comment_id"]];

    /// One `GithubCommentSync` row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GithubCommentSync {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub repo_comment_id: i64,
        pub comment_id: uuid::Uuid,
        pub issue_sync_id: uuid::Uuid,
    }
}

/// `github_app_installations` table (`github.py:121-162`).
pub mod github_app_installation {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Default `repository_count` (`github.py:139`).
    pub const DEFAULT_REPOSITORY_COUNT: i32 = 0;

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "github_app_installations";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_integration_id",
        "installation_id",
        "account_login",
        "account_type",
        "repository_selection",
        "repository_count",
        "permissions",
        "events",
        "installed_at",
        "suspended_at",
        "verified_at",
        "last_checked_at",
        "last_check_error",
    ];

    /// `workspace_integration` OneToOne (`github.py:132-136`): `CASCADE`,
    /// `related_name="github_app_installation"`; the OneToOne carries an
    /// implicit column-level unique on `workspace_integration_id`.
    pub const WORKSPACE_INTEGRATION_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_INTEGRATION_RELATED_NAME: &str = "github_app_installation";

    /// `installation_id` is column-level `unique=True` (`github.py:137`).
    pub const UNIQUE_COLUMNS: &[&str] = &["installation_id"];

    /// One `GithubAppInstallation` row. `account_type` and
    /// `repository_selection` store the enum strings (see
    /// [`super::GithubAccountType`] and
    /// [`super::GithubRepositorySelection`]); `events` is `jsonb NOT NULL`
    /// defaulting to `[]` — [`default_events`] mirrors the per-row fresh
    /// array.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GithubAppInstallation {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_integration_id: uuid::Uuid,
        pub installation_id: i64,
        pub account_login: String,
        pub account_type: String,
        pub repository_selection: String,
        pub repository_count: i32,
        pub permissions: serde_json::Value,
        pub events: serde_json::Value,
        pub installed_at: Option<chrono::DateTime<chrono::Utc>>,
        pub suspended_at: Option<chrono::DateTime<chrono::Utc>>,
        pub verified_at: Option<chrono::DateTime<chrono::Utc>>,
        pub last_checked_at: Option<chrono::DateTime<chrono::Utc>>,
        pub last_check_error: String,
    }

    /// Fresh default for `events` (`JSONField(default=list)`).
    pub fn default_events() -> serde_json::Value {
        serde_json::Value::Array(Vec::new())
    }
}

/// `github_webhook_deliveries` table (`github.py:163-189`).
pub mod github_webhook_delivery {
    use serde::{Deserialize, Serialize};

    /// Default `status` (`github.py:174`, `Status.RECEIVED`).
    pub const DEFAULT_STATUS: &str = "received";

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "github_webhook_deliveries";

    /// Default `ORDER BY` (`Meta.ordering = ("-received_at",)`).
    pub const ORDERING: &str = "-received_at";

    /// Columns in Django `_meta` field order.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "delivery_id",
        "event",
        "action",
        "installation_id",
        "payload",
        "status",
        "received_at",
        "processed_at",
        "error",
    ];

    /// `delivery_id` is a column-level unique `UUIDField` (`github.py:170`);
    /// `installation_id` is nullable with a plain `db_index`
    /// (`github.py:173`).
    pub const UNIQUE_COLUMNS: &[&str] = &["delivery_id"];

    /// One `GithubWebhookDelivery` row. `received_at` is `auto_now_add`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GithubWebhookDelivery {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub delivery_id: uuid::Uuid,
        pub event: String,
        pub action: String,
        pub installation_id: Option<i64>,
        pub payload: serde_json::Value,
        pub status: String,
        pub received_at: chrono::DateTime<chrono::Utc>,
        pub processed_at: Option<chrono::DateTime<chrono::Utc>>,
        pub error: String,
    }
}

/// `github_app_install_sessions` table (`github.py:190-216`).
pub mod github_app_install_session {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Default `status` (`github.py:203`, `Status.STARTED`).
    pub const DEFAULT_STATUS: &str = "started";

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "github_app_install_sessions";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "state",
        "workspace_id",
        "actor_id",
        "installation_id",
        "account_login",
        "status",
        "expires_at",
        "completed_at",
        "error",
    ];

    /// `state` is column-level `unique=True` (`github.py:198`).
    pub const UNIQUE_COLUMNS: &[&str] = &["state"];

    /// `workspace` FK (`github.py:199`): `CASCADE`,
    /// `related_name="github_app_install_sessions"`.
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_RELATED_NAME: &str = "github_app_install_sessions";

    /// `actor` FK (`github.py:200`): `CASCADE`,
    /// `related_name="github_app_install_sessions"`.
    pub const ACTOR_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const ACTOR_RELATED_NAME: &str = "github_app_install_sessions";

    /// One `GithubAppInstallSession` row. `expires_at` is required and
    /// indexed with no default (`github.py:204`); `installation_id` is
    /// nullable (`github.py:201`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GithubAppInstallSession {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub state: String,
        pub workspace_id: uuid::Uuid,
        pub actor_id: uuid::Uuid,
        pub installation_id: Option<i64>,
        pub account_login: String,
        pub status: String,
        pub expires_at: chrono::DateTime<chrono::Utc>,
        pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
        pub error: String,
    }
}

/// `github_pull_request_links` table (`github.py:217-262`).
pub mod github_pull_request_link {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Default `state` (`github.py:240`, `State.OPEN`).
    pub const DEFAULT_STATE: &str = "open";

    /// Default `merged` / `draft` (`github.py:241-242`).
    pub const DEFAULT_MERGED: bool = false;
    pub const DEFAULT_DRAFT: bool = false;

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "github_pull_request_links";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order.
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
        "repo_owner",
        "repo_name",
        "pr_number",
        "url",
        "title",
        "state",
        "merged",
        "draft",
        "pr_updated_at",
    ];

    /// `issue` FK (`github.py:230`): `CASCADE`,
    /// `related_name="github_pull_requests"`.
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const ISSUE_RELATED_NAME: &str = "github_pull_requests";

    /// Partial unique (`github.py:253-259`): one issue per PR while active.
    pub const UNIQUE_CONSTRAINTS: &[(&str, &[&str])] = &[(
        "github_pr_link_unique_per_pr_when_active",
        &["repo_owner", "repo_name", "pr_number"],
    )];

    /// `Meta.indexes` (`github.py:260-263`).
    pub const INDEXES: &[(&str, &[&str])] = &[
        (
            "github_pr_l_repo_ow_idx",
            &["repo_owner", "repo_name", "pr_number"],
        ),
        ("github_pr_l_issue_idx", &["issue_id"]),
    ];

    /// One `GithubPullRequestLink` row. The `title` / `state` / `merged` /
    /// `draft` snapshot is display-only, refreshed by the `pull_request`
    /// webhook; it never drives the linked issue's workflow state
    /// (`github.py:217-228`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GithubPullRequestLink {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
        pub repo_owner: String,
        pub repo_name: String,
        pub pr_number: i32,
        pub url: String,
        pub title: String,
        pub state: String,
        pub merged: bool,
        pub draft: bool,
        pub pr_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support as ts;
    use super::super::{AUDIT_COLUMNS, PROJECT_COLUMNS};
    use super::*;

    fn prefix_len(base: &str) -> usize {
        if base == "ProjectBaseModel" {
            AUDIT_COLUMNS.len() + PROJECT_COLUMNS.len()
        } else {
            AUDIT_COLUMNS.len()
        }
    }

    /// `COLUMNS` must start with the audit prefix (and the project prefix
    /// for `ProjectBaseModel` tables); the remainder must equal the fixture
    /// entries field-for-field.
    fn assert_columns(fixture: &str, base: &str, columns: &[&str]) {
        assert_eq!(ts::fixture_base(fixture), base);
        let prefix = prefix_len(base);
        let actual_head: Vec<String> = columns[..prefix].iter().map(|c| (*c).to_string()).collect();
        let mut expected_head: Vec<String> = ts::owned_columns(AUDIT_COLUMNS);
        if base == "ProjectBaseModel" {
            expected_head.extend(ts::owned_columns(PROJECT_COLUMNS));
        }
        assert_eq!(actual_head, expected_head, "{fixture} audit/project prefix");
        let actual_tail: Vec<String> = columns[prefix..].iter().map(|c| (*c).to_string()).collect();
        assert_eq!(
            actual_tail,
            ts::fixture_columns(fixture),
            "{fixture} declared columns"
        );
    }

    fn assert_table(fixture: &str, table: &str, ordering: &str) {
        assert_eq!(ts::fixture_table(fixture), table);
        assert_eq!(ts::fixture_ordering(fixture), vec![ordering.to_string()]);
    }

    fn constraint_names(constraints: &[(&str, &[&str])]) -> Vec<String> {
        constraints.iter().map(|(n, _)| (*n).to_string()).collect()
    }

    fn unique_pairs(unique: &[&[&str]]) -> Vec<Vec<String>> {
        unique.iter().map(|cols| ts::owned_columns(cols)).collect()
    }

    #[test]
    fn repository_columns_match_fixture() {
        assert_table(
            "GithubRepository",
            github_repository::TABLE,
            github_repository::ORDERING,
        );
        assert_columns(
            "GithubRepository",
            "ProjectBaseModel",
            github_repository::COLUMNS,
        );
        let table: &str = github_repository::TABLE;
        assert_eq!(table, "github_repositories");
    }

    #[test]
    fn repository_sync_columns_match_fixture() {
        assert_table(
            "GithubRepositorySync",
            github_repository_sync::TABLE,
            github_repository_sync::ORDERING,
        );
        assert_columns(
            "GithubRepositorySync",
            "ProjectBaseModel",
            github_repository_sync::COLUMNS,
        );
        assert_eq!(
            constraint_names(github_repository_sync::UNIQUE_CONSTRAINTS),
            vec!["github_repository_sync_unique_per_project_when_active"]
        );
    }

    #[test]
    fn issue_sync_columns_match_fixture() {
        assert_table(
            "GithubIssueSync",
            github_issue_sync::TABLE,
            github_issue_sync::ORDERING,
        );
        assert_columns(
            "GithubIssueSync",
            "ProjectBaseModel",
            github_issue_sync::COLUMNS,
        );
        assert_eq!(
            unique_pairs(github_issue_sync::UNIQUE_TOGETHER),
            vec![vec!["repository_sync_id", "issue_id"]]
        );
    }

    #[test]
    fn comment_sync_columns_match_fixture() {
        assert_table(
            "GithubCommentSync",
            github_comment_sync::TABLE,
            github_comment_sync::ORDERING,
        );
        assert_columns(
            "GithubCommentSync",
            "ProjectBaseModel",
            github_comment_sync::COLUMNS,
        );
        assert_eq!(
            unique_pairs(github_comment_sync::UNIQUE_TOGETHER),
            vec![vec!["issue_sync_id", "comment_id"]]
        );
    }

    #[test]
    fn app_installation_columns_match_fixture() {
        assert_table(
            "GithubAppInstallation",
            github_app_installation::TABLE,
            github_app_installation::ORDERING,
        );
        assert_columns(
            "GithubAppInstallation",
            "BaseModel",
            github_app_installation::COLUMNS,
        );
        assert_eq!(
            ts::owned_columns(github_app_installation::UNIQUE_COLUMNS),
            vec!["installation_id"]
        );
        assert_eq!(github_app_installation::DEFAULT_REPOSITORY_COUNT, 0);
        assert_eq!(
            github_app_installation::default_events(),
            serde_json::Value::Array(Vec::new())
        );
    }

    #[test]
    fn webhook_delivery_columns_match_fixture() {
        assert_table(
            "GithubWebhookDelivery",
            github_webhook_delivery::TABLE,
            github_webhook_delivery::ORDERING,
        );
        assert_columns(
            "GithubWebhookDelivery",
            "BaseModel",
            github_webhook_delivery::COLUMNS,
        );
        let ordering: &str = github_webhook_delivery::ORDERING;
        assert_eq!(ordering, "-received_at");
        assert_eq!(
            ts::owned_columns(github_webhook_delivery::UNIQUE_COLUMNS),
            vec!["delivery_id"]
        );
        assert_eq!(github_webhook_delivery::DEFAULT_STATUS, "received");
    }

    #[test]
    fn app_install_session_columns_match_fixture() {
        assert_table(
            "GithubAppInstallSession",
            github_app_install_session::TABLE,
            github_app_install_session::ORDERING,
        );
        assert_columns(
            "GithubAppInstallSession",
            "BaseModel",
            github_app_install_session::COLUMNS,
        );
        assert_eq!(
            ts::owned_columns(github_app_install_session::UNIQUE_COLUMNS),
            vec!["state"]
        );
        assert_eq!(github_app_install_session::DEFAULT_STATUS, "started");
    }

    #[test]
    fn pull_request_link_columns_match_fixture() {
        assert_table(
            "GithubPullRequestLink",
            github_pull_request_link::TABLE,
            github_pull_request_link::ORDERING,
        );
        assert_columns(
            "GithubPullRequestLink",
            "ProjectBaseModel",
            github_pull_request_link::COLUMNS,
        );
        assert_eq!(
            constraint_names(github_pull_request_link::UNIQUE_CONSTRAINTS),
            vec!["github_pr_link_unique_per_pr_when_active"]
        );
        assert_eq!(
            constraint_names(github_pull_request_link::INDEXES),
            vec!["github_pr_l_repo_ow_idx", "github_pr_l_issue_idx"]
        );
        assert_eq!(github_pull_request_link::DEFAULT_STATE, "open");
    }

    #[test]
    fn enums_round_trip() {
        assert_eq!(GithubAccountType::default(), GithubAccountType::Unknown);
        assert_eq!(
            "User".parse::<GithubAccountType>().unwrap(),
            GithubAccountType::User
        );
        assert_eq!(
            "Organization".parse::<GithubAccountType>().unwrap(),
            GithubAccountType::Organization
        );
        assert_eq!(
            "Unknown".parse::<GithubAccountType>().unwrap(),
            GithubAccountType::Unknown
        );
        assert!("user".parse::<GithubAccountType>().is_err());
        assert_eq!(
            GithubRepositorySelection::default(),
            GithubRepositorySelection::Selected
        );
        assert_eq!(
            "all".parse::<GithubRepositorySelection>().unwrap(),
            GithubRepositorySelection::All
        );
        assert_eq!(
            "selected".parse::<GithubRepositorySelection>().unwrap(),
            GithubRepositorySelection::Selected
        );
        assert!("none".parse::<GithubRepositorySelection>().is_err());
        assert_eq!(
            GithubWebhookStatus::default(),
            GithubWebhookStatus::Received
        );
        assert_eq!(
            "received".parse::<GithubWebhookStatus>().unwrap(),
            GithubWebhookStatus::Received
        );
        assert_eq!(
            "processed".parse::<GithubWebhookStatus>().unwrap(),
            GithubWebhookStatus::Processed
        );
        assert_eq!(
            "failed".parse::<GithubWebhookStatus>().unwrap(),
            GithubWebhookStatus::Failed
        );
        assert_eq!(
            "skipped".parse::<GithubWebhookStatus>().unwrap(),
            GithubWebhookStatus::Skipped
        );
        assert!("done".parse::<GithubWebhookStatus>().is_err());
        assert_eq!(GithubSessionStatus::default(), GithubSessionStatus::Started);
        assert_eq!(
            "started".parse::<GithubSessionStatus>().unwrap(),
            GithubSessionStatus::Started
        );
        assert_eq!(
            "completed".parse::<GithubSessionStatus>().unwrap(),
            GithubSessionStatus::Completed
        );
        assert_eq!(
            "expired".parse::<GithubSessionStatus>().unwrap(),
            GithubSessionStatus::Expired
        );
        assert_eq!(
            "failed".parse::<GithubSessionStatus>().unwrap(),
            GithubSessionStatus::Failed
        );
        assert!("pending".parse::<GithubSessionStatus>().is_err());
        assert_eq!(PullRequestState::default(), PullRequestState::Open);
        assert_eq!(
            "open".parse::<PullRequestState>().unwrap(),
            PullRequestState::Open
        );
        assert_eq!(
            "closed".parse::<PullRequestState>().unwrap(),
            PullRequestState::Closed
        );
        // No MERGED: merged PRs are `state=closed` + `merged=true`.
        assert!("merged".parse::<PullRequestState>().is_err());
    }
}
