//! Git-provider table models (D-05, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/integration/git.py:11-344`
//! (`GitProviderAccount`, `GitRepository`, `GitRepositoryBinding`,
//! `GitIssueSync`, `GitCommentSync`, `GitCodeReviewLink`,
//! `GitWebhookDelivery`, `GitWebhookRegistration`), adopting the
//! Django-owned schema column-for-column. Read models / row mappings only;
//! no migrations.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * Every column default below is application-level (Django); like the
//!   D-01 tables, Rust inserts must supply these values explicitly — there
//!   is no DB fallback.
//! * FK `on_delete` (`CASCADE`, `SET_NULL`, `PROTECT`) is ORM-emulated, so
//!   Rust write paths (later layer issues) must replicate the
//!   cascading / nulling / protecting explicitly.
//! * `GitRepository`'s two partial unique constraints
//!   (`git_repo_uniq_ext_active`, `git_repo_uniq_full_active`) and its
//!   `git_repo_provider_full_idx` index come from `git.py:93-106`: the
//!   fixture (`columns.json`, `GitRepository` entry) records neither, so
//!   the test for that table asserts the Python-derived values directly.
//! * All partial uniques are conditioned on `deleted_at IS NULL`, so
//!   soft-deleted rows never collide.
//! * `GitRepositoryBinding.clone_auth_mode` defaults to `RUNNER_MANAGED`,
//!   but `bind_repository` overwrites it from the remote (`RUNNER_MANAGED`
//!   when `remote.is_private`, else `PUBLIC`; `services.py:318-322`) — the
//!   services layer owns that transition, not this type.
//! * `GitWebhookDelivery.repository` is nullable (`SET_NULL`): deliveries
//!   that arrive before the repository is matched store `NULL`.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

use super::OnDelete;

/// Provider choices shared by the git tables
/// (`git.py:14-16`, `git.py:68-70`, ported as-is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum GitProvider {
    #[default]
    Github,
    Gitlab,
}

impl GitProvider {
    pub const GITHUB: &'static str = "github";
    pub const GITLAB: &'static str = "gitlab";

    pub fn as_str(self) -> &'static str {
        match self {
            GitProvider::Github => Self::GITHUB,
            GitProvider::Gitlab => Self::GITLAB,
        }
    }
}

impl std::fmt::Display for GitProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownGitProvider(pub String);

impl std::fmt::Display for UnknownGitProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown git provider: {}", self.0)
    }
}

impl std::error::Error for UnknownGitProvider {}

impl FromStr for GitProvider {
    type Err = UnknownGitProvider;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "github" => Ok(GitProvider::Github),
            "gitlab" => Ok(GitProvider::Gitlab),
            other => Err(UnknownGitProvider(other.to_string())),
        }
    }
}

/// Auth-type choices for [`git_provider_account::GitProviderAccount`]
/// (`git.py:18-23`, ported as-is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum GitAuthType {
    #[default]
    GithubApp,
    Pat,
    Oauth,
    GroupToken,
    ProjectToken,
}

impl GitAuthType {
    pub const GITHUB_APP: &'static str = "github_app";
    pub const PAT: &'static str = "pat";
    pub const OAUTH: &'static str = "oauth";
    pub const GROUP_TOKEN: &'static str = "group_token";
    pub const PROJECT_TOKEN: &'static str = "project_token";

    pub fn as_str(self) -> &'static str {
        match self {
            GitAuthType::GithubApp => Self::GITHUB_APP,
            GitAuthType::Pat => Self::PAT,
            GitAuthType::Oauth => Self::OAUTH,
            GitAuthType::GroupToken => Self::GROUP_TOKEN,
            GitAuthType::ProjectToken => Self::PROJECT_TOKEN,
        }
    }
}

impl std::fmt::Display for GitAuthType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownGitAuthType(pub String);

impl std::fmt::Display for UnknownGitAuthType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown git auth type: {}", self.0)
    }
}

impl std::error::Error for UnknownGitAuthType {}

impl FromStr for GitAuthType {
    type Err = UnknownGitAuthType;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "github_app" => Ok(GitAuthType::GithubApp),
            "pat" => Ok(GitAuthType::Pat),
            "oauth" => Ok(GitAuthType::Oauth),
            "group_token" => Ok(GitAuthType::GroupToken),
            "project_token" => Ok(GitAuthType::ProjectToken),
            other => Err(UnknownGitAuthType(other.to_string())),
        }
    }
}

