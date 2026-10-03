//! Issue link/relation/PR/review/attachment table models (D-26, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/issue.py:372-421`
//! (`IssueRelationChoices`, `IssueRelation`) and `:471-485` (`IssueLink`),
//! `db/models/integration/github.py:217-262` (`GithubPullRequestLink`),
//! `db/models/integration/git.py:224-272` (`GitCodeReviewLink`) and
//! `db/models/asset.py:28-100` (`FileAsset`), adopting the Django-owned
//! schema column-for-column; migrations are not ported — Django stays
//! schema owner until switchover. Fixture source of truth:
//! `rust-api/fixtures/app_issues/models/FX-ISS-09.links.json` (recorded by
//! PIDASHCONV-637); the `#[cfg(test)]` suite replays it section by
//! section.
//!
//! Column order in each `*_COLUMNS` const follows the fixture: the 8
//! inherited audit/project columns first (`id`, `created_at`,
//! `updated_at`, `created_by_id`, `updated_by_id`, `deleted_at`, then
//! `project_id`, `workspace_id` from `ProjectBaseModel` at
//! `db/models/project.py:302-311`), then the model fields in declaration
//! order. FK entries use the Django attnames (`project_id`, `issue_id`,
//! `related_issue_id`, …). Every application-level default below is
//! Django-side (the live tables carry no `column_default` in
//! `information_schema`, as established for D-01); Rust inserts must
//! supply these values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! All five tables inherit the soft-delete marker (`deleted_at`, from
//! `SoftDeleteModel` in `pi_dash/db/mixins.py:61-67`) and the default
//! manager filters `deleted_at IS NULL` (`objects = SoftDeletionManager`,
//! `mixins.py:66`; `all_objects` is the plain unscoped manager,
//! `mixins.py:67`). Every read built from these tables must apply
//! [`crate::soft_delete::active_condition`]; the tests pin this by
//! rendering a scoped `SELECT` per table.
//!
//! # Writes backfill the workspace
//!
//! `ProjectBaseModel.save()` (`db/models/project.py:309-311`) sets
//! `workspace` from `project.workspace` on every save: Rust inserts and
//! updates of `IssueLink`, `IssueRelation`, `GithubPullRequestLink` and
//! `GitCodeReviewLink` must resolve `workspace_id` from the `project_id`
//! row explicitly. `FileAsset` extends `BaseModel` instead, whose `save()`
//! only stamps the audit users (`db/models/base.py:23-44`) — no backfill.
//!
//! # No `delete()` override
//!
//! None of the five models defines `delete()`; deletion is
//! `SoftDeleteModel.delete` (`mixins.py:72-78`), which stamps
//! `deleted_at` and calls `save()` — so destroy DOES emit
//! `pre_save`/`post_save`. There is no code to port here (shared kernel
//! behavior); the write layers must not assume "no signal on destroy".
//!
//! # Overlap with merged defs (deliberate local ports)
//!
//! Four of the five tables also appear in other domains' merged db defs:
//! `space::columns::{issue_link, issue_relation}` (D-02, columns only),
//! `integrations::github_models::github_pull_request_link` and
//! `integrations::git_models::git_code_review_link` (D-05, columns +
//! row structs, but no `__str__` ports, no `Meta` names, no max-length,
//! choice or audit-FK consts). This module still ports all four units in
//! full: D-26 owns the relations/links/PR/review endpoints (PIDASHCONV-653)
//! and their querysets (PIDASHCONV-648/650), and the fixture records these
//! four as first-class units. Cross-domain column duplication is
//! established practice (`v1_assets` and `app_assets` carry identical
//! `file_assets` `COLUMNS`); each domain's models layer is the contract
//! its own queries layer builds against.
//!
//! `FileAsset` is the exception: D-31 (`crate::app_assets::columns`) and
//! D-21 (`crate::v1_assets::model::file_asset`) both ported it in full,
//! so [`file_asset`] reuses those defs and defines only what is missing:
//!
//! | Item | Merged def |
//! | --- | --- |
//! | `TABLE`, `ORDERING`, `COLUMNS`, `DEFAULTS`, `INDEXES`, `ENTITY_TYPES` | both (`app_assets::columns`, `v1_assets::model::file_asset`) |
//! | `ASSET_MAX_LENGTH`, `VARCHAR_MAX_LENGTH`, FK `*_ON_DELETE` consts, row struct, `Display` | `v1_assets::model::file_asset` |
//! | `upload_path_key`, `validate_file_size`, `asset_url`, `is_static_asset_type` | `app_assets::columns` |
//! | `VERBOSE_NAME`, `VERBOSE_NAME_PLURAL` | missing — defined in [`file_asset`] |
//!
//! # Foreign reads: pinned literals
//!
//! The columns D-26 reads on foreign tables (fixture `foreign_reads`)
//! are pinned as SQL literals in [`foreign`]; query builders copy those
//! literals verbatim and the tests assert they equal the fixture. They
//! are deliberately NOT reused from `integrations::*`, even though that
//! domain merged the same tables: the fixture instructs "SQL literal
//! pinned here" for all five, this issue carries no cross-domain
//! dependency, and two entries are JSON paths (`metadata.…`) plus a
//! traversal note that a plain `COLUMNS` reuse would not capture.
//!
//! # Ported quirks (translate as-is)
//!
//! * `IssueLink` has no uniqueness at all — dupes are prevented only by
//!   the write serializer's create/update checks, not the DB. Ported as
//!   [`issue_link::HAS_PARTIAL_UNIQUE`] `= false`.
//! * `IssueRelation.relation_type` declares no `choices=`
//!   (`issue.py:399-403`): the database accepts any string up to 20
//!   chars and the six `IssueRelationChoices` values are enforced only
//!   in view code. The field stays an unconstrained `String`; the
//!   choices and the reverse map are validation/display data. Note the
//!   reverse-only values (`blocking`, `start_after`, `finish_after`,
//!   `implements`) are NOT valid forward values.
//! * `repo_owner`/`repo_name` are lowercased at attach time (queries
//!   layer, `utils/github_pr_links.py`); the model stores whatever it is
//!   given. No normalization here.
//! * The review link's two active-unique constraints split on
//!   `repo_external_id` empty vs non-empty (`git.py:254-265`): rows with
//!   an id dedupe on `(provider, host_url, repo_external_id,
//!   external_iid)`, rows without on `(provider, host_url, namespace,
//!   repo_name, external_iid)`. Ported as two `NAME`/`COLUMNS`/`WHERE`
//!   triples.
//! * Review-link `state` has three values (`open`/`closed`/`merged`)
//!   while PR-link `state` has two (`open`/`closed`). Ported as-is.
//! * `DRAFT_ISSUE_ATTACHMENT` has no `asset_url` branch (`asset.py:79-100`
//!   enumerates 9 of the 10 `EntityTypeContext` values): the property
//!   returns `None`. Covered by the merged defs; noted here so the
//!   queries layer does not "fix" it.

use super::models_core::OnDelete;

