//! Git code-review link lifecycle (D-05, stage 5).
//!
//! Ports `apps/api/pi_dash/integrations/git/code_reviews.py:1-262` (the link
//! lifecycle closure): errors `InvalidCodeReviewURL` / `IssueNotFound` /
//! `CodeReviewAlreadyLinked` (22-35), `_github_legacy_link` (38-46),
//! `_github_path_review_link` (48-61), `_ensure_github_legacy_link`
//! (64-106), `detach_code_review_link` (107-124), `_account_for_review`
//! (125-166) and `attach_code_review` (167-262).
//!
//! Layering: this crate holds no database pool (no new dependency enters
//! the lockfile), so queries cross the [`CodeReviewStore`] seam as typed
//! rows from `pidash-db`, and provider calls cross the [`ReviewAdapter`]
//! seam from `pidash-types`. Every SQL statement the seam executes lives
//! here as a `*_SQL` const so tests assert the queryset shape without a
//! live database. The real pool implementation lands with the task layers
//! (PIDASHCONV-147/148); `attach`'s create-then-ensure pair must run
//! inside one transaction wherever Python is atomic, and the
//! `IntegrityError` race reread is modelled as [`LinkWriteError::Conflict`].
//!
//! Writes take an explicit [`RequestContext`][pidash_db::RequestContext]
//! (Porting guide: handlers never hold an unscoped handle); the audit
//! columns come from `ctx.audit_actor()` (`None` stays NULL, mirroring
//! `getattr(actor, "id", None)` when the actor has no id). The link row's
//! `workspace_id` is derived from the project, mirroring
//! `ProjectBaseModel.save` (`project.py:302-311`), which sets
//! `workspace = project.workspace` on every write.
//!
//! Fixture replayed alongside:
//! `rust-api/fixtures/integrations/code_reviews.golden.json`.
//!
//! Ported bugs / inherited semantics (translate, don't redesign):
//!
//! * `int(external_iid)` (38-46, 67, 110-113) is strict Python `int`
//!   grammar (surrounding whitespace, optional sign, single underscores
//!   between digits) surfaced here as [`parse_pr_number`]. The attach and
//!   ensure paths let a parse failure raise ([`CodeReviewError::BadPrNumber`]),
//!   exactly like Python's uncaught `ValueError`; only `detach` tolerates
//!   it (`try/except` → skip the legacy lookup, `code_reviews.py:110-113`).
//! * `detach` looks the legacy row up with the link's stored
//!   `namespace`/`repo_name` verbatim (no `.lower()`), unlike every other
//!   path (`code_reviews.py:116-119`); [`detach_plan`] preserves that.
//! * `_account_for_review` swallows *every* account-resolution failure —
//!   zero accounts, several accounts, unknown workspace — and returns
//!   `(None, None)` without even trying the repository lookup
//!   (`code_reviews.py:145-160`). Multi-account workspaces therefore link
//!   without a provider snapshot instead of raising `Ambiguous`.
//! * Only `GitProviderNotFoundError` propagates out of `get_code_review`;
//!   any other provider (or transport) failure silently yields an empty
//!   snapshot (`code_reviews.py:201-206`).
//! * `get_adapter` sits outside the `try` (`code_reviews.py:183-184`), so
//!   an unknown provider raises instead of yielding an empty snapshot;
//!   [`CodeReviewError::UnknownProvider`] mirrors that `KeyError`.
//! * `update_or_create`-style creation is find-then-write with a reread on
//!   conflict (`code_reviews.py:240-262`); a lost race that still finds no
//!   row re-raises, like Django.
//! * `link.delete()` / `legacy_link.delete()` are hard deletes
//!   (`code_reviews.py:120-123`).
//!
//! Deliberate approximations (unreachable with real provider payloads,
//! documented here instead of a paragraph per call site):
//!
//! * `str.lower()` vs `str::to_lowercase` differ on exotic Unicode
//!   (e.g. Turkish dotted I); provider namespaces from real URLs are
//!   ASCII, where both agree.
//! * [`parse_pr_number`] accepts ASCII digits only; Python `int` also
//!   accepts non-ASCII decimal digits. Real PR numbers are ASCII.
//! * [`py_str`] renders `str(metadata_value)` exactly for strings, bools
//!   (`"True"`/`"False"`) and JSON numbers; containers render as compact
//!   JSON where Python would render its `repr`. Real `project_id`
//!   metadata is a string or int.
//! * [`parse_remote_dt`] covers RFC 3339 (the `datetime.isoformat()` shape
//!   the adapters emit); anything else yields `None` where Python would
//!   keep the parsed datetime.
//! * `title[:500]` counts Python code points; [`truncate_title`] counts
//!   Rust `char`s, which agree except on lone surrogates (which cannot
//!   appear in Rust `str`).

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use pidash_db::integrations::git_models::git_code_review_link::GitCodeReviewLink;
use pidash_db::integrations::git_models::git_provider_account::GitProviderAccount;
use pidash_db::integrations::git_models::git_repository::GitRepository;
use pidash_db::integrations::github_models::github_pull_request_link::GithubPullRequestLink;
use pidash_types::integrations::{
    GitProviderAdapter, GitProviderError, ParsedCodeReview, RemoteCodeReview, UnknownProvider,
};

use super::accounts::{account_credential, json_truthy, normalize_host_url, GitStore, StoreError};

/// Link-lifecycle failure (`code_reviews.py:22-35` plus the propagated
/// `KeyError` / `ValueError` / provider / store failures).
#[derive(Debug, thiserror::Error)]
pub enum CodeReviewError {
    /// `InvalidCodeReviewURL` (`code_reviews.py:22-23`): `parse` returned
    /// `None`. Python raises it bare, so `str(exc)` is empty.
    #[error("")]
    InvalidUrl,
    /// `IssueNotFound` (`code_reviews.py:26-27`): raised bare, `str` empty.
    #[error("")]
    IssueNotFound,
    /// `CodeReviewAlreadyLinked` (`code_reviews.py:30-35`).
    #[error("{}", already_linked_message(.issue_id))]
    AlreadyLinked {
        /// `str(issue_id)` of the issue already holding the review.
        issue_id: String,
    },
    /// `int(external_iid)` failed on the attach/ensure path, where Python
    /// lets the `ValueError` propagate (`code_reviews.py:43,67`).
    #[error("invalid pr number: {0}")]
    BadPrNumber(String),
    /// `get_adapter` found no adapter (`registry.py:21`
    /// `KeyError("Unsupported Git provider: …")`).
    #[error("{0}")]
    UnknownProvider(#[from] UnknownProvider),
    /// `GitProviderNotFoundError` from `get_code_review`: the only provider
    /// failure `attach` propagates (`code_reviews.py:201-202`).
    #[error("{0}")]
    ProviderNotFound(#[from] GitProviderError),
    /// The store call failed.
    #[error("{0}")]
    Store(#[from] StoreError),
}

impl CodeReviewError {
    /// Python exception class name, for golden replay.
    pub fn kind(&self) -> &'static str {
        match self {
            CodeReviewError::InvalidUrl => "InvalidCodeReviewURL",
            CodeReviewError::IssueNotFound => "IssueNotFound",
            CodeReviewError::AlreadyLinked { .. } => "CodeReviewAlreadyLinked",
            CodeReviewError::BadPrNumber(_) => "ValueError",
            CodeReviewError::UnknownProvider(_) => "KeyError",
            CodeReviewError::ProviderNotFound(_) => "GitProviderNotFoundError",
            CodeReviewError::Store(_) => "StoreError",
        }
    }
}

/// `CodeReviewAlreadyLinked` message
/// (`code_reviews.py:35`: `f"This code review is already linked to issue
/// {issue_id}."`).
pub fn already_linked_message(issue_id: &str) -> String {
    format!("This code review is already linked to issue {issue_id}.")
}

/// Python `int(external_iid)` (`code_reviews.py:43,67,110`).
///
/// Surrounding whitespace stripped, optional `+`/`-` sign, ASCII digits
/// with single underscores allowed between digits (`int("1_2") == 12`).
/// Returns `None` for anything else (including overflow: Python ints are
/// unbounded, but no PR number overflows `i64`).
pub fn parse_pr_number(raw: &str) -> Option<i64> {
    let text = raw.trim();
    let digits = text
        .strip_prefix('+')
        .or_else(|| text.strip_prefix('-'))
        .unwrap_or(text);
    if digits.is_empty() {
        return None;
    }
    let mut cleaned = String::with_capacity(digits.len());
    let mut prev_underscore = true; // leading '_' rejected
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
        } else if ch.is_ascii_digit() {
            cleaned.push(ch);
            prev_underscore = false;
        } else {
            return None;
        }
    }
    if prev_underscore {
        return None; // trailing '_' rejected
    }
    let value: i64 = cleaned.parse().ok()?;
    Some(if text.starts_with('-') { -value } else { value })
}

/// Legacy-link state for a review state (`code_reviews.py:78-82,96-100`).
///
/// `GithubPullRequestLink.State` has no `MERGED`, so both `closed` and
/// `merged` collapse to `"closed"`; anything else (including `open` and
/// unknown states) becomes `"open"`.
pub fn legacy_state_for(review_state: &str) -> &'static str {
    match review_state {
        "closed" | "merged" => "closed",
        _ => "open",
    }
}

/// `review.title[:500]` (`code_reviews.py:190`): first 500 code points.
pub fn truncate_title(title: &str) -> String {
    title.chars().take(500).collect()
}

/// Python `str()` over a JSON metadata scalar (`code_reviews.py:197-198`
/// `str(review.metadata.get("project_id"))`).
pub fn py_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::Null => "None".to_owned(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

/// `review.metadata.get("project_id")` when truthy, stringified
/// (`code_reviews.py:197-198`).
pub fn metadata_project_id(metadata: &Value) -> Option<String> {
    let value = metadata.get("project_id")?;
    if !json_truthy(Some(value)) {
        return None;
    }
    Some(py_str(value))
}

/// Lookup key for `_github_legacy_link` (`code_reviews.py:38-46`):
/// `(repo_owner.lower(), repo_name.lower(), int(external_iid))`, or `None`
/// for non-GitHub reviews. A non-numeric `external_iid` is `None` here;
/// the attach/ensure orchestration maps that to [`CodeReviewError::BadPrNumber`]
/// (Python's uncaught `ValueError`), while `detach` skips the lookup.
pub fn legacy_lookup_key(parsed: &ParsedCodeReview) -> Option<(String, String, i32)> {
    if parsed.provider != "github" {
        return None;
    }
    let number = parse_pr_number(&parsed.external_iid)?;
    Some((
        parsed.namespace.to_lowercase(),
        parsed.repo_name.to_lowercase(),
        i32::try_from(number).ok()?,
    ))
}

/// Lookup key for `_github_path_review_link` (`code_reviews.py:48-61`):
/// `(provider, host_url, namespace.lower(), repo_name.lower(),
/// external_iid)`, newest first, or `None` for non-GitHub reviews.
pub fn path_lookup_key(parsed: &ParsedCodeReview) -> Option<(&str, &str, String, String, &str)> {
    if parsed.provider != "github" {
        return None;
    }
    Some((
        parsed.provider.as_str(),
        parsed.host_url.as_str(),
        parsed.namespace.to_lowercase(),
        parsed.repo_name.to_lowercase(),
        parsed.external_iid.as_str(),
    ))
}

/// Pre-rendered remote datetime (`RemoteCodeReview.updated_at`, the
/// `datetime.isoformat()` shape) back to a timestamp for the
/// `remote_updated_at` column.
pub fn parse_remote_dt(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|fixed| fixed.with_timezone(&Utc))
}