/// Webhook delivery status shared by [`git_webhook_delivery`] and the
/// github delivery table (`git.py:275-279`, ported as-is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum GitWebhookStatus {
    #[default]
    Received,
    Processed,
    Failed,
    Skipped,
}

impl GitWebhookStatus {
    pub const RECEIVED: &'static str = "received";
    pub const PROCESSED: &'static str = "processed";
    pub const FAILED: &'static str = "failed";
    pub const SKIPPED: &'static str = "skipped";

    pub fn as_str(self) -> &'static str {
        match self {
            GitWebhookStatus::Received => Self::RECEIVED,
            GitWebhookStatus::Processed => Self::PROCESSED,
            GitWebhookStatus::Failed => Self::FAILED,
            GitWebhookStatus::Skipped => Self::SKIPPED,
        }
    }
}

impl std::fmt::Display for GitWebhookStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownGitWebhookStatus(pub String);

impl std::fmt::Display for UnknownGitWebhookStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown git webhook status: {}", self.0)
    }
}

impl std::error::Error for UnknownGitWebhookStatus {}

impl FromStr for GitWebhookStatus {
    type Err = UnknownGitWebhookStatus;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "received" => Ok(GitWebhookStatus::Received),
            "processed" => Ok(GitWebhookStatus::Processed),
            "failed" => Ok(GitWebhookStatus::Failed),
            "skipped" => Ok(GitWebhookStatus::Skipped),
            other => Err(UnknownGitWebhookStatus(other.to_string())),
        }
    }
}

/// `git_provider_accounts` table (`git.py:11-64`).
pub mod git_provider_account {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Account status choices (`git.py:25-29`, ported as-is).
    pub const STATUS_CONNECTED: &str = "connected";
    pub const STATUS_DEGRADED: &str = "degraded";
    pub const STATUS_REVOKED: &str = "revoked";
    pub const STATUS_ERROR: &str = "error";
    pub const STATUSES: &[&str] = &[
        STATUS_CONNECTED,
        STATUS_DEGRADED,
        STATUS_REVOKED,
        STATUS_ERROR,
    ];

    /// Default `status` (`git.py:48`, `Status.CONNECTED`).
    pub const DEFAULT_STATUS: &str = STATUS_CONNECTED;

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "git_provider_accounts";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order. FK columns use the Django
    /// attnames (`workspace_id`, `workspace_integration_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "provider",
        "host_url",
        "auth_type",
        "external_account_id",
        "external_account_login",
        "display_name",
        "capabilities",
        "credential_config",
        "workspace_integration_id",
        "status",
        "verified_at",
        "last_check_error",
        "metadata",
    ];

    /// `workspace` FK (`git.py:31`): `CASCADE`, required,
    /// `related_name="git_provider_accounts"`.
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_NULLABLE: bool = false;
    pub const WORKSPACE_RELATED_NAME: &str = "git_provider_accounts";

    /// `workspace_integration` FK (`git.py:41-46`): `SET_NULL`, nullable.
    pub const WORKSPACE_INTEGRATION_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const WORKSPACE_INTEGRATION_NULLABLE: bool = true;
    pub const WORKSPACE_INTEGRATION_RELATED_NAME: &str = "git_provider_accounts";

    /// `Meta.indexes` (`git.py:58-61`).
    pub const INDEXES: &[(&str, &[&str])] = &[
        (
            "git_pa_ws_provider_host_idx",
            &["workspace_id", "provider", "host_url"],
        ),
        (
            "git_pa_provider_ext_idx",
            &["provider", "host_url", "external_account_id"],
        ),
    ];

    /// One `GitProviderAccount` row. `status`, `provider`, and `auth_type`
    /// store the enum strings (see [`GitProvider`], [`GitAuthType`], and
    /// `STATUS_*`); `capabilities`, `credential_config`, and `metadata`
    /// are `jsonb NOT NULL` defaulting to `{}`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GitProviderAccount {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub provider: String,
        pub host_url: String,
        pub auth_type: String,
        pub external_account_id: String,
        pub external_account_login: String,
        pub display_name: String,
        pub capabilities: serde_json::Value,
        pub credential_config: serde_json::Value,
        pub workspace_integration_id: Option<uuid::Uuid>,
        pub status: String,
        pub verified_at: Option<chrono::DateTime<chrono::Utc>>,
        pub last_check_error: String,
        pub metadata: serde_json::Value,
    }
}