/// `issue_links` table (`issue.py:471-485`).
pub mod issue_link {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `issue.py:480`).
    pub const TABLE: &str = "issue_links";
    /// Default ordering (`Meta.ordering`, `issue.py:481`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`issue.py:478`).
    pub const VERBOSE_NAME: &str = "Issue Link";
    /// `verbose_name_plural` (`issue.py:479`).
    pub const VERBOSE_NAME_PLURAL: &str = "Issue Links";

    /// Columns in fixture FX-ISS-09 order: 8 inherited audit/project
    /// columns, then `title` (`issue.py:472`), `url` (`:473`), `issue_id`
    /// (`:474`) and `metadata` (`:475`). FK columns use the Django
    /// attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "title",
        "url",
        "issue_id",
        "metadata",
    ];

    /// `title` bound (`issue.py:472`, `CharField(max_length=255, null=True,
    /// blank=True)`).
    pub const TITLE_MAX_LENGTH: usize = 255;

    /// No `unique_together`, no partial-unique constraint
    /// (`issue.py:477-481` declares `Meta` without either): duplicate
    /// (issue, url) rows are prevented only by the write serializer's
    /// create/update checks. Ported as-is — a future "fix" adding a
    /// constraint would change write semantics.
    pub const HAS_PARTIAL_UNIQUE: bool = false;

    /// `issue` FK: `CASCADE` (`issue.py:474`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `issue` FK `related_name` (`issue.py:474`).
    pub const ISSUE_RELATED_NAME: &str = "issue_link";
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One issue-link row. `title` is nullable (`null=True`); `url` is a
    /// required `TextField` (no max length); `metadata` stores `{}`, never
    /// `NULL` (`JSONField(default=dict)`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IssueLink {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub title: Option<String>,
        pub url: String,
        pub issue_id: uuid::Uuid,
        pub metadata: serde_json::Value,
    }

    impl IssueLink {
        /// `__str__` (`issue.py:483-484`): `"{issue.name} {url}"`. Rust
        /// holds only the issue FK, so the label takes the joined issue
        /// name plus the owned url; the join itself is owned by the
        /// queries layer.
        pub fn label(issue_name: &str, url: &str) -> String {
            format!("{issue_name} {url}")
        }
    }
}

/// `issue_relations` table (`issue.py:372-421`).
pub mod issue_relation {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `issue.py:416`).
    pub const TABLE: &str = "issue_relations";
    /// Default ordering (`Meta.ordering`, `issue.py:417`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`issue.py:414`).
    pub const VERBOSE_NAME: &str = "Issue Relation";
    /// `verbose_name_plural` (`issue.py:415`).
    pub const VERBOSE_NAME_PLURAL: &str = "Issue Relations";

    /// Columns in fixture FX-ISS-09 order: 8 inherited audit/project
    /// columns, then `issue_id` (`issue.py:397`), `related_issue_id`
    /// (`:398`) and `relation_type` (`:399-403`). FK columns use the
    /// Django attnames.
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
        "related_issue_id",
        "relation_type",
    ];

    /// `relation_type` bound (`issue.py:400`, `CharField(max_length=20)`).
    pub const RELATION_TYPE_MAX_LENGTH: usize = 20;
    /// `relation_type` default (`issue.py:402`,
    /// `IssueRelationChoices.BLOCKED_BY`).
    pub const RELATION_TYPE_DEFAULT: &str = "blocked_by";

    /// `IssueRelationChoices` values + labels in declaration order
    /// (`issue.py:373-378`). The model field passes no `choices=`
    /// (`:399-403`), so these are validation/display data only — the
    /// database accepts any string up to [`RELATION_TYPE_MAX_LENGTH`].
    pub const RELATION_CHOICES: &[(&str, &str)] = &[
        ("duplicate", "Duplicate"),
        ("relates_to", "Relates To"),
        ("blocked_by", "Blocked By"),
        ("start_before", "Start Before"),
        ("finish_before", "Finish Before"),
        ("implemented_by", "Implemented By"),
    ];

    /// Bidirectional relation pairs in source order
    /// (`issue.py:383-390`). `relates_to` and `duplicate` are symmetric;
    /// the other four map to reverse-only values that are NOT valid
    /// forward `relation_type`s.
    pub const RELATION_PAIRS: &[(&str, &str)] = &[
        ("blocked_by", "blocking"),
        ("relates_to", "relates_to"),
        ("duplicate", "duplicate"),
        ("start_before", "start_after"),
        ("finish_before", "finish_after"),
        ("implemented_by", "implements"),
    ];

    /// Reverse lookup over [`RELATION_PAIRS`], mirroring
    /// `IssueRelationChoices._REVERSE_MAPPING` (`issue.py:393`): `None`
    /// for unknown input (Python raises `KeyError`; the caller owns the
    /// error shape).
    pub fn reverse_of(relation_type: &str) -> Option<&str> {
        RELATION_PAIRS
            .iter()
            .find(|(forward, _)| *forward == relation_type)
            .map(|(_, reverse)| *reverse)
    }

    /// `Meta.unique_together` (`issue.py:406`).
    pub const UNIQUE_TOGETHER: &[&str] = &["issue", "related_issue", "deleted_at"];
    /// Active-unique constraint name (`issue.py:411`).
    pub const UNIQUE_ISSUE_RELATED_NAME: &str =
        "issue_relation_unique_issue_related_issue_when_deleted_at_null";
    /// Active-unique constraint columns (`issue.py:409`), as attnames.
    pub const UNIQUE_ISSUE_RELATED_COLUMNS: &[&str] = &["issue_id", "related_issue_id"];
    /// Active-unique constraint condition (`issue.py:410`,
    /// `Q(deleted_at__isnull=True)`), unquoted semantic form.
    pub const UNIQUE_ISSUE_RELATED_WHERE: &str = "deleted_at IS NULL";

    /// `issue` FK: `CASCADE` (`issue.py:397`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `issue` FK `related_name` (`issue.py:397`).
    pub const ISSUE_RELATED_NAME: &str = "issue_relation";
    /// `related_issue` FK: `CASCADE` (`issue.py:398`).
    pub const RELATED_ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `related_issue` FK `related_name` (`issue.py:398`).
    pub const RELATED_ISSUE_RELATED_NAME: &str = "issue_related";
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One issue-relation row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IssueRelation {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
        pub related_issue_id: uuid::Uuid,
        pub relation_type: String,
    }

    impl IssueRelation {
        /// `__str__` (`issue.py:419-420`):
        /// `"{issue.name} {related_issue.name}"`. Rust holds only the
        /// FKs, so the label takes both joined names; the joins are owned
        /// by the queries layer.
        pub fn label(issue_name: &str, related_issue_name: &str) -> String {
            format!("{issue_name} {related_issue_name}")
        }
    }
}

