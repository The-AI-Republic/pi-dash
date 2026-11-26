//! A project's repository list (NEWPIDASH-409).
//!
//! A project holds a list of repositories in the `repos` key of the
//! typed [`ProjectSettings`](super::project_settings::ProjectSettings)
//! bag, merged with the legacy `projects.repo_url` / `base_branch`
//! columns until NEWPIDASH-416 drops them:
//!
//! ```json
//! "repos": [
//!   {
//!     "id": "<uuid>",
//!     "url": "https://github.com/acme/web",
//!     "provider": "github",
//!     "base_branch": null
//!   }
//! ]
//! ```
//!
//! - A missing key reads as `[]` (the `#[serde(default)]` rule).
//! - `provider` is inferred from the URL host when the caller leaves
//!   it out (`github.com` → `github`, `gitlab.com` → `gitlab`,
//!   anything else → `other`); it stays an open string so a new
//!   provider needs no migration.
//! - `base_branch` uses the legacy column's character rule
//!   (`^[A-Za-z0-9._/-]*$`); `null`/missing means the repo's own
//!   default branch, stored as `""` in the legacy column.
//! - `settings.repos` is independent of `git_repository_bindings`
//!   (the Repository tab's one-per-project binding); that decision is
//!   deferred to NEWPIDASH-416.
//!
//! URL identity ([`repo_identity_key`]) is the one comparison key used
//! everywhere: the merged read, dedup on write, and double delete. It
//! folds a trailing `.git` or `/`, `https` vs. scp-style ssh
//! (`git@host:owner/name`), and host case (plus owner/name case for
//! GitHub, which ignores it). GitHub owner/name parsing reuses the
//! adapter's [`parse_github_repo_url`](crate::integrations::adapters_github::parse_github_repo_url);
//! other hosts reuse the GitLab adapter's
//! [`split_full_path`](crate::integrations::adapters_gitlab::split_full_path)
//! path rules (`.git` strip, subgroup split at the last slash, view
//! suffixes cut at `/-/`).
//!
//! Writes reach the list through the settings PATCH (full-list
//! replace) or the legacy `repo_url` PATCH; the pure sync helpers
//! below keep the two sides consistent so the temporary copy never
//! resurrects a removed repo:
//!
//! - [`sync_columns_after_replace`]: after a settings-PATCH replace,
//!   derive the legacy column write (follow a renamed URL, clear on
//!   remove).
//! - [`sync_entry_after_legacy_patch`]: after a legacy PATCH, update
//!   or drop the matching `settings.repos` entry.
//!
//! [`copy_legacy_repo_if_missing`] is TEMPORARY (NEWPIDASH-416): the
//! one-shot migration of a legacy-only repo into `settings.repos` on
//! first read.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::project_settings::SettingsError;
use crate::integrations::adapters_github::parse_github_repo_url;
use crate::integrations::adapters_gitlab::{split_full_path, strip_git_suffix};

/// The settings key holding the repository list.
pub const REPOS_KEY: &str = "repos";

/// Provider value for `github.com` repos.
pub const PROVIDER_GITHUB: &str = "github";
/// Provider value for `gitlab.com` repos.
pub const PROVIDER_GITLAB: &str = "gitlab";
/// Provider value for any other host; also the inference fallback.
pub const PROVIDER_OTHER: &str = "other";

/// Every accepted provider value. A new provider extends this list;
/// no migration is needed (the column is JSON).
const KNOWN_PROVIDERS: &[&str] = &[PROVIDER_GITHUB, PROVIDER_GITLAB, PROVIDER_OTHER];

/// Bare hosts with a non-`other` inference.
const GITHUB_BARE_HOST: &str = "github.com";
const GITLAB_BARE_HOST: &str = "gitlab.com";

/// `repo_url` bound, mirroring the legacy column's 512-char limit.
pub const REPO_URL_MAX_LEN: usize = 512;
/// `base_branch` bound, mirroring the legacy column's 128-char limit.
pub const BASE_BRANCH_MAX_LEN: usize = 128;

/// One entry of the `repos` settings list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRepo {
    /// Stable id, used to edit and remove one entry.
    pub id: Uuid,
    /// The repository URL, exactly as stored.
    pub url: String,
    /// `github` | `gitlab` | `other`; `None` infers from the host.
    #[serde(default)]
    pub provider: Option<String>,
    /// `None` (or empty) means the repo's own default branch.
    #[serde(default)]
    pub base_branch: Option<String>,
}

/// The wire shape of one merged entry: the stored entry with the
/// provider filled in, so readers never infer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedRepo {
    /// Stable id, used to edit and remove one entry.
    pub id: Uuid,
    /// The repository URL, exactly as stored.
    pub url: String,
    /// Always filled (`github` | `gitlab` | `other`).
    pub provider: String,
    /// `None` means the repo's own default branch.
    #[serde(default)]
    pub base_branch: Option<String>,
}

/// Fill in the provider every stored entry left out.
pub fn render_repos(stored: &[ProjectRepo]) -> Vec<RenderedRepo> {
    stored
        .iter()
        .map(|entry| RenderedRepo {
            id: entry.id,
            url: entry.url.clone(),
            provider: entry
                .provider
                .clone()
                .unwrap_or_else(|| infer_provider(&entry.url).to_owned()),
            base_branch: entry.base_branch.clone(),
        })
        .collect()
}

/// Read the `repos` list off a stored settings blob. Entries that do
/// not parse are skipped (writes validate, so this only meets
/// out-of-band blobs); a missing or non-array key reads as `[]`.
pub fn repos_from_stored(settings: &Value) -> Vec<ProjectRepo> {
    let Some(Value::Array(entries)) = settings.get(REPOS_KEY) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| serde_json::from_value::<ProjectRepo>(entry.clone()).ok())
        .collect()
}

// ---------------------------------------------------------------------------
// URL identity
// ---------------------------------------------------------------------------

/// Split a trimmed, whitespace-free candidate into its lowercase host
/// and its raw path part: `(authority → host, path)` for
/// `scheme://` URLs, `([user@]host, path)` for scp-like
/// `host:path`. Returns `None` for anything that is neither.
fn split_repo_location(candidate: &str) -> Option<(String, String)> {
    if let Some((scheme, rest)) = candidate.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" && scheme != "ssh" {
            return None;
        }
        let (authority, path) = match rest.find('/') {
            Some(index) => (rest[..index].to_owned(), rest[index..].to_owned()),
            None => (rest.to_owned(), String::new()),
        };
        Some((bare_host(&authority)?, path))
    } else {
        let (left, path) = candidate.split_once(':')?;
        if left.is_empty()
            || left.contains('/')
            || path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
        {
            return None;
        }
        // `rsplit('@')`: the part after the LAST `@`, as the GitLab
        // adapter's userinfo strip.
        let host = left.rsplit('@').next().unwrap_or("").to_lowercase();
        if host.is_empty() {
            return None;
        }
        Some((host, path.to_owned()))
    }
}