/// `git_repositories` table (`git.py:65-107`).
pub mod git_repository {
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "git_repositories";

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
        "provider",
        "host_url",
        "external_id",
        "namespace",
        "name",
        "full_name",
        "web_url",
        "clone_url_http",
        "clone_url_ssh",
        "default_branch",
        "is_private",
        "metadata",
    ];

    /// Partial unique constraints (`Meta.constraints`, `git.py:93-103`).
    /// Both are conditioned on `deleted_at IS NULL`; the first additionally
    /// requires `external_id != ''`. Neither is recorded in the fixture's
    /// `GitRepository` entry — these names come straight from Python.
    pub const UNIQUE_CONSTRAINTS: &[(&str, &[&str])] = &[
        (
            "git_repo_uniq_ext_active",
            &["provider", "host_url", "external_id"],
        ),
        (
            "git_repo_uniq_full_active",
            &["provider", "host_url", "full_name"],
        ),
    ];

    /// `Meta.indexes` (`git.py:104-106`; also absent from the fixture).
    pub const INDEXES: &[(&str, &[&str])] = &[(
        "git_repo_provider_full_idx",
        &["provider", "host_url", "full_name"],
    )];

    /// One `GitRepository` row. `provider` stores the [`super::GitProvider`]
    /// string; `metadata` is `jsonb NOT NULL` defaulting to `{}`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GitRepository {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub provider: String,
        pub host_url: String,
        pub external_id: String,
        pub namespace: String,
        pub name: String,
        pub full_name: String,
        pub web_url: String,
        pub clone_url_http: String,
        pub clone_url_ssh: String,
        pub default_branch: String,
        pub is_private: bool,
        pub metadata: serde_json::Value,
    }
}

/// `git_repository_bindings` table (`git.py:108-151`).
pub mod git_repository_binding {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Clone-auth-mode choices (`git.py:111-114`, ported as-is).
    pub const CLONE_AUTH_MODE_PUBLIC: &str = "public";
    pub const CLONE_AUTH_MODE_RUNNER_MANAGED: &str = "runner_managed";
    pub const CLONE_AUTH_MODE_MANAGED_EPHEMERAL: &str = "managed_ephemeral";
    pub const CLONE_AUTH_MODES: &[&str] = &[
        CLONE_AUTH_MODE_PUBLIC,
        CLONE_AUTH_MODE_RUNNER_MANAGED,
        CLONE_AUTH_MODE_MANAGED_EPHEMERAL,
    ];

    /// Default `clone_auth_mode` (`git.py:126-130`).
    pub const DEFAULT_CLONE_AUTH_MODE: &str = CLONE_AUTH_MODE_RUNNER_MANAGED;

    /// Default `is_sync_enabled` (`git.py:125`).
    pub const DEFAULT_IS_SYNC_ENABLED: bool = false;

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "git_repository_bindings";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order: audit prefix, then
    /// `project_id`, `workspace_id` (`ProjectBaseModel`), then the declared
    /// fields with Django attnames.
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
        "provider_account_id",
        "actor_id",
        "is_sync_enabled",
        "clone_auth_mode",
        "last_synced_at",
        "last_sync_error",
        "metadata",
    ];

    /// `repository` FK (`git.py:116`): `CASCADE`, `related_name="bindings"`.
    pub const REPOSITORY_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const REPOSITORY_RELATED_NAME: &str = "bindings";

    /// `provider_account` FK (`git.py:117-121`): `PROTECT`,
    /// `related_name="repository_bindings"`.
    pub const PROVIDER_ACCOUNT_ON_DELETE: OnDelete = OnDelete::Protect;
    pub const PROVIDER_ACCOUNT_RELATED_NAME: &str = "repository_bindings";

    /// `actor` FK (`git.py:122`): `CASCADE`,
    /// `related_name="git_repository_bindings"`.
    pub const ACTOR_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const ACTOR_RELATED_NAME: &str = "git_repository_bindings";

    /// Partial unique on `project` (`git.py:141-147`): one active binding
    /// per project.
    pub const UNIQUE_CONSTRAINTS: &[(&str, &[&str])] =
        &[("git_bind_uniq_project_active", &["project_id"])];

    /// `Meta.indexes` (`git.py:148-151`).
    pub const INDEXES: &[(&str, &[&str])] = &[
        ("git_bind_ws_project_idx", &["workspace_id", "project_id"]),
        (
            "git_bind_repo_acct_idx",
            &["repository_id", "provider_account_id"],
        ),
    ];

    /// One `GitRepositoryBinding` row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GitRepositoryBinding {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub repository_id: uuid::Uuid,
        pub provider_account_id: uuid::Uuid,
        pub actor_id: uuid::Uuid,
        pub is_sync_enabled: bool,
        pub clone_auth_mode: String,
        pub last_synced_at: Option<chrono::DateTime<chrono::Utc>>,
        pub last_sync_error: String,
        pub metadata: serde_json::Value,
    }
}