/// `github_pull_request_links` table (`integration/github.py:217-262`).
///
/// Standalone (not tied to `GithubIssueSync`/`GithubRepository`) so that
/// issues with no mirrored GitHub issue can still reference PRs
/// (`github.py:218-226`). The `title`/`state`/`merged`/`draft` snapshot
/// is display-only, refreshed by the GitHub App `pull_request` webhook;
/// it never drives the linked issue's workflow state.
pub mod github_pull_request_link {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `github.py:250`).
    pub const TABLE: &str = "github_pull_request_links";
    /// Default ordering (`Meta.ordering`, `github.py:251`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`github.py:248`).
    pub const VERBOSE_NAME: &str = "Github Pull Request Link";
    /// `verbose_name_plural` (`github.py:249`).
    pub const VERBOSE_NAME_PLURAL: &str = "Github Pull Request Links";

    /// Columns in fixture FX-ISS-09 order: 8 inherited audit/project
    /// columns, then `issue_id` (`github.py:232`), `repo_owner` (`:231`),
    /// `repo_name` (`:232`), `pr_number` (`:233`), `url` (`:234`),
    /// `title` (`:236`), `state` (`:237`), `merged` (`:238`), `draft`
    /// (`:239`) and `pr_updated_at` (`:242`). FK columns use the Django
    /// attnames.
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

    /// `repo_owner` bound (`github.py:233`, `CharField(max_length=255)`).
    pub const REPO_OWNER_MAX_LENGTH: usize = 255;
    /// `repo_name` bound (`github.py:234`, `CharField(max_length=255)`).
    pub const REPO_NAME_MAX_LENGTH: usize = 255;
    /// `pr_number` floor (`github.py:235`, `PositiveIntegerField`, whose
    /// implicit validator is `MinValueValidator(0)`).
    pub const PR_NUMBER_MIN: i32 = 0;
    /// `url` bound (`github.py:236`, `URLField(max_length=500)`).
    pub const URL_MAX_LENGTH: usize = 500;
    /// `title` bound (`github.py:238`, `CharField(max_length=500)`).
    pub const TITLE_MAX_LENGTH: usize = 500;
    /// `title` default (`github.py:238`, `blank=True, default=""`).
    pub const TITLE_DEFAULT: &str = "";
    /// `state` bound (`github.py:239`, `CharField(max_length=12)`).
    pub const STATE_MAX_LENGTH: usize = 12;
    /// `state` default (`github.py:239`, `State.OPEN`).
    pub const STATE_DEFAULT: &str = "open";
    /// `State` values + labels in declaration order
    /// (`github.py:228-230`).
    pub const STATE_CHOICES: &[(&str, &str)] = &[("open", "Open"), ("closed", "Closed")];
    /// `merged` default (`github.py:240`).
    pub const MERGED_DEFAULT: bool = false;
    /// `draft` default (`github.py:241`).
    pub const DRAFT_DEFAULT: bool = false;

    /// Active-unique constraint name (`github.py:256`): one issue per PR
    /// while active (the `IntegrityError` race in attach is guarded by
    /// this constraint, `utils/github_pr_links.py:165-174`).
    pub const UNIQUE_PR_NAME: &str = "github_pr_link_unique_per_pr_when_active";
    /// Active-unique constraint columns (`github.py:254`), as attnames.
    pub const UNIQUE_PR_COLUMNS: &[&str] = &["repo_owner", "repo_name", "pr_number"];
    /// Active-unique constraint condition (`github.py:255`,
    /// `Q(deleted_at__isnull=True)`), unquoted semantic form.
    pub const UNIQUE_PR_WHERE: &str = "deleted_at IS NULL";

    /// `(repo_owner, repo_name, pr_number)` index name (`github.py:260`).
    pub const INDEX_PR_NAME: &str = "github_pr_l_repo_ow_idx";
    /// `(repo_owner, repo_name, pr_number)` index columns
    /// (`github.py:260`).
    pub const INDEX_PR_COLUMNS: &[&str] = &["repo_owner", "repo_name", "pr_number"];
    /// `issue` index name (`github.py:261`).
    pub const INDEX_ISSUE_NAME: &str = "github_pr_l_issue_idx";
    /// `issue` index columns (`github.py:261`), as attnames.
    pub const INDEX_ISSUE_COLUMNS: &[&str] = &["issue_id"];

    /// `issue` FK: `CASCADE` (`github.py:232`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `issue` FK `related_name` (`github.py:232`).
    pub const ISSUE_RELATED_NAME: &str = "github_pull_requests";
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One pull-request-link row. `pr_number` is a Postgres integer
    /// (`PositiveIntegerField`); `title`/`state`/`merged`/`draft` are the
    /// webhook-refreshed display snapshot, never `NULL`.
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

    impl std::fmt::Display for GithubPullRequestLink {
        /// `__str__` (`github.py:244-245`):
        /// `"{repo_owner}/{repo_name}#{pr_number} <{issue_id}>"` — every
        /// part is owned by the row, so a full `Display` (not a label
        /// helper) ports it exactly.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{}/{}#{} <{}>",
                self.repo_owner, self.repo_name, self.pr_number, self.issue_id
            )
        }
    }
}