/// `_github_legacy_link` row fetch (`code_reviews.py:41-45`).
///
/// Parameters: `$1` lowercased owner, `$2` lowercased name, `$3` PR number.
/// Django's unordered `.first()` orders by pk; the id predicate shape
/// follows [`PROVIDER_ACCOUNT_GET_SQL`][super::accounts::PROVIDER_ACCOUNT_GET_SQL].
pub const LEGACY_LINK_FIND_SQL: &str = "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_id, repo_owner, repo_name, pr_number, url, title, state, merged, draft, pr_updated_at FROM github_pull_request_links WHERE repo_owner = $1 AND repo_name = $2 AND pr_number = $3 AND deleted_at IS NULL ORDER BY id ASC LIMIT 1";

/// `_github_path_review_link` row fetch (`code_reviews.py:51-60`).
///
/// Parameters: `$1` provider, `$2` host, `$3` lowercased namespace, `$4`
/// lowercased repo, `$5` external iid. `ORDER BY created_at DESC` mirrors
/// `.order_by("-created_at").first()`.
pub const PATH_REVIEW_LINK_FIND_SQL: &str = "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_id, provider, host_url, namespace, repo_name, repo_external_id, external_id, external_iid, url, title, state, merged, draft, remote_updated_at, metadata FROM git_code_review_links WHERE provider = $1 AND host_url = $2 AND namespace = $3 AND repo_name = $4 AND external_iid = $5 AND deleted_at IS NULL ORDER BY created_at DESC LIMIT 1";

/// Repo-scope link fetch (`code_reviews.py:216-217`: `filter(**lookup,
/// repo_external_id=…)`).
///
/// Parameters: `$1` provider, `$2` host, `$3` external iid, `$4` repo
/// external id.
pub const REPO_SCOPE_LINK_FIND_SQL: &str = "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_id, provider, host_url, namespace, repo_name, repo_external_id, external_id, external_iid, url, title, state, merged, draft, remote_updated_at, metadata FROM git_code_review_links WHERE provider = $1 AND host_url = $2 AND external_iid = $3 AND repo_external_id = $4 AND deleted_at IS NULL ORDER BY id ASC LIMIT 1";

/// Path-scope link fetch with an empty repo external id
/// (`code_reviews.py:219-224`).
///
/// Parameters: `$1` provider, `$2` host, `$3` external iid, `$4`
/// lowercased namespace, `$5` lowercased repo.
pub const PATH_EMPTY_REPO_LINK_FIND_SQL: &str = "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_id, provider, host_url, namespace, repo_name, repo_external_id, external_id, external_iid, url, title, state, merged, draft, remote_updated_at, metadata FROM git_code_review_links WHERE provider = $1 AND host_url = $2 AND external_iid = $3 AND namespace = $4 AND repo_name = $5 AND repo_external_id = '' AND deleted_at IS NULL ORDER BY id ASC LIMIT 1";

/// Legacy-link field sync (`code_reviews.py:76-87` `save(update_fields=…)`).
///
/// Parameters: `$1` id, `$2` url, `$3` title, `$4` state, `$5` merged,
/// `$6` draft, `$7` pr_updated_at, `$8` updated_at (`auto_now`).
pub const LEGACY_LINK_UPDATE_SQL: &str = "UPDATE github_pull_request_links SET url = $2, title = $3, state = $4, merged = $5, draft = $6, pr_updated_at = $7, updated_at = $8 WHERE id = $1";

/// Legacy-link create (`code_reviews.py:89-105`). `deleted_at` stays NULL.
/// `workspace_id` comes from the project (`ProjectBaseModel.save`).
///
/// Parameters in column order: `$1` id, `$2` created_at, `$3` updated_at,
/// `$4` created_by id (nullable), `$5` updated_by id (nullable), `$6`
/// project id, `$7` workspace id, `$8` issue id, `$9` owner, `$10` repo,
/// `$11` pr number, `$12` url, `$13` title, `$14` state, `$15` merged,
/// `$16` draft, `$17` pr_updated_at (nullable).
pub const LEGACY_LINK_INSERT_SQL: &str = "INSERT INTO github_pull_request_links (id, created_at, updated_at, created_by_id, updated_by_id, project_id, workspace_id, issue_id, repo_owner, repo_name, pr_number, url, title, state, merged, draft, pr_updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)";

/// Review-link create (`code_reviews.py:229-247`). `deleted_at` stays NULL.
/// `workspace_id` comes from the project (`ProjectBaseModel.save`).
///
/// Parameters in column order: `$1` id, `$2` created_at, `$3` updated_at,
/// `$4` created_by id (nullable), `$5` updated_by id (nullable), `$6`
/// project id, `$7` workspace id, `$8` issue id, `$9` provider, `$10`
/// host, `$11` namespace (lowercased), `$12` repo (lowercased), `$13` repo
/// external id, `$14` external id, `$15` external iid, `$16` url, `$17`
/// title, `$18` state, `$19` merged, `$20` draft, `$21` remote_updated_at
/// (nullable), `$22` metadata (jsonb).
pub const CODE_REVIEW_LINK_INSERT_SQL: &str = "INSERT INTO git_code_review_links (id, created_at, updated_at, created_by_id, updated_by_id, project_id, workspace_id, issue_id, provider, host_url, namespace, repo_name, repo_external_id, external_id, external_iid, url, title, state, merged, draft, remote_updated_at, metadata) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22)";

/// Issue-membership probe for `attach` (`code_reviews.py:172`:
/// `Issue.objects.filter(id, project_id, workspace__slug).exists()`).
///
/// Parameters: `$1` issue id, `$2` project id, `$3` workspace slug.
pub const ISSUE_EXISTS_SQL: &str = "SELECT 1 FROM issues WHERE id = $1 AND project_id = $2 AND workspace_id = (SELECT id FROM workspaces WHERE slug = $3) AND deleted_at IS NULL LIMIT 1";

/// Binding probe for `_account_for_review` (`code_reviews.py:126-137`):
/// workspace slug + project + provider/host/namespace (`iexact`)/name
/// (`iexact`), with the account and repository joined in (mirroring
/// `select_related("provider_account", "repository")`).
///
/// `iexact` is `LOWER()` on both sides (Porting guide semantic trap).
/// Parameters: `$1` workspace slug, `$2` project id, `$3` provider, `$4`
/// host, `$5` namespace, `$6` repo name. The pool implementation returns
/// the joined account then repository rows (`SELECT pa.*, r.*`).
pub const BINDING_FOR_REVIEW_SQL: &str = "SELECT pa.*, r.* FROM git_repository_bindings b INNER JOIN git_repositories r ON r.id = b.repository_id AND r.deleted_at IS NULL INNER JOIN git_provider_accounts pa ON pa.id = b.provider_account_id AND pa.deleted_at IS NULL WHERE b.workspace_id = (SELECT id FROM workspaces WHERE slug = $1) AND b.project_id = $2 AND r.provider = $3 AND r.host_url = $4 AND LOWER(r.namespace) = LOWER($5) AND LOWER(r.name) = LOWER($6) AND b.deleted_at IS NULL LIMIT 1";

/// Issue fallback for `_account_for_review` (`code_reviews.py:139-143`):
/// any live issue in the project/workspace, for its workspace id.
///
/// Parameters: `$1` project id, `$2` workspace slug.
pub const REVIEW_ISSUE_WORKSPACE_SQL: &str = "SELECT i.workspace_id FROM issues i WHERE i.project_id = $1 AND i.workspace_id = (SELECT id FROM workspaces WHERE slug = $2) AND i.deleted_at IS NULL LIMIT 1";

/// Repository fallback for `_account_for_review`
/// (`code_reviews.py:152-160`): provider/host/namespace (`iexact`)/name
/// (`iexact`).
///
/// Parameters: `$1` provider, `$2` host, `$3` namespace, `$4` repo name.
pub const REVIEW_REPOSITORY_FIND_SQL: &str = "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, provider, host_url, external_id, namespace, name, full_name, web_url, clone_url_http, clone_url_ssh, default_branch, is_private, metadata FROM git_repositories WHERE provider = $1 AND host_url = $2 AND LOWER(namespace) = LOWER($3) AND LOWER(name) = LOWER($4) AND deleted_at IS NULL ORDER BY id ASC LIMIT 1";

/// Project workspace for link creation (`ProjectBaseModel.save` derives
/// `workspace` from `project`, `project.py:302-311`).
///
/// Parameters: `$1` project id.
pub const REVIEW_PROJECT_WORKSPACE_SQL: &str =
    "SELECT workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL LIMIT 1";

/// Hard delete of the review link (`code_reviews.py:120`).
pub const LINK_DELETE_SQL: &str = "DELETE FROM git_code_review_links WHERE id = $1";

/// Hard delete of the legacy link (`code_reviews.py:123`).
pub const LEGACY_DELETE_SQL: &str = "DELETE FROM github_pull_request_links WHERE id = $1";

/// Provider snapshot taken from `get_code_review`
/// (`code_reviews.py:186-195`).
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewSnapshot {
    pub external_id: String,
    pub title: String,
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    pub remote_updated_at: Option<DateTime<Utc>>,
    pub metadata: Value,
}

impl ReviewSnapshot {
    /// Empty snapshot: every `except Exception` branch
    /// (`code_reviews.py:205-206`) and the no-account path.
    pub fn empty() -> Self {
        Self {
            external_id: String::new(),
            title: String::new(),
            state: "open".to_owned(),
            merged: false,
            draft: false,
            remote_updated_at: None,
            metadata: Value::Object(Default::default()),
        }
    }

    /// Snapshot from a fetched review (`code_reviews.py:187-195`): title
    /// truncated to 500 chars.
    pub fn from_review(review: &RemoteCodeReview) -> Self {
        Self {
            external_id: review.external_id.clone(),
            title: truncate_title(&review.title),
            state: review.state.clone(),
            merged: review.merged,
            draft: review.draft,
            remote_updated_at: review.updated_at.as_deref().and_then(parse_remote_dt),
            metadata: review.metadata.clone(),
        }
    }
}