/// `git_issue_syncs` table (`git.py:152-189`).
pub mod git_issue_sync {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "git_issue_syncs";

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
        "binding_id",
        "issue_id",
        "provider",
        "external_id",
        "external_iid",
        "web_url",
        "remote_state",
        "remote_created_at",
        "remote_updated_at",
        "metadata",
    ];

    /// `binding` FK (`git.py:153`): `CASCADE`, `related_name="issue_syncs"`.
    pub const BINDING_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const BINDING_RELATED_NAME: &str = "issue_syncs";

    /// `issue` FK (`git.py:154`): `CASCADE`, `related_name="git_syncs"`.
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const ISSUE_RELATED_NAME: &str = "git_syncs";

    /// Partial uniques (`git.py:171-182`).
    pub const UNIQUE_CONSTRAINTS: &[(&str, &[&str])] = &[
        ("git_issue_uniq_iid_active", &["binding_id", "external_iid"]),
        ("git_issue_uniq_issue_active", &["binding_id", "issue_id"]),
    ];

    /// `Meta.indexes` (`git.py:183-186`).
    pub const INDEXES: &[(&str, &[&str])] = &[
        ("git_issue_sync_issue_idx", &["issue_id"]),
        ("git_issue_provider_iid_idx", &["provider", "external_iid"]),
    ];

    /// One `GitIssueSync` row. `metadata` carries `author`, `remote`, and
    /// the optional `upstream_gone_at` / `completion_comment_id` /
    /// `completion_comment_error` keys (fixture note).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GitIssueSync {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub binding_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
        pub provider: String,
        pub external_id: String,
        pub external_iid: String,
        pub web_url: String,
        pub remote_state: String,
        pub remote_created_at: Option<chrono::DateTime<chrono::Utc>>,
        pub remote_updated_at: Option<chrono::DateTime<chrono::Utc>>,
        pub metadata: serde_json::Value,
    }
}

/// `git_comment_syncs` table (`git.py:190-223`).
pub mod git_comment_sync {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "git_comment_syncs";

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
        "issue_sync_id",
        "comment_id",
        "provider",
        "external_id",
        "remote_created_at",
        "remote_updated_at",
        "metadata",
    ];

    /// `issue_sync` FK (`git.py:191`): `CASCADE`,
    /// `related_name="comment_syncs"`.
    pub const ISSUE_SYNC_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const ISSUE_SYNC_RELATED_NAME: &str = "comment_syncs";

    /// `comment` FK (`git.py:192`): `CASCADE`,
    /// `related_name="git_comment_syncs"`.
    pub const COMMENT_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const COMMENT_RELATED_NAME: &str = "git_comment_syncs";

    /// Partial uniques (`git.py:208-217`).
    pub const UNIQUE_CONSTRAINTS: &[(&str, &[&str])] = &[
        (
            "git_comment_uniq_ext_active",
            &["issue_sync_id", "external_id"],
        ),
        (
            "git_comment_uniq_comment_act",
            &["issue_sync_id", "comment_id"],
        ),
    ];

    /// `Meta.indexes` (`git.py:218-220`).
    pub const INDEXES: &[(&str, &[&str])] = &[("git_comment_comment_idx", &["comment_id"])];

    /// One `GitCommentSync` row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GitCommentSync {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub issue_sync_id: uuid::Uuid,
        pub comment_id: uuid::Uuid,
        pub provider: String,
        pub external_id: String,
        pub remote_created_at: Option<chrono::DateTime<chrono::Utc>>,
        pub remote_updated_at: Option<chrono::DateTime<chrono::Utc>>,
        pub metadata: serde_json::Value,
    }
}

