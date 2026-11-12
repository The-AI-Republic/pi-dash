//! Webhook + integration base table models (D-33, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/webhook.py:34-89` (`Webhook`,
//! `WebhookLog`, `ProjectWebhook`) and
//! `apps/api/pi_dash/db/models/integration/base.py:16-60` (`Integration`,
//! `WorkspaceIntegration`), adopting the Django-owned schema
//! column-for-column. Read models / row mappings only; no migrations.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * Every column default below is application-level (Django); like the
//!   D-05 tables, Rust inserts must supply these values explicitly — there
//!   is no DB fallback.
//! * FK `on_delete` (`CASCADE`, `SET_NULL`) is ORM-emulated, so Rust write
//!   paths (later layer issues) must replicate the cascading / nulling
//!   explicitly.
//! * BUG: `WebhookLog.webhook` (`webhook.py:68`) is a plain `UUIDField`,
//!   not a `ForeignKey` — log rows orphan when their webhook is deleted.
//! * BUG: `WebhookLog.response_status` is a `TextField` that stores mixed
//!   types (int status codes, `500`, `str(e)`); see the FX-WEB-03 fixture.
//! * BUG: `WorkspaceIntegration`'s `unique_together`
//!   (`workspace`, `integration`) is soft-delete-unaware — re-adding an
//!   integration after soft-deleting it collides at the DB level.
//! * `Webhook` keeps both the legacy `unique_together`
//!   (`workspace`, `url`, `deleted_at`) and the partial unique
//!   `webhook_url_unique_url_when_deleted_at_null`; the former is
//!   redundant but still declared in `Meta`.
//! * BUG (validators owned by PIDASHCONV-365, noted here only):
//!   `validate_domain` (`webhook.py:27-31`) compares `netloc`
//!   (host:port), so `http://localhost:8000/hook` passes.

use serde::{Deserialize, Serialize};

/// Django-level FK delete behavior (ORM-emulated; same shape as the D-05
/// `integrations::OnDelete` and D-32 `app_intake::models::OnDelete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// `Integration.network` choices (`integration/base.py:20`, ported as-is).
/// Stored as an integer; labels are `Private` / `Public`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum NetworkType {
    /// `1` — private network (the Django default).
    #[default]
    Private,
    /// `2` — public network.
    Public,
}

impl NetworkType {
    /// Default `network` (`integration/base.py:20`).
    pub const DEFAULT: i32 = Self::PRIVATE;

    pub const PRIVATE: i32 = 1;
    pub const PUBLIC: i32 = 2;

    pub const PRIVATE_LABEL: &str = "Private";
    pub const PUBLIC_LABEL: &str = "Public";

    pub fn as_i32(self) -> i32 {
        match self {
            NetworkType::Private => Self::PRIVATE,
            NetworkType::Public => Self::PUBLIC,
        }
    }

    pub fn as_label(self) -> &'static str {
        match self {
            NetworkType::Private => Self::PRIVATE_LABEL,
            NetworkType::Public => Self::PUBLIC_LABEL,
        }
    }

    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            Self::PRIVATE => Some(NetworkType::Private),
            Self::PUBLIC => Some(NetworkType::Public),
            _ => None,
        }
    }
}

/// `secret_key` default prefix (`webhook.py:17-18`,
/// `"pi_dash_wh_" + uuid4().hex`).
pub const SECRET_KEY_PREFIX: &str = "pi_dash_wh_";

/// Port of `generate_token` (`webhook.py:17-18`): prefix plus 32 lowercase
/// hex chars (`uuid4().hex`).
pub fn generate_secret_key() -> String {
    format!("{}{}", SECRET_KEY_PREFIX, uuid::Uuid::new_v4().simple())
}

