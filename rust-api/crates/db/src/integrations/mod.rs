//! Integrations-library table models (D-05, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/integration/git.py:11-344` and
//! `apps/api/pi_dash/db/models/integration/github.py:15-262` for the db
//! layer, adopting the Django-owned schema column-for-column. Migrations
//! are not ported; Django stays schema owner until switchover.
//!
//! * [`git_models`] — the 8 `Git*` tables (PIDASHCONV-144, git half).
//! * [`github_models`] — the 8 `Github*` tables used by `github_sync_task`
//!   (PIDASHCONV-144, github half).
//!
//! The layout mirrors the D-01 models convention (PIDASHCONV-116,
//! `crate::license::models`): one `pub mod` per table with `TABLE`,
//! `ORDERING`, and `COLUMNS` consts in Django `_meta` field order, a row
//! struct, enum ports with stored-string round-trips, and FK / unique /
//! index consts. `#[cfg(test)]` asserts columns field-for-field against
//! `rust-api/fixtures/integrations/models/columns.json`.
//!
//! # Column order and the fixture shape
//!
//! `COLUMNS` holds the full `_meta` order: the audit prefix (`id`,
//! `created_at`, `updated_at`, `created_by_id`, `updated_by_id`,
//! `deleted_at`; see [`AUDIT_COLUMNS`]), then `project_id`,
//! `workspace_id` for `ProjectBaseModel` tables (see [`PROJECT_COLUMNS`];
//! `workspace` is auto-set from `project` on save,
//! `db/models/project.py:302-311`), then the declared fields in source
//! order with Django attnames (`workspace` -> `workspace_id`, ...). The
//! fixture lists only the declared fields (its `_note` covers the
//! inherited audit columns), so each test asserts the post-prefix tail.
//! Order is cosmetic for query building; membership is the contract.
//!
//! # Re-exports
//!
//! These mirror `db/models/integration/__init__.py`, which re-exports all
//! 16 `Git*`/`Github*` models. `SlackProjectSync` (same package, `slack.py`)
//! belongs to another domain and is not ported here.
//!
//! Wiring note: the crate root must declare `pub mod integrations;`
//! (foundation change, filed separately under PIDASHCONV-1 following the
//! PIDASHCONV-132 precedent for `license`); these files are new-files-only
//! for this issue.

pub mod git_models;
pub mod github_models;

pub use git_models::{
    git_code_review_link::GitCodeReviewLink, git_comment_sync::GitCommentSync,
    git_issue_sync::GitIssueSync, git_provider_account::GitProviderAccount,
    git_repository::GitRepository, git_repository_binding::GitRepositoryBinding,
    git_webhook_delivery::GitWebhookDelivery, git_webhook_registration::GitWebhookRegistration,
    CodeReviewState as GitCodeReviewState, GitAuthType, GitProvider, GitWebhookStatus,
};
pub use github_models::{
    github_app_install_session::GithubAppInstallSession,
    github_app_installation::GithubAppInstallation, github_comment_sync::GithubCommentSync,
    github_issue_sync::GithubIssueSync, github_pull_request_link::GithubPullRequestLink,
    github_repository::GithubRepository, github_repository_sync::GithubRepositorySync,
    github_webhook_delivery::GithubWebhookDelivery, GithubAccountType, GithubRepositorySelection,
    GithubSessionStatus, GithubWebhookStatus, PullRequestState,
};

/// Django-level FK delete behavior (ORM-emulated; the live FKs carry no
/// `ON DELETE` action, so Rust write paths must replicate the
/// nulling/cascading/protecting explicitly — same class of quirk as the
/// D-01 `license::models::OnDelete` note).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
    /// `models.PROTECT` — deleting the parent raises instead.
    Protect,
}

/// Audit prefix shared by every model (`BaseModel` + `TimeAuditModel` +
/// `UserAuditModel` + `SoftDeleteModel`; matches the D-01
/// `instance::COLUMNS` prefix order).
pub const AUDIT_COLUMNS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
];

/// `ProjectBaseModel` prefix (`db/models/project.py:302-311`), inserted
/// between [`AUDIT_COLUMNS`] and the declared fields.
pub const PROJECT_COLUMNS: &[&str] = &["project_id", "workspace_id"];

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    /// Parse one fixture `columns` entry into its Django attname: the first
    /// token, plus `_id` for FK / OneToOne fields
    /// (e.g. `"workspace FK Workspace CASCADE"` -> `"workspace_id"`,
    /// `"workspace_integration OneToOne WorkspaceIntegration CASCADE"` ->
    /// `"workspace_integration_id"`, `"provider varchar(32): ..."` ->
    /// `"provider"`).
    pub(crate) fn fixture_attname(entry: &str) -> String {
        let first = entry.split_whitespace().next().unwrap_or_default();
        if entry.contains(" FK ") || entry.contains("OneToOne") {
            format!("{first}_id")
        } else {
            first.to_string()
        }
    }

    pub(crate) fn fixtures_path() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/integrations/models/columns.json")
    }

    pub(crate) fn fixture_model(name: &str) -> serde_json::Value {
        let body = std::fs::read_to_string(fixtures_path())
            .unwrap_or_else(|e| panic!("read columns.json: {e}"));
        let v: serde_json::Value = serde_json::from_str(&body).expect("columns.json is valid JSON");
        let m = v["models"][name].clone();
        assert!(m.is_object(), "fixture has model {name}");
        m
    }

    /// Declared-field attnames from the fixture, in fixture order.
    pub(crate) fn fixture_columns(name: &str) -> Vec<String> {
        fixture_model(name)["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("fixture {name} has columns array"))
            .iter()
            .map(|c| {
                fixture_attname(
                    c.as_str()
                        .unwrap_or_else(|| panic!("fixture {name} column is a string")),
                )
            })
            .collect()
    }

    pub(crate) fn fixture_table(name: &str) -> String {
        fixture_model(name)["db_table"]
            .as_str()
            .unwrap()
            .to_string()
    }

    pub(crate) fn fixture_ordering(name: &str) -> Vec<String> {
        fixture_model(name)["ordering"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_str().unwrap().to_string())
            .collect()
    }

    pub(crate) fn fixture_base(name: &str) -> String {
        fixture_model(name)["base"].as_str().unwrap().to_string()
    }

    /// Copy a column const into an owned vec so assertions compare two
    /// runtime values.
    pub(crate) fn owned_columns(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }
}