/// Code-review link state (`git.py:227-230`, ported as-is). Unlike the
/// github PR link state, this one has `MERGED`; `merged` is still tracked
/// by a separate bool as well.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum CodeReviewState {
    #[default]
    Open,
    Closed,
    Merged,
}

impl CodeReviewState {
    pub const OPEN: &'static str = "open";
    pub const CLOSED: &'static str = "closed";
    pub const MERGED: &'static str = "merged";

    pub fn as_str(self) -> &'static str {
        match self {
            CodeReviewState::Open => Self::OPEN,
            CodeReviewState::Closed => Self::CLOSED,
            CodeReviewState::Merged => Self::MERGED,
        }
    }
}

impl std::fmt::Display for CodeReviewState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownCodeReviewState(pub String);

impl std::fmt::Display for UnknownCodeReviewState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown code review state: {}", self.0)
    }
}

impl std::error::Error for UnknownCodeReviewState {}

impl FromStr for CodeReviewState {
    type Err = UnknownCodeReviewState;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(CodeReviewState::Open),
            "closed" => Ok(CodeReviewState::Closed),
            "merged" => Ok(CodeReviewState::Merged),
            other => Err(UnknownCodeReviewState(other.to_string())),
        }
    }
}

/// `git_code_review_links` table (`git.py:224-271`).
pub mod git_code_review_link {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Default `state` (`git.py:245`, `State.OPEN`).
    pub const DEFAULT_STATE: &str = "open";

    /// Default `merged` / `draft` (`git.py:246-247`).
    pub const DEFAULT_MERGED: bool = false;
    pub const DEFAULT_DRAFT: bool = false;

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "git_code_review_links";

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
        "provider",
        "host_url",
        "namespace",
        "repo_name",
        "repo_external_id",
        "external_id",
        "external_iid",
        "url",
        "title",
        "state",
        "merged",
        "draft",
        "remote_updated_at",
        "metadata",
    ];

    /// `issue` FK (`git.py:232`): `CASCADE`,
    /// `related_name="git_code_reviews"`.
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const ISSUE_RELATED_NAME: &str = "git_code_reviews";

    /// Partial uniques (`git.py:256-265`): the id-based link when the repo
    /// carries an external id, otherwise the path-based link.
    pub const UNIQUE_CONSTRAINTS: &[(&str, &[&str])] = &[
        (
            "git_cr_uniq_repo_iid_active",
            &["provider", "host_url", "repo_external_id", "external_iid"],
        ),
        (
            "git_cr_uniq_path_iid_active",
            &[
                "provider",
                "host_url",
                "namespace",
                "repo_name",
                "external_iid",
            ],
        ),
    ];

    /// `Meta.indexes` (`git.py:266-269`).
    pub const INDEXES: &[(&str, &[&str])] = &[
        ("git_cr_issue_idx", &["issue_id"]),
        (
            "git_cr_provider_repo_idx",
            &["provider", "host_url", "namespace", "repo_name"],
        ),
    ];

    /// One `GitCodeReviewLink` row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GitCodeReviewLink {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
        pub provider: String,
        pub host_url: String,
        pub namespace: String,
        pub repo_name: String,
        pub repo_external_id: String,
        pub external_id: String,
        pub external_iid: String,
        pub url: String,
        pub title: String,
        pub state: String,
        pub merged: bool,
        pub draft: bool,
        pub remote_updated_at: Option<chrono::DateTime<chrono::Utc>>,
        pub metadata: serde_json::Value,
    }
}