/// `webhooks` table (`webhook.py:34-62`).
pub mod webhook {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "webhooks";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order. FK columns use the Django
    /// attnames (`workspace_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "url",
        "is_active",
        "secret_key",
        "project",
        "issue",
        "module",
        "cycle",
        "issue_comment",
        "is_internal",
        "version",
    ];

    /// `url` max length (`webhook.py:36`, `URLField(max_length=1024)`).
    pub const URL_MAX_LENGTH: usize = 1024;

    /// `secret_key` max length (`webhook.py:38`).
    pub const SECRET_KEY_MAX_LENGTH: usize = 255;

    /// `version` max length (`webhook.py:46`).
    pub const VERSION_MAX_LENGTH: usize = 50;

    /// Default `is_active` (`webhook.py:37`).
    pub const DEFAULT_IS_ACTIVE: bool = true;

    /// Defaults for the six event flags (`webhook.py:39-44`).
    pub const DEFAULT_EVENT_FLAG: bool = false;

    /// Default `version` (`webhook.py:46`).
    pub const DEFAULT_VERSION: &str = "v1";

    /// `workspace` FK (`webhook.py:35`): `CASCADE`, required,
    /// `related_name="workspace_webhooks"`.
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_NULLABLE: bool = false;
    pub const WORKSPACE_RELATED_NAME: &str = "workspace_webhooks";

    /// Partial unique (`Meta.constraints`, `webhook.py:53-58`):
    /// one active webhook per workspace+url.
    pub const UNIQUE_CONSTRAINTS: &[(&str, &[&str])] = &[(
        "webhook_url_unique_url_when_deleted_at_null",
        &["workspace_id", "url"],
    )];

    /// Legacy `unique_together` (`webhook.py:51`), redundant with the
    /// partial unique above but still declared in `Meta`.
    pub const LEGACY_UNIQUE_TOGETHER: &[&str] = &["workspace_id", "url", "deleted_at"];

    /// One `Webhook` row. `url`, `secret_key`, and `version` are
    /// `NOT NULL` with Django-side defaults (`''`, `generate_token`,
    /// `"v1"`); the flags store plain booleans.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Webhook {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub url: String,
        pub is_active: bool,
        pub secret_key: String,
        pub project: bool,
        pub issue: bool,
        pub module: bool,
        pub cycle: bool,
        pub issue_comment: bool,
        pub is_internal: bool,
        pub version: String,
    }
}

/// `webhook_logs` table (`webhook.py:65-88`).
pub mod webhook_log {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "webhook_logs";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order. `webhook` is a plain UUID
    /// column, NOT a foreign key (`webhook.py:68`) — hence no `_id`
    /// attname remap.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "webhook",
        "event_type",
        "request_method",
        "request_headers",
        "request_body",
        "response_status",
        "response_headers",
        "response_body",
        "retry_count",
    ];

    /// `event_type` max length (`webhook.py:71`).
    pub const EVENT_TYPE_MAX_LENGTH: usize = 255;

    /// `request_method` max length (`webhook.py:72`).
    pub const REQUEST_METHOD_MAX_LENGTH: usize = 10;

    /// Default `retry_count` (`webhook.py:84`).
    pub const DEFAULT_RETRY_COUNT: i16 = 0;

    /// `workspace` FK (`webhook.py:66`): `CASCADE`, required,
    /// `related_name="webhook_logs"`.
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_NULLABLE: bool = false;
    pub const WORKSPACE_RELATED_NAME: &str = "webhook_logs";

    /// `webhook` (`webhook.py:68`) is NOT a FK — no `OnDelete`, no
    /// related name; BUG: rows orphan when the webhook is deleted.
    pub const WEBHOOK_IS_FOREIGN_KEY: bool = false;
    pub const WEBHOOK_NULLABLE: bool = false;

    /// Nullability of the optional request/response columns
    /// (`webhook.py:71-82`, all `blank=True, null=True`).
    pub const EVENT_TYPE_NULLABLE: bool = true;
    pub const REQUEST_METHOD_NULLABLE: bool = true;
    pub const REQUEST_HEADERS_NULLABLE: bool = true;
    pub const REQUEST_BODY_NULLABLE: bool = true;
    pub const RESPONSE_STATUS_NULLABLE: bool = true;
    pub const RESPONSE_HEADERS_NULLABLE: bool = true;
    pub const RESPONSE_BODY_NULLABLE: bool = true;
    pub const RETRY_COUNT_NULLABLE: bool = false;

    /// One `WebhookLog` row. `response_status` stores mixed types (int
    /// status codes, `500`, `str(e)`) as text — BUG, ported as-is.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct WebhookLog {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub webhook: uuid::Uuid,
        pub event_type: Option<String>,
        pub request_method: Option<String>,
        pub request_headers: Option<String>,
        pub request_body: Option<String>,
        pub response_status: Option<String>,
        pub response_headers: Option<String>,
        pub response_body: Option<String>,
        pub retry_count: i16,
    }
}