/// `git_code_review_links` table (`integration/git.py:224-272`).
///
/// The provider-agnostic sibling of [`github_pull_request_link`]: same
/// display-snapshot shape (`title`/`state`/`merged`/`draft`, refreshed
/// externally, never driving issue workflow), but keyed by provider +
/// repo identity + external iid instead of owner/name/number.
pub mod git_code_review_link {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `git.py:250`).
    pub const TABLE: &str = "git_code_review_links";
    /// Default ordering (`Meta.ordering`, `git.py:253`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`git.py:251`).
    pub const VERBOSE_NAME: &str = "Git Code Review Link";
    /// `verbose_name_plural` (`git.py:252`).
    pub const VERBOSE_NAME_PLURAL: &str = "Git Code Review Links";

    /// Columns in fixture FX-ISS-09 order: 8 inherited audit/project
    /// columns, then `issue_id` (`git.py:230`), `provider` (`:232`),
    /// `host_url` (`:233`), `namespace` (`:234`), `repo_name` (`:235`),
    /// `repo_external_id` (`:236`), `external_id` (`:237`),
    /// `external_iid` (`:238`), `url` (`:239`), `title` (`:240`),
    /// `state` (`:241`), `merged` (`:242`), `draft` (`:243`),
    /// `remote_updated_at` (`:244`) and `metadata` (`:245`). FK columns
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

    /// `provider` bound (`git.py:231`, `CharField(max_length=32)`).
    pub const PROVIDER_MAX_LENGTH: usize = 32;
    /// `host_url` bound (`git.py:232`, `URLField(max_length=500)`).
    pub const HOST_URL_MAX_LENGTH: usize = 500;
    /// `namespace` bound (`git.py:233`, `CharField(max_length=500)`).
    pub const NAMESPACE_MAX_LENGTH: usize = 500;
    /// `repo_name` bound (`git.py:234`, `CharField(max_length=500)`).
    pub const REPO_NAME_MAX_LENGTH: usize = 500;
    /// `repo_external_id` bound (`git.py:235`,
    /// `CharField(max_length=255)`).
    pub const REPO_EXTERNAL_ID_MAX_LENGTH: usize = 255;
    /// `repo_external_id` default (`git.py:235`, `blank=True, default=""`).
    pub const REPO_EXTERNAL_ID_DEFAULT: &str = "";
    /// `external_id` bound (`git.py:236`, `CharField(max_length=255)`).
    pub const EXTERNAL_ID_MAX_LENGTH: usize = 255;
    /// `external_id` default (`git.py:236`, `blank=True, default=""`).
    pub const EXTERNAL_ID_DEFAULT: &str = "";
    /// `external_iid` bound (`git.py:237`, `CharField(max_length=255)`,
    /// required).
    pub const EXTERNAL_IID_MAX_LENGTH: usize = 255;
    /// `url` bound (`git.py:238`, `URLField(max_length=1000)`).
    pub const URL_MAX_LENGTH: usize = 1000;
    /// `title` bound (`git.py:239`, `CharField(max_length=500)`).
    pub const TITLE_MAX_LENGTH: usize = 500;
    /// `title` default (`git.py:239`, `blank=True, default=""`).
    pub const TITLE_DEFAULT: &str = "";
    /// `state` bound (`git.py:240`, `CharField(max_length=16)`).
    pub const STATE_MAX_LENGTH: usize = 16;
    /// `state` default (`git.py:240`, `State.OPEN`).
    pub const STATE_DEFAULT: &str = "open";
    /// `State` values + labels in declaration order (`git.py:225-228`).
    /// Three values here vs two on the PR link — ported as-is.
    pub const STATE_CHOICES: &[(&str, &str)] =
        &[("open", "Open"), ("closed", "Closed"), ("merged", "Merged")];
    /// `merged` default (`git.py:241`).
    pub const MERGED_DEFAULT: bool = false;
    /// `draft` default (`git.py:242`).
    pub const DRAFT_DEFAULT: bool = false;

    /// Active-unique constraint name for rows WITH a repo id
    /// (`git.py:258`).
    pub const UNIQUE_REPO_NAME: &str = "git_cr_uniq_repo_iid_active";
    /// Its columns (`git.py:256`), as attnames.
    pub const UNIQUE_REPO_COLUMNS: &[&str] =
        &["provider", "host_url", "repo_external_id", "external_iid"];
    /// Its condition (`git.py:257`, `Q(deleted_at__isnull=True) &
    /// ~Q(repo_external_id="")`), unquoted semantic form.
    pub const UNIQUE_REPO_WHERE: &str = "deleted_at IS NULL AND repo_external_id <> ''";

    /// Active-unique constraint name for rows WITHOUT a repo id
    /// (`git.py:263`): identity falls back to the namespace/repo path.
    pub const UNIQUE_PATH_NAME: &str = "git_cr_uniq_path_iid_active";
    /// Its columns (`git.py:261`), as attnames.
    pub const UNIQUE_PATH_COLUMNS: &[&str] = &[
        "provider",
        "host_url",
        "namespace",
        "repo_name",
        "external_iid",
    ];
    /// Its condition (`git.py:262`, `Q(deleted_at__isnull=True) &
    /// Q(repo_external_id="")`), unquoted semantic form.
    pub const UNIQUE_PATH_WHERE: &str = "deleted_at IS NULL AND repo_external_id = ''";

    /// `issue` index name (`git.py:267`).
    pub const INDEX_ISSUE_NAME: &str = "git_cr_issue_idx";
    /// `issue` index columns (`git.py:267`), as attnames.
    pub const INDEX_ISSUE_COLUMNS: &[&str] = &["issue_id"];
    /// `(provider, host_url, namespace, repo_name)` index name
    /// (`git.py:268`).
    pub const INDEX_REPO_NAME: &str = "git_cr_provider_repo_idx";
    /// `(provider, host_url, namespace, repo_name)` index columns
    /// (`git.py:268`).
    pub const INDEX_REPO_COLUMNS: &[&str] = &["provider", "host_url", "namespace", "repo_name"];

    /// `issue` FK: `CASCADE` (`git.py:230`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `issue` FK `related_name` (`git.py:230`).
    pub const ISSUE_RELATED_NAME: &str = "git_code_reviews";
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One code-review-link row. `repo_external_id`/`external_id` are
    /// never `NULL` (empty string when unset); `external_iid` is the
    /// required per-repo number; `metadata` stores `{}`, never `NULL`.
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

    impl std::fmt::Display for GitCodeReviewLink {
        /// `__str__` (`git.py:246-247`):
        /// `"{provider}:{namespace}/{repo_name}!{external_iid}
        /// <{issue_id}>"` — every part is owned by the row, so a full
        /// `Display` (not a label helper) ports it exactly.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{}:{}/{}!{} <{}>",
                self.provider, self.namespace, self.repo_name, self.external_iid, self.issue_id
            )
        }
    }
}

/// `file_assets` table (`db/models/asset.py:28-100`) — reuse module.
///
/// D-31 (`crate::app_assets::columns`) and D-21
/// (`crate::v1_assets::model::file_asset`) both ported this table in
/// full, so nothing is re-ported here: this module defines only the two
/// `Meta` names neither merged def carries, re-exports the merged row
/// struct so D-26 consumers import from one domain, and documents where
/// everything else lives. The tests assert the merged defs replay the
/// FX-ISS-09 `file_asset` section.
///
/// | Item | Merged def |
/// | --- | --- |
/// | `TABLE`, `ORDERING`, `COLUMNS`, `DEFAULTS`, `INDEXES`, `ENTITY_TYPES` | both |
/// | `ASSET_MAX_LENGTH`, `VARCHAR_MAX_LENGTH`, FK `*_ON_DELETE` consts, `FileAsset`, `Display` | `v1_assets::model::file_asset` |
/// | `upload_path_key`, `validate_file_size`, `asset_url`, `is_static_asset_type` | `app_assets::columns` |
pub mod file_asset {
    /// `verbose_name` (`asset.py:65`).
    pub const VERBOSE_NAME: &str = "File Asset";
    /// `verbose_name_plural` (`asset.py:66`).
    pub const VERBOSE_NAME_PLURAL: &str = "File Assets";

    /// The merged D-21 row struct, re-exported (not re-defined) so the
    /// attachment consumers (PIDASHCONV-655) import it from this domain.
    pub use crate::v1_assets::model::file_asset::FileAsset;
}

/// Foreign-table columns D-26 reads that this issue pins as SQL literals
/// (fixture FX-ISS-09 `foreign_reads`).
///
/// All five tables also have merged `integrations::*` defs (D-05), which
/// are deliberately NOT reused here: the fixture instructs "SQL literal
/// pinned here" for each, this issue carries no cross-domain dependency,
/// and the dotted `metadata.…` entries plus the installation traversal
/// note are read shapes a plain `COLUMNS` reuse would not capture. Query
/// builders copy these literals verbatim.
pub mod foreign {
    /// Physical `github_app_installations` table
    /// (`integration/github.py:121-141`, `db_table` at `:159`); D-26
    /// reads the snapshot fetch columns (`utils/github_pr_links.py`).
    pub const GITHUB_APP_INSTALLATION_TABLE: &str = "github_app_installations";
    /// Columns D-26 reads on [`GITHUB_APP_INSTALLATION_TABLE`], fixture
    /// order. `installation_id` (bigint, unique) feeds
    /// `GithubClient.for_installation`; the row is located via
    /// `workspace_integration__workspace__slug` +
    /// `account_login__iexact` (`github_pr_links.py:48-51`).
    pub const GITHUB_APP_INSTALLATION_COLUMNS_READ: &[&str] = &[
        "id",
        "workspace_integration_id",
        "installation_id",
        "account_login",
    ];

    /// Physical `git_issue_syncs` table (`integration/git.py:152-170`).
    pub const GIT_ISSUE_SYNC_TABLE: &str = "git_issue_syncs";
    /// Columns D-26 reads on [`GIT_ISSUE_SYNC_TABLE`], fixture order.
    /// The dotted entry is a JSON path into the `metadata` column (the
    /// `completion_comment_id` idempotency key,
    /// `.ai_design/github_sync/design.md` §6.5): query builders select
    /// `metadata` and extract the path.
    pub const GIT_ISSUE_SYNC_COLUMNS_READ: &[&str] =
        &["id", "issue_id", "metadata.completion_comment_id"];