/// `git_webhook_deliveries` table (`git.py:272-315`).
pub mod git_webhook_delivery {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Default `status` (`git.py:292`, `Status.RECEIVED`).
    pub const DEFAULT_STATUS: &str = "received";

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "git_webhook_deliveries";

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
        "provider",
        "host_url",
        "delivery_id",
        "event",
        "action",
        "repository_id",
        "raw_headers",
        "payload",
        "status",
        "received_at",
        "processed_at",
        "error",
        "metadata",
    ];

    /// `repository` FK (`git.py:281-286`): `SET_NULL`, nullable — deliveries
    /// that arrive before the repository is matched store `NULL`.
    pub const REPOSITORY_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const REPOSITORY_NULLABLE: bool = true;
    pub const REPOSITORY_RELATED_NAME: &str = "webhook_deliveries";

    /// Partial unique (`git.py:307-313`).
    pub const UNIQUE_CONSTRAINTS: &[(&str, &[&str])] = &[(
        "git_wh_delivery_uniq_active",
        &["provider", "host_url", "delivery_id"],
    )];

    /// One `GitWebhookDelivery` row. `received_at` is `auto_now_add`
    /// (Django stamps it on insert); `delivery_id` carries a plain
    /// `db_index` (`git.py:278`), uniqueness comes from the partial
    /// constraint above.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GitWebhookDelivery {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub provider: String,
        pub host_url: String,
        pub delivery_id: String,
        pub event: String,
        pub action: String,
        pub repository_id: Option<uuid::Uuid>,
        pub raw_headers: serde_json::Value,
        pub payload: serde_json::Value,
        pub status: String,
        pub received_at: chrono::DateTime<chrono::Utc>,
        pub processed_at: Option<chrono::DateTime<chrono::Utc>>,
        pub error: String,
        pub metadata: serde_json::Value,
    }
}