/// `integrations` table (`integration/base.py:16-38`).
///
/// `Integration` extends `AuditModel` directly (NOT `BaseModel`) and
/// declares its own UUID primary key (`base.py:17`); the resulting column
/// set is the same six-column audit prefix plus the declared fields.
pub mod integration {
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "integrations";

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
        "title",
        "provider",
        "network",
        "description",
        "author",
        "webhook_url",
        "webhook_secret",
        "redirect_url",
        "metadata",
        "verified",
        "avatar_url",
    ];

    /// `title` / `provider` / `author` max length (`base.py:18-19,23`).
    pub const TITLE_MAX_LENGTH: usize = 400;
    pub const PROVIDER_MAX_LENGTH: usize = 400;
    pub const AUTHOR_MAX_LENGTH: usize = 400;

    /// `provider` carries `unique=True` (`base.py:19`); the backing
    /// constraint name is Django-generated, so uniqueness is recorded as
    /// a flag rather than a named constraint.
    pub const PROVIDER_UNIQUE: bool = true;

    /// Default `network` (`base.py:20`); see [`super::NetworkType`].
    pub const DEFAULT_NETWORK: i32 = super::NetworkType::DEFAULT;

    /// Default `verified` (`base.py:28`).
    pub const DEFAULT_VERIFIED: bool = false;

    /// `avatar_url` nullability (`base.py:29`, `blank=True, null=True`).
    pub const AVATAR_URL_NULLABLE: bool = true;

    /// One `Integration` row. `description` and `metadata` are
    /// `jsonb NOT NULL` defaulting to `{}`; `author`, `webhook_url`,
    /// `webhook_secret`, and `redirect_url` are `NOT NULL` defaulting to
    /// `''` (`blank=True` without `null=True`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Integration {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub title: String,
        pub provider: String,
        pub network: i32,
        pub description: serde_json::Value,
        pub author: String,
        pub webhook_url: String,
        pub webhook_secret: String,
        pub redirect_url: String,
        pub metadata: serde_json::Value,
        pub verified: bool,
        pub avatar_url: Option<String>,
    }
}

/// `workspace_integrations` table (`integration/base.py:41-60`).
pub mod workspace_integration {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "workspace_integrations";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order. FK columns use the Django
    /// attnames (`workspace_id`, `actor_id`, `integration_id`,
    /// `api_token_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "actor_id",
        "integration_id",
        "api_token_id",
        "metadata",
        "config",
    ];

    /// `workspace` FK (`base.py:42`): `CASCADE`, required,
    /// `related_name="workspace_integrations"`.
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_NULLABLE: bool = false;
    pub const WORKSPACE_RELATED_NAME: &str = "workspace_integrations";

    /// `actor` FK — the bot user (`base.py:44`): `CASCADE`, required,
    /// `related_name="integrations"`.
    pub const ACTOR_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const ACTOR_NULLABLE: bool = false;
    pub const ACTOR_RELATED_NAME: &str = "integrations";

    /// `integration` FK (`base.py:45`): `CASCADE`, required,
    /// `related_name="integrated_workspaces"`.
    pub const INTEGRATION_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const INTEGRATION_NULLABLE: bool = false;
    pub const INTEGRATION_RELATED_NAME: &str = "integrated_workspaces";

    /// `api_token` FK (`base.py:46`): `CASCADE`, required (the PAT flow
    /// mints an INACTIVE shim token, `app/views/integration/github.py:460-467`),
    /// `related_name="integrations"`.
    pub const API_TOKEN_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const API_TOKEN_NULLABLE: bool = false;
    pub const API_TOKEN_RELATED_NAME: &str = "integrations";

    /// `unique_together = ["workspace", "integration"]` (`base.py:57`):
    /// soft-delete-unaware, so re-adding after a soft delete collides —
    /// BUG, ported as-is.
    pub const UNIQUE_TOGETHER: &[&[&str]] = &[&["workspace_id", "integration_id"]];

    /// One `WorkspaceIntegration` row. `metadata` and `config` are
    /// `jsonb NOT NULL` defaulting to `{}`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct WorkspaceIntegration {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub actor_id: uuid::Uuid,
        pub integration_id: uuid::Uuid,
        pub api_token_id: uuid::Uuid,
        pub metadata: serde_json::Value,
        pub config: serde_json::Value,
    }
}