/// Lowercase an authority: strip userinfo, drop a default port. Any
/// other port stays part of the key.
fn bare_host(authority: &str) -> Option<String> {
    if authority.is_empty() {
        return None;
    }
    let host_port = authority.rsplit('@').next().unwrap_or("");
    if host_port.is_empty() {
        return None;
    }
    let lower = host_port.to_lowercase();
    let bare = lower
        .strip_suffix(":443")
        .or_else(|| lower.strip_suffix(":80"))
        .unwrap_or(&lower);
    if bare.is_empty() {
        return None;
    }
    Some(bare.to_owned())
}

/// Normalize a non-GitHub path with the GitLab adapter's rules:
/// `/-/` view suffixes address the repo at their head, one trailing
/// `.git` is stripped, subgroups split at the last slash. Query and
/// fragment never belong to a repo address. Returns the
/// case-preserved `(namespace, name)`.
fn normalize_repo_path(path: &str) -> Option<(String, String)> {
    let head = path.split_once("/-/").map(|(head, _)| head).unwrap_or(path);
    let head = head.split(['?', '#']).next().unwrap_or("");
    let clean = strip_git_suffix(head.trim_matches('/')).trim_matches('/');
    if clean.is_empty() || clean.contains(':') || clean.split('/').any(str::is_empty) {
        return None;
    }
    let (namespace, name) = split_full_path(clean)?;
    if namespace.is_empty() || name.is_empty() {
        return None;
    }
    Some((namespace, name))
}

/// Whether a project create request carries any repo: a non-empty
/// `repo_url`, or a non-empty `repos` list in the validated settings
/// fragment. Drives the `software` default work type (NEWPIDASH-409).
pub fn create_request_has_repo(repo_url: Option<&str>, settings: Option<&Value>) -> bool {
    if repo_url.is_some_and(|url| !url.is_empty()) {
        return true;
    }
    settings
        .and_then(|bag| bag.get(REPOS_KEY))
        .and_then(Value::as_array)
        .is_some_and(|entries| !entries.is_empty())
}

/// The URL identity (dedup) key: one function mapping a URL to the
/// key used for every comparison — merge, dedup on write, and double
/// delete. A trailing `.git` or `/`, `https` vs. scp-style ssh
/// (`git@host:owner/name`), and host case (and owner/name case for
/// GitHub) all map to the same key. Returns `None` for anything that
/// is not a usable repository URL.
pub fn repo_identity_key(url: &str) -> Option<String> {
    let candidate = url.trim();
    if candidate.is_empty()
        || candidate
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return None;
    }
    let (host, path) = split_repo_location(candidate)?;
    if host == GITHUB_BARE_HOST {
        // The adapter's exact owner/name rules (lowercase scheme and
        // host first: the adapter's lowercase-only scheme prefix is a
        // ported regex quirk, while identity folds case).
        let canonical = if candidate.contains("://") {
            format!("https://{host}{path}")
        } else {
            format!("git@{host}:{path}")
        };
        let (owner, name) = parse_github_repo_url(&canonical)?;
        Some(format!("{host}/{owner}/{name}"))
    } else {
        let (namespace, name) = normalize_repo_path(&path)?;
        Some(format!("{host}/{namespace}/{name}"))
    }
}

/// The lowercase host of a repository URL, for provider inference.
fn repo_url_host(url: &str) -> Option<String> {
    let candidate = url.trim();
    if candidate.is_empty()
        || candidate
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return None;
    }
    split_repo_location(candidate).map(|(host, _)| host)
}

/// Infer the provider from the URL host when the caller leaves it
/// out: `github.com` means `github`, `gitlab.com` means `gitlab`,
/// anything else (including unparseable URLs) means `other`.
pub fn infer_provider(url: &str) -> &'static str {
    match repo_url_host(url).as_deref() {
        Some(GITHUB_BARE_HOST) => PROVIDER_GITHUB,
        Some(GITLAB_BARE_HOST) => PROVIDER_GITLAB,
        _ => PROVIDER_OTHER,
    }
}

// ---------------------------------------------------------------------------
// Validation (the `repos` arm of `check_key_value`)
// ---------------------------------------------------------------------------

fn invalid(detail: String) -> SettingsError {
    SettingsError::InvalidValue {
        key: REPOS_KEY.to_owned(),
        detail,
    }
}

/// Whether a branch name passes the legacy column's character rule
/// (`^[A-Za-z0-9._/-]*$`).
pub fn valid_base_branch(branch: &str) -> bool {
    branch
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '/' | '-'))
}

/// Type-check one incoming `repos` value: an array of entries with a
/// UUID id, a parseable URL (at most 512 chars), a known provider
/// (when given), and a branch name under the legacy rule; no
/// duplicate ids and no two entries with the same identity key.
pub fn check_repos_value(value: &Value) -> Result<(), SettingsError> {
    let Value::Array(entries) = value else {
        return Err(invalid("must be an array of repository entries".to_owned()));
    };
    let mut seen_ids = HashSet::new();
    let mut seen_keys = HashSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let Value::Object(map) = entry else {
            return Err(invalid(format!("entry {index} must be an object")));
        };
        for field in map.keys() {
            if !matches!(field.as_str(), "id" | "url" | "provider" | "base_branch") {
                return Err(invalid(format!("entry {index} has unknown field {field}")));
            }
        }
        let id_text = match map.get("id") {
            Some(Value::String(text)) => text,
            _ => return Err(invalid(format!("entry {index} id must be a UUID"))),
        };
        let id = Uuid::parse_str(id_text)
            .map_err(|_| invalid(format!("entry {index} id must be a UUID")))?;
        if !seen_ids.insert(id) {
            return Err(invalid(format!("duplicate repository id {id_text}")));
        }
        let url = match map.get("url") {
            Some(Value::String(text)) => text,
            _ => return Err(invalid(format!("entry {index} url must be a string"))),
        };
        if url.len() > REPO_URL_MAX_LEN {
            return Err(invalid(format!(
                "entry {index} url must be at most {REPO_URL_MAX_LEN} characters"
            )));
        }
        let key = repo_identity_key(url)
            .ok_or_else(|| invalid(format!("entry {index} url must be a valid repository URL")))?;
        if !seen_keys.insert(key) {
            return Err(invalid(format!(
                "entry {index} is a duplicate of another repository in the list"
            )));
        }
        match map.get("provider") {
            None | Some(Value::Null) => {}
            Some(Value::String(provider)) if KNOWN_PROVIDERS.contains(&provider.as_str()) => {}
            _ => {
                return Err(invalid(format!(
                    "entry {index} provider must be one of github, gitlab, other"
                )));
            }
        }
        match map.get("base_branch") {
            None | Some(Value::Null) => {}
            Some(Value::String(branch)) => check_entry_branch(index, branch)?,
            _ => {
                return Err(invalid(format!(
                    "entry {index} base_branch must be a string"
                )))
            }
        }
    }
    Ok(())
}