/// New `git_code_review_links` row (`code_reviews.py:229-247`).
#[derive(Debug, Clone, PartialEq)]
pub struct NewCodeReviewLink {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub issue_id: Uuid,
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
    pub remote_updated_at: Option<DateTime<Utc>>,
    pub metadata: Value,
}

/// New `github_pull_request_links` row (`code_reviews.py:89-105`).
#[derive(Debug, Clone, PartialEq)]
pub struct NewLegacyLink {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub issue_id: Uuid,
    pub repo_owner: String,
    pub repo_name: String,
    pub pr_number: i32,
    pub url: String,
    pub title: String,
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    pub pr_updated_at: Option<DateTime<Utc>>,
}

/// Legacy-link field sync (`code_reviews.py:76-87` `update_fields`).
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyLinkUpdate {
    pub url: String,
    pub title: String,
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    pub pr_updated_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

/// Write failure for the link tables. `Conflict` is the `IntegrityError`
/// race (`code_reviews.py:248`): the caller rereads instead of failing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinkWriteError {
    /// Unique violation on insert; reread the row.
    #[error("conflicting link row")]
    Conflict,
    /// Any other database failure.
    #[error("{0}")]
    Store(#[from] StoreError),
}

/// Storage seam for the link-lifecycle closure.
///
/// Extends the services [`GitStore`] (account listing for
/// `_account_for_review` reuses `list_provider_accounts` over
/// [`PROVIDER_ACCOUNT_LIST_SQL`], with the host normalized via
/// [`normalize_host_url`]). Every other method mirrors one Django ORM
/// call in `code_reviews.py`; the SQL text lives in the adjacent `*_SQL`
/// consts, which the pool implementation must execute verbatim.
#[allow(async_fn_in_trait)]
pub trait CodeReviewStore: GitStore {
    /// [`ISSUE_EXISTS_SQL`].
    async fn review_issue_exists(
        &self,
        issue_id: Uuid,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<bool, StoreError>;

    /// [`LEGACY_LINK_FIND_SQL`].
    async fn find_legacy_link(
        &self,
        repo_owner_lower: &str,
        repo_name_lower: &str,
        pr_number: i32,
    ) -> Result<Option<GithubPullRequestLink>, StoreError>;

    /// [`PATH_REVIEW_LINK_FIND_SQL`].
    async fn find_path_review_link(
        &self,
        provider: &str,
        host_url: &str,
        namespace_lower: &str,
        repo_name_lower: &str,
        external_iid: &str,
    ) -> Result<Option<GitCodeReviewLink>, StoreError>;

    /// [`REPO_SCOPE_LINK_FIND_SQL`].
    async fn find_repo_scope_link(
        &self,
        provider: &str,
        host_url: &str,
        external_iid: &str,
        repo_external_id: &str,
    ) -> Result<Option<GitCodeReviewLink>, StoreError>;

    /// [`PATH_EMPTY_REPO_LINK_FIND_SQL`].
    async fn find_path_empty_repo_link(
        &self,
        provider: &str,
        host_url: &str,
        external_iid: &str,
        namespace_lower: &str,
        repo_name_lower: &str,
    ) -> Result<Option<GitCodeReviewLink>, StoreError>;

    /// [`BINDING_FOR_REVIEW_SQL`]: the joined provider account first,
    /// then its repository.
    async fn find_binding_review(
        &self,
        workspace_slug: &str,
        project_id: Uuid,
        provider: &str,
        host_url: &str,
        namespace: &str,
        repo_name: &str,
    ) -> Result<Option<(GitProviderAccount, GitRepository)>, StoreError>;

    /// [`REVIEW_ISSUE_WORKSPACE_SQL`].
    async fn find_review_issue_workspace(
        &self,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<Option<Uuid>, StoreError>;

    /// [`REVIEW_REPOSITORY_FIND_SQL`].
    async fn find_review_repository(
        &self,
        provider: &str,
        host_url: &str,
        namespace: &str,
        repo_name: &str,
    ) -> Result<Option<GitRepository>, StoreError>;

    /// [`REVIEW_PROJECT_WORKSPACE_SQL`].
    async fn find_review_project_workspace(
        &self,
        project_id: Uuid,
    ) -> Result<Option<Uuid>, StoreError>;

    /// [`CODE_REVIEW_LINK_INSERT_SQL`]. A unique violation surfaces as
    /// [`LinkWriteError::Conflict`] (the `IntegrityError` race).
    async fn insert_code_review_link(
        &self,
        row: NewCodeReviewLink,
    ) -> Result<GitCodeReviewLink, LinkWriteError>;

    /// [`LEGACY_LINK_INSERT_SQL`].
    async fn insert_legacy_link(
        &self,
        row: NewLegacyLink,
    ) -> Result<GithubPullRequestLink, StoreError>;

    /// [`LEGACY_LINK_UPDATE_SQL`].
    async fn update_legacy_link(
        &self,
        id: Uuid,
        update: LegacyLinkUpdate,
    ) -> Result<GithubPullRequestLink, StoreError>;

    /// [`LINK_DELETE_SQL`].
    async fn delete_code_review_link(&self, id: Uuid) -> Result<(), StoreError>;

    /// [`LEGACY_DELETE_SQL`].
    async fn delete_legacy_link(&self, id: Uuid) -> Result<(), StoreError>;
}

/// URL-parse seam: one `parse_code_review_url` per registered adapter
/// (`registry.py:29-42` first-match order). The blanket impl forwards, so
/// the real GitHub/GitLab adapters plug in unchanged.
pub trait ReviewParser {
    /// `parse_code_review_url` (`base.py:47-48`).
    fn parse_code_review_url(&self, url: &str) -> Option<ParsedCodeReview>;
}

impl<T: GitProviderAdapter> ReviewParser for T {
    fn parse_code_review_url(&self, url: &str) -> Option<ParsedCodeReview> {
        GitProviderAdapter::parse_code_review_url(self, url)
    }
}

/// Registry first-match parse (`registry.py:29-42`): each adapter in
/// registration order, first non-`None` result wins.
pub fn parse_review_url(
    parsers_in_order: &[&dyn ReviewParser],
    url: &str,
) -> Option<ParsedCodeReview> {
    parsers_in_order
        .iter()
        .find_map(|parser| parser.parse_code_review_url(url))
}

/// `get_code_review` seam (`base.py:82-83`). Only this member is needed
/// here; the blanket impl forwards so the real adapters plug in unchanged.
pub trait ReviewAdapter {
    /// `get_code_review` (`base.py:82-83`).
    fn get_code_review(
        &self,
        credential: &Value,
        parsed: &ParsedCodeReview,
    ) -> Result<RemoteCodeReview, GitProviderError>;
}

impl<T: GitProviderAdapter> ReviewAdapter for T {
    fn get_code_review(
        &self,
        credential: &Value,
        parsed: &ParsedCodeReview,
    ) -> Result<RemoteCodeReview, GitProviderError> {
        GitProviderAdapter::get_code_review(self, credential, parsed)
    }
}

/// Adapter-resolution seam (`get_adapter`, `registry.py:18-22`).
///
/// Returns the review-capable adapter for a parsed provider key, or the
/// `KeyError` when no adapter is registered. A trait (rather than a
/// closure) so the borrowed adapter outlives the call.
pub trait AdapterSource {
    /// `get_adapter(provider)` (`registry.py:18-22`).
    fn adapter_for(&self, provider: &str) -> Result<&dyn ReviewAdapter, UnknownProvider>;
}

/// Pure decision half of `_ensure_github_legacy_link` (`code_reviews.py:64-106`).
#[derive(Debug, Clone, PartialEq)]
pub enum LegacySyncAction {
    /// The update path (`code_reviews.py:71-87`): sync url/title/state
    /// (`CLOSED`/`MERGED` collapse, anything else `OPEN`), merged, draft,
    /// `remote_updated_at` → `pr_updated_at`.
    Update { id: Uuid, update: LegacyLinkUpdate },
    /// The create path (`code_reviews.py:89-105`): namespace/repo carried
    /// over verbatim (already lowercased at link creation).
    Create(NewLegacyLink),
    /// The row belongs to another issue (`code_reviews.py:73-74`).
    Conflict { issue_id: String },
}

/// [`LegacySyncAction`] for one review link and its existing legacy row.
pub fn legacy_sync_action(
    existing: Option<&GithubPullRequestLink>,
    link: &GitCodeReviewLink,
    pr_number: i32,
    now: DateTime<Utc>,
) -> LegacySyncAction {
    let state = legacy_state_for(&link.state).to_owned();
    match existing {
        Some(row) if row.issue_id != link.issue_id => LegacySyncAction::Conflict {
            issue_id: row.issue_id.to_string(),
        },
        Some(row) => LegacySyncAction::Update {
            id: row.id,
            update: LegacyLinkUpdate {
                url: link.url.clone(),
                title: link.title.clone(),
                state,
                merged: link.merged,
                draft: link.draft,
                pr_updated_at: link.remote_updated_at,
                updated_at: now,
            },
        },
        None => LegacySyncAction::Create(NewLegacyLink {
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            created_by_id: None,
            updated_by_id: None,
            project_id: link.project_id,
            workspace_id: link.workspace_id,
            issue_id: link.issue_id,
            repo_owner: link.namespace.clone(),
            repo_name: link.repo_name.clone(),
            pr_number,
            url: link.url.clone(),
            title: link.title.clone(),
            state,
            merged: link.merged,
            draft: link.draft,
            pr_updated_at: link.remote_updated_at,
        }),
    }
}

/// `_ensure_github_legacy_link` (`code_reviews.py:64-106`): no-op for
/// non-GitHub links; otherwise update-or-create the legacy row, raising
/// [`CodeReviewError::AlreadyLinked`] when it belongs to another issue.
///
/// The existing-row lookup uses the link's stored namespace/repo verbatim
/// (no `.lower()`, `code_reviews.py:68-72`); an unparsable `external_iid`
/// raises [`CodeReviewError::BadPrNumber`] like Python's uncaught
/// `ValueError`.
pub async fn ensure_github_legacy_link<S: CodeReviewStore>(
    store: &S,
    link: &GitCodeReviewLink,
    now: DateTime<Utc>,
) -> Result<(), CodeReviewError> {
    if link.provider != "github" {
        return Ok(());
    }
    let pr_number = parse_pr_number(&link.external_iid)
        .and_then(|number| i32::try_from(number).ok())
        .ok_or_else(|| CodeReviewError::BadPrNumber(link.external_iid.clone()))?;
    let existing = store
        .find_legacy_link(&link.namespace, &link.repo_name, pr_number)
        .await?;
    match legacy_sync_action(existing.as_ref(), link, pr_number, now) {
        LegacySyncAction::Conflict { issue_id } => Err(CodeReviewError::AlreadyLinked { issue_id }),
        LegacySyncAction::Update { id, update } => {
            store.update_legacy_link(id, update).await?;
            Ok(())
        }
        LegacySyncAction::Create(row) => {
            store.insert_legacy_link(row).await?;
            Ok(())
        }
    }
}

/// Detach pre-computation (`code_reviews.py:107-119`): the legacy lookup
/// key, or `None` when the link is not GitHub or its `external_iid` is
/// not an int (`try/except (TypeError, ValueError)`).
///
/// The stored namespace/repo go in verbatim — `detach` never lowercases
/// (`code_reviews.py:116-119`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetachPlan {
    pub legacy_lookup: Option<(String, String, i32)>,
}

/// [`DetachPlan`] for one review link.
pub fn detach_plan(link: &GitCodeReviewLink) -> DetachPlan {
    let legacy_lookup = if link.provider == "github" {
        parse_pr_number(&link.external_iid)
            .and_then(|number| i32::try_from(number).ok())
            .map(|number| (link.namespace.clone(), link.repo_name.clone(), number))
    } else {
        None
    };
    DetachPlan { legacy_lookup }
}

/// `detach_code_review_link` (`code_reviews.py:107-124`): resolve the
/// legacy row first, delete the link (hard), then delete the legacy row
/// only when it points at the same issue.
pub async fn detach_code_review_link<S: CodeReviewStore>(
    store: &S,
    link: &GitCodeReviewLink,
) -> Result<(), CodeReviewError> {
    let plan = detach_plan(link);
    let legacy = match plan.legacy_lookup {
        Some((owner, name, number)) => store.find_legacy_link(&owner, &name, number).await?,
        None => None,
    };
    store.delete_code_review_link(link.id).await?;
    if let Some(row) = legacy {
        if row.issue_id == link.issue_id {
            store.delete_legacy_link(row.id).await?;
        }
    }
    Ok(())
}

/// `_account_for_review` (`code_reviews.py:125-166`): the bound
/// `(account, repository)` pair, or the single-account fallback.
///
/// Returns `(None, None)` — without touching the repository lookup — when
/// no binding matches and the workspace has zero or several eligible
/// accounts (the `Required`/`Ambiguous` branches swallowed by the broad
/// `except`, `code_reviews.py:157-160`), or when no issue anchors the
/// project/workspace pair.
pub async fn account_for_review<S: CodeReviewStore>(
    store: &S,
    workspace_slug: &str,
    project_id: Uuid,
    parsed: &ParsedCodeReview,
) -> Result<(Option<GitProviderAccount>, Option<GitRepository>), CodeReviewError> {
    if let Some((account, repo)) = store
        .find_binding_review(
            workspace_slug,
            project_id,
            &parsed.provider,
            &parsed.host_url,
            &parsed.namespace,
            &parsed.repo_name,
        )
        .await?
    {
        return Ok((Some(account), Some(repo)));
    }
    let workspace_id = match store
        .find_review_issue_workspace(project_id, workspace_slug)
        .await?
    {
        Some(id) => id,
        None => return Ok((None, None)),
    };
    // `select_provider_account` with no id over the normalized-host
    // queryset (`services.py:118-138`): exactly one account resolves;
    // zero or several raise, which Python swallows into `(None, None)`.
    let mut accounts = store
        .list_provider_accounts(
            workspace_id,
            &parsed.provider,
            &normalize_host_url(&parsed.host_url),
        )
        .await?;
    if accounts.len() != 1 {
        return Ok((None, None));
    }
    let account = accounts.pop();
    let repo = store
        .find_review_repository(
            &parsed.provider,
            &parsed.host_url,
            &parsed.namespace,
            &parsed.repo_name,
        )
        .await?;
    Ok((account, repo))
}

/// Input for [`attach_code_review`] (`code_reviews.py:167-170`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachRequest {
    pub project_id: Uuid,
    pub issue_id: Uuid,
    pub workspace_slug: String,
    pub raw_url: String,
}

/// `attach_code_review` (`code_reviews.py:167-262`).
///
/// Returns the link and whether it was created (`True`) or already
/// existed (`False`, every early-return path). Raises
/// [`CodeReviewError::InvalidUrl`] for unparsable URLs,
/// [`CodeReviewError::IssueNotFound`] for unknown issues, and
/// [`CodeReviewError::AlreadyLinked`] whenever any of the four lookups
/// (legacy, path-scope, repo-scope, race reread) belongs to another
/// issue. Only provider `NotFound` propagates; every other
/// `get_code_review` failure yields an empty snapshot.
#[allow(clippy::too_many_lines)]
pub async fn attach_code_review<S: CodeReviewStore>(
    store: &S,
    parsers_in_order: &[&dyn ReviewParser],
    adapters: &dyn AdapterSource,
    request: &AttachRequest,
    now: DateTime<Utc>,
) -> Result<(GitCodeReviewLink, bool), CodeReviewError> {
    let parsed = parse_review_url(parsers_in_order, request.raw_url.trim())
        .ok_or(CodeReviewError::InvalidUrl)?;
    if !store
        .review_issue_exists(
            request.issue_id,
            request.project_id,
            &request.workspace_slug,
        )
        .await?
    {
        return Err(CodeReviewError::IssueNotFound);
    }

    if parsed.provider == "github" {
        let (owner, name, number) = legacy_lookup_key(&parsed)
            .ok_or_else(|| CodeReviewError::BadPrNumber(parsed.external_iid.clone()))?;
        if let Some(legacy) = store.find_legacy_link(&owner, &name, number).await? {
            if legacy.issue_id != request.issue_id {
                return Err(CodeReviewError::AlreadyLinked {
                    issue_id: legacy.issue_id.to_string(),
                });
            }
        }
    }

    let (account, repo) =
        account_for_review(store, &request.workspace_slug, request.project_id, &parsed).await?;
    let mut repo_external_id = repo.map(|repo| repo.external_id).unwrap_or_default();
    let mut snapshot = ReviewSnapshot::empty();
    if let Some(account) = &account {
        let adapter = adapters.adapter_for(&parsed.provider)?;
        let credential = account_credential(
            &account.credential_config,
            &account.auth_type,
            &account.host_url,
        );
        match adapter.get_code_review(&credential, &parsed) {
            Ok(review) => {
                if repo_external_id.is_empty() {
                    if let Some(project_id) = metadata_project_id(&review.metadata) {
                        repo_external_id = project_id;
                    }
                }
                snapshot = ReviewSnapshot::from_review(&review);
            }
            Err(err) if err.is_not_found() => {
                return Err(CodeReviewError::ProviderNotFound(err));
            }
            Err(_) => {}
        }
    }

    if let Some((provider, host, namespace, repo_name, iid)) = path_lookup_key(&parsed) {
        if let Some(existing) = store
            .find_path_review_link(provider, host, &namespace, &repo_name, iid)
            .await?
        {
            if existing.issue_id != request.issue_id {
                return Err(CodeReviewError::AlreadyLinked {
                    issue_id: existing.issue_id.to_string(),
                });
            }
            ensure_github_legacy_link(store, &existing, now).await?;
            return Ok((existing, false));
        }
    }

    let existing = if repo_external_id.is_empty() {
        store
            .find_path_empty_repo_link(
                &parsed.provider,
                &parsed.host_url,
                &parsed.external_iid,
                &parsed.namespace.to_lowercase(),
                &parsed.repo_name.to_lowercase(),
            )
            .await?
    } else {
        store
            .find_repo_scope_link(
                &parsed.provider,
                &parsed.host_url,
                &parsed.external_iid,
                &repo_external_id,
            )
            .await?
    };
    if let Some(existing) = existing {
        if existing.issue_id != request.issue_id {
            return Err(CodeReviewError::AlreadyLinked {
                issue_id: existing.issue_id.to_string(),
            });
        }
        ensure_github_legacy_link(store, &existing, now).await?;
        return Ok((existing, false));
    }

    let workspace_id = store
        .find_review_project_workspace(request.project_id)
        .await?
        .ok_or(StoreError::NotFound("project"))?;
    let row = NewCodeReviewLink {
        id: Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        created_by_id: None,
        updated_by_id: None,
        project_id: request.project_id,
        workspace_id,
        issue_id: request.issue_id,
        provider: parsed.provider.clone(),
        host_url: parsed.host_url.clone(),
        namespace: parsed.namespace.to_lowercase(),
        repo_name: parsed.repo_name.to_lowercase(),
        repo_external_id: repo_external_id.clone(),
        external_id: snapshot.external_id.clone(),
        external_iid: parsed.external_iid.clone(),
        url: parsed.url.clone(),
        title: snapshot.title.clone(),
        state: snapshot.state.clone(),
        merged: snapshot.merged,
        draft: snapshot.draft,
        remote_updated_at: snapshot.remote_updated_at,
        metadata: snapshot.metadata.clone(),
    };
    match store.insert_code_review_link(row).await {
        Ok(link) => {
            ensure_github_legacy_link(store, &link, now).await?;
            Ok((link, true))
        }
        Err(LinkWriteError::Store(err)) => Err(CodeReviewError::Store(err)),
        Err(LinkWriteError::Conflict) => {
            let reread = if repo_external_id.is_empty() {
                store
                    .find_path_empty_repo_link(
                        &parsed.provider,
                        &parsed.host_url,
                        &parsed.external_iid,
                        &parsed.namespace.to_lowercase(),
                        &parsed.repo_name.to_lowercase(),
                    )
                    .await?
            } else {
                store
                    .find_repo_scope_link(
                        &parsed.provider,
                        &parsed.host_url,
                        &parsed.external_iid,
                        &repo_external_id,
                    )
                    .await?
            };
            let existing = match reread {
                Some(row) => row,
                None => {
                    return Err(CodeReviewError::Store(StoreError::Db(
                        "link insert conflicted but the row vanished on reread".to_owned(),
                    )));
                }
            };
            if existing.issue_id != request.issue_id {
                return Err(CodeReviewError::AlreadyLinked {
                    issue_id: existing.issue_id.to_string(),
                });
            }
            ensure_github_legacy_link(store, &existing, now).await?;
            Ok((existing, false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::accounts::{PROVIDER_ACCOUNT_LIST_SQL, PROVIDER_ACCOUNT_SCOPE_SQL};
    use super::*;
    use pidash_db::integrations::git_models::git_provider_account;
    use std::sync::Mutex;

    fn golden() -> Value {
        let path = format!(
            "{}/../../fixtures/integrations/code_reviews.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn stamp() -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2024, 5, 1, 12, 0, 0).unwrap()
    }

    fn parsed() -> ParsedCodeReview {
        ParsedCodeReview {
            provider: "github".into(),
            host_url: "https://github.com".into(),
            namespace: "Octo".into(),
            repo_name: "Hello".into(),
            external_iid: "7".into(),
            url: "https://github.com/Octo/Hello/pull/7".into(),
        }
    }

    fn project_id() -> Uuid {
        Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()
    }

    fn workspace_id() -> Uuid {
        Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap()
    }

    fn issue_id() -> Uuid {
        Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap()
    }

    fn other_issue_id() -> Uuid {
        Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap()
    }

    fn probe_gitlab_account(workspace: Uuid) -> GitProviderAccount {
        let mut account = probe_account(workspace);
        account.provider = "gitlab".into();
        account
    }

    fn probe_account(workspace: Uuid) -> GitProviderAccount {
        GitProviderAccount {
            id: Uuid::new_v4(),
            created_at: stamp(),
            updated_at: stamp(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: workspace,
            provider: "github".into(),
            host_url: "https://github.com".into(),
            auth_type: "pat".into(),
            external_account_id: "octo".into(),
            external_account_login: "octo".into(),
            display_name: "octo".into(),
            capabilities: serde_json::json!({"write_comments": true}),
            credential_config: serde_json::json!({"token": "t"}),
            workspace_integration_id: None,
            status: git_provider_account::STATUS_CONNECTED.into(),
            verified_at: Some(stamp()),
            last_check_error: String::new(),
            metadata: serde_json::json!({}),
        }
    }

    fn probe_repo() -> GitRepository {
        GitRepository {
            id: Uuid::new_v4(),
            created_at: stamp(),
            updated_at: stamp(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            provider: "github".into(),
            host_url: "https://github.com".into(),
            external_id: "repo-9".into(),
            namespace: "Octo".into(),
            name: "Hello".into(),
            full_name: "Octo/Hello".into(),
            web_url: "https://github.com/Octo/Hello".into(),
            clone_url_http: String::new(),
            clone_url_ssh: String::new(),
            default_branch: "main".into(),
            is_private: false,
            metadata: serde_json::json!({}),
        }
    }

    fn probe_link(issue: Uuid) -> GitCodeReviewLink {
        GitCodeReviewLink {
            id: Uuid::new_v4(),
            created_at: stamp(),
            updated_at: stamp(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: project_id(),
            workspace_id: workspace_id(),
            issue_id: issue,
            provider: "github".into(),
            host_url: "https://github.com".into(),
            namespace: "octo".into(),
            repo_name: "hello".into(),
            repo_external_id: String::new(),
            external_id: "900".into(),
            external_iid: "7".into(),
            url: "https://github.com/Octo/Hello/pull/7".into(),
            title: "Fix it".into(),
            state: "open".into(),
            merged: false,
            draft: false,
            remote_updated_at: None,
            metadata: serde_json::json!({}),
        }
    }

    fn probe_legacy(issue: Uuid) -> GithubPullRequestLink {
        GithubPullRequestLink {
            id: Uuid::new_v4(),
            created_at: stamp(),
            updated_at: stamp(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: project_id(),
            workspace_id: workspace_id(),
            issue_id: issue,
            repo_owner: "octo".into(),
            repo_name: "hello".into(),
            pr_number: 7,
            url: "https://github.com/Octo/Hello/pull/7".into(),
            title: "Fix it".into(),
            state: "open".into(),
            merged: false,
            draft: false,
            pr_updated_at: None,
        }
    }

    fn probe_review() -> RemoteCodeReview {
        RemoteCodeReview {
            external_id: "900".into(),
            external_iid: "7".into(),
            title: "Fix it".into(),
            state: "merged".into(),
            merged: true,
            draft: false,
            web_url: "https://github.com/Octo/Hello/pull/7".into(),
            updated_at: Some("2024-05-02T03:04:05+00:00".into()),
            metadata: serde_json::json!({}),
        }
    }

    /// One bound `(account, repository)` pair plus its match parameters
    /// (workspace slug, project, provider, host, namespace, repo name).
    type BindingHit = (
        String,
        Uuid,
        String,
        String,
        String,
        String,
        GitProviderAccount,
        GitRepository,
    );

    /// In-memory [`CodeReviewStore`]: comparisons mirror the SQL consts
    /// (`LOWER()` both sides where the const lowercases, newest-first for
    /// the path lookup).
    struct MemStore {
        issues: Vec<(Uuid, Uuid, Uuid)>,
        workspaces: Vec<(String, Uuid)>,
        projects: Vec<(Uuid, Uuid)>,
        accounts: Mutex<Vec<GitProviderAccount>>,
        binding: Option<BindingHit>,
        repos: Mutex<Vec<GitRepository>>,
        legacy: Mutex<Vec<GithubPullRequestLink>>,
        links: Mutex<Vec<GitCodeReviewLink>>,
        conflict_on_insert: bool,
    }

    impl MemStore {
        fn basic() -> Self {
            Self {
                issues: vec![(issue_id(), project_id(), workspace_id())],
                workspaces: vec![("ws".to_owned(), workspace_id())],
                projects: vec![(project_id(), workspace_id())],
                accounts: Mutex::new(Vec::new()),
                binding: None,
                repos: Mutex::new(Vec::new()),
                legacy: Mutex::new(Vec::new()),
                links: Mutex::new(Vec::new()),
                conflict_on_insert: false,
            }
        }

        fn slug_of(&self, id: &Uuid) -> Option<String> {
            self.workspaces
                .iter()
                .find(|(_, wid)| wid == id)
                .map(|(slug, _)| slug.clone())
        }
    }

    impl GitStore for MemStore {
        async fn list_provider_accounts(
            &self,
            workspace_id: Uuid,
            provider: &str,
            host_url: &str,
        ) -> Result<Vec<GitProviderAccount>, StoreError> {
            Ok(self
                .accounts
                .lock()
                .unwrap()
                .iter()
                .filter(|account| {
                    account.workspace_id == workspace_id
                        && account.provider == provider
                        && account.host_url == host_url
                        && account.deleted_at.is_none()
                })
                .cloned()
                .collect())
        }

        async fn get_provider_account(
            &self,
            _workspace_id: Uuid,
            _provider: &str,
            _host_url: &str,
            _account_id: Uuid,
        ) -> Result<Option<GitProviderAccount>, StoreError> {
            unimplemented!("code-review tests never fetch a single account")
        }

        async fn insert_provider_account(
            &self,
            _row: super::super::accounts::NewProviderAccount,
        ) -> Result<GitProviderAccount, StoreError> {
            unimplemented!("code-review tests never insert accounts")
        }

        async fn find_repository_by_external(
            &self,
            _provider: &str,
            _host_url: &str,
            _external_id: &str,
        ) -> Result<Option<GitRepository>, StoreError> {
            unimplemented!("code-review tests use the review repository lookup")
        }

        async fn find_repository_by_full_name(
            &self,
            _provider: &str,
            _host_url: &str,
            _full_name: &str,
        ) -> Result<Option<GitRepository>, StoreError> {
            unimplemented!("code-review tests use the review repository lookup")
        }

        async fn update_repository(
            &self,
            _id: Uuid,
            _defaults: super::super::repositories::RepositoryDefaults,
            _now: DateTime<Utc>,
        ) -> Result<GitRepository, StoreError> {
            unimplemented!("code-review tests never update repositories")
        }

        async fn insert_repository(
            &self,
            _row: super::super::repositories::NewRepository,
        ) -> Result<GitRepository, StoreError> {
            unimplemented!("code-review tests never insert repositories")
        }

        async fn find_workspace_id(&self, _slug: &str) -> Result<Option<Uuid>, StoreError> {
            unimplemented!("code-review tests resolve workspaces through issues")
        }

        async fn find_project(
            &self,
            _workspace_id: Uuid,
            _project_id: Uuid,
        ) -> Result<Option<super::super::repositories::ProjectRef>, StoreError> {
            unimplemented!("code-review tests resolve project workspaces directly")
        }

        async fn apply_bind(
            &self,
            _plan: super::super::repositories::BindPlan,
        ) -> Result<GitRepositoryBindingAlias, StoreError> {
            unimplemented!("code-review tests never bind")
        }

        async fn get_binding(
            &self,
            _project_id: Uuid,
            _workspace_slug: &str,
        ) -> Result<Option<super::super::repositories::BindingView>, StoreError> {
            unimplemented!("code-review tests use the review binding lookup")
        }

        async fn set_binding_sync(
            &self,
            _binding_id: Uuid,
            _enabled: bool,
            _now: DateTime<Utc>,
        ) -> Result<GitRepositoryBindingAlias, StoreError> {
            unimplemented!("code-review tests never flip sync")
        }

        async fn set_github_syncs_enabled(
            &self,
            _project_id: Uuid,
            _workspace_slug: &str,
            _enabled: bool,
        ) -> Result<u64, StoreError> {
            unimplemented!("code-review tests never touch sync rows")
        }

        async fn delete_binding(&self, _binding_id: Uuid) -> Result<(), StoreError> {
            unimplemented!("code-review tests never delete bindings")
        }

        async fn delete_github_syncs_for_project(
            &self,
            _project_id: Uuid,
        ) -> Result<u64, StoreError> {
            unimplemented!("code-review tests never delete sync rows")
        }
    }

    use pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding as GitRepositoryBindingAlias;

    impl CodeReviewStore for MemStore {
        async fn review_issue_exists(
            &self,
            issue_id: Uuid,
            project_id: Uuid,
            workspace_slug: &str,
        ) -> Result<bool, StoreError> {
            Ok(self.issues.iter().any(|(iid, pid, wid)| {
                iid == &issue_id
                    && pid == &project_id
                    && self.slug_of(wid).as_deref() == Some(workspace_slug)
            }))
        }

        async fn find_legacy_link(
            &self,
            repo_owner_lower: &str,
            repo_name_lower: &str,
            pr_number: i32,
        ) -> Result<Option<GithubPullRequestLink>, StoreError> {
            Ok(self
                .legacy
                .lock()
                .unwrap()
                .iter()
                .find(|row| {
                    row.repo_owner == repo_owner_lower
                        && row.repo_name == repo_name_lower
                        && row.pr_number == pr_number
                        && row.deleted_at.is_none()
                })
                .cloned())
        }

        async fn find_path_review_link(
            &self,
            provider: &str,
            host_url: &str,
            namespace_lower: &str,
            repo_name_lower: &str,
            external_iid: &str,
        ) -> Result<Option<GitCodeReviewLink>, StoreError> {
            Ok(self
                .links
                .lock()
                .unwrap()
                .iter()
                .filter(|row| {
                    row.provider == provider
                        && row.host_url == host_url
                        && row.namespace == namespace_lower
                        && row.repo_name == repo_name_lower
                        && row.external_iid == external_iid
                        && row.deleted_at.is_none()
                })
                .max_by_key(|row| row.created_at)
                .cloned())
        }

        async fn find_repo_scope_link(
            &self,
            provider: &str,
            host_url: &str,
            external_iid: &str,
            repo_external_id: &str,
        ) -> Result<Option<GitCodeReviewLink>, StoreError> {
            Ok(self
                .links
                .lock()
                .unwrap()
                .iter()
                .find(|row| {
                    row.provider == provider
                        && row.host_url == host_url
                        && row.external_iid == external_iid
                        && row.repo_external_id == repo_external_id
                        && row.deleted_at.is_none()
                })
                .cloned())
        }

        async fn find_path_empty_repo_link(
            &self,
            provider: &str,
            host_url: &str,
            external_iid: &str,
            namespace_lower: &str,
            repo_name_lower: &str,
        ) -> Result<Option<GitCodeReviewLink>, StoreError> {
            Ok(self
                .links
                .lock()
                .unwrap()
                .iter()
                .find(|row| {
                    row.provider == provider
                        && row.host_url == host_url
                        && row.external_iid == external_iid
                        && row.namespace == namespace_lower
                        && row.repo_name == repo_name_lower
                        && row.repo_external_id.is_empty()
                        && row.deleted_at.is_none()
                })
                .cloned())
        }

        async fn find_binding_review(
            &self,
            workspace_slug: &str,
            project_id: Uuid,
            provider: &str,
            host_url: &str,
            namespace: &str,
            repo_name: &str,
        ) -> Result<Option<(GitProviderAccount, GitRepository)>, StoreError> {
            let hit = self.binding.as_ref().and_then(|binding| {
                let (slug, pid, bprovider, bhost, bns, bname, account, repo) = binding;
                (slug == workspace_slug
                    && pid == &project_id
                    && bprovider == provider
                    && bhost == host_url
                    && bns.to_lowercase() == namespace.to_lowercase()
                    && bname.to_lowercase() == repo_name.to_lowercase())
                .then(|| (account.clone(), repo.clone()))
            });
            Ok(hit)
        }

        async fn find_review_issue_workspace(
            &self,
            project_id: Uuid,
            workspace_slug: &str,
        ) -> Result<Option<Uuid>, StoreError> {
            Ok(self
                .issues
                .iter()
                .find(|(_, pid, wid)| {
                    pid == &project_id && self.slug_of(wid).as_deref() == Some(workspace_slug)
                })
                .map(|(_, _, wid)| *wid))
        }

        async fn find_review_repository(
            &self,
            provider: &str,
            host_url: &str,
            namespace: &str,
            repo_name: &str,
        ) -> Result<Option<GitRepository>, StoreError> {
            Ok(self
                .repos
                .lock()
                .unwrap()
                .iter()
                .find(|repo| {
                    repo.provider == provider
                        && repo.host_url == host_url
                        && repo.namespace.to_lowercase() == namespace.to_lowercase()
                        && repo.name.to_lowercase() == repo_name.to_lowercase()
                        && repo.deleted_at.is_none()
                })
                .cloned())
        }

        async fn find_review_project_workspace(
            &self,
            project_id: Uuid,
        ) -> Result<Option<Uuid>, StoreError> {
            Ok(self
                .projects
                .iter()
                .find(|(pid, _)| pid == &project_id)
                .map(|(_, wid)| *wid))
        }

        async fn insert_code_review_link(
            &self,
            row: NewCodeReviewLink,
        ) -> Result<GitCodeReviewLink, LinkWriteError> {
            if self.conflict_on_insert {
                return Err(LinkWriteError::Conflict);
            }
            let link = GitCodeReviewLink {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                created_by_id: row.created_by_id,
                updated_by_id: row.updated_by_id,
                deleted_at: None,
                project_id: row.project_id,
                workspace_id: row.workspace_id,
                issue_id: row.issue_id,
                provider: row.provider,
                host_url: row.host_url,
                namespace: row.namespace,
                repo_name: row.repo_name,
                repo_external_id: row.repo_external_id,
                external_id: row.external_id,
                external_iid: row.external_iid,
                url: row.url,
                title: row.title,
                state: row.state,
                merged: row.merged,
                draft: row.draft,
                remote_updated_at: row.remote_updated_at,
                metadata: row.metadata,
            };
            self.links.lock().unwrap().push(link.clone());
            Ok(link)
        }

        async fn insert_legacy_link(
            &self,
            row: NewLegacyLink,
        ) -> Result<GithubPullRequestLink, StoreError> {
            let link = GithubPullRequestLink {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                created_by_id: row.created_by_id,
                updated_by_id: row.updated_by_id,
                deleted_at: None,
                project_id: row.project_id,
                workspace_id: row.workspace_id,
                issue_id: row.issue_id,
                repo_owner: row.repo_owner,
                repo_name: row.repo_name,
                pr_number: row.pr_number,
                url: row.url,
                title: row.title,
                state: row.state,
                merged: row.merged,
                draft: row.draft,
                pr_updated_at: row.pr_updated_at,
            };
            self.legacy.lock().unwrap().push(link.clone());
            Ok(link)
        }

        async fn update_legacy_link(
            &self,
            id: Uuid,
            update: LegacyLinkUpdate,
        ) -> Result<GithubPullRequestLink, StoreError> {
            let mut legacy = self.legacy.lock().unwrap();
            let row = legacy
                .iter_mut()
                .find(|row| row.id == id)
                .ok_or_else(|| StoreError::Db("missing legacy link".into()))?;
            row.url = update.url;
            row.title = update.title;
            row.state = update.state;
            row.merged = update.merged;
            row.draft = update.draft;
            row.pr_updated_at = update.pr_updated_at;
            row.updated_at = update.updated_at;
            Ok(row.clone())
        }

        async fn delete_code_review_link(&self, id: Uuid) -> Result<(), StoreError> {
            self.links.lock().unwrap().retain(|row| row.id != id);
            Ok(())
        }

        async fn delete_legacy_link(&self, id: Uuid) -> Result<(), StoreError> {
            self.legacy.lock().unwrap().retain(|row| row.id != id);
            Ok(())
        }
    }

    struct StubParser(Option<ParsedCodeReview>);

    impl ReviewParser for StubParser {
        fn parse_code_review_url(&self, _url: &str) -> Option<ParsedCodeReview> {
            self.0.clone()
        }
    }

    enum ReviewOutcome {
        Review(Box<RemoteCodeReview>),
        NotFound,
        Other,
    }

    fn fetched(review: RemoteCodeReview) -> ReviewOutcome {
        ReviewOutcome::Review(Box::new(review))
    }

    struct StubAdapter(ReviewOutcome);

    impl ReviewAdapter for StubAdapter {
        fn get_code_review(
            &self,
            _credential: &Value,
            _parsed: &ParsedCodeReview,
        ) -> Result<RemoteCodeReview, GitProviderError> {
            match &self.0 {
                ReviewOutcome::Review(review) => Ok(review.as_ref().clone()),
                ReviewOutcome::NotFound => Err(GitProviderError::NotFound("gone".into())),
                ReviewOutcome::Other => Err(GitProviderError::General("boom".into())),
            }
        }
    }

    struct StubSource<'a> {
        adapter: Option<&'a StubAdapter>,
    }

    impl AdapterSource for StubSource<'_> {
        fn adapter_for(&self, provider: &str) -> Result<&dyn ReviewAdapter, UnknownProvider> {
            match self.adapter {
                Some(adapter) => Ok(adapter),
                None => Err(
                    pidash_types::integrations::registry::resolve_adapter_key(provider)
                        .unwrap_err(),
                ),
            }
        }
    }

    fn attach_request() -> AttachRequest {
        AttachRequest {
            project_id: project_id(),
            issue_id: issue_id(),
            workspace_slug: "ws".to_owned(),
            raw_url: "  https://github.com/Octo/Hello/pull/7  ".to_owned(),
        }
    }

    #[test]
    fn goldens_replay() {
        let gold = golden();
        // already_linked.message with the placeholder filled.
        let template = gold["already_linked"]["message"]
            .as_str()
            .expect("already_linked message");
        let rendered = template.replace("<issue_id>", &issue_id().to_string());
        assert_eq!(rendered, already_linked_message(&issue_id().to_string()));
        assert_eq!(
            gold["already_linked"]["source"].as_str().unwrap(),
            "integrations/git/code_reviews.py:30-35"
        );
        // invalid_url names the exception raised when parsing returns None.
        assert!(gold["invalid_url"]["error"]
            .as_str()
            .unwrap()
            .contains("InvalidCodeReviewURL"));
        assert_eq!(CodeReviewError::InvalidUrl.kind(), "InvalidCodeReviewURL");
        assert_eq!(CodeReviewError::InvalidUrl.to_string(), "");
        assert_eq!(CodeReviewError::IssueNotFound.to_string(), "");
        // State mapping: CLOSED/MERGED collapse (no MERGED on the legacy
        // table), everything else stays open.
        assert_eq!(
            gold["github_legacy_state_mapping"]["CLOSED_or_MERGED"],
            "closed"
        );
        assert_eq!(gold["github_legacy_state_mapping"]["OPEN"], "open");
        assert_eq!(legacy_state_for("closed"), "closed");
        assert_eq!(legacy_state_for("merged"), "closed");
        assert_eq!(legacy_state_for("open"), "open");
        assert_eq!(legacy_state_for("unknown"), "open");
    }

    #[test]
    fn already_linked_kinds_and_message() {
        let err = CodeReviewError::AlreadyLinked {
            issue_id: issue_id().to_string(),
        };
        assert_eq!(err.kind(), "CodeReviewAlreadyLinked");
        assert_eq!(
            err.to_string(),
            format!(
                "This code review is already linked to issue {}.",
                issue_id()
            )
        );
        assert_eq!(CodeReviewError::IssueNotFound.kind(), "IssueNotFound");
        assert_eq!(
            CodeReviewError::BadPrNumber("x".into()).kind(),
            "ValueError"
        );
    }

    #[test]
    fn parse_pr_number_matches_python_int() {
        assert_eq!(parse_pr_number("7"), Some(7));
        assert_eq!(parse_pr_number("  42 "), Some(42));
        assert_eq!(parse_pr_number("+5"), Some(5));
        assert_eq!(parse_pr_number("-3"), Some(-3));
        assert_eq!(parse_pr_number("1_2"), Some(12));
        assert_eq!(parse_pr_number("007"), Some(7));
        assert_eq!(parse_pr_number("abc"), None);
        assert_eq!(parse_pr_number(""), None);
        assert_eq!(parse_pr_number("   "), None);
        assert_eq!(parse_pr_number("1__2"), None);
        assert_eq!(parse_pr_number("_12"), None);
        assert_eq!(parse_pr_number("12_"), None);
        assert_eq!(parse_pr_number("1.5"), None);
        assert_eq!(parse_pr_number("99999999999999999999999"), None);
    }

    #[test]
    fn lookup_keys_lowercase_and_guard_provider() {
        let key = legacy_lookup_key(&parsed()).expect("github legacy key");
        assert_eq!(key, ("octo".to_owned(), "hello".to_owned(), 7));
        let probe = parsed();
        let (provider, host, ns, repo, iid) = path_lookup_key(&probe).expect("path key");
        assert_eq!(
            (provider, host, ns.as_str(), repo.as_str(), iid),
            ("github", "https://github.com", "octo", "hello", "7")
        );
        let mut gitlab = parsed();
        gitlab.provider = "gitlab".into();
        assert_eq!(legacy_lookup_key(&gitlab), None);
        assert_eq!(path_lookup_key(&gitlab), None);
        let mut bad = parsed();
        bad.external_iid = "nope".into();
        assert_eq!(legacy_lookup_key(&bad), None);
    }

    #[test]
    fn parse_review_url_first_match_wins() {
        let first = StubParser(None);
        let second = StubParser(Some(parsed()));
        let parsers: [&dyn ReviewParser; 2] = [&first, &second];
        assert_eq!(parse_review_url(&parsers, "https://x").unwrap(), parsed());
        let none: [&dyn ReviewParser; 1] = [&first];
        assert_eq!(parse_review_url(&none, "https://x"), None);
    }

    #[test]
    fn sql_shapes_match_python_querysets() {
        assert!(LEGACY_LINK_FIND_SQL.contains("FROM github_pull_request_links"));
        assert!(
            LEGACY_LINK_FIND_SQL.contains("repo_owner = $1 AND repo_name = $2 AND pr_number = $3")
        );
        assert!(LEGACY_LINK_FIND_SQL.contains("deleted_at IS NULL"));
        assert!(LEGACY_LINK_FIND_SQL.contains("ORDER BY id ASC LIMIT 1"));
        assert!(PATH_REVIEW_LINK_FIND_SQL.contains("FROM git_code_review_links"));
        assert!(PATH_REVIEW_LINK_FIND_SQL.contains("ORDER BY created_at DESC LIMIT 1"));
        assert!(REPO_SCOPE_LINK_FIND_SQL.contains("repo_external_id = $4"));
        assert!(PATH_EMPTY_REPO_LINK_FIND_SQL.contains("repo_external_id = ''"));
        assert!(LEGACY_LINK_UPDATE_SQL.starts_with("UPDATE github_pull_request_links SET"));
        assert!(LEGACY_LINK_UPDATE_SQL.contains("pr_updated_at = $7"));
        assert!(LEGACY_LINK_INSERT_SQL.starts_with("INSERT INTO github_pull_request_links"));
        assert!(CODE_REVIEW_LINK_INSERT_SQL.starts_with("INSERT INTO git_code_review_links"));
        assert!(ISSUE_EXISTS_SQL.contains("FROM issues"));
        assert!(ISSUE_EXISTS_SQL.contains("(SELECT id FROM workspaces WHERE slug = $3)"));
        assert!(BINDING_FOR_REVIEW_SQL.contains("LOWER(r.namespace) = LOWER($5)"));
        assert!(BINDING_FOR_REVIEW_SQL.contains("LOWER(r.name) = LOWER($6)"));
        assert!(REVIEW_REPOSITORY_FIND_SQL.contains("LOWER(namespace) = LOWER($3)"));
        assert!(REVIEW_PROJECT_WORKSPACE_SQL.contains("FROM projects WHERE id = $1"));
        assert!(LINK_DELETE_SQL.starts_with("DELETE FROM git_code_review_links"));
        assert!(LEGACY_DELETE_SQL.starts_with("DELETE FROM github_pull_request_links"));
        // The account half of `_account_for_review` reuses the services
        // queryset: workspace + provider + normalized host, connected or
        // degraded, live rows only.
        assert!(PROVIDER_ACCOUNT_LIST_SQL.contains(PROVIDER_ACCOUNT_SCOPE_SQL));
    }

    #[test]
    fn title_truncation_counts_chars() {
        assert_eq!(truncate_title("abc"), "abc");
        assert_eq!(truncate_title(&"a".repeat(600)).len(), 500);
        let wide = "é".repeat(600);
        let cut = truncate_title(&wide);
        assert_eq!(cut.chars().count(), 500);
        assert_eq!(cut, "é".repeat(500));
    }

    #[test]
    fn metadata_project_id_follows_truthiness() {
        assert_eq!(
            metadata_project_id(&serde_json::json!({"project_id": "42"})),
            Some("42".to_owned())
        );
        assert_eq!(
            metadata_project_id(&serde_json::json!({"project_id": 42})),
            Some("42".to_owned())
        );
        assert_eq!(
            metadata_project_id(&serde_json::json!({"project_id": true})),
            Some("True".to_owned())
        );
        assert_eq!(metadata_project_id(&serde_json::json!({})), None);
        assert_eq!(
            metadata_project_id(&serde_json::json!({"project_id": ""})),
            None
        );
        assert_eq!(
            metadata_project_id(&serde_json::json!({"project_id": 0})),
            None
        );
        assert_eq!(
            metadata_project_id(&serde_json::json!({"project_id": null})),
            None
        );
        assert_eq!(
            metadata_project_id(&serde_json::json!({"project_id": false})),
            None
        );
    }

    #[test]
    fn snapshot_from_review_truncates_and_parses_dt() {
        let mut review = probe_review();
        review.title = "t".repeat(600);
        let snapshot = ReviewSnapshot::from_review(&review);
        assert_eq!(snapshot.title.chars().count(), 500);
        assert_eq!(snapshot.state, "merged");
        assert!(snapshot.merged);
        assert_eq!(
            snapshot.remote_updated_at.map(|dt| dt.to_rfc3339()),
            Some("2024-05-02T03:04:05+00:00".to_owned())
        );
        let empty = ReviewSnapshot::empty();
        assert_eq!(empty.state, "open");
        assert_eq!(empty.metadata, serde_json::json!({}));
    }

    #[test]
    fn legacy_sync_action_branches() {
        let mut link = probe_link(issue_id());
        link.state = "merged".into();
        link.title = "New".into();
        link.merged = true;
        // Conflict: legacy row on another issue.
        let foreign = probe_legacy(other_issue_id());
        match legacy_sync_action(Some(&foreign), &link, 7, stamp()) {
            LegacySyncAction::Conflict { issue_id } => {
                assert_eq!(issue_id, other_issue_id().to_string())
            }
            _ => panic!("expected conflict"),
        }
        // Update: same issue, state collapses merged -> closed.
        let own = probe_legacy(issue_id());
        match legacy_sync_action(Some(&own), &link, 7, stamp()) {
            LegacySyncAction::Update { id, update } => {
                assert_eq!(id, own.id);
                assert_eq!(update.state, "closed");
                assert_eq!(update.title, "New");
                assert!(update.merged);
                assert_eq!(update.updated_at, stamp());
            }
            _ => panic!("expected update"),
        }
        // Create: namespace carried over verbatim.
        match legacy_sync_action(None, &link, 7, stamp()) {
            LegacySyncAction::Create(row) => {
                assert_eq!(row.repo_owner, "octo");
                assert_eq!(row.pr_number, 7);
                assert_eq!(row.state, "closed");
                assert_eq!(row.issue_id, issue_id());
            }
            _ => panic!("expected create"),
        }
    }

    #[tokio::test]
    async fn ensure_noop_for_non_github() {
        let store = MemStore::basic();
        let mut link = probe_link(issue_id());
        link.provider = "gitlab".into();
        ensure_github_legacy_link(&store, &link, stamp())
            .await
            .unwrap();
        assert!(store.legacy.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn ensure_update_syncs_and_conflict_raises() {
        let store = MemStore::basic();
        store.legacy.lock().unwrap().push(probe_legacy(issue_id()));
        let mut link = probe_link(issue_id());
        link.state = "merged".into();
        link.title = "Synced".into();
        link.merged = true;
        ensure_github_legacy_link(&store, &link, stamp())
            .await
            .unwrap();
        {
            let rows = store.legacy.lock().unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].state, "closed");
            assert_eq!(rows[0].title, "Synced");
            assert!(rows[0].merged);
        }

        let store = MemStore::basic();
        store
            .legacy
            .lock()
            .unwrap()
            .push(probe_legacy(other_issue_id()));
        let err = ensure_github_legacy_link(&store, &probe_link(issue_id()), stamp())
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "This code review is already linked to issue {}.",
                other_issue_id()
            )
        );
    }

    #[tokio::test]
    async fn attach_rejects_bad_url_and_unknown_issue() {
        let store = MemStore::basic();
        let no_parser = StubParser(None);
        let parsers: [&dyn ReviewParser; 1] = [&no_parser];
        let source = StubSource { adapter: None };
        let err = attach_code_review(&store, &parsers, &source, &attach_request(), stamp())
            .await
            .unwrap_err();
        assert_eq!(err.kind(), "InvalidCodeReviewURL");

        let store = MemStore::basic();
        let parser = StubParser(Some(parsed()));
        let parsers: [&dyn ReviewParser; 1] = [&parser];
        let mut request = attach_request();
        request.issue_id = Uuid::new_v4();
        let err = attach_code_review(&store, &parsers, &source, &request, stamp())
            .await
            .unwrap_err();
        assert_eq!(err.kind(), "IssueNotFound");
    }

    #[tokio::test]
    async fn attach_rejects_legacy_conflict_with_exact_message() {
        let store = MemStore::basic();
        store
            .legacy
            .lock()
            .unwrap()
            .push(probe_legacy(other_issue_id()));
        let parser = StubParser(Some(parsed()));
        let parsers: [&dyn ReviewParser; 1] = [&parser];
        let source = StubSource { adapter: None };
        let err = attach_code_review(&store, &parsers, &source, &attach_request(), stamp())
            .await
            .unwrap_err();
        assert_eq!(err.kind(), "CodeReviewAlreadyLinked");
        assert_eq!(
            err.to_string(),
            format!(
                "This code review is already linked to issue {}.",
                other_issue_id()
            )
        );
    }

    #[tokio::test]
    async fn attach_creates_link_with_snapshot_and_legacy() {
        let mut store = MemStore::basic();
        let account = probe_account(workspace_id());
        let repo = probe_repo();
        store.binding = Some((
            "ws".to_owned(),
            project_id(),
            "github".to_owned(),
            "https://github.com".to_owned(),
            "OCTO".to_owned(),
            "hello".to_owned(),
            account,
            repo,
        ));
        let mut review = probe_review();
        review.title = "t".repeat(600);
        review.metadata = serde_json::json!({"project_id": "123"});
        let adapter = StubAdapter(fetched(review));
        let source = StubSource {
            adapter: Some(&adapter),
        };
        let parser = StubParser(Some(parsed()));
        let parsers: [&dyn ReviewParser; 1] = [&parser];

        let (link, created) =
            attach_code_review(&store, &parsers, &source, &attach_request(), stamp())
                .await
                .unwrap();
        assert!(created);
        assert_eq!(link.namespace, "octo");
        assert_eq!(link.repo_name, "hello");
        assert_eq!(link.title.chars().count(), 500);
        assert_eq!(link.state, "merged");
        assert!(link.merged);
        // The bound repository's external id wins; review metadata only
        // fills an empty repo_external_id (code_reviews.py:197-198).
        assert_eq!(link.repo_external_id, "repo-9");
        assert_eq!(link.external_id, "900");
        assert_eq!(link.issue_id, issue_id());
        let legacy = store.legacy.lock().unwrap();
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].state, "closed");
        assert_eq!(legacy[0].pr_number, 7);
        assert_eq!(legacy[0].title.chars().count(), 500);
    }

    #[tokio::test]
    async fn attach_returns_existing_path_link_without_creating() {
        let store = MemStore::basic();
        store.links.lock().unwrap().push(probe_link(issue_id()));
        store
            .accounts
            .lock()
            .unwrap()
            .push(probe_account(workspace_id()));
        let adapter = StubAdapter(ReviewOutcome::Other);
        let source = StubSource {
            adapter: Some(&adapter),
        };
        let parser = StubParser(Some(parsed()));
        let parsers: [&dyn ReviewParser; 1] = [&parser];

        let (link, created) =
            attach_code_review(&store, &parsers, &source, &attach_request(), stamp())
                .await
                .unwrap();
        assert!(!created);
        assert_eq!(link.title, "Fix it");
        assert_eq!(store.links.lock().unwrap().len(), 1);
        // The existing row still syncs its legacy shadow.
        assert_eq!(store.legacy.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn attach_rejects_path_and_scope_conflicts() {
        // Path-scope row on another issue.
        let store = MemStore::basic();
        store
            .links
            .lock()
            .unwrap()
            .push(probe_link(other_issue_id()));
        let parser = StubParser(Some(parsed()));
        let parsers: [&dyn ReviewParser; 1] = [&parser];
        let source = StubSource { adapter: None };
        let err = attach_code_review(&store, &parsers, &source, &attach_request(), stamp())
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "This code review is already linked to issue {}.",
                other_issue_id()
            )
        );

        // Repo-scope row on another issue (GitLab path skips the
        // path-scope lookup). The single account lets the review fetch
        // run, so metadata fills the empty repo_external_id.
        let store = MemStore::basic();
        store
            .accounts
            .lock()
            .unwrap()
            .push(probe_gitlab_account(workspace_id()));
        let mut row = probe_link(other_issue_id());
        row.provider = "gitlab".into();
        row.repo_external_id = "123".into();
        store.links.lock().unwrap().push(row);
        let mut gitlab = parsed();
        gitlab.provider = "gitlab".into();
        let parser = StubParser(Some(gitlab));
        let parsers: [&dyn ReviewParser; 1] = [&parser];
        let mut review = probe_review();
        review.metadata = serde_json::json!({"project_id": "123"});
        let adapter = StubAdapter(fetched(review));
        let source = StubSource {
            adapter: Some(&adapter),
        };
        let mut request = attach_request();
        request.raw_url = "https://gitlab.example.com/g/n!5".into();
        let err = attach_code_review(&store, &parsers, &source, &request, stamp())
            .await
            .unwrap_err();
        assert_eq!(err.kind(), "CodeReviewAlreadyLinked");
    }

    #[tokio::test]
    async fn attach_fills_repo_external_id_from_review_metadata() {
        // No bound repo and no stored repo: metadata project_id fills the
        // empty repo_external_id on create (code_reviews.py:197-198).
        let store = MemStore::basic();
        store
            .accounts
            .lock()
            .unwrap()
            .push(probe_gitlab_account(workspace_id()));
        let mut gitlab = parsed();
        gitlab.provider = "gitlab".into();
        let parser = StubParser(Some(gitlab));
        let parsers: [&dyn ReviewParser; 1] = [&parser];
        let mut review = probe_review();
        review.metadata = serde_json::json!({"project_id": "123"});
        let adapter = StubAdapter(fetched(review));
        let source = StubSource {
            adapter: Some(&adapter),
        };
        let mut request = attach_request();
        request.raw_url = "https://gitlab.example.com/g/n!5".into();
        let (link, created) = attach_code_review(&store, &parsers, &source, &request, stamp())
            .await
            .unwrap();
        assert!(created);
        assert_eq!(link.provider, "gitlab");
        assert_eq!(link.repo_external_id, "123");
        // GitLab links never grow a legacy shadow.
        assert!(store.legacy.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn attach_rereads_after_conflict() {
        // Same-issue row appears between insert and reread.
        let mut store = MemStore::basic();
        store.conflict_on_insert = true;
        store.links.lock().unwrap().push(probe_link(issue_id()));
        let parser = StubParser(Some(parsed()));
        let parsers: [&dyn ReviewParser; 1] = [&parser];
        let source = StubSource { adapter: None };
        let (link, created) =
            attach_code_review(&store, &parsers, &source, &attach_request(), stamp())
                .await
                .unwrap();
        assert!(!created);
        assert_eq!(link.issue_id, issue_id());

        // Another issue's row wins the race instead.
        let mut store = MemStore::basic();
        store.conflict_on_insert = true;
        store
            .links
            .lock()
            .unwrap()
            .push(probe_link(other_issue_id()));
        let err = attach_code_review(&store, &parsers, &source, &attach_request(), stamp())
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "This code review is already linked to issue {}.",
                other_issue_id()
            )
        );
    }

    #[tokio::test]
    async fn attach_propagates_not_found_but_swallows_other_errors() {
        let store = MemStore::basic();
        store
            .accounts
            .lock()
            .unwrap()
            .push(probe_account(workspace_id()));
        let parser = StubParser(Some(parsed()));
        let parsers: [&dyn ReviewParser; 1] = [&parser];

        let adapter = StubAdapter(ReviewOutcome::NotFound);
        let source = StubSource {
            adapter: Some(&adapter),
        };
        let err = attach_code_review(&store, &parsers, &source, &attach_request(), stamp())
            .await
            .unwrap_err();
        assert_eq!(err.kind(), "GitProviderNotFoundError");

        let adapter = StubAdapter(ReviewOutcome::Other);
        let source = StubSource {
            adapter: Some(&adapter),
        };
        let (link, created) =
            attach_code_review(&store, &parsers, &source, &attach_request(), stamp())
                .await
                .unwrap();
        assert!(created);
        assert_eq!(link.title, "");
        assert_eq!(link.state, "open");
    }

    #[tokio::test]
    async fn attach_without_account_links_with_defaults() {
        // Zero accounts: (None, None), still creates with empty snapshot.
        let store = MemStore::basic();
        let parser = StubParser(Some(parsed()));
        let parsers: [&dyn ReviewParser; 1] = [&parser];
        let source = StubSource { adapter: None };
        let (link, created) =
            attach_code_review(&store, &parsers, &source, &attach_request(), stamp())
                .await
                .unwrap();
        assert!(created);
        assert_eq!(link.title, "");
        assert_eq!(link.state, "open");
        assert_eq!(link.repo_external_id, "");

        // Several accounts: same (None, None) swallowing.
        let store = MemStore::basic();
        store
            .accounts
            .lock()
            .unwrap()
            .push(probe_account(workspace_id()));
        store
            .accounts
            .lock()
            .unwrap()
            .push(probe_account(workspace_id()));
        let (link, created) =
            attach_code_review(&store, &parsers, &source, &attach_request(), stamp())
                .await
                .unwrap();
        assert!(created);
        assert_eq!(link.title, "");
    }

    #[tokio::test]
    async fn account_for_review_prefers_binding_then_single_account() {
        // Binding hit returns its own pair without consulting accounts.
        let mut store = MemStore::basic();
        let account = probe_account(workspace_id());
        let repo = probe_repo();
        store.binding = Some((
            "ws".to_owned(),
            project_id(),
            "github".to_owned(),
            "https://github.com".to_owned(),
            "octo".to_owned(),
            "HELLO".to_owned(),
            account.clone(),
            repo.clone(),
        ));
        let (found_account, found_repo) = account_for_review(&store, "ws", project_id(), &parsed())
            .await
            .unwrap();
        assert_eq!(found_account.map(|a| a.id), Some(account.id));
        assert_eq!(found_repo.map(|r| r.id), Some(repo.id));

        // Fallback: single account plus case-insensitive repo.
        let store = MemStore::basic();
        store
            .accounts
            .lock()
            .unwrap()
            .push(probe_account(workspace_id()));
        store.repos.lock().unwrap().push(probe_repo());
        let mut mixed = parsed();
        mixed.namespace = "OCTO".into();
        let (found_account, found_repo) = account_for_review(&store, "ws", project_id(), &mixed)
            .await
            .unwrap();
        assert!(found_account.is_some());
        assert!(found_repo.is_some());

        // No issue anchors the pair: (None, None).
        let store = MemStore::basic();
        let (account, repo) = account_for_review(&store, "ws", project_id(), &parsed())
            .await
            .unwrap();
        assert!(account.is_none());
        assert!(repo.is_none());
    }

    #[tokio::test]
    async fn detach_deletes_link_and_matching_legacy_only() {
        // Same issue: both rows go.
        let store = MemStore::basic();
        let link = probe_link(issue_id());
        let legacy = probe_legacy(issue_id());
        store.links.lock().unwrap().push(link.clone());
        store.legacy.lock().unwrap().push(legacy.clone());
        detach_code_review_link(&store, &link).await.unwrap();
        assert!(store.links.lock().unwrap().is_empty());
        assert!(store.legacy.lock().unwrap().is_empty());

        // Foreign legacy row survives.
        let store = MemStore::basic();
        let link = probe_link(issue_id());
        store.links.lock().unwrap().push(link.clone());
        store
            .legacy
            .lock()
            .unwrap()
            .push(probe_legacy(other_issue_id()));
        detach_code_review_link(&store, &link).await.unwrap();
        assert!(store.links.lock().unwrap().is_empty());
        assert_eq!(store.legacy.lock().unwrap().len(), 1);

        // Non-GitHub links never touch the legacy table.
        let store = MemStore::basic();
        let mut link = probe_link(issue_id());
        link.provider = "gitlab".into();
        store.links.lock().unwrap().push(link.clone());
        store.legacy.lock().unwrap().push(probe_legacy(issue_id()));
        detach_code_review_link(&store, &link).await.unwrap();
        assert!(store.links.lock().unwrap().is_empty());
        assert_eq!(store.legacy.lock().unwrap().len(), 1);

        // Unparsable iid skips the legacy lookup like the try/except.
        let store = MemStore::basic();
        let mut link = probe_link(issue_id());
        link.external_iid = "nope".into();
        store.links.lock().unwrap().push(link.clone());
        store.legacy.lock().unwrap().push(probe_legacy(issue_id()));
        detach_code_review_link(&store, &link).await.unwrap();
        assert_eq!(store.legacy.lock().unwrap().len(), 1);
    }

    #[test]
    fn detach_plan_never_lowercases() {
        let mut link = probe_link(issue_id());
        link.namespace = "Octo".into();
        link.repo_name = "Hello".into();
        assert_eq!(
            detach_plan(&link).legacy_lookup,
            Some(("Octo".to_owned(), "Hello".to_owned(), 7))
        );
        link.provider = "gitlab".into();
        assert_eq!(detach_plan(&link).legacy_lookup, None);
    }
}