    /// Physical `github_issue_syncs` table
    /// (`integration/github.py:76-90`).
    pub const GITHUB_ISSUE_SYNC_TABLE: &str = "github_issue_syncs";
    /// Columns D-26 reads on [`GITHUB_ISSUE_SYNC_TABLE`], fixture order
    /// (dotted entry is a `metadata` JSON path, as above).
    pub const GITHUB_ISSUE_SYNC_COLUMNS_READ: &[&str] =
        &["id", "issue_id", "metadata.completion_comment_id"];

    /// Physical `git_comment_syncs` table (`integration/git.py:190-223`).
    pub const GIT_COMMENT_SYNC_TABLE: &str = "git_comment_syncs";
    /// Columns D-26 reads on [`GIT_COMMENT_SYNC_TABLE`], fixture order.
    pub const GIT_COMMENT_SYNC_COLUMNS_READ: &[&str] = &["id", "comment_id"];

    /// Physical `github_comment_syncs` table
    /// (`integration/github.py:104-120`).
    pub const GITHUB_COMMENT_SYNC_TABLE: &str = "github_comment_syncs";
    /// Columns D-26 reads on [`GITHUB_COMMENT_SYNC_TABLE`], fixture
    /// order.
    pub const GITHUB_COMMENT_SYNC_COLUMNS_READ: &[&str] = &["id", "comment_id"];
}