fn check_entry_branch(index: usize, branch: &str) -> Result<(), SettingsError> {
    if branch.len() > BASE_BRANCH_MAX_LEN {
        return Err(invalid(format!(
            "entry {index} base_branch must be at most {BASE_BRANCH_MAX_LEN} characters"
        )));
    }
    if !valid_base_branch(branch) {
        return Err(invalid(format!(
            "entry {index} base_branch: Branch name may contain only letters, numbers, and . _ / -"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Merged read
// ---------------------------------------------------------------------------

/// The pure merge of `settings.repos` with the legacy columns: the
/// stored list wins on identity match, and a legacy-only repo needs
/// the temporary copy.
pub enum MergePlan {
    /// The list to return; no write needed.
    Merged(Vec<ProjectRepo>),
    /// The legacy columns hold a repo `settings.repos` does not: the
    /// caller must copy it, then re-derive.
    LegacyOnly,
}

/// Merge `settings.repos` with a legacy `repo_url`: a stored entry
/// with the same identity key wins (one record, the settings entry);
/// a missing legacy URL (or an unparseable one, which no entry can
/// match) returns the stored list as is, writing nothing.
pub fn merge_repos(stored: &[ProjectRepo], repo_url: &str) -> MergePlan {
    let Some(key) = repo_identity_key(repo_url) else {
        return MergePlan::Merged(stored.to_vec());
    };
    if stored
        .iter()
        .any(|entry| repo_identity_key(&entry.url).as_deref() == Some(key.as_str()))
    {
        return MergePlan::Merged(stored.to_vec());
    }
    MergePlan::LegacyOnly
}

// ---------------------------------------------------------------------------
// Write sync (full-list replace through the settings PATCH)
// ---------------------------------------------------------------------------

/// The legacy column write a repo-list change carries so the columns
/// stay consistent (double delete, edit-follow).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyColumnSync {
    /// New `projects.repo_url` value (`""` clears it).
    pub repo_url: String,
    /// New `projects.base_branch` value (`""` clears it).
    pub base_branch: String,
}

fn branch_or_none(branch: &str) -> Option<String> {
    if branch.is_empty() {
        None
    } else {
        Some(branch.to_owned())
    }
}

/// After a settings-PATCH replace of the repos list, derive the
/// legacy column write that keeps the columns consistent:
///
/// - The legacy-matching entry is still present (same identity): the
///   columns follow it when its stored form or branch changed.
/// - It was removed, or its URL changed (same id, new identity):
///   the columns follow the renamed entry, or clear when the entry
///   is gone (double delete: the next read must not copy it back).
/// - The legacy URL was empty or matches nothing: no write.
pub fn sync_columns_after_replace(
    old_stored: &[ProjectRepo],
    new_stored: &[ProjectRepo],
    legacy_repo_url: &str,
    legacy_base_branch: &str,
) -> Option<LegacyColumnSync> {
    let legacy_key = repo_identity_key(legacy_repo_url)?;
    let old_entry = old_stored
        .iter()
        .find(|entry| repo_identity_key(&entry.url).as_deref() == Some(legacy_key.as_str()))?;
    match new_stored
        .iter()
        .find(|entry| repo_identity_key(&entry.url).as_deref() == Some(legacy_key.as_str()))
    {
        Some(current) => {
            let desired = LegacyColumnSync {
                repo_url: current.url.clone(),
                base_branch: current.base_branch.clone().unwrap_or_default(),
            };
            if desired.repo_url == legacy_repo_url && desired.base_branch == legacy_base_branch {
                None
            } else {
                Some(desired)
            }
        }
        None => match new_stored.iter().find(|entry| entry.id == old_entry.id) {
            Some(renamed) => Some(LegacyColumnSync {
                repo_url: renamed.url.clone(),
                base_branch: renamed.base_branch.clone().unwrap_or_default(),
            }),
            None => Some(LegacyColumnSync {
                repo_url: String::new(),
                base_branch: String::new(),
            }),
        },
    }
}

/// Outcome of [`plan_repo_sync`]: legacy column overrides plus an
/// optional `repos` fragment to fold into the settings merge, so the
/// columns and the list stay consistent in one write path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepoSyncPlan {
    /// Override for the legacy `repo_url` column when the list drives
    /// it (settings-PATCH replace that follows a rename or a remove).
    pub repo_url: Option<String>,
    /// Override for the legacy `base_branch` column, same trigger.
    pub base_branch: Option<String>,
    /// `{"repos": [...]}` to fold into the settings merge when the
    /// legacy PATCH changed the list (replace or remove).
    pub repos_fragment: Option<Value>,
}

/// Ways [`plan_repo_sync`] refuses the write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoSyncError {
    /// The legacy URL change would duplicate another settings entry:
    /// the old repo matched one entry while the new URL matches a
    /// different one, so updating the old entry would leave two
    /// entries with the same identity key.
    DuplicateLegacyUrl,
    /// A validated `repos` fragment failed to parse (unreachable
    /// through the validated settings path; the writer bug-checks).
    InvalidFragment,
}

/// Plan the column/list sync for a project write carrying an optional
/// validated `repos` fragment (settings PATCH full-list replace) and
/// optional new legacy column values (PATCH/PUT of `repo_url` /
/// `base_branch`):
///
/// - With a fragment, the list is authoritative: the columns follow a
///   rename (same id, new identity) or clear when the legacy-matching
///   entry is gone (double delete).
/// - With changed legacy columns, the matching list entry is replaced
///   (or removed when the URL is emptied); a change that would
///   duplicate another entry is refused.
/// - With neither, the plan is empty (no write).
pub fn plan_repo_sync(
    instance_settings: &Value,
    instance_repo_url: &str,
    instance_base_branch: &str,
    fragment_repos: Option<&Value>,
    new_repo_url: Option<&str>,
    new_base_branch: Option<&str>,
) -> Result<RepoSyncPlan, RepoSyncError> {
    let old_stored = repos_from_stored(instance_settings);
    let fragment_list: Option<Vec<ProjectRepo>> = fragment_repos
        .map(|fragment| serde_json::from_value(fragment.clone()))
        .transpose()
        .map_err(|_| RepoSyncError::InvalidFragment)?;
    let mut base_list: Vec<ProjectRepo> =
        fragment_list.clone().unwrap_or_else(|| old_stored.clone());
    let legacy_changed = new_repo_url.is_some_and(|url| url != instance_repo_url)
        || new_base_branch.is_some_and(|branch| branch != instance_base_branch);
    if legacy_changed {
        let new_url = new_repo_url.unwrap_or(instance_repo_url);
        let new_branch = new_base_branch.unwrap_or(instance_base_branch);
        if new_url.trim().is_empty() {
            let updated = sync_entry_after_legacy_patch(&base_list, instance_repo_url, "", "");
            if updated != base_list {
                base_list = updated;
            }
        } else {
            if let Some(key) = repo_identity_key(new_url) {
                let old_key = repo_identity_key(instance_repo_url);
                let old_match = old_key.as_ref().and_then(|old| {
                    base_list.iter().find(|entry| {
                        repo_identity_key(&entry.url).as_deref() == Some(old.as_str())
                    })
                });
                if let Some(matched) = old_match {
                    if base_list.iter().any(|entry| {
                        entry.id != matched.id
                            && repo_identity_key(&entry.url).as_deref() == Some(key.as_str())
                    }) {
                        return Err(RepoSyncError::DuplicateLegacyUrl);
                    }
                }
            }
            let updated =
                sync_entry_after_legacy_patch(&base_list, instance_repo_url, new_url, new_branch);
            if updated != base_list {
                base_list = updated;
            }
        }
    }
    let mut plan = RepoSyncPlan::default();
    let baseline: Vec<ProjectRepo> = fragment_list.unwrap_or_else(|| old_stored.clone());
    if base_list != baseline {
        plan.repos_fragment =
            Some(serde_json::to_value(&base_list).map_err(|_| RepoSyncError::InvalidFragment)?);
    }
    if fragment_repos.is_some() {
        if let Some(sync) = sync_columns_after_replace(
            &old_stored,
            &base_list,
            instance_repo_url,
            instance_base_branch,
        ) {
            plan.repo_url = Some(sync.repo_url);
            plan.base_branch = Some(sync.base_branch);
        }
    }
    Ok(plan)
}

/// After a legacy `repo_url`/`base_branch` PATCH (with the PATCH's
/// final column values), update the matching `settings.repos` entry
/// so the repo is replaced instead of duplicated by the next
/// copy-on-read; an emptied `repo_url` removes the matching entries
/// (double delete). A legacy URL that matches no entry leaves the
/// list alone — the merged read picks the new URL up as a legacy
/// repo.
pub fn sync_entry_after_legacy_patch(
    stored: &[ProjectRepo],
    old_repo_url: &str,
    new_repo_url: &str,
    new_base_branch: &str,
) -> Vec<ProjectRepo> {
    let old_key = repo_identity_key(old_repo_url);
    if new_repo_url.trim().is_empty() {
        return match old_key {
            Some(key) => stored
                .iter()
                .filter(|entry| repo_identity_key(&entry.url).as_deref() != Some(key.as_str()))
                .cloned()
                .collect(),
            None => stored.to_vec(),
        };
    }
    let Some(key) = old_key else {
        return stored.to_vec();
    };
    let new_host = repo_url_host(new_repo_url);
    stored
        .iter()
        .map(|entry| {
            if repo_identity_key(&entry.url).as_deref() != Some(key.as_str()) {
                return entry.clone();
            }
            let provider = match (&entry.provider, &new_host) {
                // An explicit provider survives a move within its host;
                // across hosts it is re-inferred (a github→gitlab move
                // must not keep "github").
                (Some(previous), Some(host))
                    if repo_url_host(&entry.url).as_deref() == Some(host.as_str()) =>
                {
                    Some(previous.clone())
                }
                (Some(_), _) => Some(infer_provider(new_repo_url).to_owned()),
                (None, _) => None,
            };
            ProjectRepo {
                id: entry.id,
                url: new_repo_url.to_owned(),
                provider,
                base_branch: branch_or_none(new_base_branch),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Temporary copy-on-read (TEMPORARY: NEWPIDASH-416 removes this)
// ---------------------------------------------------------------------------

/// The authoritative stored list after
/// [`copy_legacy_repo_if_missing`]: the pre-existing entries plus
/// the copy when one was needed.
pub struct CopyOutcome {
    /// Whether this call appended the legacy repo.
    pub copied: bool,
    /// The stored list after this call.
    pub repos: Vec<ProjectRepo>,
}

/// Copy a legacy-only repo into `settings.repos` (TEMPORARY,
/// NEWPIDASH-416: the one-shot migration of a legacy-only repo into
/// `settings.repos` on first read; the cleanup issue removes this
/// function and the merge).
///
/// The check-and-append runs in one transaction under a row lock
/// (`SELECT ... FOR UPDATE`), so two concurrent first reads cannot
/// append two copies: the loser re-reads under the lock, sees the
/// winner's entry, and skips the write. The update ORs the `repos`
/// key into the stored blob, so unknown settings keys are never
/// dropped. Each copy is logged (project id) so the cleanup issue
/// can tell when no projects are left to copy.
pub async fn copy_legacy_repo_if_missing(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    actor: Uuid,
    now: DateTime<Utc>,
) -> Result<CopyOutcome, sqlx::Error> {
    let mut tx = pool.begin().await?;
    // The copy serializes concurrent first reads on this row lock.
    // raw-sql: SeaORM cannot take SELECT ... FOR UPDATE on the sqlx-pool read path.
    let row: Option<(Value, String, String)> = sqlx::query_as(
        "SELECT settings, repo_url, base_branch FROM projects WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(project_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((settings, repo_url, base_branch)) = row else {
        return Ok(CopyOutcome {
            copied: false,
            repos: Vec::new(),
        });
    };
    let stored = repos_from_stored(&settings);
    let outcome = match repo_identity_key(&repo_url) {
        Some(key)
            if !stored
                .iter()
                .any(|entry| repo_identity_key(&entry.url).as_deref() == Some(key.as_str())) =>
        {
            let mut repos = stored;
            repos.push(ProjectRepo {
                id: Uuid::new_v4(),
                url: repo_url.clone(),
                provider: Some(infer_provider(&repo_url).to_owned()),
                base_branch: branch_or_none(&base_branch),
            });
            let merge =
                serde_json::to_value(&repos).map(|repos| serde_json::json!({"repos": repos}));
            match merge {
                Ok(merge) => {
                    // Merge under the row lock above, preserving unknown keys.
                    // raw-sql: SeaORM cannot run the jsonb merge on a sqlx transaction.
                    sqlx::query(
                        "UPDATE projects SET settings = settings || $1::jsonb, updated_at = $2, updated_by_id = $3 WHERE id = $4 AND deleted_at IS NULL",
                    )
                    .bind(merge)
                    .bind(now)
                    .bind(actor)
                    .bind(project_id)
                    .execute(&mut *tx)
                    .await?;
                    tx.commit().await?;
                    tracing::info!(project_id = %project_id, "project_repos.legacy_copied");
                    CopyOutcome {
                        copied: true,
                        repos,
                    }
                }
                Err(_) => CopyOutcome {
                    copied: false,
                    repos,
                },
            }
        }
        _ => CopyOutcome {
            copied: false,
            repos: stored,
        },
    };
    Ok(outcome)
}

/// The merged list for readers that cannot trigger the temporary copy
/// (NEWPIDASH-414: agent prompt context and run envelope — no actor at
/// hand, or inside a creation transaction where a settings write does
/// not belong): `settings.repos` merged with the legacy columns, with
/// a legacy-only repo resolved to the same entry the copy would append
/// (fresh id, URL as stored, provider inferred, branch copied as is),
/// without writing. The project serializers still trigger the real
/// copy (TEMPORARY, NEWPIDASH-416).
pub fn merged_repos_in_memory(
    stored: &[ProjectRepo],
    repo_url: &str,
    base_branch: &str,
) -> Vec<ProjectRepo> {
    match merge_repos(stored, repo_url) {
        MergePlan::Merged(repos) => repos,
        MergePlan::LegacyOnly => {
            let mut repos = stored.to_vec();
            repos.push(ProjectRepo {
                id: Uuid::new_v4(),
                url: repo_url.to_owned(),
                provider: Some(infer_provider(repo_url).to_owned()),
                base_branch: branch_or_none(base_branch),
            });
            repos
        }
    }
}

/// The rendered merged list in one call: [`repos_from_stored`] through
/// [`merged_repos_in_memory`] to [`render_repos`]. The shape agent
/// readers (prompt context, run envelope) embed.
pub fn rendered_merged_repos(
    settings: &Value,
    repo_url: &str,
    base_branch: &str,
) -> Vec<RenderedRepo> {
    render_repos(&merged_repos_in_memory(
        &repos_from_stored(settings),
        repo_url,
        base_branch,
    ))
}

/// The one merged read every reader uses: `settings.repos` merged
/// with the legacy columns. A legacy-only repo is copied into
/// `settings.repos` first (TEMPORARY, NEWPIDASH-416); every other
/// case writes nothing.
pub async fn merged_project_repos(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    stored: &[ProjectRepo],
    repo_url: &str,
    actor: Uuid,
    now: DateTime<Utc>,
) -> Result<Vec<ProjectRepo>, sqlx::Error> {
    match merge_repos(stored, repo_url) {
        MergePlan::Merged(repos) => Ok(repos),
        MergePlan::LegacyOnly => Ok(copy_legacy_repo_if_missing(pool, project_id, actor, now)
            .await?
            .repos),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn entry(id: u128, url: &str) -> ProjectRepo {
        ProjectRepo {
            id: Uuid::from_u128(id),
            url: url.to_owned(),
            provider: None,
            base_branch: None,
        }
    }

    fn stored(urls: &[&str]) -> Vec<ProjectRepo> {
        urls.iter()
            .enumerate()
            .map(|(index, url)| entry(index as u128 + 1, url))
            .collect()
    }

    // -- identity key ---------------------------------------------------

    #[test]
    fn identity_folds_url_forms_to_one_key() {
        let forms = [
            "https://github.com/acme/web",
            "https://github.com/acme/web.git",
            "https://github.com/acme/web/",
            "https://github.com/acme/web.git/",
            "http://github.com/acme/web",
            "git@github.com:acme/web.git",
            "git@github.com:acme/web",
            "https://GITHUB.COM/acme/web",
            "https://github.com/Acme/Web",
            "https://github.com/Acme/Web.git",
            "  https://github.com/acme/web  ",
            "ssh://git@github.com/acme/web.git",
        ];
        let first = repo_identity_key(forms[0]).expect("parses");
        assert_eq!(first, "github.com/acme/web");
        for form in forms {
            assert_eq!(
                repo_identity_key(form).as_deref(),
                Some(first.as_str()),
                "{form}"
            );
        }
    }

    #[test]
    fn identity_covers_gitlab_shapes() {
        let forms = [
            "https://gitlab.com/acme/group/web",
            "https://gitlab.com/acme/group/web.git",
            "git@gitlab.com:acme/group/web.git",
            "https://GITLAB.COM/acme/group/web",
        ];
        let first = repo_identity_key(forms[0]).expect("parses");
        assert_eq!(first, "gitlab.com/acme/group/web");
        for form in forms {
            assert_eq!(
                repo_identity_key(form).as_deref(),
                Some(first.as_str()),
                "{form}"
            );
        }
        // Subgroups survive; view suffixes address their repo.
        assert_eq!(
            repo_identity_key("https://gitlab.com/a/b/c").as_deref(),
            Some("gitlab.com/a/b/c")
        );
        assert_eq!(
            repo_identity_key("https://gitlab.com/a/b/-/merge_requests/1").as_deref(),
            Some("gitlab.com/a/b")
        );
    }

    #[test]
    fn identity_keeps_other_hosts_case_preserved() {
        assert_eq!(
            repo_identity_key("https://git.example.com/Acme/Web").as_deref(),
            Some("git.example.com/Acme/Web")
        );
        assert_eq!(
            repo_identity_key("GIT@GIT.EXAMPLE.COM:Acme/Web.git").as_deref(),
            Some("git.example.com/Acme/Web")
        );
        assert_eq!(
            repo_identity_key("https://git.example.com/Acme/Web?x=1#frag").as_deref(),
            Some("git.example.com/Acme/Web")
        );
    }

    #[test]
    fn identity_rejects_non_repo_urls() {
        for bad in [
            "",
            "   ",
            "not a url",
            "https://github.com/acme",
            "https://github.com/",
            "https://github.com",
            "https://",
            "ftp://github.com/acme/web",
            "C:\\repos\\web",
            "/local/path/web",
            "https://github.com/acme/web with space",
            "git@github.com:",
            "git@github.com:web",
            "https://gitlab.com/onlyone",
        ] {
            assert_eq!(repo_identity_key(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn identity_distinguishes_repos() {
        assert_ne!(
            repo_identity_key("https://github.com/acme/web-a"),
            repo_identity_key("https://github.com/acme/web-b")
        );
        assert_ne!(
            repo_identity_key("https://github.com/acme/web"),
            repo_identity_key("https://gitlab.com/acme/web")
        );
    }

    #[test]
    fn create_request_has_repo_checks_both_sources() {
        assert!(!create_request_has_repo(None, None));
        assert!(!create_request_has_repo(Some(""), None));
        assert!(create_request_has_repo(
            Some("https://github.com/a/b"),
            None
        ));
        assert!(!create_request_has_repo(None, Some(&json!({}))));
        assert!(!create_request_has_repo(None, Some(&json!({"repos": []}))));
        assert!(create_request_has_repo(
            None,
            Some(
                &json!({"repos": [{"id": "11111111-1111-1111-1111-111111111111", "url": "https://github.com/a/b"}]})
            )
        ));
    }

    #[test]
    fn provider_inference_follows_the_host() {
        assert_eq!(infer_provider("https://github.com/a/b"), PROVIDER_GITHUB);
        assert_eq!(infer_provider("git@github.com:a/b.git"), PROVIDER_GITHUB);
        assert_eq!(infer_provider("https://GITHUB.COM/a/b"), PROVIDER_GITHUB);
        assert_eq!(infer_provider("https://gitlab.com/a/b"), PROVIDER_GITLAB);
        assert_eq!(
            infer_provider("https://git.example.com/a/b"),
            PROVIDER_OTHER
        );
        assert_eq!(infer_provider(""), PROVIDER_OTHER);
        assert_eq!(infer_provider("garbage"), PROVIDER_OTHER);
    }

    // -- validation -----------------------------------------------------

    fn value_entry(id: &str, url: &str, provider: Value, branch: Value) -> Value {
        let mut map = serde_json::Map::new();
        map.insert("id".to_owned(), Value::String(id.to_owned()));
        map.insert("url".to_owned(), Value::String(url.to_owned()));
        map.insert("provider".to_owned(), provider);
        map.insert("base_branch".to_owned(), branch);
        Value::Object(map)
    }

    fn valid_list() -> Value {
        Value::Array(vec![
            value_entry(
                "11111111-1111-1111-1111-111111111111",
                "https://github.com/acme/web",
                Value::String("github".to_owned()),
                Value::Null,
            ),
            value_entry(
                "22222222-2222-2222-2222-222222222222",
                "https://gitlab.com/acme/api",
                Value::Null,
                Value::String("main".to_owned()),
            ),
        ])
    }

    #[test]
    fn repos_validation_accepts_a_valid_list() {
        assert_eq!(check_repos_value(&Value::Array(vec![])), Ok(()));
        assert_eq!(check_repos_value(&valid_list()), Ok(()));
    }

    #[test]
    fn repos_validation_rejects_bad_shapes() {
        for bad in [json!(null), json!({}), json!("x"), json!(1), json!(true)] {
            assert_eq!(
                check_repos_value(&bad),
                Err(invalid("must be an array of repository entries".to_owned())),
                "{bad}"
            );
        }
        assert!(matches!(
            check_repos_value(&Value::Array(vec![json!(1)])),
            Err(SettingsError::InvalidValue { .. })
        ));
        let entry = value_entry(
            "not-a-uuid",
            "https://github.com/a/b",
            Value::Null,
            Value::Null,
        );
        assert!(matches!(
            check_repos_value(&Value::Array(vec![entry])),
            Err(SettingsError::InvalidValue { .. })
        ));
    }

    #[test]
    fn repos_validation_rejects_bad_urls_providers_and_branches() {
        let bad_url = value_entry(
            "11111111-1111-1111-1111-111111111111",
            "not a repo",
            Value::Null,
            Value::Null,
        );
        assert!(matches!(
            check_repos_value(&Value::Array(vec![bad_url])),
            Err(SettingsError::InvalidValue { .. })
        ));
        let bad_provider = value_entry(
            "11111111-1111-1111-1111-111111111111",
            "https://github.com/a/b",
            Value::String("bitbucket".to_owned()),
            Value::Null,
        );
        assert_eq!(
            check_repos_value(&Value::Array(vec![bad_provider])),
            Err(invalid(
                "entry 0 provider must be one of github, gitlab, other".to_owned()
            ))
        );
        let bad_branch = value_entry(
            "11111111-1111-1111-1111-111111111111",
            "https://github.com/a/b",
            Value::Null,
            Value::String("main;rm".to_owned()),
        );
        assert_eq!(
            check_repos_value(&Value::Array(vec![bad_branch])),
            Err(invalid(
                "entry 0 base_branch: Branch name may contain only letters, numbers, and . _ / -"
                    .to_owned()
            ))
        );
        // Unknown entry fields are rejected.
        let mut extra = value_entry(
            "11111111-1111-1111-1111-111111111111",
            "https://github.com/a/b",
            Value::Null,
            Value::Null,
        );
        extra
            .as_object_mut()
            .expect("object")
            .insert("clone_url".to_owned(), Value::String("x".to_owned()));
        assert!(matches!(
            check_repos_value(&Value::Array(vec![extra])),
            Err(SettingsError::InvalidValue { .. })
        ));
    }

    #[test]
    fn repos_validation_rejects_duplicates_including_other_url_forms() {
        // Same URL twice.
        let dup = Value::Array(vec![
            value_entry(
                "11111111-1111-1111-1111-111111111111",
                "https://github.com/acme/web",
                Value::Null,
                Value::Null,
            ),
            value_entry(
                "22222222-2222-2222-2222-222222222222",
                "https://github.com/acme/web",
                Value::Null,
                Value::Null,
            ),
        ]);
        assert!(matches!(
            check_repos_value(&dup),
            Err(SettingsError::InvalidValue { .. })
        ));
        // Same repo in another URL form.
        let dup_form = Value::Array(vec![
            value_entry(
                "11111111-1111-1111-1111-111111111111",
                "https://github.com/acme/web",
                Value::Null,
                Value::Null,
            ),
            value_entry(
                "22222222-2222-2222-2222-222222222222",
                "git@github.com:acme/web.git",
                Value::Null,
                Value::Null,
            ),
        ]);
        assert_eq!(
            check_repos_value(&dup_form),
            Err(invalid(
                "entry 1 is a duplicate of another repository in the list".to_owned()
            ))
        );
        // Duplicate ids.
        let dup_id = Value::Array(vec![
            value_entry(
                "11111111-1111-1111-1111-111111111111",
                "https://github.com/acme/web",
                Value::Null,
                Value::Null,
            ),
            value_entry(
                "11111111-1111-1111-1111-111111111111",
                "https://github.com/acme/other",
                Value::Null,
                Value::Null,
            ),
        ]);
        assert!(matches!(
            check_repos_value(&dup_id),
            Err(SettingsError::InvalidValue { .. })
        ));
    }

    #[test]
    fn repos_key_requires_project_admin() {
        use super::super::project_settings::authorize_and_validate;
        use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};

        let fragment = json!({"repos": []});
        assert!(matches!(
            authorize_and_validate(&fragment, ROLE_MEMBER, ROLE_MEMBER),
            Err(SettingsError::Forbidden { .. })
        ));
        assert!(matches!(
            authorize_and_validate(&fragment, ROLE_GUEST, ROLE_GUEST),
            Err(SettingsError::Forbidden { .. })
        ));
        assert!(authorize_and_validate(&fragment, ROLE_ADMIN, ROLE_GUEST).is_ok());
        assert!(authorize_and_validate(&fragment, ROLE_GUEST, ROLE_ADMIN).is_ok());
        // A bad value is still a 400 field error through the settings path.
        let bad = json!({"repos": [{"id": "x"}]});
        let error = authorize_and_validate(&bad, ROLE_ADMIN, ROLE_GUEST).expect_err("400");
        assert_eq!(error.status(), 400);
        assert!(error.body().starts_with("{\"settings\":["));
    }

    // -- merge ----------------------------------------------------------

    #[test]
    fn merge_returns_the_settings_entry_on_identity_match() {
        let list = stored(&["https://github.com/acme/web"]);
        for legacy in [
            "https://github.com/acme/web",
            "https://github.com/acme/web.git",
            "git@github.com:acme/web.git",
            "https://GITHUB.COM/Acme/Web/",
        ] {
            match merge_repos(&list, legacy) {
                MergePlan::Merged(merged) => assert_eq!(merged, list, "{legacy}"),
                MergePlan::LegacyOnly => panic!("{legacy} should dedup"),
            }
        }
    }

    #[test]
    fn merge_returns_stored_as_is_without_writing() {
        let list = stored(&["https://github.com/acme/web"]);
        match merge_repos(&list, "") {
            MergePlan::Merged(merged) => assert_eq!(merged, list),
            MergePlan::LegacyOnly => panic!("empty legacy needs no copy"),
        }
        // An unparseable legacy URL can match nothing: stored as is.
        match merge_repos(&list, "not a url") {
            MergePlan::Merged(merged) => assert_eq!(merged, list),
            MergePlan::LegacyOnly => panic!("garbage legacy needs no copy"),
        }
        // Neither source: an empty list.
        match merge_repos(&[], "") {
            MergePlan::Merged(merged) => assert!(merged.is_empty()),
            MergePlan::LegacyOnly => panic!("nothing to copy"),
        }
    }

    #[test]
    fn merge_marks_legacy_only_for_copy() {
        let list = stored(&["https://github.com/acme/other"]);
        assert!(matches!(
            merge_repos(&list, "https://github.com/acme/web"),
            MergePlan::LegacyOnly
        ));
        assert!(matches!(
            merge_repos(&[], "https://github.com/acme/web"),
            MergePlan::LegacyOnly
        ));
    }

    #[test]
    fn in_memory_merge_matches_the_copy_shape_without_writing() {
        // Legacy-only: the same entry the copy would append (URL as
        // stored, provider inferred, branch copied as is).
        let merged = merged_repos_in_memory(&[], "https://github.com/acme/web", "main");
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].url, "https://github.com/acme/web");
        assert_eq!(merged[0].provider.as_deref(), Some("github"));
        assert_eq!(merged[0].base_branch.as_deref(), Some("main"));
        // Empty branch stays empty, like the copy.
        let merged = merged_repos_in_memory(&[], "https://github.com/acme/web", "");
        assert_eq!(merged[0].base_branch, None);
        // Dedup and stored-only cases mirror `merge_repos`.
        let list = stored(&["https://github.com/acme/web"]);
        assert_eq!(
            merged_repos_in_memory(&list, "git@github.com:acme/web.git", "other"),
            list
        );
        assert_eq!(merged_repos_in_memory(&list, "", ""), list);
        assert!(merged_repos_in_memory(&[], "", "").is_empty());
    }

    #[test]
    fn render_fills_a_missing_provider() {
        let rendered = render_repos(&stored(&["https://github.com/acme/web"]));
        assert_eq!(rendered.len(), 1);
        assert_eq!(rendered[0].provider, "github");
        assert_eq!(rendered[0].base_branch, None);
    }

    #[test]
    fn stored_read_is_tolerant_and_defaults_to_empty() {
        assert!(repos_from_stored(&json!({})).is_empty());
        assert!(repos_from_stored(&json!({"repos": null})).is_empty());
        assert!(repos_from_stored(&json!({"repos": [json!(1)]})).is_empty());
    }

    // -- write sync -----------------------------------------------------

    #[test]
    fn replace_sync_clears_columns_on_remove() {
        let old = stored(&["https://github.com/acme/web"]);
        let sync = sync_columns_after_replace(&old, &[], "https://github.com/acme/web", "main");
        assert_eq!(
            sync,
            Some(LegacyColumnSync {
                repo_url: String::new(),
                base_branch: String::new(),
            })
        );
    }

    #[test]
    fn replace_sync_follows_a_renamed_url() {
        let old = stored(&["https://github.com/acme/web"]);
        let mut new = stored(&["https://github.com/acme/site"]);
        new[0].id = old[0].id;
        let sync = sync_columns_after_replace(&old, &new, "https://github.com/acme/web", "main");
        assert_eq!(
            sync,
            Some(LegacyColumnSync {
                repo_url: "https://github.com/acme/site".to_owned(),
                base_branch: String::new(),
            })
        );
    }

    #[test]
    fn replace_sync_follows_a_branch_change_and_ignores_no_ops() {
        let old = stored(&["https://github.com/acme/web"]);
        let mut changed = stored(&["https://github.com/acme/web"]);
        changed[0].base_branch = Some("trunk".to_owned());
        let sync =
            sync_columns_after_replace(&old, &changed, "https://github.com/acme/web", "main");
        assert_eq!(
            sync,
            Some(LegacyColumnSync {
                repo_url: "https://github.com/acme/web".to_owned(),
                base_branch: "trunk".to_owned(),
            })
        );
        let sync_none = sync_columns_after_replace(&old, &old, "https://github.com/acme/web", "");
        assert_eq!(sync_none, None);
    }

    #[test]
    fn replace_sync_ignores_an_empty_or_unmatched_legacy() {
        let old = stored(&["https://github.com/acme/web"]);
        assert_eq!(sync_columns_after_replace(&old, &[], "", ""), None);
        assert_eq!(
            sync_columns_after_replace(&old, &[], "https://github.com/acme/gone", "main"),
            None
        );
    }

    #[test]
    fn legacy_patch_updates_the_matching_entry() {
        let old = stored(&["https://github.com/acme/web"]);
        let updated = sync_entry_after_legacy_patch(
            &old,
            "https://github.com/acme/web",
            "https://github.com/acme/site",
            "trunk",
        );
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].id, old[0].id);
        assert_eq!(updated[0].url, "https://github.com/acme/site");
        assert_eq!(updated[0].base_branch.as_deref(), Some("trunk"));
    }

    #[test]
    fn legacy_patch_empty_url_removes_the_matching_entry() {
        let old = stored(&[
            "https://github.com/acme/web",
            "https://github.com/acme/other",
        ]);
        let updated = sync_entry_after_legacy_patch(&old, "git@github.com:acme/web.git", "", "");
        assert_eq!(updated, vec![old[1].clone()]);
    }

    #[test]
    fn legacy_patch_without_a_match_leaves_the_list_alone() {
        let old = stored(&["https://github.com/acme/web"]);
        let updated = sync_entry_after_legacy_patch(
            &old,
            "https://github.com/acme/gone",
            "https://github.com/acme/site",
            "main",
        );
        assert_eq!(updated, old);
    }

    fn stored_value(urls: &[&str]) -> Value {
        serde_json::to_value(stored(urls)).expect("serializes")
    }

    fn settings_with(urls: &[&str]) -> Value {
        json!({"repos": stored_value(urls)})
    }

    #[test]
    fn plan_is_empty_without_inputs() {
        let settings = settings_with(&["https://github.com/acme/web"]);
        let plan = plan_repo_sync(
            &settings,
            "https://github.com/acme/web",
            "main",
            None,
            None,
            None,
        )
        .expect("plans");
        assert_eq!(plan, RepoSyncPlan::default());
    }

    #[test]
    fn plan_follows_a_fragment_remove_with_column_clear() {
        let settings = settings_with(&["https://github.com/acme/web"]);
        let plan = plan_repo_sync(
            &settings,
            "https://github.com/acme/web",
            "main",
            Some(&Value::Array(vec![])),
            None,
            None,
        )
        .expect("plans");
        assert_eq!(plan.repo_url.as_deref(), Some(""));
        assert_eq!(plan.base_branch.as_deref(), Some(""));
        assert_eq!(plan.repos_fragment, None);
    }

    #[test]
    fn plan_replaces_the_entry_on_legacy_patch() {
        let settings = settings_with(&["https://github.com/acme/web"]);
        let plan = plan_repo_sync(
            &settings,
            "https://github.com/acme/web",
            "main",
            None,
            Some("https://github.com/acme/site"),
            Some("trunk"),
        )
        .expect("plans");
        assert_eq!(plan.repo_url, None);
        assert_eq!(plan.base_branch, None);
        let fragment = plan.repos_fragment.expect("fragment");
        let list: Vec<ProjectRepo> = serde_json::from_value(fragment).expect("parses");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].url, "https://github.com/acme/site");
        assert_eq!(list[0].base_branch.as_deref(), Some("trunk"));
    }

    #[test]
    fn plan_refuses_a_legacy_change_that_duplicates_another_entry() {
        let settings = settings_with(&[
            "https://github.com/acme/web",
            "https://github.com/acme/other",
        ]);
        let error = plan_repo_sync(
            &settings,
            "https://github.com/acme/web",
            "main",
            None,
            Some("git@github.com:acme/other.git"),
            None,
        )
        .expect_err("duplicates");
        assert_eq!(error, RepoSyncError::DuplicateLegacyUrl);
    }

    #[test]
    fn plan_leaves_the_list_alone_when_legacy_matches_nothing() {
        // A legacy PATCH whose old URL no entry holds leaves the list
        // alone: the merged read picks the new URL up as a legacy repo.
        let settings = settings_with(&["https://github.com/acme/other"]);
        let plan = plan_repo_sync(
            &settings,
            "https://github.com/acme/web",
            "main",
            None,
            Some("https://github.com/acme/site"),
            None,
        )
        .expect("plans");
        assert_eq!(plan, RepoSyncPlan::default());
    }

    // -- copy (live scratch DB) -----------------------------------------

    const COPY_DDL: &str = "CREATE TEMPORARY TABLE projects (
        id UUID PRIMARY KEY, settings JSONB NOT NULL,
        repo_url TEXT NOT NULL, base_branch TEXT NOT NULL,
        updated_at TIMESTAMPTZ NOT NULL, updated_by_id UUID, deleted_at TIMESTAMPTZ)";

    async fn scratch_pool() -> Option<sqlx::PgPool> {
        // One connection: the DDL below is a TEMPORARY table, visible
        // only on the connection that created it. The concurrent test
        // still exercises the locked re-check path (the second reader
        // re-reads under FOR UPDATE and skips the write).
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::postgres::PgPoolOptions::new()
                    .max_connections(1)
                    .connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    async fn seed_project(
        pool: &sqlx::PgPool,
        settings: Value,
        repo_url: &str,
        base_branch: &str,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let actor = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO projects (id, settings, repo_url, base_branch, updated_at, updated_by_id, deleted_at)
             VALUES ($1, $2, $3, $4, now(), $5, NULL)",
        )
        .bind(id)
        .bind(settings)
        .bind(repo_url)
        .bind(base_branch)
        .bind(actor)
        .execute(pool)
        .await
        .expect("seed");
        id
    }

    async fn read_settings(pool: &sqlx::PgPool, id: Uuid) -> Value {
        sqlx::query_scalar("SELECT settings FROM projects WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("read back")
    }

    #[tokio::test]
    async fn copy_appends_a_legacy_only_repo() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(COPY_DDL).execute(&pool).await.expect("ddl");
        let id = seed_project(
            &pool,
            json!({"unknown": true}),
            "https://github.com/acme/web",
            "trunk",
        )
        .await;
        let actor = Uuid::new_v4();
        let outcome = copy_legacy_repo_if_missing(&pool, id, actor, Utc::now())
            .await
            .expect("copy");
        assert!(outcome.copied);
        assert_eq!(outcome.repos.len(), 1);
        assert_eq!(outcome.repos[0].url, "https://github.com/acme/web");
        assert_eq!(outcome.repos[0].provider.as_deref(), Some("github"));
        assert_eq!(outcome.repos[0].base_branch.as_deref(), Some("trunk"));
        // Unknown keys survive; a second read writes nothing.
        let blob = read_settings(&pool, id).await;
        assert_eq!(blob.get("unknown"), Some(&json!(true)));
        let again = copy_legacy_repo_if_missing(&pool, id, actor, Utc::now())
            .await
            .expect("second read");
        assert!(!again.copied);
        assert_eq!(again.repos, outcome.repos);
    }

    #[tokio::test]
    async fn copy_skips_when_nothing_is_missing() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(COPY_DDL).execute(&pool).await.expect("ddl");
        let actor = Uuid::new_v4();
        // Legacy already in settings (another URL form): no write.
        let id = seed_project(
            &pool,
            json!({"repos": [{"id": Uuid::new_v4(), "url": "git@github.com:acme/web.git"}]}),
            "https://github.com/acme/web",
            "main",
        )
        .await;
        let outcome = copy_legacy_repo_if_missing(&pool, id, actor, Utc::now())
            .await
            .expect("copy");
        assert!(!outcome.copied);
        assert_eq!(outcome.repos.len(), 1);
        // No legacy repo at all: no write.
        let bare = seed_project(&pool, json!({}), "", "").await;
        let outcome = copy_legacy_repo_if_missing(&pool, bare, actor, Utc::now())
            .await
            .expect("copy");
        assert!(!outcome.copied);
        assert!(outcome.repos.is_empty());
    }

    #[tokio::test]
    async fn concurrent_first_reads_leave_exactly_one_copy() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(COPY_DDL).execute(&pool).await.expect("ddl");
        let id = seed_project(&pool, json!({}), "https://github.com/acme/web", "main").await;
        let actor = Uuid::new_v4();
        let (first, second) = tokio::join!(
            merged_project_repos(
                &pool,
                id,
                &[],
                "https://github.com/acme/web",
                actor,
                Utc::now()
            ),
            merged_project_repos(
                &pool,
                id,
                &[],
                "https://github.com/acme/web",
                actor,
                Utc::now()
            ),
        );
        let first = first.expect("first read");
        let second = second.expect("second read");
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(first[0].id, second[0].id);
        let count: i32 = sqlx::query_scalar(
            "SELECT jsonb_array_length(settings->'repos') FROM projects WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(count, 1);
    }
}