/// `git_webhook_registrations` table (`git.py:316-344`).
pub mod git_webhook_registration {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "git_webhook_registrations";

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
        "repository_id",
        "provider_account_id",
        "provider_hook_id",
        "events",
        "secret_ref",
        "last_verified_at",
        "last_check_error",
        "metadata",
    ];

    /// `repository` FK (`git.py:317`): `CASCADE`,
    /// `related_name="webhook_registrations"`.
    pub const REPOSITORY_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const REPOSITORY_RELATED_NAME: &str = "webhook_registrations";

    /// `provider_account` FK (`git.py:318-322`): `CASCADE`,
    /// `related_name="webhook_registrations"`.
    pub const PROVIDER_ACCOUNT_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const PROVIDER_ACCOUNT_RELATED_NAME: &str = "webhook_registrations";

    /// Partial unique (`git.py:337-343`): one active registration per
    /// repository + provider account.
    pub const UNIQUE_CONSTRAINTS: &[(&str, &[&str])] = &[(
        "git_wh_reg_uniq_repo_acct",
        &["repository_id", "provider_account_id"],
    )];

    /// One `GitWebhookRegistration` row. `events` is `jsonb NOT NULL`
    /// defaulting to `[]` (`JSONField(default=list)`, `git.py:323`); each
    /// call yields a fresh array — [`default_events`] mirrors that.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct GitWebhookRegistration {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub repository_id: uuid::Uuid,
        pub provider_account_id: uuid::Uuid,
        pub provider_hook_id: String,
        pub events: serde_json::Value,
        pub secret_ref: String,
        pub last_verified_at: Option<chrono::DateTime<chrono::Utc>>,
        pub last_check_error: String,
        pub metadata: serde_json::Value,
    }

    /// Fresh default for `events` (`JSONField(default=list)`).
    pub fn default_events() -> serde_json::Value {
        serde_json::Value::Array(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support as ts;
    use super::super::{AUDIT_COLUMNS, PROJECT_COLUMNS};
    use super::*;

    fn audit_len() -> usize {
        AUDIT_COLUMNS.len()
    }

    fn project_len() -> usize {
        PROJECT_COLUMNS.len()
    }

    /// `COLUMNS` must start with the audit prefix (and the project prefix
    /// for `ProjectBaseModel` tables); the remainder must equal the fixture
    /// entries field-for-field.
    fn assert_columns(fixture: &str, base: &str, columns: &[&str]) {
        assert_eq!(ts::fixture_base(fixture), base);
        let prefix = if base == "ProjectBaseModel" {
            audit_len() + project_len()
        } else {
            audit_len()
        };
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

    #[test]
    fn provider_account_columns_match_fixture() {
        assert_table(
            "GitProviderAccount",
            git_provider_account::TABLE,
            git_provider_account::ORDERING,
        );
        assert_columns(
            "GitProviderAccount",
            "BaseModel",
            git_provider_account::COLUMNS,
        );
        let table: &str = git_provider_account::TABLE;
        assert_eq!(table, "git_provider_accounts");
        let ordering: &str = git_provider_account::ORDERING;
        assert_eq!(ordering, "-created_at");
        assert_eq!(git_provider_account::DEFAULT_STATUS, "connected");
        assert_eq!(
            ts::owned_columns(git_provider_account::STATUSES),
            vec!["connected", "degraded", "revoked", "error"]
        );
        assert_eq!(
            constraint_names(git_provider_account::INDEXES),
            vec!["git_pa_ws_provider_host_idx", "git_pa_provider_ext_idx"]
        );
    }

    #[test]
    fn repository_columns_match_fixture() {
        assert_table(
            "GitRepository",
            git_repository::TABLE,
            git_repository::ORDERING,
        );
        assert_columns("GitRepository", "BaseModel", git_repository::COLUMNS);
        let table: &str = git_repository::TABLE;
        assert_eq!(table, "git_repositories");
        // Python-derived (absent from the fixture): two partial uniques plus
        // the provider/full-name index (`git.py:93-106`).
        assert_eq!(
            constraint_names(git_repository::UNIQUE_CONSTRAINTS),
            vec!["git_repo_uniq_ext_active", "git_repo_uniq_full_active"]
        );
        assert_eq!(
            constraint_names(git_repository::INDEXES),
            vec!["git_repo_provider_full_idx"]
        );
    }

    #[test]
    fn repository_binding_columns_match_fixture() {
        assert_table(
            "GitRepositoryBinding",
            git_repository_binding::TABLE,
            git_repository_binding::ORDERING,
        );
        assert_columns(
            "GitRepositoryBinding",
            "ProjectBaseModel",
            git_repository_binding::COLUMNS,
        );
        assert_eq!(
            constraint_names(git_repository_binding::UNIQUE_CONSTRAINTS),
            vec!["git_bind_uniq_project_active"]
        );
        assert_eq!(
            constraint_names(git_repository_binding::INDEXES),
            vec!["git_bind_ws_project_idx", "git_bind_repo_acct_idx"]
        );
        assert_eq!(
            git_repository_binding::DEFAULT_CLONE_AUTH_MODE,
            "runner_managed"
        );
        assert_eq!(
            ts::owned_columns(git_repository_binding::CLONE_AUTH_MODES),
            vec!["public", "runner_managed", "managed_ephemeral"]
        );
    }

    #[test]
    fn issue_sync_columns_match_fixture() {
        assert_table(
            "GitIssueSync",
            git_issue_sync::TABLE,
            git_issue_sync::ORDERING,
        );
        assert_columns("GitIssueSync", "ProjectBaseModel", git_issue_sync::COLUMNS);
        assert_eq!(
            constraint_names(git_issue_sync::UNIQUE_CONSTRAINTS),
            vec!["git_issue_uniq_iid_active", "git_issue_uniq_issue_active"]
        );
        assert_eq!(
            constraint_names(git_issue_sync::INDEXES),
            vec!["git_issue_sync_issue_idx", "git_issue_provider_iid_idx"]
        );
    }

    #[test]
    fn comment_sync_columns_match_fixture() {
        assert_table(
            "GitCommentSync",
            git_comment_sync::TABLE,
            git_comment_sync::ORDERING,
        );
        assert_columns(
            "GitCommentSync",
            "ProjectBaseModel",
            git_comment_sync::COLUMNS,
        );
        assert_eq!(
            constraint_names(git_comment_sync::UNIQUE_CONSTRAINTS),
            vec![
                "git_comment_uniq_ext_active",
                "git_comment_uniq_comment_act"
            ]
        );
        assert_eq!(
            constraint_names(git_comment_sync::INDEXES),
            vec!["git_comment_comment_idx"]
        );
    }

    #[test]
    fn code_review_link_columns_match_fixture() {
        assert_table(
            "GitCodeReviewLink",
            git_code_review_link::TABLE,
            git_code_review_link::ORDERING,
        );
        assert_columns(
            "GitCodeReviewLink",
            "ProjectBaseModel",
            git_code_review_link::COLUMNS,
        );
        assert_eq!(
            constraint_names(git_code_review_link::UNIQUE_CONSTRAINTS),
            vec!["git_cr_uniq_repo_iid_active", "git_cr_uniq_path_iid_active"]
        );
        assert_eq!(
            constraint_names(git_code_review_link::INDEXES),
            vec!["git_cr_issue_idx", "git_cr_provider_repo_idx"]
        );
        assert_eq!(git_code_review_link::DEFAULT_STATE, "open");
    }

    #[test]
    fn webhook_delivery_columns_match_fixture() {
        assert_table(
            "GitWebhookDelivery",
            git_webhook_delivery::TABLE,
            git_webhook_delivery::ORDERING,
        );
        assert_columns(
            "GitWebhookDelivery",
            "BaseModel",
            git_webhook_delivery::COLUMNS,
        );
        let ordering: &str = git_webhook_delivery::ORDERING;
        assert_eq!(ordering, "-received_at");
        assert_eq!(
            constraint_names(git_webhook_delivery::UNIQUE_CONSTRAINTS),
            vec!["git_wh_delivery_uniq_active"]
        );
        assert_eq!(git_webhook_delivery::DEFAULT_STATUS, "received");
    }

    #[test]
    fn webhook_registration_columns_match_fixture() {
        assert_table(
            "GitWebhookRegistration",
            git_webhook_registration::TABLE,
            git_webhook_registration::ORDERING,
        );
        assert_columns(
            "GitWebhookRegistration",
            "BaseModel",
            git_webhook_registration::COLUMNS,
        );
        assert_eq!(
            constraint_names(git_webhook_registration::UNIQUE_CONSTRAINTS),
            vec!["git_wh_reg_uniq_repo_acct"]
        );
        assert_eq!(
            git_webhook_registration::default_events(),
            serde_json::Value::Array(Vec::new())
        );
    }

    #[test]
    fn enums_round_trip() {
        assert_eq!(GitProvider::default(), GitProvider::Github);
        assert_eq!(
            "github".parse::<GitProvider>().unwrap(),
            GitProvider::Github
        );
        assert_eq!(
            "gitlab".parse::<GitProvider>().unwrap(),
            GitProvider::Gitlab
        );
        assert!("bitbucket".parse::<GitProvider>().is_err());
        assert_eq!(GitAuthType::default(), GitAuthType::GithubApp);
        assert_eq!(
            "github_app".parse::<GitAuthType>().unwrap(),
            GitAuthType::GithubApp
        );
        assert_eq!("pat".parse::<GitAuthType>().unwrap(), GitAuthType::Pat);
        assert_eq!("oauth".parse::<GitAuthType>().unwrap(), GitAuthType::Oauth);
        assert_eq!(
            "group_token".parse::<GitAuthType>().unwrap(),
            GitAuthType::GroupToken
        );
        assert_eq!(
            "project_token".parse::<GitAuthType>().unwrap(),
            GitAuthType::ProjectToken
        );
        assert!("token".parse::<GitAuthType>().is_err());
        assert_eq!(GitWebhookStatus::default(), GitWebhookStatus::Received);
        assert_eq!(
            "received".parse::<GitWebhookStatus>().unwrap(),
            GitWebhookStatus::Received
        );
        assert_eq!(
            "processed".parse::<GitWebhookStatus>().unwrap(),
            GitWebhookStatus::Processed
        );
        assert_eq!(
            "failed".parse::<GitWebhookStatus>().unwrap(),
            GitWebhookStatus::Failed
        );
        assert_eq!(
            "skipped".parse::<GitWebhookStatus>().unwrap(),
            GitWebhookStatus::Skipped
        );
        assert!("done".parse::<GitWebhookStatus>().is_err());
        assert_eq!(CodeReviewState::default(), CodeReviewState::Open);
        assert_eq!(
            "open".parse::<CodeReviewState>().unwrap(),
            CodeReviewState::Open
        );
        assert_eq!(
            "closed".parse::<CodeReviewState>().unwrap(),
            CodeReviewState::Closed
        );
        assert_eq!(
            "merged".parse::<CodeReviewState>().unwrap(),
            CodeReviewState::Merged
        );
        assert!("draft".parse::<CodeReviewState>().is_err());
    }
}