#[cfg(test)]
mod tests {
    use super::file_asset;
    use super::foreign;
    use super::git_code_review_link;
    use super::github_pull_request_link;
    use super::issue_link;
    use super::issue_relation;
    use super::OnDelete;
    use crate::app_assets::columns as app_asset_columns;
    use crate::app_issues::models_core;
    use crate::soft_delete::active_condition;
    use crate::v1_assets::model::file_asset as merged_file_asset;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/app_issues/models")
    }

    fn fixture() -> serde_json::Value {
        let path = fixtures_dir().join("FX-ISS-09.links.json");
        let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read FX-ISS-09: {e}"));
        serde_json::from_str(&body).expect("FX-ISS-09.links.json is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Expand the compact string-list entries (`"<a>/<b> <type> …"`)
    /// into column names in order.
    fn string_column_names(value: &serde_json::Value, model: &str) -> Vec<String> {
        value[model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has columns array"))
            .iter()
            .flat_map(|c| {
                let s = c
                    .as_str()
                    .unwrap_or_else(|| panic!("{model} column entry is str"));
                let head = s.split_whitespace().next().expect("entry has head");
                head.split('/').map(str::to_string).collect::<Vec<_>>()
            })
            .collect()
    }

    /// Nullable column names from the compact string-list entries: an
    /// entry marks `NULL` (vs `NOT NULL` or the bare `PK`).
    fn string_nullable(value: &serde_json::Value, model: &str) -> Vec<String> {
        value[model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has columns array"))
            .iter()
            .flat_map(|c| {
                let s = c.as_str().expect("column entry is str");
                let nullable = s.contains("NULL") && !s.contains("NOT NULL");
                let head = s.split_whitespace().next().expect("entry has head");
                if nullable {
                    head.split('/').map(str::to_string).collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            })
            .collect()
    }

    fn constraint_strings(value: &serde_json::Value, model: &str) -> Vec<String> {
        value[model]["constraints"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has constraints"))
            .iter()
            .map(|c| c.as_str().expect("constraint is str").to_string())
            .collect()
    }

    fn string_list(value: &serde_json::Value, model: &str, key: &str) -> Vec<String> {
        value[model][key]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has {key}"))
            .iter()
            .map(|c| c.as_str().expect("entry is str").to_string())
            .collect()
    }

    fn foreign_columns_read(value: &serde_json::Value, table: &str) -> Vec<String> {
        value["foreign_reads"][table]["columns_read"]
            .as_array()
            .unwrap_or_else(|| panic!("foreign_reads has {table}"))
            .iter()
            .map(|c| c.as_str().expect("columns_read entry is str").to_string())
            .collect()
    }

    #[test]
    fn fixture_loads() {
        let v = fixture();
        assert_eq!(v["_fixture"].as_str().unwrap(), "FX-ISS-09");
        assert_eq!(
            v["_source"]["consumers"][0].as_str().unwrap(),
            "PIDASHCONV-645 (models_links.rs)"
        );
    }

    #[test]
    fn issue_link_columns_match_fixture() {
        let v = fixture();
        assert_eq!(
            owned(issue_link::COLUMNS),
            string_column_names(&v, "issue_link")
        );
        assert_eq!(issue_link::COLUMNS.len(), 12);
        // Same 8 inherited audit/project columns as the core issue table.
        assert_eq!(
            owned(&issue_link::COLUMNS[..8]),
            owned(&models_core::issue::COLUMNS[..8])
        );
        assert_eq!(
            owned(&issue_link::COLUMNS[8..]),
            vec!["title", "url", "issue_id", "metadata"]
        );
        let table: &str = issue_link::TABLE;
        assert_eq!(table, v["issue_link"]["table"].as_str().unwrap());
        assert_eq!(table, "issue_links");
        assert_eq!(issue_link::ORDERING, "-created_at");
        assert_eq!(issue_link::VERBOSE_NAME, "Issue Link");
        assert_eq!(issue_link::VERBOSE_NAME_PLURAL, "Issue Links");
        assert_eq!(
            string_nullable(&v, "issue_link"),
            vec!["created_by_id", "updated_by_id", "deleted_at", "title"]
        );
        assert_eq!(issue_link::TITLE_MAX_LENGTH, 255);
        // No uniqueness at all — ported as-is.
        let constraints = constraint_strings(&v, "issue_link");
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains("none"));
        assert!(constraints[0].contains("serializer"));
        let has_partial_unique: bool = issue_link::HAS_PARTIAL_UNIQUE;
        assert!(!has_partial_unique);
        assert_eq!(issue_link::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_link::ISSUE_RELATED_NAME, "issue_link");
        assert_eq!(issue_link::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_link::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_link::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue_link::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn issue_relation_columns_match_fixture() {
        let v = fixture();
        assert_eq!(
            owned(issue_relation::COLUMNS),
            string_column_names(&v, "issue_relation")
        );
        assert_eq!(issue_relation::COLUMNS.len(), 11);
        assert_eq!(
            owned(&issue_relation::COLUMNS[..8]),
            owned(&models_core::issue::COLUMNS[..8])
        );
        assert_eq!(
            owned(&issue_relation::COLUMNS[8..]),
            vec!["issue_id", "related_issue_id", "relation_type"]
        );
        let table: &str = issue_relation::TABLE;
        assert_eq!(table, v["issue_relation"]["table"].as_str().unwrap());
        assert_eq!(table, "issue_relations");
        assert_eq!(issue_relation::ORDERING, "-created_at");
        assert_eq!(issue_relation::VERBOSE_NAME, "Issue Relation");
        assert_eq!(issue_relation::VERBOSE_NAME_PLURAL, "Issue Relations");
        assert_eq!(
            string_nullable(&v, "issue_relation"),
            vec!["created_by_id", "updated_by_id", "deleted_at"]
        );
        assert_eq!(issue_relation::RELATION_TYPE_MAX_LENGTH, 20);
        assert_eq!(issue_relation::RELATION_TYPE_DEFAULT, "blocked_by");
        // Choice values in declaration order.
        let values: Vec<String> = issue_relation::RELATION_CHOICES
            .iter()
            .map(|(value, _)| (*value).to_string())
            .collect();
        assert_eq!(
            values,
            string_list(&v, "issue_relation", "relation_choices")
        );
        assert_eq!(
            issue_relation::RELATION_CHOICES,
            &[
                ("duplicate", "Duplicate"),
                ("relates_to", "Relates To"),
                ("blocked_by", "Blocked By"),
                ("start_before", "Start Before"),
                ("finish_before", "Finish Before"),
                ("implemented_by", "Implemented By"),
            ]
        );
        // Active-unique constraint (name recorded verbatim from Python;
        // the fixture carries the descriptive shape).
        let constraints = constraint_strings(&v, "issue_relation");
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains("(issue,related_issue)"));
        assert!(constraints[0].contains("deleted_at IS NULL"));
        assert_eq!(
            issue_relation::UNIQUE_ISSUE_RELATED_NAME,
            "issue_relation_unique_issue_related_issue_when_deleted_at_null"
        );
        assert_eq!(
            issue_relation::UNIQUE_ISSUE_RELATED_COLUMNS,
            &["issue_id", "related_issue_id"]
        );
        assert_eq!(
            issue_relation::UNIQUE_ISSUE_RELATED_WHERE,
            "deleted_at IS NULL"
        );
        assert_eq!(
            issue_relation::UNIQUE_TOGETHER,
            &["issue", "related_issue", "deleted_at"]
        );
        assert_eq!(issue_relation::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_relation::ISSUE_RELATED_NAME, "issue_relation");
        assert_eq!(issue_relation::RELATED_ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_relation::RELATED_ISSUE_RELATED_NAME, "issue_related");
        assert_eq!(issue_relation::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_relation::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_relation::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue_relation::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn relation_reverse_map_matches_python() {
        let v = fixture();
        let pairs = &v["issue_relation"]["reverse_pairs"];
        assert_eq!(pairs.as_object().unwrap().len(), 6);
        for (forward, reverse) in issue_relation::RELATION_PAIRS {
            assert_eq!(
                pairs[forward].as_str().unwrap(),
                *reverse,
                "reverse of {forward}"
            );
            assert_eq!(issue_relation::reverse_of(forward), Some(*reverse));
        }
        // Reverse-only values are NOT valid forward values.
        for reverse_only in ["blocking", "start_after", "finish_after", "implements"] {
            assert_eq!(
                issue_relation::reverse_of(reverse_only),
                None,
                "{reverse_only}"
            );
        }
        assert_eq!(issue_relation::reverse_of("no_such_relation"), None);
    }

    #[test]
    fn github_pr_link_columns_match_fixture() {
        let v = fixture();
        assert_eq!(
            owned(github_pull_request_link::COLUMNS),
            string_column_names(&v, "github_pr_link")
        );
        assert_eq!(github_pull_request_link::COLUMNS.len(), 18);
        assert_eq!(
            owned(&github_pull_request_link::COLUMNS[..8]),
            owned(&models_core::issue::COLUMNS[..8])
        );
        let table: &str = github_pull_request_link::TABLE;
        assert_eq!(table, v["github_pr_link"]["table"].as_str().unwrap());
        assert_eq!(table, "github_pull_request_links");
        assert_eq!(github_pull_request_link::ORDERING, "-created_at");
        assert_eq!(
            github_pull_request_link::VERBOSE_NAME,
            "Github Pull Request Link"
        );
        assert_eq!(
            github_pull_request_link::VERBOSE_NAME_PLURAL,
            "Github Pull Request Links"
        );
        assert_eq!(
            string_nullable(&v, "github_pr_link"),
            vec![
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "pr_updated_at"
            ]
        );
        assert_eq!(github_pull_request_link::REPO_OWNER_MAX_LENGTH, 255);
        assert_eq!(github_pull_request_link::REPO_NAME_MAX_LENGTH, 255);
        assert_eq!(github_pull_request_link::PR_NUMBER_MIN, 0);
        assert_eq!(github_pull_request_link::URL_MAX_LENGTH, 500);
        assert_eq!(github_pull_request_link::TITLE_MAX_LENGTH, 500);
        assert_eq!(github_pull_request_link::TITLE_DEFAULT, "");
        assert_eq!(github_pull_request_link::STATE_MAX_LENGTH, 12);
        assert_eq!(github_pull_request_link::STATE_DEFAULT, "open");
        assert_eq!(
            github_pull_request_link::STATE_CHOICES,
            &[("open", "Open"), ("closed", "Closed")]
        );
        let merged_default: bool = github_pull_request_link::MERGED_DEFAULT;
        assert!(!merged_default);
        let draft_default: bool = github_pull_request_link::DRAFT_DEFAULT;
        assert!(!draft_default);
        let constraints = constraint_strings(&v, "github_pr_link");
        assert_eq!(constraints.len(), 2);
        assert!(constraints[0].contains("(repo_owner,repo_name,pr_number)"));
        assert!(constraints[0].contains("deleted_at IS NULL"));
        assert!(constraints[0].contains(github_pull_request_link::UNIQUE_PR_NAME));
        assert_eq!(
            github_pull_request_link::UNIQUE_PR_NAME,
            "github_pr_link_unique_per_pr_when_active"
        );
        assert_eq!(
            github_pull_request_link::UNIQUE_PR_COLUMNS,
            &["repo_owner", "repo_name", "pr_number"]
        );
        assert_eq!(
            github_pull_request_link::UNIQUE_PR_WHERE,
            "deleted_at IS NULL"
        );
        assert!(constraints[1].contains(github_pull_request_link::INDEX_PR_NAME));
        assert!(constraints[1].contains(github_pull_request_link::INDEX_ISSUE_NAME));
        assert_eq!(
            github_pull_request_link::INDEX_PR_NAME,
            "github_pr_l_repo_ow_idx"
        );
        assert_eq!(
            github_pull_request_link::INDEX_PR_COLUMNS,
            &["repo_owner", "repo_name", "pr_number"]
        );
        assert_eq!(
            github_pull_request_link::INDEX_ISSUE_NAME,
            "github_pr_l_issue_idx"
        );
        assert_eq!(github_pull_request_link::INDEX_ISSUE_COLUMNS, &["issue_id"]);
        assert_eq!(github_pull_request_link::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            github_pull_request_link::ISSUE_RELATED_NAME,
            "github_pull_requests"
        );
        assert_eq!(
            github_pull_request_link::PROJECT_ON_DELETE,
            OnDelete::Cascade
        );
        assert_eq!(
            github_pull_request_link::WORKSPACE_ON_DELETE,
            OnDelete::Cascade
        );
        assert_eq!(
            github_pull_request_link::CREATED_BY_ON_DELETE,
            OnDelete::SetNull
        );
        assert_eq!(
            github_pull_request_link::UPDATED_BY_ON_DELETE,
            OnDelete::SetNull
        );
    }

    #[test]
    fn git_code_review_link_columns_match_fixture() {
        let v = fixture();
        assert_eq!(
            owned(git_code_review_link::COLUMNS),
            string_column_names(&v, "git_code_review_link")
        );
        assert_eq!(git_code_review_link::COLUMNS.len(), 23);
        assert_eq!(
            owned(&git_code_review_link::COLUMNS[..8]),
            owned(&models_core::issue::COLUMNS[..8])
        );
        let table: &str = git_code_review_link::TABLE;
        assert_eq!(table, v["git_code_review_link"]["table"].as_str().unwrap());
        assert_eq!(table, "git_code_review_links");
        assert_eq!(git_code_review_link::ORDERING, "-created_at");
        assert_eq!(git_code_review_link::VERBOSE_NAME, "Git Code Review Link");
        assert_eq!(
            git_code_review_link::VERBOSE_NAME_PLURAL,
            "Git Code Review Links"
        );
        assert_eq!(
            string_nullable(&v, "git_code_review_link"),
            vec![
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "remote_updated_at"
            ]
        );
        assert_eq!(git_code_review_link::PROVIDER_MAX_LENGTH, 32);
        assert_eq!(git_code_review_link::HOST_URL_MAX_LENGTH, 500);
        assert_eq!(git_code_review_link::NAMESPACE_MAX_LENGTH, 500);
        assert_eq!(git_code_review_link::REPO_NAME_MAX_LENGTH, 500);
        assert_eq!(git_code_review_link::REPO_EXTERNAL_ID_MAX_LENGTH, 255);
        assert_eq!(git_code_review_link::REPO_EXTERNAL_ID_DEFAULT, "");
        assert_eq!(git_code_review_link::EXTERNAL_ID_MAX_LENGTH, 255);
        assert_eq!(git_code_review_link::EXTERNAL_ID_DEFAULT, "");
        assert_eq!(git_code_review_link::EXTERNAL_IID_MAX_LENGTH, 255);
        assert_eq!(git_code_review_link::URL_MAX_LENGTH, 1000);
        assert_eq!(git_code_review_link::TITLE_MAX_LENGTH, 500);
        assert_eq!(git_code_review_link::TITLE_DEFAULT, "");
        assert_eq!(git_code_review_link::STATE_MAX_LENGTH, 16);
        assert_eq!(git_code_review_link::STATE_DEFAULT, "open");
        assert_eq!(
            git_code_review_link::STATE_CHOICES,
            &[("open", "Open"), ("closed", "Closed"), ("merged", "Merged")]
        );
        let merged_default: bool = git_code_review_link::MERGED_DEFAULT;
        assert!(!merged_default);
        let draft_default: bool = git_code_review_link::DRAFT_DEFAULT;
        assert!(!draft_default);
        // Dual active-unique constraints split on repo_external_id.
        let constraints = constraint_strings(&v, "git_code_review_link");
        assert_eq!(constraints.len(), 3);
        assert!(constraints[0].contains("(provider,host_url,repo_external_id,external_iid)"));
        assert!(constraints[0].contains("repo_external_id<>''"));
        assert_eq!(
            git_code_review_link::UNIQUE_REPO_NAME,
            "git_cr_uniq_repo_iid_active"
        );
        assert_eq!(
            git_code_review_link::UNIQUE_REPO_COLUMNS,
            &["provider", "host_url", "repo_external_id", "external_iid"]
        );
        assert_eq!(
            git_code_review_link::UNIQUE_REPO_WHERE,
            "deleted_at IS NULL AND repo_external_id <> ''"
        );
        assert!(constraints[1].contains("(provider,host_url,namespace,repo_name,external_iid)"));
        assert!(constraints[1].contains("repo_external_id=''"));
        assert_eq!(
            git_code_review_link::UNIQUE_PATH_NAME,
            "git_cr_uniq_path_iid_active"
        );
        assert_eq!(
            git_code_review_link::UNIQUE_PATH_COLUMNS,
            &[
                "provider",
                "host_url",
                "namespace",
                "repo_name",
                "external_iid"
            ]
        );
        assert_eq!(
            git_code_review_link::UNIQUE_PATH_WHERE,
            "deleted_at IS NULL AND repo_external_id = ''"
        );
        assert!(constraints[2].contains(git_code_review_link::INDEX_ISSUE_NAME));
        assert!(constraints[2].contains(git_code_review_link::INDEX_REPO_NAME));
        assert_eq!(git_code_review_link::INDEX_ISSUE_NAME, "git_cr_issue_idx");
        assert_eq!(git_code_review_link::INDEX_ISSUE_COLUMNS, &["issue_id"]);
        assert_eq!(
            git_code_review_link::INDEX_REPO_NAME,
            "git_cr_provider_repo_idx"
        );
        assert_eq!(
            git_code_review_link::INDEX_REPO_COLUMNS,
            &["provider", "host_url", "namespace", "repo_name"]
        );
        assert_eq!(git_code_review_link::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(git_code_review_link::ISSUE_RELATED_NAME, "git_code_reviews");
        assert_eq!(git_code_review_link::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(git_code_review_link::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            git_code_review_link::CREATED_BY_ON_DELETE,
            OnDelete::SetNull
        );
        assert_eq!(
            git_code_review_link::UPDATED_BY_ON_DELETE,
            OnDelete::SetNull
        );
    }

    #[test]
    fn file_asset_replays_merged_defs() {
        let v = fixture();
        // Both merged defs carry the same table/columns/ordering/indexes
        // (their `Index` structs are distinct types, so names are
        // collected up front and the shared asserts loop over tables).
        let d31_names: Vec<String> = app_asset_columns::INDEXES
            .iter()
            .map(|i| i.name.to_string())
            .collect();
        let d21_names: Vec<String> = merged_file_asset::INDEXES
            .iter()
            .map(|i| i.name.to_string())
            .collect();
        for (label, table, columns, names) in [
            (
                "d31",
                app_asset_columns::TABLE,
                app_asset_columns::COLUMNS,
                &d31_names,
            ),
            (
                "d21",
                merged_file_asset::TABLE,
                merged_file_asset::COLUMNS,
                &d21_names,
            ),
        ] {
            assert_eq!(table, v["file_asset"]["table"].as_str().unwrap(), "{label}");
            assert_eq!(table, "file_assets", "{label}");
            assert_eq!(
                owned(columns),
                string_column_names(&v, "file_asset"),
                "{label}"
            );
            assert_eq!(columns.len(), 24, "{label}");
            assert_eq!(*names, string_list(&v, "file_asset", "indexes"), "{label}");
            assert_eq!(
                *names,
                vec![
                    "asset_entity_type_idx",
                    "asset_entity_identifier_idx",
                    "asset_entity_idx",
                    "asset_asset_idx",
                ],
                "{label}"
            );
        }
        assert_eq!(app_asset_columns::ORDERING, &["-created_at"]);
        assert_eq!(merged_file_asset::ORDERING, "-created_at");
        assert_eq!(
            owned(app_asset_columns::ENTITY_TYPES),
            string_list(&v, "file_asset", "entity_types")
        );
        assert_eq!(
            owned(merged_file_asset::ENTITY_TYPES),
            string_list(&v, "file_asset", "entity_types")
        );
        assert_eq!(merged_file_asset::ENTITY_TYPES.len(), 10);
        assert_eq!(
            app_asset_columns::DEFAULTS.len(),
            app_asset_columns::COLUMNS.len()
        );
        assert_eq!(
            merged_file_asset::DEFAULTS.len(),
            merged_file_asset::COLUMNS.len()
        );
        // Nullability contract the merged row struct satisfies.
        assert_eq!(
            string_nullable(&v, "file_asset"),
            vec![
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "user_id",
                "workspace_id",
                "draft_issue_id",
                "project_id",
                "issue_id",
                "comment_id",
                "page_id",
                "entity_type",
                "entity_identifier",
                "external_id",
                "external_source",
                "storage_metadata",
            ]
        );
        // Max lengths (Python-derived; the fixture column strings
        // corroborate the 800/255 bounds).
        assert_eq!(merged_file_asset::ASSET_MAX_LENGTH, 800);
        assert_eq!(merged_file_asset::VARCHAR_MAX_LENGTH, 255);
        let columns_text = v["file_asset"]["columns"].to_string();
        assert!(columns_text.contains("varchar(800)"));
        assert!(columns_text.contains("varchar(255)"));
        // FK delete behavior is CASCADE on every merged const.
        for on_delete in [
            merged_file_asset::USER_ON_DELETE,
            merged_file_asset::WORKSPACE_ON_DELETE,
            merged_file_asset::DRAFT_ISSUE_ON_DELETE,
            merged_file_asset::PROJECT_ON_DELETE,
            merged_file_asset::ISSUE_ON_DELETE,
            merged_file_asset::COMMENT_ON_DELETE,
            merged_file_asset::PAGE_ON_DELETE,
        ] {
            assert_eq!(format!("{on_delete:?}"), "Cascade");
        }
        // The attachment-url note names the branch the merged `asset_url`
        // covers (the merged defs' own suites pin the function).
        assert!(v["file_asset"]["asset_url_property"]
            .as_str()
            .unwrap()
            .contains("ISSUE_ATTACHMENT"));
        // The only bits missing from both merged defs, defined here.
        assert_eq!(file_asset::VERBOSE_NAME, "File Asset");
        assert_eq!(file_asset::VERBOSE_NAME_PLURAL, "File Assets");
    }

    #[test]
    fn foreign_reads_replay_fixture() {
        let v = fixture();
        assert_eq!(
            owned(foreign::GITHUB_APP_INSTALLATION_COLUMNS_READ),
            foreign_columns_read(&v, "github_app_installation")
        );
        assert_eq!(
            foreign::GITHUB_APP_INSTALLATION_COLUMNS_READ,
            &[
                "id",
                "workspace_integration_id",
                "installation_id",
                "account_login"
            ]
        );
        let table: &str = foreign::GITHUB_APP_INSTALLATION_TABLE;
        assert_eq!(table, "github_app_installations");
        assert!(v["foreign_reads"]["github_app_installation"]["traversal"]
            .as_str()
            .unwrap()
            .contains("account_login__iexact"));
        assert_eq!(
            owned(foreign::GIT_ISSUE_SYNC_COLUMNS_READ),
            foreign_columns_read(&v, "git_issue_sync")
        );
        assert_eq!(
            foreign::GIT_ISSUE_SYNC_COLUMNS_READ,
            &["id", "issue_id", "metadata.completion_comment_id"]
        );
        let table: &str = foreign::GIT_ISSUE_SYNC_TABLE;
        assert_eq!(table, "git_issue_syncs");
        assert_eq!(
            owned(foreign::GITHUB_ISSUE_SYNC_COLUMNS_READ),
            foreign_columns_read(&v, "github_issue_sync")
        );
        assert_eq!(
            foreign::GITHUB_ISSUE_SYNC_COLUMNS_READ,
            &["id", "issue_id", "metadata.completion_comment_id"]
        );
        let table: &str = foreign::GITHUB_ISSUE_SYNC_TABLE;
        assert_eq!(table, "github_issue_syncs");
        assert_eq!(
            owned(foreign::GIT_COMMENT_SYNC_COLUMNS_READ),
            foreign_columns_read(&v, "git_comment_sync")
        );
        assert_eq!(
            foreign::GIT_COMMENT_SYNC_COLUMNS_READ,
            &["id", "comment_id"]
        );
        let table: &str = foreign::GIT_COMMENT_SYNC_TABLE;
        assert_eq!(table, "git_comment_syncs");
        assert_eq!(
            owned(foreign::GITHUB_COMMENT_SYNC_COLUMNS_READ),
            foreign_columns_read(&v, "github_comment_sync")
        );
        assert_eq!(
            foreign::GITHUB_COMMENT_SYNC_COLUMNS_READ,
            &["id", "comment_id"]
        );
        let table: &str = foreign::GITHUB_COMMENT_SYNC_TABLE;
        assert_eq!(table, "github_comment_syncs");
    }

    #[test]
    fn reads_are_soft_delete_scoped() {
        for table in [
            issue_link::TABLE,
            issue_relation::TABLE,
            github_pull_request_link::TABLE,
            git_code_review_link::TABLE,
            merged_file_asset::TABLE,
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
        let issue_id = uuid::Uuid::nil();
        let link = issue_link::IssueLink {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            title: Some("spec".to_string()),
            url: "https://example.com/spec".to_string(),
            issue_id,
            metadata: serde_json::json!({}),
        };
        assert_eq!(
            issue_link::IssueLink::label("Ship it", &link.url),
            "Ship it https://example.com/spec"
        );
        assert_eq!(issue_relation::IssueRelation::label("A", "B"), "A B");
        let pr = github_pull_request_link::GithubPullRequestLink {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            issue_id,
            repo_owner: "octo".to_string(),
            repo_name: "hello".to_string(),
            pr_number: 42,
            url: "https://github.com/octo/hello/pull/42".to_string(),
            title: "Fix".to_string(),
            state: github_pull_request_link::STATE_DEFAULT.to_string(),
            merged: false,
            draft: false,
            pr_updated_at: None,
        };
        assert_eq!(pr.to_string(), format!("octo/hello#42 <{issue_id}>"));
        let review = git_code_review_link::GitCodeReviewLink {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            issue_id,
            provider: "github".to_string(),
            host_url: "https://github.com".to_string(),
            namespace: "octo".to_string(),
            repo_name: "hello".to_string(),
            repo_external_id: String::new(),
            external_id: String::new(),
            external_iid: "7".to_string(),
            url: "https://github.com/octo/hello/pull/7".to_string(),
            title: "Fix".to_string(),
            state: git_code_review_link::STATE_DEFAULT.to_string(),
            merged: false,
            draft: false,
            remote_updated_at: None,
            metadata: serde_json::json!({}),
        };
        assert_eq!(
            review.to_string(),
            format!("github:octo/hello!7 <{issue_id}>")
        );
        // The re-exported merged struct renders the storage key.
        let asset = file_asset::FileAsset {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            attributes: serde_json::json!({}),
            asset: "w-id/abc-spec.png".to_string(),
            user_id: None,
            workspace_id: None,
            draft_issue_id: None,
            project_id: None,
            issue_id: None,
            comment_id: None,
            page_id: None,
            entity_type: Some("ISSUE_ATTACHMENT".to_string()),
            entity_identifier: None,
            is_deleted: false,
            is_archived: false,
            external_id: None,
            external_source: None,
            size: 0.0,
            is_uploaded: true,
            storage_metadata: None,
        };
        assert_eq!(asset.to_string(), "w-id/abc-spec.png");
    }
}