/// `project_webhooks` table (`webhook.py:91+`).
///
/// Topological closure of `webhook.py`: no sub-issue owns this model (it
/// is absent from FX-MDL-01's 15 models), so it is ported here rather than
/// left without an owner.
pub mod project_webhook {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "project_webhooks";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order: the audit prefix, then the
    /// `ProjectBaseModel` prefix (`project_id`, `workspace_id`), then the
    /// declared `webhook` field.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "webhook_id",
    ];

    /// `webhook` FK: `CASCADE`, required,
    /// `related_name="project_webhooks"`.
    pub const WEBHOOK_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WEBHOOK_NULLABLE: bool = false;
    pub const WEBHOOK_RELATED_NAME: &str = "project_webhooks";

    /// Partial unique (`Meta.constraints`): one active link per
    /// project+webhook.
    pub const UNIQUE_CONSTRAINTS: &[(&str, &[&str])] = &[(
        "project_webhook_unique_project_webhook_when_deleted_at_null",
        &["project_id", "webhook_id"],
    )];

    /// Legacy `unique_together` (`project`, `webhook`, `deleted_at`),
    /// redundant with the partial unique above but still declared.
    pub const LEGACY_UNIQUE_TOGETHER: &[&str] = &["project_id", "webhook_id", "deleted_at"];

    /// One `ProjectWebhook` row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ProjectWebhook {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub webhook_id: uuid::Uuid,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support as ts;
    use super::super::{AUDIT_COLUMNS, PROJECT_COLUMNS};
    use super::*;

    /// `COLUMNS` must equal the FX-MDL-01 fixture entries expanded to
    /// attnames, field-for-field.
    fn assert_columns(fixture: &str, columns: &[&str]) {
        assert_eq!(
            ts::owned_columns(columns),
            ts::fixture_columns(fixture),
            "{fixture} columns"
        );
    }

    fn assert_table(fixture: &str, table: &str, ordering: &str) {
        assert_eq!(ts::fixture_table(fixture), table);
        assert_eq!(ordering, "-created_at");
    }

    fn constraint_names(constraints: &[(&str, &[&str])]) -> Vec<String> {
        constraints.iter().map(|(n, _)| (*n).to_string()).collect()
    }

    #[test]
    fn audit_prefix_matches_d05_convention() {
        assert_eq!(
            ts::owned_columns(AUDIT_COLUMNS),
            vec![
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
            ]
        );
        assert_eq!(
            ts::owned_columns(PROJECT_COLUMNS),
            vec!["project_id", "workspace_id"]
        );
    }

    #[test]
    fn webhook_columns_match_fixture() {
        assert_table("Webhook", webhook::TABLE, webhook::ORDERING);
        assert_eq!(webhook::TABLE, "webhooks");
        assert_columns("Webhook", webhook::COLUMNS);
        assert_eq!(webhook::URL_MAX_LENGTH, 1024);
        assert_eq!(webhook::SECRET_KEY_MAX_LENGTH, 255);
        assert_eq!(webhook::VERSION_MAX_LENGTH, 50);
        let is_active: bool = webhook::DEFAULT_IS_ACTIVE;
        assert!(is_active);
        let event_flag: bool = webhook::DEFAULT_EVENT_FLAG;
        assert!(!event_flag);
        assert_eq!(webhook::DEFAULT_VERSION, "v1");
        assert_eq!(webhook::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        let workspace_nullable: bool = webhook::WORKSPACE_NULLABLE;
        assert!(!workspace_nullable);
        assert_eq!(webhook::WORKSPACE_RELATED_NAME, "workspace_webhooks");
        // Python-derived (named in the fixture's `unique_active` entry):
        // one active webhook per workspace+url.
        assert_eq!(
            constraint_names(webhook::UNIQUE_CONSTRAINTS),
            vec!["webhook_url_unique_url_when_deleted_at_null"]
        );
        assert_eq!(webhook::UNIQUE_CONSTRAINTS[0].1, &["workspace_id", "url"]);
        assert_eq!(
            ts::owned_columns(webhook::LEGACY_UNIQUE_TOGETHER),
            vec!["workspace_id", "url", "deleted_at"]
        );
    }

    #[test]
    fn webhook_log_columns_match_fixture() {
        assert_table("WebhookLog", webhook_log::TABLE, webhook_log::ORDERING);
        assert_eq!(webhook_log::TABLE, "webhook_logs");
        assert_columns("WebhookLog", webhook_log::COLUMNS);
        assert_eq!(webhook_log::EVENT_TYPE_MAX_LENGTH, 255);
        assert_eq!(webhook_log::REQUEST_METHOD_MAX_LENGTH, 10);
        assert_eq!(webhook_log::DEFAULT_RETRY_COUNT, 0);
        assert_eq!(webhook_log::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        let workspace_nullable: bool = webhook_log::WORKSPACE_NULLABLE;
        assert!(!workspace_nullable);
        assert_eq!(webhook_log::WORKSPACE_RELATED_NAME, "webhook_logs");
        // BUG ported as-is: plain UUID column, not a FK.
        let webhook_is_fk: bool = webhook_log::WEBHOOK_IS_FOREIGN_KEY;
        assert!(!webhook_is_fk);
        let webhook_nullable: bool = webhook_log::WEBHOOK_NULLABLE;
        assert!(!webhook_nullable);
        let nullable: bool = webhook_log::EVENT_TYPE_NULLABLE;
        assert!(nullable);
        let nullable: bool = webhook_log::REQUEST_METHOD_NULLABLE;
        assert!(nullable);
        let nullable: bool = webhook_log::REQUEST_HEADERS_NULLABLE;
        assert!(nullable);
        let nullable: bool = webhook_log::REQUEST_BODY_NULLABLE;
        assert!(nullable);
        let nullable: bool = webhook_log::RESPONSE_STATUS_NULLABLE;
        assert!(nullable);
        let nullable: bool = webhook_log::RESPONSE_HEADERS_NULLABLE;
        assert!(nullable);
        let nullable: bool = webhook_log::RESPONSE_BODY_NULLABLE;
        assert!(nullable);
        let retry_nullable: bool = webhook_log::RETRY_COUNT_NULLABLE;
        assert!(!retry_nullable);
    }

    #[test]
    fn integration_columns_match_fixture() {
        assert_table("Integration", integration::TABLE, integration::ORDERING);
        assert_eq!(integration::TABLE, "integrations");
        // The fixture lists `Integration` descriptively (`id`, declared
        // fields, audit tail last) while this module follows the
        // crate-wide id-first audit-prefix convention; membership is the
        // contract (order is cosmetic for query building), so this one
        // table compares unordered. Every other table compares ordered.
        let mut actual = ts::owned_columns(integration::COLUMNS);
        actual.sort();
        let mut expected = ts::fixture_columns("Integration");
        expected.sort();
        assert_eq!(actual, expected, "Integration columns");
        assert_eq!(&integration::COLUMNS[..AUDIT_COLUMNS.len()], AUDIT_COLUMNS);
        assert_eq!(integration::TITLE_MAX_LENGTH, 400);
        assert_eq!(integration::PROVIDER_MAX_LENGTH, 400);
        assert_eq!(integration::AUTHOR_MAX_LENGTH, 400);
        let provider_unique: bool = integration::PROVIDER_UNIQUE;
        assert!(provider_unique);
        assert_eq!(integration::DEFAULT_NETWORK, NetworkType::PRIVATE);
        assert_eq!(integration::DEFAULT_NETWORK, NetworkType::DEFAULT);
        let verified: bool = integration::DEFAULT_VERIFIED;
        assert!(!verified);
        let avatar_nullable: bool = integration::AVATAR_URL_NULLABLE;
        assert!(avatar_nullable);
    }

    #[test]
    fn workspace_integration_columns_match_fixture() {
        assert_table(
            "WorkspaceIntegration",
            workspace_integration::TABLE,
            workspace_integration::ORDERING,
        );
        assert_eq!(workspace_integration::TABLE, "workspace_integrations");
        assert_columns("WorkspaceIntegration", workspace_integration::COLUMNS);
        assert_eq!(
            workspace_integration::WORKSPACE_ON_DELETE,
            OnDelete::Cascade
        );
        let workspace_nullable: bool = workspace_integration::WORKSPACE_NULLABLE;
        assert!(!workspace_nullable);
        assert_eq!(
            workspace_integration::WORKSPACE_RELATED_NAME,
            "workspace_integrations"
        );
        assert_eq!(workspace_integration::ACTOR_ON_DELETE, OnDelete::Cascade);
        let actor_nullable: bool = workspace_integration::ACTOR_NULLABLE;
        assert!(!actor_nullable);
        assert_eq!(workspace_integration::ACTOR_RELATED_NAME, "integrations");
        assert_eq!(
            workspace_integration::INTEGRATION_ON_DELETE,
            OnDelete::Cascade
        );
        let integration_nullable: bool = workspace_integration::INTEGRATION_NULLABLE;
        assert!(!integration_nullable);
        assert_eq!(
            workspace_integration::INTEGRATION_RELATED_NAME,
            "integrated_workspaces"
        );
        assert_eq!(
            workspace_integration::API_TOKEN_ON_DELETE,
            OnDelete::Cascade
        );
        let api_token_nullable: bool = workspace_integration::API_TOKEN_NULLABLE;
        assert!(!api_token_nullable);
        assert_eq!(
            workspace_integration::API_TOKEN_RELATED_NAME,
            "integrations"
        );
        // Python-derived (`base.py:57`; fixture records it as
        // `unique_together: workspace+integration`).
        assert_eq!(
            workspace_integration::UNIQUE_TOGETHER,
            &[&["workspace_id", "integration_id"]]
        );
    }

    #[test]
    fn project_webhook_matches_python() {
        // No FX-MDL-01 entry (topological closure of `webhook.py`); assert
        // the Python-derived values directly.
        assert_eq!(project_webhook::TABLE, "project_webhooks");
        assert_eq!(project_webhook::ORDERING, "-created_at");
        let mut expected = ts::owned_columns(AUDIT_COLUMNS);
        expected.extend(ts::owned_columns(PROJECT_COLUMNS));
        expected.push("webhook_id".to_string());
        assert_eq!(ts::owned_columns(project_webhook::COLUMNS), expected);
        assert_eq!(project_webhook::WEBHOOK_ON_DELETE, OnDelete::Cascade);
        let webhook_nullable: bool = project_webhook::WEBHOOK_NULLABLE;
        assert!(!webhook_nullable);
        assert_eq!(project_webhook::WEBHOOK_RELATED_NAME, "project_webhooks");
        assert_eq!(
            constraint_names(project_webhook::UNIQUE_CONSTRAINTS),
            vec!["project_webhook_unique_project_webhook_when_deleted_at_null"]
        );
        assert_eq!(
            project_webhook::UNIQUE_CONSTRAINTS[0].1,
            &["project_id", "webhook_id"]
        );
        assert_eq!(
            ts::owned_columns(project_webhook::LEGACY_UNIQUE_TOGETHER),
            vec!["project_id", "webhook_id", "deleted_at"]
        );
    }

    #[test]
    fn network_type_round_trip() {
        assert_eq!(NetworkType::default(), NetworkType::Private);
        assert_eq!(NetworkType::Private.as_i32(), 1);
        assert_eq!(NetworkType::Public.as_i32(), 2);
        assert_eq!(NetworkType::Private.as_label(), "Private");
        assert_eq!(NetworkType::Public.as_label(), "Public");
        assert_eq!(NetworkType::from_i32(1), Some(NetworkType::Private));
        assert_eq!(NetworkType::from_i32(2), Some(NetworkType::Public));
        assert_eq!(NetworkType::from_i32(0), None);
        assert_eq!(NetworkType::from_i32(3), None);
    }

    #[test]
    fn secret_key_has_generate_token_shape() {
        // Port of `generate_token` (`webhook.py:17-18`): prefix plus 32
        // lowercase hex chars.
        let key = generate_secret_key();
        assert!(key.starts_with(SECRET_KEY_PREFIX), "{key}");
        let hex = &key[SECRET_KEY_PREFIX.len()..];
        assert_eq!(hex.len(), 32, "{key}");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()), "{key}");
        assert_eq!(key.len(), SECRET_KEY_PREFIX.len() + 32);
    }

    #[test]
    fn row_structs_round_trip_json() {
        let now = chrono::Utc::now();
        let id = uuid::Uuid::new_v4();
        let row = webhook::Webhook {
            id,
            created_at: now,
            updated_at: now,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: uuid::Uuid::new_v4(),
            url: "https://example.com/hook".to_string(),
            is_active: true,
            secret_key: generate_secret_key(),
            project: false,
            issue: true,
            module: false,
            cycle: false,
            issue_comment: false,
            is_internal: false,
            version: "v1".to_string(),
        };
        let back: webhook::Webhook =
            serde_json::from_value(serde_json::to_value(&row).unwrap()).unwrap();
        assert_eq!(row, back);
    }
}
